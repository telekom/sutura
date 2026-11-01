//! A failed combine read as a lost node from the status of the link that failed, and from nothing else.

use datafusion::error::DataFusionError;
use datafusion_distributed::GetWorkerInfoRequest;
use datafusion_distributed::grpc::{BoxCloneSyncChannel, create_worker_client};
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

#[test]
fn a_node_whose_connection_ended_under_a_call_is_a_lost_node() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime builds");
    let failure = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port binds");
        let address = listener.local_addr().expect("a bound port has an address");
        let node = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("the call connects");
            let mut connection = h2::server::handshake(socket).await.expect("the call's connection opens");
            connection.accept().await.is_some()
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("a node address is an endpoint")
            .connect_lazy();
        let failure = create_worker_client(BoxCloneSyncChannel::new(channel))
            .get_worker_info(GetWorkerInfoRequest {})
            .await
            .map(drop)
            .expect_err("a node that went away answers nothing");
        assert!(
            node.await.expect("the node ran"),
            "the call reached the node before it went away"
        );
        failure
    });
    let refused = lost(CombineError::Execute { cause: failure });
    assert!(matches!(refused, CombineError::NodeLost { .. }), "{refused:?}");
}
