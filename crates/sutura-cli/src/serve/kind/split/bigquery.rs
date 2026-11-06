//! The `BigQuery` arm of [`SplitKind`], not offered yet: [`AnyWarehouse::splits`](super::super::AnyWarehouse::splits)
//! answers no `BigQuery` source, so every `BigQuery` leg is still read whole and a reading worker
//! reads none of its partitions.

use core::num::{NonZeroU64, NonZeroUsize};

use sutura_domain::identity::Presented;
use sutura_domain::plan::{Grantee, LegPlan, LookupSplit, Partitioned};
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_exec_datafusion::Unread;

use super::super::super::BigQuerySource;
use super::super::AnyWarehouseError;
use super::SplitKind;

impl SplitKind for BigQuerySource {
    /// Read whole.
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

    /// Refused, as a source a reading worker does not read.
    fn read_granted(&self, leg: &LegPlan, _partition: &[u8], _deadline: Deadline) -> Result<ResultBatches, Unread> {
        Err(Unread::source_not_read(leg.source()))
    }
}
