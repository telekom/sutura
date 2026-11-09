//! The Oracle arm of [`SplitKind`]: a lookup leg split by the dictionary's estimate into `ORA_HASH`
//! buckets at one snapshot, granted to the nodes that read the source or read here, each read as
//! this node's own declared service user (D1 of the workers-read-sources record's fourth amendment).
//!
//! Not built yet: no leg splits, and every granted partition is refused naming the source, as for
//! a kind that does not split. Nothing offers an Oracle leg to this arm: [`AnyWarehouse::splits`]
//! answers `None` for Oracle.
//!
//! [`AnyWarehouse::splits`]: super::super::AnyWarehouse::splits

use core::num::{NonZeroU64, NonZeroUsize};

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{Grantee, LegPlan, LookupSplit, Partitioned};
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_exec_datafusion::{SchemaRef, Unread};
use sutura_exec_oracle::OracleError;

use super::super::super::OracleSource;
use super::super::AnyWarehouseError;
use super::{PartitionReads, SplitKind};

impl SplitKind for OracleSource {
    /// Not built yet: every leg is read whole.
    fn split(
        &self,
        _leg: &LegPlan,
        _presented: &Presented,
        _deadline: Deadline,
        _above_bytes: NonZeroU64,
        _readers: NonZeroUsize,
        _grantee: Option<Grantee>,
        _labels: &JobLabels,
    ) -> LookupSplit<'_, AnyWarehouseError> {
        Ok(Partitioned::Whole)
    }

    /// Not built yet: every partition is refused naming the source.
    fn read_granted(
        &self,
        leg: &LegPlan,
        _partition: &[u8],
        _granted: &SchemaRef,
        _reads: &PartitionReads,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Unread> {
        Err(Unread::source_not_read(leg.source()))
    }
}

/// `cause` as this reader answers it. Not built yet: every cause is the engine's failure.
#[expect(dead_code, reason = "the API of the Oracle reader; the next change reads through it")]
pub(in super::super) fn unread(_source: &SourceName, cause: OracleError) -> Unread {
    Unread::failed(cause)
}

#[cfg(test)]
mod tests;
