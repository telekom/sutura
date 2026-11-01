//! A combine that lost a node it was planned onto is a refusal worth a retry, not a service error,
//! and the ceiling is read before it.

use sutura_domain::plan::WorkersShort;
use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ResultBatches;

use super::refusal;

/// A combiner whose every combine fails, read as a lost node and as over `ceiling_bytes` when set.
struct Lost {
    ceiling_bytes: Option<u64>,
}

impl FederationCombiner for Lost {
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
        true
    }
}

#[test]
fn a_combine_that_lost_a_node_is_refused_as_node_lost() {
    let combiner = Lost { ceiling_bytes: None };
    assert_eq!(refusal(&combiner, &core::fmt::Error), Some(RefusalReason::NodeLost));
}

#[test]
fn a_combine_over_its_ceiling_is_refused_for_the_ceiling_whether_or_not_a_node_was_lost() {
    let combiner = Lost { ceiling_bytes: Some(64) };
    assert_eq!(
        refusal(&combiner, &core::fmt::Error),
        Some(RefusalReason::ResourcesExhausted { ceiling_bytes: 64 })
    );
}
