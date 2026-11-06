//! A split lookup leg granted to the nodes that read its source, or read here.
//!
//! [`granted_or_read`](grant::granted_or_read) is the arm each [`SplitKind`](crate::serve::kind::split::SplitKind)
//! runs once its source split a leg, over [`HeldSplit`](grant::HeldSplit) so a fake can stand in for
//! the driver.
//!
//! **Limits:** a split of one partition is read here, never granted: the combine plans a granted leg
//! of one partition as one task, and that task's leaf only reads inside a reading task. A split whose
//! columns no one schema stands for is read here too.

use core::num::NonZeroU32;
use std::time::SystemTime;

use sutura_domain::plan::{Grantee, LookupSplit, Partitioned};
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_exec_datafusion::SchemaRef;

/// The columns every partition answers, `None` where no one schema stands for them, or the failure.
pub(super) type Described<E> = Result<Option<SchemaRef>, E>;

/// One partition read here, `None` past the last.
pub(super) type Read<E> = Option<Result<ResultBatches, E>>;

/// A split its source holds open across its partitions, as the grant arm reads it.
pub(super) trait HeldSplit: Send + Sync {
    /// Why the split could not describe or read.
    type Error;

    /// Each partition's descriptor in page order, `None` for one only this node can read.
    fn descriptors(&self) -> Vec<Option<Vec<u8>>>;

    /// The columns every partition answers, or `None` where no one schema stands for all of them.
    ///
    /// # Errors
    ///
    /// The source's failure to describe them.
    fn columns(&mut self, deadline: Deadline) -> Described<Self::Error>;

    /// The source's estimate of the whole leg.
    fn estimate(&self) -> EstimatedBytes;

    /// Partition `index` read here, or `None` past the last.
    fn read(&self, index: usize, deadline: Deadline) -> Read<Self::Error>;
}

/// `split` granted to `grantee` when it has two or more partitions and one schema, held by the
/// grant so what it keeps open lives as long as the grant does, and checked against `now()` read
/// after the describe; otherwise read here, one partition after another, in page order.
///
/// # Errors
///
/// The split's failure to describe its columns. A grant [`Grantee::grant`] refuses is
/// [`Partitioned::NotGranted`], and nothing of it is read here.
pub(super) fn granted_or_read<'s, S>(
    mut split: S,
    grantee: Option<Grantee>,
    deadline: Deadline,
    stream_rows: NonZeroU32,
    now: impl FnOnce() -> SystemTime,
) -> LookupSplit<'s, S::Error>
where
    S: HeldSplit + 's,
{
    if let Some(grantee) = grantee {
        let partitions = split.descriptors();
        if partitions.len() > 1
            && let Some(schema) = split.columns(deadline)?
        {
            let estimate = split.estimate();
            return Ok(grantee
                .grant(schema, partitions, stream_rows, estimate, split, now())
                .map_or_else(Partitioned::NotGranted, Partitioned::Granted));
        }
    }
    Ok(Partitioned::Split(Box::new(
        (0..).map_while(move |index| split.read(index, deadline)),
    )))
}

#[cfg(test)]
mod tests;
