//! The governance refusal a combine failure is, when it is one.

use sutura_domain::plan::FederationCombiner;
use sutura_domain::query::RefusalReason;

/// The refusal `cause` is, taken off the combiner in the order the mono path asks its own adapter:
/// the ceiling, then no other node ready in time, then a deterministic refusal about what the legs
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
        .or_else(|| {
            combiner
                .answer_not_well_formed(cause)
                .map(|federated| RefusalReason::FederatedAnswerNotWellFormed { federated })
        })
}

/// The order the refusals are read in, over a combiner that reads each as it is told.
#[cfg(test)]
mod tests;
