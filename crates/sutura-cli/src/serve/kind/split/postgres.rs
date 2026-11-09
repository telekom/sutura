//! The `PostgreSQL` arm of [`SplitKind`]: a lookup leg split by its estimate into partitions of
//! one exported snapshot, granted to the nodes that read the source or read here, each read as this
//! node's own declared service user.

use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::time::SystemTime;

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{Grantee, LegPlan, LookupSplit, Partitioned};
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_exec_datafusion::{SchemaRef, Unread};
use sutura_exec_postgres::PostgresError;
use sutura_exec_postgres::adbc::{MOST_RESULT_ROWS, Planned, SplitLeg};

use super::super::super::PostgresSource;
use super::super::AnyWarehouseError;
use super::super::grant::{self, Described, HeldSplit, Read};
use super::{PartitionReads, SplitKind};

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
        _labels: &JobLabels,
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

    /// Runs as this node's own declared service user for the source, whatever `_reads` names, and
    /// reads the partition's own columns rather than `_granted`: `AdbcPostgres::read_partition`'s
    /// own limit; nothing checks either against the planning node's.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one production caller: the admit rule verified the task's reading grant, and the leg was re-certified against this node's catalog first"
    )]
    fn read_granted(
        &self,
        leg: &LegPlan,
        partition: &[u8],
        _granted: &SchemaRef,
        _reads: &PartitionReads,
        deadline: Deadline,
    ) -> Result<ResultBatches, Unread> {
        self.read_partition(leg, partition, deadline)
            .map_err(|cause| unread(leg.source(), cause))
    }
}

/// `cause` as this reader answers it: a partition past the per-stream bound as
/// [`Unread::DidNotFit`], which the planning node refuses as a result that did not fit, and anything
/// else as the engine's failure. Logged here, naming the source and never the cause's text, which
/// can quote what the server read.
pub(in super::super) fn unread(source: &SourceName, cause: PostgresError) -> Unread {
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
