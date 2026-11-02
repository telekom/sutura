//! A stage another node refused over the federated row ceiling is refused with this node's own
//! ceiling, and the working-set ceiling is read before it.

use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs};
use sutura_domain::plan::{FederatedRowCeiling, WorkersShort};
use sutura_domain::query::{RefusalReason, ResultBound};
use sutura_domain::warehouse::ResultBatches;

use super::refusal_within;

/// A combiner whose every combine fails, read as a relayed row-ceiling refusal and as over
/// `ceiling_bytes` when set.
struct Relayed {
    ceiling_bytes: Option<u64>,
}

impl FederationCombiner for Relayed {
    type Error = core::fmt::Error;

    fn combine(&self, _plan: &FederatedPlan, _legs: Legs<'_>, _working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        Err(core::fmt::Error)
    }

    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        self.ceiling_bytes
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

    fn rows_over_ceiling(&self, _error: &Self::Error) -> bool {
        true
    }
}

fn ceiling() -> FederatedRowCeiling {
    FederatedRowCeiling::parse(500).expect("a federated row ceiling")
}

#[test]
fn a_stage_another_node_refused_over_the_row_ceiling_is_refused_with_this_nodes_ceiling() {
    let combiner = Relayed { ceiling_bytes: None };
    assert_eq!(
        refusal_within(&combiner, &core::fmt::Error, ceiling()),
        Some(RefusalReason::ResultTooLarge {
            bound: ResultBound::Rows { limit: 500 }
        })
    );
}

#[test]
fn a_combine_over_its_working_set_is_refused_for_that_whether_or_not_a_node_refused_its_rows() {
    let combiner = Relayed { ceiling_bytes: Some(64) };
    assert_eq!(
        refusal_within(&combiner, &core::fmt::Error, ceiling()),
        Some(RefusalReason::ResourcesExhausted { ceiling_bytes: 64 })
    );
}
