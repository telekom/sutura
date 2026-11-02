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

use core::num::NonZeroUsize;

use adbc_driver_manager::ManagedConnection;
use sutura_adbc::parameter_batch;
use sutura_domain::identity::Presented;
use sutura_domain::plan::LegPlan;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{ParamValue, ResultBatches, RowSet, Value};
use sutura_sql::GeneratedQuery;
use sutura_sql::generate::generate_leg_pages;

use super::session::{self, SnapshotId};
use super::{AdbcError, AdbcPostgres, numeric};
use crate::PostgresError;
use crate::deadline::refuse_if_spent;

/// A leg cut into partitions, and the open transaction their snapshot belongs to.
pub struct SplitLeg<'adapter> {
    adapter: &'adapter AdbcPostgres,
    snapshot: Option<SnapshotId>,
    partitions: Vec<GeneratedQuery>,
    /// Never read: the exporting transaction is open for as long as this connection is.
    _exporting: ManagedConnection,
}

/// One range of a [`SplitLeg`].
pub struct Partition<'split> {
    split: &'split SplitLeg<'split>,
    query: &'split GeneratedQuery,
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
    let relation = parameter_batch(&[ParamValue::Text(regclass(leg))]).map_err(AdbcError::Parameters)?;
    let mut exporting = adapter.connection()?;
    let exported = session::export(&mut exporting, relation, adapter.statement_timeout_ceiling_ms, deadline)?;
    let (snapshot, pages) = (numeric::rows(&exported.snapshot)?, numeric::rows(&exported.pages)?);
    let snapshot = match first(&snapshot) {
        Some(Value::Text(id)) => SnapshotId::parse(id),
        _ => None,
    };
    let pages = match first(&pages) {
        Some(&Value::Integer(pages)) if snapshot.is_some() => u32::try_from(pages).unwrap_or(0),
        _ => 0,
    };
    let ranges = cut(pages, partitions);
    let partitions = if ranges.len() == 1 {
        vec![whole]
    } else {
        ranges
            .into_iter()
            .map(|(from, to)| generate_leg_pages(leg, from, to))
            .collect::<Result<_, _>>()
            .map_err(|cause| PostgresError::Render { cause })?
    };
    Ok(SplitLeg {
        adapter,
        snapshot,
        partitions,
        _exporting: exporting,
    })
}

impl SplitLeg<'_> {
    /// Each range, in page order.
    pub fn partitions(&self) -> impl ExactSizeIterator<Item = Partition<'_>> {
        self.partitions.iter().map(move |query| Partition { split: self, query })
    }
}

impl Partition<'_> {
    /// This range's rows, on a connection of its own that imports the leg's snapshot first.
    ///
    /// # Errors
    ///
    /// A spent deadline, a stream past the per-stream bound, and the driver's or the server's
    /// failure - among them a snapshot that is no longer importable.
    pub fn read(&self, deadline: Deadline) -> Result<ResultBatches, PostgresError> {
        refuse_if_spent(deadline)?;
        let adapter = self.split.adapter;
        let bound = parameter_batch(self.query.params()).map_err(AdbcError::Parameters)?;
        let (sql, ceiling) = (self.query.sql(), adapter.statement_timeout_ceiling_ms);
        let batches = adapter.connected(|connection| match self.split.snapshot {
            Some(ref snapshot) => session::partition(connection, snapshot, sql, bound, ceiling, deadline),
            None => session::answer(connection, sql, bound, ceiling, deadline),
        })?;
        numeric::batches(batches)
    }
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
