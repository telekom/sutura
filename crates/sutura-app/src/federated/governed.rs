//! The governance refusal a combine failure is, when it is one.

use sutura_domain::plan::{FederatedRowCeiling, FederationCombiner};
use sutura_domain::query::{RefusalReason, ResultBound};

/// The refusal `cause` is, taken off the combiner in the order the mono path asks its own adapter:
/// the ceiling, then no other node ready in time, then a node lost while the combine ran, then a
/// deterministic refusal about what the legs returned.
///
/// D19 + A4: a deterministic combine failure - the same plan against the same legs fails again - is
/// a governance refusal and not a data-system outage, so a caller is never told to retry one. `None`
/// is this workspace's own wiring, which stays an internal `ServiceError` because no caller caused it
/// and none can fix it.
pub(super) fn refusal<C>(combiner: &C, cause: &C::Error) -> Option<RefusalReason>
where
    C: FederationCombiner,
{
    combiner
        .working_set_exhausted(cause)
        .map(|ceiling_bytes| RefusalReason::ResourcesExhausted { ceiling_bytes })
        .or_else(|| {
            combiner.workers_not_ready(cause).map(|short| RefusalReason::WorkersNotReady {
                wanted: short.wanted(),
                ready: short.ready(),
            })
        })
        .or_else(|| combiner.node_lost(cause).then_some(RefusalReason::NodeLost))
        .or_else(|| {
            combiner
                .answer_not_well_formed(cause)
                .map(|federated| RefusalReason::FederatedAnswerNotWellFormed { federated })
        })
}

/// [`refusal`], then a stage another node refused over the federated row ceiling, answered with
/// `ceiling` - this node's own - and never a number the combiner read off that node.
pub(super) fn refusal_within<C>(combiner: &C, cause: &C::Error, ceiling: FederatedRowCeiling) -> Option<RefusalReason>
where
    C: FederationCombiner,
{
    refusal(combiner, cause).or_else(|| {
        combiner.rows_over_ceiling(cause).then_some(RefusalReason::ResultTooLarge {
            bound: ResultBound::Rows { limit: ceiling.get() },
        })
    })
}

/// A node lost while the combine ran, over a combiner that reads every failure as one.
#[cfg(test)]
mod lost;
/// A stage another node refused over the federated row ceiling, answered with this node's own.
#[cfg(test)]
mod rows;
/// The order the refusals are read in, over a combiner that reads each as it is told.
#[cfg(test)]
mod tests;
