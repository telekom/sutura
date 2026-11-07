//! The `ClickHouse` bucket split's public API, parent to the behaviour PR: a leg whose source
//! opts in to `hash_buckets` is estimated, described and cut into hash buckets, each read as the
//! identity a whole read of the leg already runs as - the deployment's service user for a
//! `shared-service-user` source, or the caller's switched user for an `impersonation-at-source`
//! one without a `delegation`.
//!
//! **This PR carries the API and no behaviour.** [`AdbcClickHouseWarehouse::split_by_estimate`]
//! always answers [`Planned::Whole`] and [`AdbcClickHouseWarehouse::read_partition`] refuses
//! [`AdbcError::BucketsUnbuilt`]; the signature and types here are what the behaviour PR fills in.
//!
//! **Limits, measured for the behaviour PR**: a partition is consistent per key only, there is no
//! snapshot across buckets, and the estimate reads the root table's parts alone.

use core::num::{NonZeroU64, NonZeroUsize};

use arrow_schema::SchemaRef;
use sutura_domain::identity::{Presented, PrincipalName};
use sutura_domain::plan::{LegPlan, SplitRefusal};
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_sql::GeneratedQuery;
use sutura_sql::generate::Bucket;

use super::{AdbcClickHouseWarehouse, AdbcError};

mod descriptor;

pub use descriptor::{BucketDescriptor, InvalidBucketDescriptor};

/// Whom a granted partition of a `ClickHouse` leg is read as, decided at the planner and held by
/// the partition: the same identity a whole read of the leg runs as - see the module doc.
pub enum PartitionIdentity {
    /// A `shared-service-user` source: read as the deployment's declared user.
    ServiceUser,
    /// An `impersonation-at-source` source without a `delegation`: read as the caller's switched
    /// principal.
    Switched(PrincipalName),
}

impl core::fmt::Debug for PartitionIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ServiceUser => f.write_str("ServiceUser"),
            Self::Switched(_) => f.write_str("Switched(..)"),
        }
    }
}

/// [`AdbcClickHouseWarehouse::split_by_estimate`]'s outcome: [`Planned::Whole`] for a leg read
/// whole, [`Planned::Split`] for one cut into hash buckets, or its refusal.
pub enum Planned<'adapter> {
    /// At or under the threshold, or no usable estimate: the leg's own statement, read whole.
    Whole,
    /// Over the threshold: the leg cut into hash buckets.
    Split(BucketSplit<'adapter>),
    /// Over the threshold, but even the most partitions allowed still leave one over the bound.
    Refused(SplitRefusal),
}

/// A leg cut into hash buckets, each read as [`BucketSplit::reads`]. Its fields are private so a
/// value can only be built through [`BucketSplit::new`], which a caller must not call with a split
/// its source did not declare.
pub struct BucketSplit<'adapter> {
    adapter: &'adapter AdbcClickHouseWarehouse,
    described: SchemaRef,
    queries: Vec<(Bucket, GeneratedQuery)>,
    estimate: EstimatedBytes,
    reads: PartitionIdentity,
}

impl<'adapter> BucketSplit<'adapter> {
    /// A split of `adapter`'s leg: the columns it answers, the statement per bucket, the estimate
    /// the decision came from, and the identity each partition is read as.
    #[must_use]
    pub const fn new(
        adapter: &'adapter AdbcClickHouseWarehouse,
        described: SchemaRef,
        queries: Vec<(Bucket, GeneratedQuery)>,
        estimate: EstimatedBytes,
        reads: PartitionIdentity,
    ) -> Self {
        Self {
            adapter,
            described,
            queries,
            estimate,
            reads,
        }
    }

    /// The adapter whose source the leg is read from.
    #[must_use]
    pub const fn adapter(&self) -> &'adapter AdbcClickHouseWarehouse {
        self.adapter
    }

    /// The columns a split's partitions answer, described without reading a row.
    #[must_use]
    pub const fn described(&self) -> &SchemaRef {
        &self.described
    }

    /// One generated statement per bucket, each restricted to its bucket.
    #[must_use]
    pub fn queries(&self) -> &[(Bucket, GeneratedQuery)] {
        &self.queries
    }

    /// The estimate the split was decided from.
    #[must_use]
    pub const fn estimate(&self) -> &EstimatedBytes {
        &self.estimate
    }

    /// The identity every partition of this split is read as.
    #[must_use]
    pub const fn reads(&self) -> &PartitionIdentity {
        &self.reads
    }
}

/// [`AdbcClickHouseWarehouse::split_by_estimate`]: stubbed to the whole read.
///
/// # Errors
///
/// Nothing in the parent: this always answers [`Planned::Whole`].
#[expect(clippy::unnecessary_wraps, reason = "the parent never errors; the behaviour PR does")]
pub(super) const fn split_by_estimate<'adapter>(
    _adapter: &'adapter AdbcClickHouseWarehouse,
    _leg: &LegPlan,
    _presented: &Presented,
    _deadline: Deadline,
    _above_bytes: NonZeroU64,
    _per_stream_bound: NonZeroUsize,
    _ready_workers: NonZeroUsize,
) -> Result<Planned<'adapter>, AdbcError> {
    Ok(Planned::Whole)
}

/// [`AdbcClickHouseWarehouse::read_partition`]: stubbed to a refusal.
///
/// # Errors
///
/// [`AdbcError::BucketsUnbuilt`] always, in the parent.
pub(super) fn read_partition(
    _adapter: &AdbcClickHouseWarehouse,
    _leg: &LegPlan,
    _descriptor: &[u8],
    _granted: &SchemaRef,
    _identity: PartitionIdentity,
    _deadline: Deadline,
) -> Result<ResultBatches, AdbcError> {
    Err(AdbcError::BucketsUnbuilt)
}
