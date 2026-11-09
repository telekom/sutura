//! The Oracle arm of [`SplitKind`]: a lookup leg split by the dictionary's estimate into `ORA_HASH`
//! buckets at one snapshot, granted to the nodes that read the source or read here, each read as
//! this node's own declared service user (D1 of the workers-read-sources record's fourth amendment).
//!
//! Only a source whose boot probe passed (`split: flashback`) splits, and an
//! `impersonation-at-source` source reads every leg whole: `OracleWarehouse::split_by_estimate`
//! answers `Planned::Whole` for both.
//!
//! **Limits:** nothing is held open across a split - the snapshot is in each partition's
//! statement - so a grant keeps a copy of the leg to read a partition here. A split is granted only
//! where the database declares every column fixed (`OracleWarehouse::describe_split`); any other is
//! read here, one bucket after another.

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
use sutura_exec_oracle::{OracleError, Planned, SplitLeg};

use super::super::super::OracleSource;
use super::super::AnyWarehouseError;
use super::super::grant::{self, Described, HeldSplit, Read};
use super::{PartitionReads, SplitKind};

/// The row target the estimate divides the leg by, signed into each grant. Neither the reader nor
/// the combine enforces it per stream; a bucket past it can still be read if byte budgets permit.
/// The reader bounds bytes by its own result budget.
const STREAM_ROWS: usize = 1_000_000;

impl SplitKind for OracleSource {
    /// Through `split_by_estimate`. A split of two or more buckets offered a `grantee`, whose
    /// columns the database declares fixed, is granted to the nodes that read the source
    /// ([`grant::granted_or_read`]); [`Grantee::grant`]'s refusal is the answer. Otherwise the
    /// buckets are read here, in order.
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
        let stream_rows = NonZeroUsize::new(STREAM_ROWS).unwrap_or(NonZeroUsize::MIN);
        let split = match self.split_by_estimate(leg, presented, deadline, above_bytes, stream_rows, readers) {
            Ok(Planned::Whole) => return Ok(Partitioned::Whole),
            Ok(Planned::Refused(refusal)) => return Ok(Partitioned::Refused(refusal)),
            Ok(Planned::Split(split)) => split,
            Err(cause) => return Err(AnyWarehouseError::Oracle(cause)),
        };
        let rows = NonZeroU32::try_from(stream_rows).unwrap_or(NonZeroU32::MAX);
        let held = Held {
            warehouse: self,
            leg: leg.clone(),
            split,
        };
        grant::granted_or_read(held, grantee, deadline, rows, SystemTime::now)
    }

    /// Runs as this node's own declared service user for the source, whatever `_reads` names, and
    /// answers the partition's declared columns rather than `_granted`, as the planning node's
    /// describe read them. Nothing here compares the reader's schema with the planning node's.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one reading-worker caller: the admit rule verified the task's reading grant, and the leg was re-certified against this node's catalog first"
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

/// `cause` as this reader answers it: a partition past the reader's result budget as
/// [`Unread::DidNotFit`], which the planning node refuses as a result that did not fit, and anything
/// else - a stale or gone snapshot among them - as the engine's failure. Logged here, naming the
/// source and never the cause's text, which can quote what the server read.
pub(in super::super) fn unread(source: &SourceName, cause: OracleError) -> Unread {
    if matches!(cause, OracleError::OverBudget { .. }) {
        tracing::warn!(%source, "a granted partition did not fit this reader's result budget, refused");
        Unread::DidNotFit
    } else {
        tracing::warn!(%source, "a granted partition could not be read");
        Unread::failed(cause)
    }
}

/// An Oracle split as the grant arm holds it, with the leg it renders each bucket from.
struct Held<'w> {
    warehouse: &'w OracleSource,
    leg: LegPlan,
    split: SplitLeg,
}

impl HeldSplit for Held<'_> {
    type Error = AnyWarehouseError;

    fn descriptors(&self) -> Vec<Option<Vec<u8>>> {
        self.split
            .partitions()
            .iter()
            .map(|partition| Some(partition.to_bytes()))
            .collect()
    }

    fn columns(&mut self, deadline: Deadline) -> Described<Self::Error> {
        self.warehouse
            .describe_split(&self.leg, &self.split, deadline)
            .map_err(AnyWarehouseError::Oracle)
    }

    fn estimate(&self) -> EstimatedBytes {
        self.split.estimate()
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a bucket read on the planning node that split this question's leg, under the question's own authorisation; nothing was granted"
    )]
    fn read(&self, index: usize, deadline: Deadline) -> Read<Self::Error> {
        self.split.partitions().get(index).map(|partition| {
            self.warehouse
                .read_partition(&self.leg, &partition.to_bytes(), deadline)
                .map_err(AnyWarehouseError::Oracle)
        })
    }
}

#[cfg(test)]
mod tests;
