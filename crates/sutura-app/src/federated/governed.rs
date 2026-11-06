//! The governance refusal a combine failure is, when it is one.

use sutura_domain::model::SourceName;
use sutura_domain::plan::{FederatedPlan, FederatedRowCeiling, FederationCombiner};
use sutura_domain::query::{RefusalReason, ResultBound};

/// The refusal `cause` is, taken off the combiner in the order the mono path asks its own adapter:
/// the ceiling, then no other node ready in time, then a node lost while the combine ran, then a
/// granted leg whose source no node the combine can reach reads - `SourceUnavailable`, the refusal a
/// source this deployment does not serve gets - then a deterministic refusal about what the legs
/// returned.
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
                .no_reading_node(cause)
                .map(|source| RefusalReason::SourceUnavailable { source: source.clone() })
        })
        .or_else(|| {
            combiner
                .answer_not_well_formed(cause)
                .map(|federated| RefusalReason::FederatedAnswerNotWellFormed { federated })
        })
}

/// [`refusal`], then a stage another node refused over the federated row ceiling, answered with
/// `ceiling` - this node's own - and never a number the combiner read off that node; then a granted
/// partition past a reading node's per-stream bound, answered as a leg read whole here that did not
/// fit is: [`ResultBound::Volume`], which carries no number.
pub(super) fn refusal_within<C>(combiner: &C, cause: &C::Error, ceiling: FederatedRowCeiling) -> Option<RefusalReason>
where
    C: FederationCombiner,
{
    refusal(combiner, cause)
        .or_else(|| {
            combiner.rows_over_ceiling(cause).then_some(RefusalReason::ResultTooLarge {
                bound: ResultBound::Rows { limit: ceiling.get() },
            })
        })
        .or_else(|| {
            combiner
                .partition_did_not_fit(cause)
                .then_some(RefusalReason::ResultTooLarge {
                    bound: ResultBound::Volume,
                })
        })
}

/// [`refusal_within`], then a reading node refusing to certify a leg it was asked to read, answered
/// naming `split` - the source this node split lookup legs off - and never a name the combiner read
/// off that node.
///
/// **The limit:** with `split` `None` the failure stays an internal `ServiceError`, because no
/// partition carries back which leg it was split from.
pub(super) fn refusal_split<C>(
    combiner: &C,
    cause: &C::Error,
    ceiling: FederatedRowCeiling,
    split: Option<&SourceName>,
) -> Option<RefusalReason>
where
    C: FederationCombiner,
{
    refusal_within(combiner, cause, ceiling).or_else(|| {
        split
            .filter(|_| combiner.leg_not_certified(cause))
            .map(|source| crate::remote_leg::leg_not_certified(source.clone()))
    })
}

/// The one source every lookup leg of `plan` reads - the only legs a combine splits - or `None` when
/// they read more than one, or there are none.
pub(super) fn split_source(plan: &FederatedPlan) -> Option<&SourceName> {
    let mut sources = plan.lookups().legs().iter().map(|lookup| lookup.leg().source());
    let first = sources.next()?;
    sources.all(|source| source == first).then_some(first)
}

/// A reading node's refusal to certify a leg, named with this node's own source.
#[cfg(test)]
mod certified;
/// A node lost while the combine ran, over a combiner that reads every failure as one.
#[cfg(test)]
mod lost;
/// A granted partition past a reading node's per-stream bound, answered as a leg that did not fit.
#[cfg(test)]
mod partition;
/// A stage another node refused over the federated row ceiling, answered with this node's own.
#[cfg(test)]
mod rows;
/// The order the refusals are read in, over a combiner that reads each as it is told.
#[cfg(test)]
mod tests;
