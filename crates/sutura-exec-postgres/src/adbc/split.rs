//! One leg read as several partitions under one snapshot - `github.com/telekom/sutura#828`.
//!
//! [`AdbcPostgres::split_leg`] opens one connection that exports a `REPEATABLE READ` snapshot and
//! reads its root table's `pg_class.relpages`, then cuts those pages into `ctid` ranges, the last one
//! open because `relpages` is the catalog's estimate and rows arrive after it. Each
//! [`Partition::read`] opens a connection of its own, imports that snapshot, and runs the leg's
//! statement restricted to its range under the per-stream bound every statement here has - a
//! partition past it is refused like any stream, never split again. Every connection is the declared
//! service user's; nothing here adds an identity, a credential or a setting.
//!
//! **The limits**:
//!
//! - A root that is not a plain table (a view, a partitioned table's parent, a foreign table), one
//!   the catalog holds no pages for yet (`0` before its first `VACUUM` or `ANALYZE`), or a snapshot id
//!   the server returned in a form [`session::SnapshotId`] does not admit is ONE partition: the leg's
//!   own statement, read whole.
//! - The snapshot lives as long as [`SplitLeg`] does, and no longer than the deadline it was opened
//!   under; a partition read after either is refused by the server.
//! - Each partition aggregates its own range: a fact leg's key whose rows sit on pages of two ranges
//!   comes back once from each, with that range's partial terms. Combining them is the caller's and
//!   nothing here does it; the tier cells hold a lookup leg over unique keys, whose ranges combine
//!   by union.
//! - Nothing here reads partitions concurrently, and whether the server answers a range with a TID
//!   range scan is its planner's choice, not observed by any cell.
//!
//! [`AdbcPostgres::split_by_estimate`] decides whether to split at all over that same export:
//! `pg_class.relpages`/`reltuples` and the session's page size, turned into
//! [`sutura_domain::plan::SplitDecision`] by `sutura_domain::plan::decide`. **One export serves
//! both the decision and the cut it leads to** - a [`sutura_domain::plan::SplitDecision::Split`]
//! cuts [`SplitLeg`] from the very snapshot the estimate was read from, never a second one.
//! [`SplitLeg::schema`] describes the columns a split's partitions answer on that same exporting
//! connection, without reading a row, so the split can be granted to the nodes that read the source
//! rather than read here - `github.com/telekom/sutura#828`.
//!
//! [`Partition::descriptor`] is one range as bytes for another node: the snapshot id and the page
//! range, [`PartitionDescriptor`]'s layout. [`AdbcPostgres::read_partition`] reads them back against
//! a leg the reading side holds - the statement regenerated from that leg, the snapshot imported on
//! a fresh connection, under the same per-stream bound, through the one body [`Partition::read`]
//! runs. **Its limits**:
//!
//! - Only a reading worker's partition reader calls it, for a partition of a leg its planning node
//!   granted - `github.com/telekom/sutura#828`.
//! - It takes no presented identity and does not authenticate the descriptor: both are the task
//!   grant's, which carries the descriptor and authorises the leg before this runs. Every connection
//!   is the adapter's one conninfo, since its `IMPERSONATION` is `NoPlaceForASubject`. That the
//!   source was declared `shared-service-user` is held at startup by `sutura-cli`'s Postgres connect
//!   step, whose posture cross-check refuses an `impersonation-at-source` entry - `read_partition`
//!   reads no posture itself.
//! - A forged but well-formed descriptor names at most a page range of the leg the reading side
//!   passed - up to the whole leg, never other SQL, since the statement is generated from `leg`
//!   alone - read under any snapshot still exported in this database. The server checks only the
//!   database and that the exporter still runs, not its role, so that snapshot can be another
//!   session's, or an older one than the split's. The id is held to the hex digits and hyphens
//!   [`session::SnapshotId`] admits before it is sent.
//! - A snapshot whose exporting transaction ended - its [`SplitLeg`] dropped, its deadline spent,
//!   its connection gone - fails the import, and so the read.

use core::num::{NonZeroU64, NonZeroUsize};

use adbc_driver_manager::ManagedConnection;
use arrow_schema::SchemaRef;
use sutura_adbc::parameter_batch;
use sutura_domain::identity::Presented;
use sutura_domain::plan::{EstimatedRows, LegPlan, SplitDecision, SplitRefusal, decide};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{ParamValue, ResultBatches, RowSet, Value};
use sutura_sql::GeneratedQuery;
use sutura_sql::generate::generate_leg_pages;

use super::session::{self, SnapshotId};
use super::{AdbcError, AdbcPostgres, numeric};
use crate::PostgresError;
use crate::deadline::refuse_if_spent;

mod descriptor;

pub use descriptor::{InvalidDescriptor, PartitionDescriptor};

/// A leg cut into partitions, and the open transaction their snapshot belongs to.
pub struct SplitLeg<'adapter> {
    adapter: &'adapter AdbcPostgres,
    snapshot: Option<SnapshotId>,
    partitions: Vec<(Pages, GeneratedQuery)>,
    estimate: EstimatedBytes,
    /// The exporting transaction, open for as long as this connection is; [`SplitLeg::schema`] is
    /// its one further statement.
    exporting: ManagedConnection,
}

/// One range of a [`SplitLeg`].
pub struct Partition<'split> {
    split: &'split SplitLeg<'split>,
    pages: Pages,
    query: &'split GeneratedQuery,
}

/// [`AdbcPostgres::split_by_estimate`]'s outcome: [`decide`]'s verdict, carrying the one snapshot
/// the estimate was read from - over to [`SplitLeg`] for a [`SplitDecision::Split`], never a second
/// export of its own.
pub enum Planned<'adapter> {
    /// At or under the threshold, or no usable estimate: `leg`'s own statement, read whole exactly
    /// as [`Warehouse::execute`](sutura_domain::warehouse::Warehouse::execute) would. The exporting
    /// connection this read is over is dropped here - nothing past this point needs its snapshot.
    Whole,
    /// Over the threshold: `leg` cut under the very snapshot its estimate was read from.
    Split(SplitLeg<'adapter>),
    /// Over the threshold, but even the most partitions allowed still leave one over the bound.
    Refused(SplitRefusal),
}

/// `leg`'s root, read once over [`session::export`]'s round trip: the snapshot id (`None` for a
/// root that is not a plain table, one the catalog holds no pages for, or a snapshot id the server
/// returned in a form [`SnapshotId`] does not admit) and [`PAGES`](session)'s row - pages, rows and
/// the session's page size, each `0` where there was no row to trust. [`open`] and
/// [`split_by_estimate`] both read this the same one round trip their own way.
struct Exported {
    exporting: ManagedConnection,
    snapshot: Option<SnapshotId>,
    relpages: u32,
    reltuples: u64,
    block_size: u64,
}

fn export(adapter: &AdbcPostgres, leg: &LegPlan, deadline: Deadline) -> Result<Exported, PostgresError> {
    let relation = parameter_batch(&[ParamValue::Text(regclass(leg))]).map_err(AdbcError::Parameters)?;
    let mut exporting = adapter.connection()?;
    let exported = session::export(&mut exporting, relation, adapter.statement_timeout_ceiling_ms, deadline)?;
    let (snapshot, pages) = (numeric::rows(&exported.snapshot)?, numeric::rows(&exported.pages)?);
    let row = pages.rows().first();
    let snapshot = match (first(&snapshot), row.and_then(|row| row.first())) {
        (Some(Value::Text(id)), Some(&Value::Integer(1..))) => SnapshotId::parse(id),
        _ => None,
    };
    let relpages = match row.and_then(|row| row.first()) {
        Some(&Value::Integer(pages)) if snapshot.is_some() => u32::try_from(pages).unwrap_or(0),
        _ => 0,
    };
    let (reltuples, block_size) = if snapshot.is_some() {
        (
            u64::try_from(column(row, 1).max(0)).unwrap_or(0),
            u64::try_from(column(row, 2).max(0)).unwrap_or(0),
        )
    } else {
        (0, 0)
    };
    Ok(Exported {
        exporting,
        snapshot,
        relpages,
        reltuples,
        block_size,
    })
}

impl Exported {
    /// The root's pages at the server's page size.
    fn bytes(&self) -> EstimatedBytes {
        EstimatedBytes::parse(u64::from(self.relpages).saturating_mul(self.block_size))
    }
}

/// `row`'s column `index`, or `0` for a missing row, a short row, or anything but an integer -
/// [`PAGES`](session) selects three integers and nothing else ever reaches this.
fn column(row: Option<&Vec<Value>>, index: usize) -> i64 {
    match row.and_then(|row| row.get(index)) {
        Some(&Value::Integer(value)) => value,
        _ => 0,
    }
}

/// [`AdbcPostgres::split_leg`].
pub(super) fn open<'adapter>(
    adapter: &'adapter AdbcPostgres,
    leg: &LegPlan,
    partitions: NonZeroUsize,
    presented: &Presented,
    deadline: Deadline,
) -> Result<SplitLeg<'adapter>, PostgresError> {
    crate::deliverable(&adapter.source, &adapter.posture, presented)?;
    let whole = generate_leg_pages(leg, 0, None).map_err(|cause| PostgresError::Render { cause })?;
    refuse_if_spent(deadline)?;
    let exported = export(adapter, leg, deadline)?;
    cut_under(adapter, leg, whole, exported, partitions)
}

/// `leg` cut into `partitions` page ranges under `exported`'s snapshot - `whole`, the leg's own
/// statement, where the cut leaves one range.
fn cut_under<'adapter>(
    adapter: &'adapter AdbcPostgres,
    leg: &LegPlan,
    whole: GeneratedQuery,
    exported: Exported,
    partitions: NonZeroUsize,
) -> Result<SplitLeg<'adapter>, PostgresError> {
    let estimate = exported.bytes();
    let ranges = cut(exported.relpages, partitions);
    let partitions = match *ranges.as_slice() {
        [only] => vec![(only, whole)],
        _ => ranges
            .into_iter()
            .map(|(from, to)| generate_leg_pages(leg, from, to).map(|query| ((from, to), query)))
            .collect::<Result<_, _>>()
            .map_err(|cause| PostgresError::Render { cause })?,
    };
    Ok(SplitLeg {
        adapter,
        snapshot: exported.snapshot,
        partitions,
        estimate,
        exporting: exported.exporting,
    })
}

/// [`AdbcPostgres::split_by_estimate`]: [`decide`] over the one export [`open`] would otherwise cut
/// under its own, so a [`SplitDecision::Split`] here cuts [`SplitLeg`] from that very snapshot
/// rather than exporting a second one.
///
/// # Errors
///
/// What `Warehouse::execute` refuses before it sends a statement - a presented identity this
/// source does not serve, a spent deadline, a leg whose root does not quote - and the exporting
/// connection's driver or server failure.
pub(super) fn split_by_estimate<'adapter>(
    adapter: &'adapter AdbcPostgres,
    leg: &LegPlan,
    presented: &Presented,
    deadline: Deadline,
    above_bytes: NonZeroU64,
    per_stream_bound: NonZeroUsize,
    ready_workers: NonZeroUsize,
) -> Result<Planned<'adapter>, PostgresError> {
    crate::deliverable(&adapter.source, &adapter.posture, presented)?;
    let whole = generate_leg_pages(leg, 0, None).map_err(|cause| PostgresError::Render { cause })?;
    refuse_if_spent(deadline)?;
    let exported = export(adapter, leg, deadline)?;
    let bytes = exported.bytes();
    let rows = EstimatedRows::parse(exported.reltuples);
    match decide(bytes, rows, above_bytes, per_stream_bound, ready_workers) {
        SplitDecision::Whole => Ok(Planned::Whole),
        SplitDecision::Refused(refusal) => Ok(Planned::Refused(refusal)),
        SplitDecision::Split(partitions) => cut_under(adapter, leg, whole, exported, partitions).map(Planned::Split),
    }
}

impl SplitLeg<'_> {
    /// The columns every partition answers, described by the server on the exporting connection
    /// inside its open transaction and never run - or `None` where a `NUMERIC` column's type is
    /// decided by the cells each partition holds, so no one schema stands for all of them, and for a
    /// split holding no partition.
    ///
    /// **The limit:** the described schema is the driver's `execute_schema`, not a partition's own
    /// stream; `tests/partition_read.rs` compares the two for a plain `bigint` column only.
    ///
    /// # Errors
    ///
    /// A spent deadline, and the driver's or the server's failure - a server failure aborts the
    /// exporting transaction, so no partition imports its snapshot after it.
    pub fn schema(&mut self, deadline: Deadline) -> Result<Option<SchemaRef>, PostgresError> {
        refuse_if_spent(deadline)?;
        let Some((_, query)) = self.partitions.first() else {
            return Ok(None);
        };
        Ok(numeric::fixed(session::columns(&mut self.exporting, query.sql())?))
    }

    /// The catalog's estimate of the leg's root, in bytes: its pages at the server's page size.
    #[must_use]
    pub const fn estimate(&self) -> EstimatedBytes {
        self.estimate
    }

    /// Each range, in page order.
    pub fn partitions(&self) -> impl ExactSizeIterator<Item = Partition<'_>> {
        self.partitions.iter().map(move |&(pages, ref query)| Partition {
            split: self,
            pages,
            query,
        })
    }
}

impl Partition<'_> {
    /// This range's rows, on a connection of its own that imports the leg's snapshot first where it holds one.
    ///
    /// # Errors
    ///
    /// A spent deadline, a stream past the per-stream bound, and the driver's or the server's
    /// failure - among them a snapshot that is no longer importable.
    pub fn read(&self, deadline: Deadline) -> Result<ResultBatches, PostgresError> {
        read_range(self.split.adapter, self.split.snapshot.as_ref(), self.query, deadline)
    }

    /// This range as bytes [`AdbcPostgres::read_partition`] reads on another connection - `None`
    /// where the split holds no snapshot, the roots the module header reads whole: that one
    /// partition is a read of the table as it is now, which no other connection can repeat.
    pub fn descriptor(&self) -> Option<PartitionDescriptor> {
        self.split
            .snapshot
            .as_ref()
            .map(|snapshot| PartitionDescriptor::new(snapshot.clone(), self.pages))
    }
}

/// [`AdbcPostgres::read_partition`]: `descriptor` parsed before any connection opens, its range's
/// statement generated from `leg` alone, and read under its snapshot.
pub(super) fn read_partition(
    adapter: &AdbcPostgres,
    leg: &LegPlan,
    descriptor: &[u8],
    deadline: Deadline,
) -> Result<ResultBatches, PostgresError> {
    let parsed = PartitionDescriptor::try_from(descriptor).map_err(|cause| PostgresError::MalformedDescriptor { cause })?;
    let (from, to) = parsed.pages();
    let query = generate_leg_pages(leg, from, to).map_err(|cause| PostgresError::Render { cause })?;
    read_range(adapter, Some(parsed.snapshot()), &query, deadline)
}

/// `query` on a connection of its own, importing `snapshot` first where there is one - the one body
/// [`Partition::read`] and [`read_partition`] share, so both read under the same bound.
fn read_range(
    adapter: &AdbcPostgres,
    snapshot: Option<&SnapshotId>,
    query: &GeneratedQuery,
    deadline: Deadline,
) -> Result<ResultBatches, PostgresError> {
    refuse_if_spent(deadline)?;
    let bound = parameter_batch(query.params()).map_err(AdbcError::Parameters)?;
    let (sql, ceiling) = (query.sql(), adapter.statement_timeout_ceiling_ms);
    let batches = adapter.connected(|connection| match snapshot {
        Some(snapshot) => session::partition(connection, snapshot, sql, bound, ceiling, deadline),
        None => session::answer(connection, sql, bound, ceiling, deadline),
    })?;
    numeric::batches(batches).map(|rows| {
        rows.with_execution(sutura_domain::warehouse::execution::Execution::sql(
            adapter.source.clone(),
            sutura_domain::warehouse::schema::TypeDialect::Postgres,
            query.sql().to_owned(),
            query.params(),
        ))
    })
}

/// The leg's root table as `to_regclass` reads a name: each part double-quoted, as the statement
/// quotes it, so both resolve the same relation.
fn regclass(leg: &LegPlan) -> String {
    leg.table()
        .to_string()
        .split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn first(rows: &RowSet) -> Option<&Value> {
    rows.rows().first().and_then(|row| row.first())
}

/// `[from, to)` in heap pages, `to` absent for a range open to the end of the table.
type Pages = (u32, Option<u32>);

/// [`Pages`] ranges over `pages`, as many as `partitions` asks and never more than there are pages,
/// the last one open.
fn cut(pages: u32, partitions: NonZeroUsize) -> Vec<Pages> {
    let pages = u64::from(pages.max(1));
    let count = u64::try_from(partitions.get()).unwrap_or(u64::MAX).min(pages);
    let start = |index: u64| u32::try_from((pages * index).div_euclid(count)).unwrap_or(u32::MAX);
    (0..count)
        .map(|index| (start(index), (index + 1 < count).then(|| start(index + 1))))
        .collect()
}

#[cfg(test)]
mod tests;
