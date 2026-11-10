//! A combine that wanted other nodes and found none ready is a refusal worth a retry, not a service
//! error, and the ceiling is read before it. A granted leg no node reads is its source unavailable.

use sutura_domain::plan::WorkersShort;
use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs};
use sutura_domain::query::RefusalReason;
use sutura_domain::warehouse::ResultBatches;

use super::refusal;

/// A combiner whose every combine fails, and which reads that failure as the governance outcomes it
/// is given.
struct Reads {
    ceiling_bytes: Option<u64>,
    short: Option<WorkersShort>,
}

impl FederationCombiner for Reads {
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
        self.short
    }

    fn node_lost(&self, _error: &Self::Error) -> bool {
        false
    }
}

#[test]
fn a_combine_no_other_node_was_ready_for_is_refused_as_workers_not_ready() {
    let combiner = Reads {
        ceiling_bytes: None,
        short: Some(WorkersShort::new(2, 0)),
    };
    assert_eq!(
        refusal(&combiner, &core::fmt::Error),
        Some(RefusalReason::WorkersNotReady { wanted: 2, ready: 0 })
    );
}

#[test]
fn a_combine_over_its_ceiling_is_refused_for_the_ceiling_whether_or_not_other_nodes_were_ready() {
    let combiner = Reads {
        ceiling_bytes: Some(64),
        short: Some(WorkersShort::new(2, 0)),
    };
    assert_eq!(
        refusal(&combiner, &core::fmt::Error),
        Some(RefusalReason::ResourcesExhausted { ceiling_bytes: 64 })
    );
}

#[test]
fn a_granted_leg_no_reachable_node_reads_is_refused_as_its_source_unavailable() {
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    let source = sutura_domain::model::SourceName::parse("geo").expect("a test source");
    let unread = sutura_exec_datafusion::CombineError::NoReadingNode {
        source_name: source.clone(),
    };
    assert_eq!(refusal(&combiner, &unread), Some(RefusalReason::SourceUnavailable { source }));
}
