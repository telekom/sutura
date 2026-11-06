//! One Oracle lookup leg read as several partitions at one snapshot - `github.com/telekom/sutura#828`.
//!
//! [`OracleWarehouse::split_by_estimate`] reads the dictionary's statistics for the leg's root
//! (`ALL_TABLES.NUM_ROWS` and `AVG_ROW_LEN`) and its integer columns, decides through
//! [`sutura_domain::plan::decide`], and for a split reads one system change number,
//! `TIMESTAMP_TO_SCN(SYSTIMESTAMP)`. Each partition is one `ORA_HASH` bucket of one integer key of
//! the root, every table read `AS OF SCN` that number ([`sutura_sql::generate_leg_as_of`]).
//! [`OracleWarehouse::read_partition`] reads one back on any node holding the same leg: it asks the
//! database when the snapshot was taken and refuses one older than its own request budget, then
//! runs the statement. Every statement runs on the boot connection, as the declared service user.
//!
//! **Only a source [`OracleWarehouse::with_flashback`] probed splits.** Its probe refuses a service
//! user holding `FLASHBACK ANY TABLE`, and one that cannot read each table it is handed at a
//! snapshot. Every other source, a fact leg, a root the dictionary holds no table row for (a view, a
//! synonym), a root with no integer `NUMBER` column among the leg's keys, and an estimate at or
//! under the threshold are read whole.
//!
//! **The limits**:
//!
//! - The statistics are the dictionary's, as fresh as the last time they were gathered. A table
//!   with none estimates at zero and is read whole.
//! - `TIMESTAMP_TO_SCN` is approximate: the snapshot can be a few seconds older than the instant it
//!   was asked for, so a row committed in those seconds is not read. `SCN_TO_TIMESTAMP` is as
//!   approximate, so the freshness refusal is that close to the budget and no closer.
//! - How long a snapshot stays readable is the database's undo retention, which the operator
//!   configures and nothing here sets. A snapshot past it is [`OracleError::SnapshotGone`].
//! - Every partition reads the whole root table and keeps its bucket; `ORA_HASH` buckets are not
//!   uniform, and every NULL key lands in bucket 0.
//! - Nothing calls this yet: no composition root offers an Oracle leg for splitting, and no reading
//!   worker reads an Oracle partition.

use core::num::{NonZeroU64, NonZeroUsize};
use std::collections::BTreeSet;

use sutura_domain::identity::Presented;
use sutura_domain::model::QualifiedTable;
use sutura_domain::plan::{EstimatedRows, KeyValue, LegPlan, SplitDecision, SplitRefusal, decide};
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{ParamValue, ResultBatches, RowSet, Value};
use sutura_sql::{KeyBucket, Scn, generate_leg_as_of, generate_table_as_of};

use crate::{OracleError, OracleWarehouse, Session};

mod descriptor;

pub use descriptor::{InvalidDescriptor, PartitionDescriptor};

/// The root's statistics and its integer columns, in the session's own schema.
const ROOT_IN_SCHEMA: &str = "SELECT t.NUM_ROWS, t.AVG_ROW_LEN, c.COLUMN_NAME FROM ALL_TABLES t LEFT JOIN \
                              ALL_TAB_COLUMNS c ON c.OWNER = t.OWNER AND c.TABLE_NAME = t.TABLE_NAME AND \
                              c.DATA_TYPE = 'NUMBER' AND c.DATA_SCALE = 0 WHERE t.TABLE_NAME = :1 AND \
                              t.OWNER = SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA')";

/// [`ROOT_IN_SCHEMA`], for a root its leg names the schema of.
const ROOT_IN_OWNER: &str = "SELECT t.NUM_ROWS, t.AVG_ROW_LEN, c.COLUMN_NAME FROM ALL_TABLES t LEFT JOIN \
                             ALL_TAB_COLUMNS c ON c.OWNER = t.OWNER AND c.TABLE_NAME = t.TABLE_NAME AND \
                             c.DATA_TYPE = 'NUMBER' AND c.DATA_SCALE = 0 WHERE t.TABLE_NAME = :1 AND t.OWNER = :2";

/// The question's snapshot (D3).
const SNAPSHOT: &str = "SELECT TIMESTAMP_TO_SCN(SYSTIMESTAMP) FROM DUAL";

/// `1` where the snapshot `:1` was taken no more than `:2` seconds ago, else `0` (D4).
const FRESH: &str = "SELECT CASE WHEN SCN_TO_TIMESTAMP(TO_NUMBER(:1)) >= CAST(SYSTIMESTAMP AS TIMESTAMP) - \
                     NUMTODSINTERVAL(TO_NUMBER(:2), 'SECOND') THEN 1 ELSE 0 END FROM DUAL";

/// Whether the session holds the system privilege the probe refuses.
const ANY_TABLE: &str = "SELECT COUNT(*) FROM SESSION_PRIVS WHERE PRIVILEGE = 'FLASHBACK ANY TABLE'";

/// The Oracle error numbers of a snapshot the database can no longer read: `ORA-01555` snapshot too
/// old, `ORA-08181` not a valid system change number, `ORA-01466` the table's definition changed.
#[expect(dead_code, reason = "read by `snapshot_gone`, which lands with the partition read")]
const GONE: [usize; 3] = [1555, 8181, 1466];

/// [`OracleWarehouse::split_by_estimate`]'s outcome.
#[derive(Debug)]
pub enum Planned {
    /// Read the leg whole, through [`Warehouse::execute`](sutura_domain::warehouse::Warehouse::execute).
    Whole,
    /// Over the threshold: one partition per bucket, at one snapshot.
    Split(SplitLeg),
    /// Over the threshold, but even the most partitions allowed still leave one over the bound.
    Refused(SplitRefusal),
}

/// A leg cut into buckets at one snapshot. Nothing is held open: the snapshot is in each
/// partition's statement.
#[derive(Debug)]
pub struct SplitLeg {
    scn: Scn,
    partitions: Vec<PartitionDescriptor>,
    estimate: EstimatedBytes,
}

impl SplitLeg {
    /// The snapshot every partition reads.
    #[must_use]
    pub const fn scn(&self) -> Scn {
        self.scn
    }

    /// One descriptor per bucket, in bucket order.
    #[must_use]
    pub fn partitions(&self) -> &[PartitionDescriptor] {
        &self.partitions
    }

    /// The dictionary's estimate of the root, in bytes.
    #[must_use]
    pub const fn estimate(&self) -> EstimatedBytes {
        self.estimate
    }
}

/// [`OracleWarehouse::with_flashback`].
pub(super) fn with_flashback(
    mut warehouse: OracleWarehouse,
    tables: &BTreeSet<QualifiedTable>,
) -> Result<OracleWarehouse, OracleError> {
    refuse_impersonating(&warehouse.routing.posture, &warehouse.routing.source)?;
    if count(&warehouse.run((ANY_TABLE, &[]))?, "the system privilege")? > 0 {
        return Err(OracleError::FlashbackAnyTable);
    }
    let scn = snapshot(&warehouse.run((SNAPSHOT, &[]))?)?;
    for table in tables {
        let probe = generate_table_as_of(&warehouse.routing.source, table, scn)
            .map_err(|cause| OracleError::PartitionRender { cause })?;
        warehouse
            .run((probe.sql(), probe.params()))
            .map_err(|cause| OracleError::FlashbackRefused {
                table: table.to_string(),
                cause: Box::new(cause),
            })?;
    }
    warehouse.flashback = true;
    Ok(warehouse)
}

/// [`OracleWarehouse::split_by_estimate`].
pub(super) fn split_by_estimate(
    warehouse: &OracleWarehouse,
    leg: &LegPlan,
    presented: &Presented,
    deadline: Deadline,
    above_bytes: NonZeroU64,
    per_stream_bound: NonZeroUsize,
    ready_workers: NonZeroUsize,
) -> Result<Planned, OracleError> {
    let boot = matches!(warehouse.routing.deliverable(presented)?, Session::Boot);
    let Some(asked) = dictionary(warehouse.flashback, leg, boot) else {
        return Ok(Planned::Whole);
    };
    let name = ParamValue::Text(String::from(leg.table().name().as_str()));
    let answer = match asked {
        Dictionary::Schema => warehouse.run_with_deadline(Session::Boot, (ROOT_IN_SCHEMA, &[name]), deadline, None)?,
        Dictionary::Owner(owner) => {
            let owner = ParamValue::Text(String::from(owner));
            warehouse.run_with_deadline(Session::Boot, (ROOT_IN_OWNER, &[name, owner]), deadline, None)?
        }
    };
    let Some(root) = root(&answer)? else {
        return Ok(Planned::Whole);
    };
    let Some(key) = hashed_key(leg, &root.integers) else {
        return Ok(Planned::Whole);
    };
    match decide(root.bytes, root.rows, above_bytes, per_stream_bound, ready_workers) {
        SplitDecision::Whole => Ok(Planned::Whole),
        SplitDecision::Refused(refusal) => Ok(Planned::Refused(refusal)),
        SplitDecision::Split(partitions) if partitions.get() < 2 => Ok(Planned::Whole),
        SplitDecision::Split(partitions) => {
            let scn = snapshot(&warehouse.run_with_deadline(Session::Boot, (SNAPSHOT, &[]), deadline, None)?)?;
            Ok(Planned::Split(SplitLeg {
                scn,
                partitions: cut(scn, key, partitions),
                estimate: root.bytes,
            }))
        }
    }
}

/// [`OracleWarehouse::read_partition`].
pub(super) fn read_partition(
    warehouse: &OracleWarehouse,
    leg: &LegPlan,
    descriptor: &[u8],
    deadline: Deadline,
) -> Result<ResultBatches, OracleError> {
    let parsed = admit(warehouse.flashback, &warehouse.routing.source, descriptor)?;
    let query =
        generate_leg_as_of(leg, parsed.scn(), Some(parsed.bucket())).map_err(|cause| OracleError::PartitionRender { cause })?;
    let budget = deadline.budget();
    let asked = [
        ParamValue::Text(parsed.scn().get().to_string()),
        ParamValue::Text(budget.seconds().to_string()),
    ];
    fresh(
        &warehouse
            .run_with_deadline(Session::Boot, (FRESH, &asked), deadline, None)
            .map_err(gone)?,
        budget,
    )?;
    let rows = warehouse
        .run_with_deadline(Session::Boot, (query.sql(), query.params()), deadline, None)
        .map_err(gone)?;
    of_row_set(&rows).map_err(|cause| OracleError::Shape { cause })
}

/// The descriptor `bytes` parse into, at a source that declared and probed its flashback split.
fn admit(flashback: bool, at: &sutura_domain::model::SourceName, bytes: &[u8]) -> Result<PartitionDescriptor, OracleError> {
    if !flashback {
        return Err(OracleError::FlashbackNotDeclared {
            at: String::from(at.as_str()),
        });
    }
    PartitionDescriptor::try_from(bytes).map_err(|cause| OracleError::MalformedDescriptor { cause })
}

/// [`OracleError::SnapshotGone`] for a statement the database refused for its snapshot; every other
/// error unchanged.
fn gone(error: OracleError) -> OracleError {
    match error {
        OracleError::Execute { cause } if cause.ora_code().is_some_and(snapshot_gone) => OracleError::SnapshotGone { cause },
        other => other,
    }
}

/// Whether Oracle error number `code` says a snapshot can no longer be read - one of [`GONE`].
///
/// **Not built yet:** no code is.
const fn snapshot_gone(_code: usize) -> bool {
    false
}

/// [`OracleError::FlashbackAtImpersonatingSource`] for a source that reads as each caller.
///
/// **Not built yet:** no source is refused.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the refusal is what fails, and it lands with the partition read"
)]
const fn refuse_impersonating(_posture: &SourcePosture, _at: &sutura_domain::model::SourceName) -> Result<(), OracleError> {
    Ok(())
}

/// Where the dictionary is asked about a leg's root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(dead_code, reason = "built by `dictionary`, which lands with the split")]
enum Dictionary<'leg> {
    /// The root names no schema: the session's own.
    Schema,
    /// The root names this schema.
    Owner(&'leg str),
}

/// Where the dictionary is asked about `leg`'s root, or `None` for a leg read whole: a fact leg, a
/// source whose split was not probed, a session other than the boot one, a root path of three parts.
///
/// **Not built yet:** every leg is read whole.
const fn dictionary(_flashback: bool, _leg: &LegPlan, _boot: bool) -> Option<Dictionary<'_>> {
    None
}

/// What the dictionary holds for a leg's root table.
#[derive(Debug, PartialEq, Eq)]
struct Root {
    rows: EstimatedRows,
    bytes: EstimatedBytes,
    integers: BTreeSet<String>,
}

/// [`ROOT_IN_SCHEMA`]'s answer: `None` where the dictionary holds no table of that name - a view, a
/// synonym, a table in another schema. A table with no statistics estimates at zero.
fn root(answer: &RowSet) -> Result<Option<Root>, OracleError> {
    let Some(first) = answer.rows().first() else {
        return Ok(None);
    };
    let statistic = |index: usize| match first.get(index) {
        Some(&Value::Null) => Ok(0),
        Some(&Value::Integer(value)) => u64::try_from(value).map_err(|_negative| unanswered("the root's statistics")),
        _ => Err(unanswered("the root's statistics")),
    };
    let (rows, width) = (statistic(0)?, statistic(1)?);
    let mut integers = BTreeSet::new();
    for row in answer.rows() {
        match row.get(2) {
            Some(Value::Text(name)) => {
                integers.insert(name.clone());
            }
            Some(&Value::Null) => {}
            _ => return Err(unanswered("the root's statistics")),
        }
    }
    Ok(Some(Root {
        rows: EstimatedRows::parse(rows),
        bytes: EstimatedBytes::parse(rows.saturating_mul(width)),
        integers,
    }))
}

/// The position of the leg's first key that is a column of its root table among `integers`.
fn hashed_key(leg: &LegPlan, integers: &BTreeSet<String>) -> Option<u16> {
    let position = leg.keys().iter().position(|key| {
        matches!(key.value(), KeyValue::Column(column)
            if column.table() == leg.table_name() && integers.contains(column.column().as_str()))
    })?;
    u16::try_from(position).ok()
}

/// One descriptor per bucket of `partitions`, each of `key` at `scn`.
fn cut(scn: Scn, key: u16, partitions: NonZeroUsize) -> Vec<PartitionDescriptor> {
    let count = u32::try_from(partitions.get()).unwrap_or(u32::MAX);
    (0..count)
        .filter_map(|index| KeyBucket::new(key, index, count).ok())
        .map(|bucket| PartitionDescriptor::new(scn, bucket))
        .collect()
}

/// [`SNAPSHOT`]'s one positive number.
fn snapshot(answer: &RowSet) -> Result<Scn, OracleError> {
    match *answer.rows() {
        [ref row] => match row.as_slice() {
            &[Value::Integer(scn)] => u64::try_from(scn).ok().and_then(Scn::parse),
            _ => None,
        },
        _ => None,
    }
    .ok_or_else(|| unanswered("the snapshot"))
}

/// A one-row, one-number answer, read as a count.
fn count(answer: &RowSet, asked: &'static str) -> Result<u64, OracleError> {
    match *answer.rows() {
        [ref row] => match row.as_slice() {
            &[Value::Integer(counted)] => u64::try_from(counted).ok(),
            _ => None,
        },
        _ => None,
    }
    .ok_or_else(|| unanswered(asked))
}

/// [`FRESH`]'s answer: `1` reads, `0` is [`OracleError::SnapshotStale`].
fn fresh(answer: &RowSet, budget: Budget) -> Result<(), OracleError> {
    match count(answer, "the snapshot's age")? {
        1 => Ok(()),
        0 => Err(OracleError::SnapshotStale {
            most_seconds: budget.seconds(),
        }),
        _ => Err(unanswered("the snapshot's age")),
    }
}

const fn unanswered(asked: &'static str) -> OracleError {
    OracleError::Unanswered { asked }
}

#[cfg(test)]
mod tests;
