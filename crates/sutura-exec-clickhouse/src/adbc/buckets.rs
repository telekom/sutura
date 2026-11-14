//! Opted-in hash buckets of a lookup leg, read under its whole-read identity.
//!
//! Each bucket is consistent per integer key. There is no snapshot across buckets, and the
//! estimate reads the root table's active parts alone; joined tables and row policies can make
//! that estimate conservative. Every partition retains the adapter's row and byte bounds.
//! The estimate uses the leg's identity, which needs
//! `SELECT(rows, bytes_on_disk, active, database, table) ON system.parts`. This column grant does
//! not restrict metadata access to the root table. A source denial is returned without retrying
//! as the deployment user.

use core::num::{NonZeroU64, NonZeroUsize};

use arrow_schema::SchemaRef;
use sutura_domain::identity::{Presented, PrincipalName};
use sutura_domain::plan::{EstimatedRows, LegPlan, SplitDecision, SplitRefusal, decide};
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{ResultBatches, Value};
use sutura_sql::Dialect;
use sutura_sql::GeneratedQuery;
use sutura_sql::generate::{Bucket, BucketsRefused, generate_leg, generate_leg_buckets};

use super::{AdbcClickHouseWarehouse, AdbcError, MOST_RESULT_ROWS, RunsAs};

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

/// A leg cut into hash buckets, each read as [`BucketSplit::reads`].
///
/// Its fields are private so a value can only be built through [`BucketSplit::new`], which a caller must not call with a split
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

/// [`AdbcClickHouseWarehouse::split_by_estimate`].
pub(super) fn split_by_estimate<'adapter>(
    adapter: &'adapter AdbcClickHouseWarehouse,
    leg: &LegPlan,
    presented: &Presented,
    deadline: Deadline,
    above_bytes: NonZeroU64,
    per_stream_bound: NonZeroUsize,
    ready_workers: NonZeroUsize,
) -> Result<Planned<'adapter>, AdbcError> {
    let runs_as = adapter.deliverable(presented)?;
    if !adapter.hash_buckets || !matches!(leg, LegPlan::Lookup { .. }) || matches!(runs_as, RunsAs::SignedIn { .. }) {
        return Ok(Planned::Whole);
    }
    // The generator rejects a project-qualified table before the metadata query can read a
    // different database. Names below are parsed ASCII identifiers, never SQL fragments.
    let query = generate_leg(leg, Dialect::ClickHouse).map_err(AdbcError::Render)?;
    let database = leg.table().qualifier().map_or_else(
        || String::from("currentDatabase()"),
        |qualifier| format!("'{}'", qualifier.dataset().as_str()),
    );
    let estimate = format!(
        "SELECT sum(rows) AS rows, sum(bytes_on_disk) AS bytes FROM system.parts \
         WHERE active AND database = {database} AND table = '{}'",
        leg.table().name().as_str(),
    );
    let estimated = adapter.run_sql(&estimate, &[], deadline, 1, &runs_as)?;
    let rows = estimated.to_rows().map_err(AdbcError::Decode)?;
    let [counts] = rows.rows() else {
        return Err(AdbcError::EstimateNotCounts);
    };
    let [Value::Integer(rows), Value::Integer(bytes)] = counts.as_slice() else {
        return Err(AdbcError::EstimateNotCounts);
    };
    let rows = EstimatedRows::parse(u64::try_from(*rows).map_err(|_negative| AdbcError::EstimateNotCounts)?);
    let bytes = EstimatedBytes::parse(u64::try_from(*bytes).map_err(|_negative| AdbcError::EstimateNotCounts)?);
    if bytes.bytes() <= above_bytes.get() {
        return Ok(Planned::Whole);
    }
    let described = describe(adapter, &query, deadline, &runs_as)?;
    let first = Bucket::new(0, 2).map_err(|cause| AdbcError::BucketDescriptor(InvalidBucketDescriptor::Bucket(cause)))?;
    match generate_leg_buckets(leg, &described, first, Dialect::ClickHouse) {
        Ok(_) => {}
        Err(BucketsRefused::NoIntegerKey) => return Ok(Planned::Whole),
        Err(cause) => return Err(AdbcError::BucketRender(cause)),
    }
    let partitions = match decide(bytes, rows, above_bytes, per_stream_bound, ready_workers) {
        SplitDecision::Whole => return Ok(Planned::Whole),
        SplitDecision::Refused(refusal) => return Ok(Planned::Refused(refusal)),
        SplitDecision::Split(partitions) if partitions.get() < 2 => return Ok(Planned::Whole),
        SplitDecision::Split(partitions) => u32::try_from(partitions.get()).unwrap_or(u32::MAX),
    };
    let mut queries = Vec::with_capacity(usize::try_from(partitions).unwrap_or(usize::MAX));
    for index in 0..partitions {
        let bucket = Bucket::new(index, partitions)
            .map_err(|cause| AdbcError::BucketDescriptor(InvalidBucketDescriptor::Bucket(cause)))?;
        match generate_leg_buckets(leg, &described, bucket, Dialect::ClickHouse) {
            Ok(query) => queries.push((bucket, query)),
            Err(BucketsRefused::NoIntegerKey) => return Ok(Planned::Whole),
            Err(cause) => return Err(AdbcError::BucketRender(cause)),
        }
    }
    let reads = match presented {
        Presented::SubjectPrincipal { name } => PartitionIdentity::Switched(name.clone()),
        Presented::SharedServiceUser { .. } => PartitionIdentity::ServiceUser,
        Presented::SubjectToken { .. } | Presented::SubjectSignIn { .. } => return Ok(Planned::Whole),
    };
    Ok(Planned::Split(BucketSplit::new(adapter, described, queries, bytes, reads)))
}

/// Describes the leg's projection without fetching its rows. A lookup's key types are fixed by
/// the source; no sum tuple needs the value-dependent normalization used by a fact leg.
fn describe(
    adapter: &AdbcClickHouseWarehouse,
    query: &GeneratedQuery,
    deadline: Deadline,
    runs_as: &RunsAs<'_>,
) -> Result<SchemaRef, AdbcError> {
    let sql = format!("SELECT * FROM ({}) LIMIT 0", query.sql());
    adapter
        .run_sql(&sql, query.params(), deadline, 0, runs_as)
        .map(|rows| std::sync::Arc::clone(rows.schema()))
}

/// [`AdbcClickHouseWarehouse::read_partition`]. The source's own schema must still agree with the
/// grant before it supplies the types used to hash keys. The result is held until its complete
/// row/byte and schema checks pass; these checks do not bound a driver's prefetched allocation.
pub(super) fn read_partition(
    adapter: &AdbcClickHouseWarehouse,
    leg: &LegPlan,
    descriptor: &[u8],
    granted: &SchemaRef,
    identity: PartitionIdentity,
    deadline: Deadline,
) -> Result<ResultBatches, AdbcError> {
    if !adapter.hash_buckets {
        return Err(AdbcError::HashBucketsNotDeclared);
    }
    let runs_as = match (&adapter.posture, identity) {
        (SourcePosture::SharedServiceUser { .. }, PartitionIdentity::ServiceUser) => RunsAs::Deployment,
        (SourcePosture::ImpersonationAtSource, PartitionIdentity::Switched(name)) if name.as_str() != adapter.username => {
            RunsAs::Switched(crate::execute_as::ClickHouseUser::parse(name.as_str()).map_err(AdbcError::NotAClickHouseUser)?)
        }
        _ => return Err(AdbcError::PartitionIdentityDisagrees),
    };
    let descriptor = BucketDescriptor::try_from(descriptor).map_err(AdbcError::BucketDescriptor)?;
    let query = generate_leg(leg, Dialect::ClickHouse).map_err(AdbcError::Render)?;
    let described = describe(adapter, &query, deadline, &runs_as)?;
    if described != *granted {
        return Err(AdbcError::PartitionSchemaChanged);
    }
    let query =
        generate_leg_buckets(leg, &described, descriptor.bucket(), Dialect::ClickHouse).map_err(AdbcError::BucketRender)?;
    let rows = adapter.run(&query, deadline, MOST_RESULT_ROWS, &runs_as)?;
    if rows.schema() != granted {
        return Err(AdbcError::PartitionSchemaChanged);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use sutura_adbc::DriverLocation;
    use sutura_conformance::corpus;
    use sutura_domain::identity::Secret;
    use sutura_domain::source::{AcknowledgementReason, HashBucketsDeclared};
    use sutura_domain::warehouse::ResultBudget;

    use super::*;

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "this cell reaches the partition guard directly and must fail if driver loading is reached"
    )]
    fn a_switched_bucket_refuses_the_reading_workers_own_service_user_before_loading_the_driver() {
        let warehouse = AdbcClickHouseWarehouse::new(
            corpus::source(),
            SourcePosture::ImpersonationAtSource,
            DriverLocation::parse("/nonexistent/libadbc_driver_clickhouse.so").expect("an absolute driver path"),
            String::from("http://127.0.0.1:1"),
            String::from("sutura_worker_service"),
            Secret::new(String::from("unused")),
            None,
            ResultBudget::of_bytes(NonZeroUsize::MIN),
        )
        .with_hash_buckets(&HashBucketsDeclared::of(
            AcknowledgementReason::written_under("hash_buckets", "the fixture is immutable").expect("a reason"),
        ));
        let leg = corpus::leg_case();
        let descriptor = BucketDescriptor::from(Bucket::new(0, 2).expect("one of two buckets")).to_bytes();
        let granted = std::sync::Arc::new(arrow_schema::Schema::empty());
        let error = warehouse
            .read_partition(
                leg.leg(),
                &descriptor,
                &granted,
                PartitionIdentity::Switched(PrincipalName::parse("sutura_worker_service").expect("the worker's own user")),
                corpus::deadline(),
            )
            .expect_err("a worker must refuse its own service user before any connection");
        assert!(matches!(error, AdbcError::PartitionIdentityDisagrees), "{error}");
    }
}
