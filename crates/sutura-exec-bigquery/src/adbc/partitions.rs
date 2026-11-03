//! Partitioned execution: the coordinating side runs a statement once and gets back descriptors
//! for several result streams; the reading side drains one of those streams.
//!
//! **Nothing here is called from anywhere yet.** `AdbcBigQuery::run` still goes through
//! `execute_to_completion`/`run_to_deadline` alone - this module is reachable only from its own
//! tests. Wiring either function into the federated or combine path, or into a worker that reads a
//! stream this process did not execute the statement for, is a later slice; stating that here is
//! the limit this module's own doc has to carry, because nothing else in this diff does.
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
//! # The stream count is not a parameter here
//!
//! The patch requests a FIXED number of Storage Read API streams per partitioned statement - the Go
//! constant `partitionStreamCount = 4`, read off the patch itself, with its own comment saying
//! wiring it to a knob is a later slice. So [`execute_leg_partitions`] takes none: there is nothing
//! pinned driver exposes to ask for a different count, and adding a Rust-side parameter the driver
//! cannot act on would be a knob that changes nothing.

use adbc_core::{Connection, Statement};
use arrow_schema::SchemaRef;

use super::{AdbcError, DriverMessage, accumulate_bounded, race_to_deadline};
use crate::transport::JobDeadline;

/// One partition of a statement's result, as the driver's own opaque bytes.
///
/// **A newtype rather than a bare `Vec<u8>`**, so a caller cannot build one from anything but
/// [`execute_leg_partitions`]'s own answer and cannot read what is inside - the field is private
/// to this module, not merely `pub(crate)`, so no other module in this crate can build one either.
/// [`Self::as_bytes`] exists only for handing the value back to [`read_partition_to_deadline`],
/// which is the one place it is read at all.
#[derive(Debug, Clone)]
pub struct PartitionDescriptor(Vec<u8>);

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

/// Named because `Result<(SchemaRef, Vec<PartitionDescriptor>), AdbcError>` is over the
/// `type_complexity` threshold this workspace tightened - the same reason `crate::Mapped` exists.
type Partitioned = Result<(SchemaRef, Vec<PartitionDescriptor>), AdbcError>;

/// Runs `stmt`'s statement once, at the coordinating node, and returns the schema its result was
/// announced under plus one descriptor per Storage Read API stream the driver opened over it.
///
/// **No bound of its own, in rows, bytes or time** - unlike [`super::execute_to_completion`], this
/// reads no result at all. `execute_partitions` hands back descriptors for streams nothing has
/// drained yet, so there is nothing here for a row or byte ceiling to apply to; [`read_partition_to_deadline`]
/// is where those ceilings and the deadline both apply, once per stream, when a stream is actually
/// read.
///
/// # Errors
///
/// [`AdbcError::Adbc`] for a call the driver refused - including a `bigquery.query.dry_run`
/// statement or an empty query, both of which the patch's `ExecutePartitions` refuses outright
/// rather than answering with a partition list.
pub fn execute_leg_partitions<S>(stmt: &mut S) -> Partitioned
where
    S: Statement,
{
    let partitioned = stmt
        .execute_partitions()
        .map_err(|e| AdbcError::Adbc(DriverMessage::of(&e)))?;
    let descriptors = partitioned.partitions.into_iter().map(PartitionDescriptor).collect();
    Ok((SchemaRef::new(partitioned.schema), descriptors))
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
/// **What stands in for [`adbc_core::Statement::cancel`] here is [`Connection::cancel`]** - the
/// same trait's own method, over the connection the stream is being read on rather than over a
/// statement, because a partition's read never goes through a `Statement` at all.
///
/// `keep_alive` carries whatever has to outlive a worker that may still be running after this
/// function has already returned [`AdbcError::DeadlineElapsed`] - the driver handle, in every real
/// caller - for the identical reason [`super::run_to_deadline`]'s own doc gives.
pub fn read_partition_to_deadline<C>(
    conn: C,
    descriptor: PartitionDescriptor,
    keep_alive: impl Send + 'static,
    deadline: JobDeadline,
    now: std::time::Instant,
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
