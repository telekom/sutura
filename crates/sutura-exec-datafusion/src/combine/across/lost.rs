//! A failed combine read as a lost node from the status of the link that failed, and from nothing else.

use datafusion::error::DataFusionError;
use sutura_domain::plan::FederationCombiner as _;

use super::lost;
use crate::combine::{CombineError, DataFusionCombiner};

fn failed(status: tonic::Status) -> CombineError {
    CombineError::Execute {
        cause: DataFusionError::External(Box::new(status)),
    }
}

#[test]
fn a_combine_whose_link_was_unavailable_is_refused_as_a_lost_node() {
    let combiner = DataFusionCombiner::new().expect("a combiner builds");
    let refused = lost(failed(tonic::Status::unavailable("tcp connect error")));
    assert!(combiner.node_lost(&refused), "{refused:?}");
}

#[test]
fn a_link_status_the_engine_wrapped_on_its_way_out_is_still_read() {
    let unavailable = DataFusionError::External(Box::new(tonic::Status::unavailable("tcp connect error")));
    let wrapped = CombineError::Execute {
        cause: DataFusionError::Context(String::from("a stage on another node"), Box::new(unavailable)),
    };
    assert!(matches!(lost(wrapped), CombineError::NodeLost { .. }));
}

#[test]
fn a_link_that_failed_with_any_other_status_is_no_lost_node() {
    let combiner = DataFusionCombiner::new().expect("a combiner builds");
    let failure = lost(failed(tonic::Status::internal("h2 protocol error")));
    assert!(!combiner.node_lost(&failure), "{failure:?}");
    assert!(matches!(failure, CombineError::Execute { .. }), "{failure:?}");
}
