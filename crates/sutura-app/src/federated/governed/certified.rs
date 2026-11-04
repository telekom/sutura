//! A reading node's refusal to certify a leg is refused naming this node's own source, and with no
//! source of its own to name it stays an internal error.

use sutura_domain::model::SourceName;
use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs};
use sutura_domain::plan::{FederatedRowCeiling, WorkersShort};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ResultBatches;

use super::refusal_split;

/// A combiner whose every combine fails, read as a reading node's refusal to certify a leg.
struct NotCertified;

impl FederationCombiner for NotCertified {
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

    fn leg_not_certified(&self, _error: &Self::Error) -> bool {
        true
    }
}

#[test]
fn a_reading_node_refusing_to_certify_a_leg_is_refused_naming_this_nodes_split_source() {
    let ceiling = FederatedRowCeiling::parse(500).expect("a federated row ceiling");
    let ours = SourceName::parse("warehouse_here").expect("a source name");
    assert_eq!(
        refusal_split(&NotCertified, &core::fmt::Error, ceiling, Some(&ours)),
        Some(RefusalReason::RemoteLegNotCertified { source: ours })
    );
    assert_eq!(refusal_split(&NotCertified, &core::fmt::Error, ceiling, None), None);
}
