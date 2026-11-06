//! The one per-kind split dispatch: which linked kinds split a lookup leg across the nodes that read
//! their source, and how a reading worker reads a partition of each.
//!
//! A kind that splits implements [`SplitKind`](crate::serve::kind::split::SplitKind) and takes one
//! arm in [`AnyWarehouse::splits`], and a single-kind deployment of it one arm in
//! [`worker_read`](crate::serve::kind::split::worker_read), plus the `cfg` of the modules and
//! methods that name the kind (see the PR's limits). Every other kind is read whole by the planning
//! node and refused by a reading worker, naming the source. `PostgreSQL` is the one kind that splits.
//!
//! **Limit:** `BigQuery`'s split needs what [`SplitKind`](crate::serve::kind::split::SplitKind) does not carry - the caller's forwarded
//! token for a read, and the job `ends` and `labels` for a split - so it does not fit this seam as it
//! stands, and the trait grows before a second kind joins it.

use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::time::SystemTime;

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{Grantee, LegPlan, LookupSplit, Partitioned};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{ResultBatches, Warehouse};
use sutura_exec_datafusion::Unread;
use sutura_exec_postgres::PostgresError;
use sutura_exec_postgres::adbc::{MOST_RESULT_ROWS, Planned, SplitLeg};

use super::super::{OpenedSources, PostgresSource};
use super::grant::{self, Described, HeldSplit, Read};
use super::{AnyWarehouse, AnyWarehouseError};

/// A source kind whose lookup leg splits across the nodes that read its source.
///
/// Visible to [`kind`](super) alone, and [`read_granted`](SplitKind::read_granted) is banned by
/// `clippy.toml` beyond the one call in [`worker_read`].
pub(super) trait SplitKind {
    /// `leg` split by this kind's own estimate: granted to `grantee`, read here, read whole, or
    /// refused - the port's [`Warehouse::split_lookup`] for this kind.
    ///
    /// # Errors
    ///
    /// The source's failure to estimate, split or describe the leg.
    fn split(
        &self,
        leg: &LegPlan,
        presented: &Presented,
        deadline: Deadline,
        above_bytes: NonZeroU64,
        readers: NonZeroUsize,
        grantee: Option<Grantee>,
    ) -> LookupSplit<'_, AnyWarehouseError>;

    /// The granted `partition` of `leg`, which this node re-certified, read on this node.
    ///
    /// # Errors
    ///
    /// A partition past the source's per-stream bound, as [`Unread::DidNotFit`]; the source's other
    /// failures to read it, as [`Unread::Failed`].
    fn read_granted(&self, leg: &LegPlan, partition: &[u8], deadline: Deadline) -> Result<ResultBatches, Unread>;
}

impl AnyWarehouse {
    /// This adapter as a kind that splits, `None` for one read whole.
    pub(super) fn splits(&self) -> Option<&dyn SplitKind> {
        match self {
            Self::Postgres(postgres) => Some(&**postgres),
            _ => None,
        }
    }
}

/// `source`'s `partition` of the leg `wire` holds, read only once `pinned` re-certified the leg and
/// only for a kind that splits.
///
/// # Errors
///
/// [`Unread::LegNotCertified`] for a leg `pinned` does not certify, in a deployment that holds a kind
/// that splits; a source of a kind that does not split, named - without certifying anything when the
/// deployment holds no kind that splits; then the source's own failure to read.
pub(crate) fn worker_read(
    pinned: &PinnedDefinitions,
    sources: &OpenedSources,
    source: &SourceName,
    wire: &[u8],
    partition: &[u8],
    deadline: Deadline,
) -> Result<ResultBatches, Unread> {
    let not_split = || Unread::source_not_read(source);
    match sources {
        OpenedSources::Postgres(sources) => read_in(pinned, sources, source, wire, |postgres, leg| {
            read_here(postgres, leg, partition, deadline)
        }),
        OpenedSources::Mixed(mixed) => read_in(pinned, &mixed.engines, source, wire, |engine, leg| {
            engine
                .splits()
                .map_or_else(|| Err(not_split()), |kind| read_here(kind, leg, partition, deadline))
        }),
        _ => Err(not_split()),
    }
}

/// `kind`'s read of the granted `partition` of `leg`, the one call site of
/// [`read_granted`](SplitKind::read_granted).
#[expect(
    clippy::disallowed_methods,
    reason = "only worker_read calls this, and only from inside read_recertified: the admit rule verified the task's reading grant, and the leg was re-certified against this node's catalog first"
)]
fn read_here(kind: &dyn SplitKind, leg: &LegPlan, partition: &[u8], deadline: Deadline) -> Result<ResultBatches, Unread> {
    kind.read_granted(leg, partition, deadline)
}

/// [`sutura_app::read_recertified`] over `opened`, a refusal answered as an uncertified leg.
fn read_in<W>(
    pinned: &PinnedDefinitions,
    opened: &sutura_app::Warehouses<W>,
    source: &SourceName,
    wire: &[u8],
    read: impl FnOnce(&W, &LegPlan) -> Result<ResultBatches, Unread>,
) -> Result<ResultBatches, Unread>
where
    W: Warehouse,
{
    sutura_app::read_recertified(pinned, opened, source, wire, |engine, leg| read(engine, &leg))
        .unwrap_or_else(|_refused| Err(Unread::LegNotCertified))
}

impl SplitKind for PostgresSource {
    /// Through `split_by_estimate`. A split of two or more partitions offered a `grantee` is granted
    /// to the nodes that read the source ([`grant::granted_or_read`]): its columns described on the
    /// exporting connection, each partition's descriptor, and the [`SplitLeg`] itself held by the
    /// grant, so the exporting transaction stays open as long as the grant does.
    /// [`Grantee::grant`]'s refusal is the answer, and no partition of a granted or refused leg is
    /// read here.
    ///
    /// Without a `grantee`, with one partition, or with a column whose type its cells decide
    /// (`NUMERIC`), the reader owns the split and reads by position until it holds no further
    /// partition, so each is read once, in page order, and the exporting transaction stays open until
    /// the reader is dropped.
    ///
    /// **Limits:** a leg answered [`Partitioned::Whole`] was estimated first, one round trip more
    /// than reading it whole outright; a granted one is described too, one more. And a link value two
    /// partitions both carry is one table's repeated tuple to the combine, refused as an ambiguous
    /// link - so a leg the whole read answered can be refused once split, never answered differently.
    fn split(
        &self,
        leg: &LegPlan,
        presented: &Presented,
        deadline: Deadline,
        above_bytes: NonZeroU64,
        readers: NonZeroUsize,
        grantee: Option<Grantee>,
    ) -> LookupSplit<'_, AnyWarehouseError> {
        let stream_rows = NonZeroUsize::new(MOST_RESULT_ROWS).unwrap_or(NonZeroUsize::MIN);
        let split = match self.split_by_estimate(leg, presented, deadline, above_bytes, stream_rows, readers) {
            Ok(Planned::Whole) => return Ok(Partitioned::Whole),
            Ok(Planned::Refused(refusal)) => return Ok(Partitioned::Refused(refusal)),
            Ok(Planned::Split(split)) => split,
            Err(cause) => return Err(AnyWarehouseError::Postgres(cause)),
        };
        let rows = NonZeroU32::try_from(stream_rows).unwrap_or(NonZeroU32::MAX);
        grant::granted_or_read(split, grantee, deadline, rows, SystemTime::now)
    }

    /// Runs as this node's own declared service user for the source,
    /// `AdbcPostgres::read_partition`'s own limit; nothing checks it against the planning node's.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one production caller: the admit rule verified the task's reading grant, and the leg was re-certified against this node's catalog first"
    )]
    fn read_granted(&self, leg: &LegPlan, partition: &[u8], deadline: Deadline) -> Result<ResultBatches, Unread> {
        self.read_partition(leg, partition, deadline)
            .map_err(|cause| unread(leg.source(), cause))
    }
}

/// `cause` as this reader answers it: a partition past the per-stream bound as
/// [`Unread::DidNotFit`], which the planning node refuses as a result that did not fit, and anything
/// else as the engine's failure. Logged here, naming the source and never the cause's text, which
/// can quote what the server read.
fn unread(source: &SourceName, cause: PostgresError) -> Unread {
    if cause.did_not_fit() {
        tracing::warn!(%source, "a granted partition did not fit this reader's per-stream bound, refused");
        Unread::DidNotFit
    } else {
        tracing::warn!(%source, "a granted partition could not be read");
        Unread::failed(cause)
    }
}

/// A split with a column whose type its cells decide (`NUMERIC`) describes no one schema, so it is
/// read here rather than granted.
impl HeldSplit for SplitLeg<'_> {
    type Error = AnyWarehouseError;

    fn descriptors(&self) -> Vec<Option<Vec<u8>>> {
        self.partitions()
            .map(|partition| partition.descriptor().map(|descriptor| descriptor.to_bytes()))
            .collect()
    }

    fn columns(&mut self, deadline: Deadline) -> Described<Self::Error> {
        self.schema(deadline).map_err(AnyWarehouseError::Postgres)
    }

    fn estimate(&self) -> EstimatedBytes {
        SplitLeg::estimate(self)
    }

    fn read(&self, index: usize, deadline: Deadline) -> Read<Self::Error> {
        self.partitions()
            .nth(index)
            .map(|partition| partition.read(deadline).map_err(AnyWarehouseError::Postgres))
    }
}

/// A reader's failure answered by its kind: a partition past the bound, typed.
#[cfg(test)]
mod tests;
