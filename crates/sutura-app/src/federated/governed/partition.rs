//! A granted partition past a reading node's per-stream bound is refused as a result that did not
//! fit, whether or not this node knows which source it split, and never as the internal failure a
//! `503` answers.

use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs};
use sutura_domain::plan::{FederatedRowCeiling, WorkersShort};
use sutura_domain::query::{RefusalReason, ResultBound};
use sutura_domain::warehouse::ResultBatches;

use super::refusal_split;

/// A combiner whose every combine fails, read as a reading node's partition that did not fit.
struct DidNotFit;

impl FederationCombiner for DidNotFit {
    type Error = core::fmt::Error;

    fn combine(&self, _plan: &FederatedPlan, _legs: Legs<'_>, _working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        Err(core::fmt::Error)
    }

    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }

    fn answer_not_well_formed(&self, _error: &Self::Error) -> Option<FederatedAnswerRefusal> {
        None
    }

    fn workers_not_ready(&self, _error: &Self::Error) -> Option<WorkersShort> {
        None
    }

    fn node_lost(&self, _error: &Self::Error) -> bool {
        false
    }

    fn partition_did_not_fit(&self, _error: &Self::Error) -> bool {
        true
    }
}

#[test]
fn a_granted_partition_past_a_reading_nodes_bound_is_refused_as_a_result_that_did_not_fit() {
    let ceiling = FederatedRowCeiling::parse(500).expect("a federated row ceiling");
    let too_large = Some(RefusalReason::ResultTooLarge {
        bound: ResultBound::Volume,
    });
    assert_eq!(refusal_split(&DidNotFit, &core::fmt::Error, ceiling, None), too_large);
    let split = sutura_domain::model::SourceName::parse("crm").expect("a source name");
    assert_eq!(refusal_split(&DidNotFit, &core::fmt::Error, ceiling, Some(&split)), too_large);
}
