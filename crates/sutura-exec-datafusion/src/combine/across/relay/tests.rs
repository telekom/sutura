//! The refusal code read back off a failure, under the errors the engine wraps a stage's failure in,
//! and nothing that is not exactly that code.

use std::sync::Arc;

use datafusion::error::DataFusionError;
use sutura_domain::plan::FederationCombiner as _;

use super::{over_ceiling, relayed};
use crate::combine::{CombineError, DataFusionCombiner};

fn stage_failure() -> DataFusionError {
    DataFusionError::Execution(String::from("a stage refused its rows"))
}

fn failed(cause: DataFusionError) -> CombineError {
    relayed(CombineError::Execute { cause })
}

#[test]
fn a_stage_refused_over_the_row_ceiling_is_read_back_as_this_nodes_refusal() {
    let combiner = DataFusionCombiner::new().expect("a combiner builds");
    let refused = failed(over_ceiling(stage_failure()));
    assert!(matches!(refused, CombineError::RowsOverCeiling), "{refused:?}");
    assert!(combiner.rows_over_ceiling(&refused));
    assert!(!combiner.node_lost(&refused));
    assert!(combiner.answer_not_well_formed(&refused).is_none());
}

#[test]
fn the_code_is_read_under_the_errors_the_engine_wraps_a_stage_failure_in() {
    let wrapped = [
        DataFusionError::Shared(Arc::new(over_ceiling(stage_failure()))),
        DataFusionError::Context(
            String::from("a stage on another node"),
            Box::new(over_ceiling(stage_failure())),
        ),
        DataFusionError::External(Box::new(over_ceiling(stage_failure()))),
        DataFusionError::External(Box::new(Arc::new(over_ceiling(stage_failure())))),
    ];
    for cause in wrapped {
        let refused = failed(cause);
        assert!(matches!(refused, CombineError::RowsOverCeiling), "{refused:?}");
    }
}

#[test]
fn a_failure_that_does_not_carry_exactly_the_code_stays_the_engines_failure() {
    let code = "sutura.refusal.rows-over-ceiling";
    let failures = [
        DataFusionError::Context(String::from("sutura.refusal.rows-over-ceiling.v2"), Box::new(stage_failure())),
        DataFusionError::Context(String::from("SUTURA.REFUSAL.ROWS-OVER-CEILING"), Box::new(stage_failure())),
        DataFusionError::Context(format!(" {code}"), Box::new(stage_failure())),
        DataFusionError::Execution(String::from(code)),
        DataFusionError::External(String::from(code).into()),
        DataFusionError::External(Box::new(tonic::Status::internal(code))),
    ];
    for cause in failures {
        let failure = failed(cause);
        assert!(matches!(failure, CombineError::Execute { .. }), "{failure:?}");
    }
}

#[test]
fn only_a_failed_execution_is_read_for_the_code() {
    let lost = relayed(CombineError::NodeLost {
        cause: over_ceiling(stage_failure()),
    });
    assert!(matches!(lost, CombineError::NodeLost { .. }), "{lost:?}");
}
