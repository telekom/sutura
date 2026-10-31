use std::sync::Arc;

use axum::http::StatusCode;

use crate::server::NodeCertified;
use crate::state::WorkerMount;
use crate::testing::{broker, bundle, call, catalog_of, fake_warehouse, request, settings_with, sink, state_over};

const EXECUTE_TASK: &str = "/worker.WorkerService/ExecuteTask";

/// The real router with a worker mount whose service answers `200` to a call that reaches it on its
/// full method path, as a generated gRPC server routes, and `404` to any other.
fn with_a_worker() -> axum::Router {
    let service = crate::surface::LocalService::start(
        &catalog_of(bundle()),
        fake_warehouse(),
        sink(),
        broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    let reached = tower::service_fn(|request: axum::extract::Request| async move {
        Ok::<_, std::convert::Infallible>(if request.uri().path() == EXECUTE_TASK {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        })
    });
    let state = state_over(Arc::new(service), settings_with("")).with_worker_surface(WorkerMount::new(reached));
    crate::router(&state).expect("the test router assembles")
}

#[tokio::test]
async fn a_worker_call_reaches_the_engine_only_over_a_connection_that_presented_a_node_certificate() {
    let app = with_a_worker();

    let (status, body) = call(&app, request("POST", EXECUTE_TASK, None, axum::body::Body::empty())).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a caller with no node certificate ran a task"
    );
    assert!(body.is_empty(), "a gRPC client reads a body as frames: {body}");

    let mut certified = request("POST", EXECUTE_TASK, None, axum::body::Body::empty());
    let _previous = certified.extensions_mut().insert(NodeCertified);
    let (status, _body) = call(&app, certified).await;
    assert_eq!(status, StatusCode::OK, "a node's call did not reach the worker service");
}
