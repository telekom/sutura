//! Partitioned execution: the coordinating side runs a statement once and gets back descriptors
//! for several result streams; the reading side drains one of those streams.
//!
//! # The descriptor is opaque on purpose
//!
//! `nix/bigquery-adbc-partitions.patch` encodes a partition as a version byte plus a Storage Read
//! API stream name, and decodes that shape only inside the driver
//! (`decodePartitionDescriptor` in the patch's Go). [`PartitionDescriptor`] never reads what it
//! carries - it is bytes in, bytes out - so a malformed or empty one is still a value of this type;
//! what refuses it is the driver's own `ReadPartition`, which answers an empty or version-mismatched
//! descriptor `StatusInvalidArgument`. That refusal reaches Rust exactly like any other ADBC call
//! that failed, through [`AdbcError::Adbc`] - there is no second, Rust-side check of the version
//! byte, because the one authority on the format is the driver that defined it.
//!
//! # Limit
//!
//! Neither function here builds its own connection or statement. When either is wired into a
//! caller, the connection or statement handed in must come from [`super::AdbcBigQuery::connect`]
//! the same as every other call this transport makes - never built or reused outside that path -
//! because `connect` is where the identity-first check, the bytes-billed ceiling and the job
//! timeout all live, and nothing here re-applies any of them.

use std::time::Instant;

use adbc_core::{Connection, Statement};
use arrow_schema::SchemaRef;

use super::{AdbcError, DriverMessage, accumulate_bounded, race_to_deadline};
use crate::transport::JobDeadline;

/// One partition of a statement's result, as the driver's own opaque bytes.
///
/// **A newtype rather than a bare `Vec<u8>`**, so a caller cannot build one from anything but
/// [`execute_leg_partitions`]'s own answer and cannot read what is inside. [`Self::as_bytes`] exists
/// only for handing the value back to [`read_partition_to_deadline`], which is the one place it is
/// read at all.
pub(crate) struct PartitionDescriptor(Vec<u8>);

impl core::fmt::Debug for PartitionDescriptor {
    /// Prints the byte COUNT alone. The bytes themselves are a Storage Read API stream name plus
    /// the project, location and session it was opened under - every one of which is sensitive, and
    /// `Debug` is what a log line or a panic message reaches for first.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PartitionDescriptor(<{} bytes>)", self.0.len())
    }
}

impl PartitionDescriptor {
    /// The bytes this descriptor carries, for [`Connection::read_partition`] alone.
    fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Builds a descriptor from bytes a test made up, standing in for a real one
    /// [`execute_leg_partitions`] would have answered - the one bypass of this type's own
    /// invariant, kept to cfg(test) so nothing outside a test can reach it.
    #[cfg(test)]
    pub(crate) const fn for_test(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

/// [`execute_leg_partitions`]'s answer: the schema the result was announced under, one descriptor
/// per Storage Read API stream the driver opened, and the row count the driver reported for the
/// statement as a whole, if it reported one at all.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "not wired into AdbcBigQuery::run yet - W2 of telekom/sutura#828, this slice is the reading side"
    )
)]
#[derive(Debug)]
pub(crate) struct Partitioned {
    pub(crate) schema: SchemaRef,
    pub(crate) descriptors: Vec<PartitionDescriptor>,
    /// `adbc_core::PartitionedResult::rows_affected` is `-1` for "not known", not absent - mapped
    /// here once with [`u64::try_from`] so nothing downstream has to re-learn that one sentinel:
    /// `-1` becomes [`None`], every other value (including `0`) becomes [`Some`].
    pub(crate) rows: Option<u64>,
}

/// Runs `stmt`'s statement once, at the coordinating node, and returns the schema its result was
/// announced under, one descriptor per Storage Read API stream the driver opened over it, and the
/// row count the driver reported - refusing to outlive `deadline` as measured from `now`.
///
/// `execute_partitions` hands back descriptors for streams nothing has drained yet, so there is
/// nothing here for a row or byte ceiling to apply to; [`read_partition_to_deadline`] is where those
/// ceilings apply, once per stream, when a stream is actually read. The client-side TIME bound is
/// the same one [`super::run_to_deadline`] applies to an unpartitioned statement, carried here by
/// the identical [`super::race_to_deadline`] over [`Statement::execute_partitions`] instead of
/// [`Statement::execute`] - see that function's own doc for exactly what cancelling a still-blocking
/// call can and cannot promise, and for why `keep_alive` exists; both apply here unchanged.
///
/// # Errors
///
/// [`AdbcError::Adbc`] for a call the driver refused - including a `bigquery.query.dry_run`
/// statement or an empty query, both of which the patch's `ExecutePartitions` refuses outright
/// rather than answering with a partition list. [`AdbcError::DeadlineElapsed`] if `deadline` fires
/// first.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "not wired into AdbcBigQuery::run yet - W2 of telekom/sutura#828, this slice is the reading side"
    )
)]
pub(crate) fn execute_leg_partitions<S>(
    stmt: S,
    keep_alive: impl Send + 'static,
    deadline: JobDeadline,
    now: Instant,
) -> Result<Partitioned, AdbcError>
where
    S: Statement + Clone + Send + 'static,
{
    let mut stmt = stmt;
    let mut canceller = stmt.clone();
    race_to_deadline(
        move || execute_partitions_now(&mut stmt),
        move || canceller.cancel(),
        keep_alive,
        deadline,
        now,
    )
}

fn execute_partitions_now<S>(stmt: &mut S) -> Result<Partitioned, AdbcError>
where
    S: Statement,
{
    let partitioned = stmt
        .execute_partitions()
        .map_err(|e| AdbcError::Adbc(DriverMessage::of(&e)))?;
    Ok(Partitioned {
        schema: SchemaRef::new(partitioned.schema),
        descriptors: partitioned.partitions.into_iter().map(PartitionDescriptor).collect(),
        rows: u64::try_from(partitioned.rows_affected).ok(),
    })
}

/// Reads one partition's stream to completion, under the same row and byte ceilings
/// [`super::execute_to_completion`] applies to an unpartitioned result.
fn read_partition_to_completion<C>(conn: &C, descriptor: &PartitionDescriptor) -> Result<super::ResultBatches, AdbcError>
where
    C: Connection,
{
    let reader = conn
        .read_partition(descriptor.as_bytes())
        .map_err(|e| AdbcError::Adbc(DriverMessage::of(&e)))?;
    accumulate_bounded(reader)
}

/// Reads one partition's stream, refusing to outlive `deadline` as measured from `now`.
///
/// The same client-side bound [`super::run_to_deadline`] applies to the statement that produced
/// the descriptors, carried here by [`super::race_to_deadline`] so both races share one
/// worker-thread, one timeout and one cancel-on-timeout shape.
///
/// The cancel here is [`Connection::cancel`], because a partition's read never goes through a
/// `Statement`. The pinned driver's C entry points track ONE context per connection (driverbase-go's
/// `CancellableContext`): every call on a connection cancels the previous call's context before it
/// starts, and `ConnectionCancel` cancels the current one. So this cancel reaches the read only while
/// the read is the connection's latest call.
///
/// `keep_alive` carries whatever has to outlive a worker that may still be running after this
/// function has already returned [`AdbcError::DeadlineElapsed`] - the driver handle, in every real
/// caller - for the identical reason [`super::run_to_deadline`]'s own doc gives.
///
/// # Limit
///
/// Every read needs its own connection. Any later call on `conn` - another read, a new statement, an
/// option get or set - cancels a stream this read is still draining; executing a statement does not,
/// as a statement tracks its own context. Nothing here refuses a shared connection.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "not wired into AdbcBigQuery::run yet - W2 of telekom/sutura#828, this slice is the reading side"
    )
)]
pub(crate) fn read_partition_to_deadline<C>(
    conn: C,
    descriptor: PartitionDescriptor,
    keep_alive: impl Send + 'static,
    deadline: JobDeadline,
    now: Instant,
) -> Result<super::ResultBatches, AdbcError>
where
    C: Connection + Clone + Send + 'static,
{
    let mut canceller = conn.clone();
    race_to_deadline(
        move || read_partition_to_completion(&conn, &descriptor),
        move || canceller.cancel(),
        keep_alive,
        deadline,
        now,
    )
}
