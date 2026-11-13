//! Opted-in `ClickHouse` buckets, granted as the same identity the whole leg runs as.
//! Buckets have per-key consistency, without a shared snapshot across reads.

use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use std::time::SystemTime;

use sutura_domain::identity::Presented;
use sutura_domain::plan::{Grantee, LegPlan, LookupSplit, Partitioned};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::{ResultBatches, Warehouse as _};
use sutura_exec_clickhouse::adbc::{BucketDescriptor, BucketSplit, MOST_RESULT_ROWS, PartitionIdentity, Planned};
use sutura_exec_datafusion::{SchemaRef, Unread};

use super::super::AnyWarehouseError;
use super::super::grant::{self, Described, HeldSplit, Read};
use super::{PartitionReads, SplitKind};
use crate::serve::ClickHouseSource;

impl SplitKind for ClickHouseSource {
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
            Err(cause) => return Err(AnyWarehouseError::ClickHouse(cause)),
        };
        let grantee = match (grantee, split.reads()) {
            (Some(grantee), PartitionIdentity::Switched(name)) => Some(
                grantee
                    .switching(name.clone())
                    .map_err(AnyWarehouseError::NotTheCallersPrincipal)?,
            ),
            (grantee, PartitionIdentity::ServiceUser) => grantee,
            (None, PartitionIdentity::Switched(_)) => None,
        };
        let held = Held { split, leg: leg.clone() };
        grant::granted_or_read(
            held,
            grantee,
            deadline,
            NonZeroU32::try_from(stream_rows).unwrap_or(NonZeroU32::MAX),
            SystemTime::now,
        )
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the admitted reading task grants this partition and identity; the worker re-certified the leg before this call"
    )]
    fn read_granted(
        &self,
        leg: &LegPlan,
        partition: &[u8],
        granted: &SchemaRef,
        reads: &PartitionReads,
        deadline: Deadline,
    ) -> Result<ResultBatches, Unread> {
        let identity = match reads {
            PartitionReads::ServiceUser => PartitionIdentity::ServiceUser,
            PartitionReads::SwitchedPrincipal(name) => PartitionIdentity::Switched(name.clone()),
        };
        self.read_partition(leg, partition, granted, identity, deadline)
            .map_err(|cause| {
                if self.result_did_not_fit(&cause) {
                    Unread::DidNotFit
                } else {
                    Unread::failed(cause)
                }
            })
    }
}

struct Held<'a> {
    split: BucketSplit<'a>,
    leg: LegPlan,
}

impl HeldSplit for Held<'_> {
    type Error = AnyWarehouseError;

    fn descriptors(&self) -> Vec<Option<Vec<u8>>> {
        self.split
            .queries()
            .iter()
            .map(|(bucket, _)| Some(BucketDescriptor::from(*bucket).to_bytes()))
            .collect()
    }

    fn columns(&mut self, _deadline: Deadline) -> Described<Self::Error> {
        Ok(Some(Arc::clone(self.split.described())))
    }

    fn estimate(&self) -> EstimatedBytes {
        *self.split.estimate()
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the planning node reads its own split under the question's presented identity; no remote grant is used"
    )]
    fn read(&self, index: usize, deadline: Deadline) -> Read<Self::Error> {
        let (bucket, _) = self.split.queries().get(index)?;
        let identity = match self.split.reads() {
            PartitionIdentity::ServiceUser => PartitionIdentity::ServiceUser,
            PartitionIdentity::Switched(name) => PartitionIdentity::Switched(name.clone()),
        };
        Some(
            self.split
                .adapter()
                .read_partition(
                    &self.leg,
                    &BucketDescriptor::from(*bucket).to_bytes(),
                    self.split.described(),
                    identity,
                    deadline,
                )
                .map_err(AnyWarehouseError::ClickHouse),
        )
    }
}
