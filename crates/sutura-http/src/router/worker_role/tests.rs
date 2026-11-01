use axum::body::Body;
use axum::http::StatusCode;

use crate::constants::HEALTH_PATH;
use crate::server::NodeCertified;
use crate::state::WorkerMount;
use crate::testing::{call, request, settings_with};

const EXECUTE_TASK: &str = "/worker.WorkerService/ExecuteTask";

#[tokio::test]
async fn a_worker_router_serves_liveness_and_the_worker_subtree_and_nothing_a_caller_asks() {
    let reached = tower::service_fn(|request: axum::extract::Request| async move {
        Ok::<_, std::convert::Infallible>(if request.uri().path() == EXECUTE_TASK {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        })
    });
    let app = super::worker_router(&settings_with(""), &WorkerMount::new(reached)).expect("the worker router builds");

    for (method, path) in [
        ("POST", "/v1/query"),
        ("GET", "/v1/catalog"),
        ("POST", "/mcp"),
        ("POST", "/arrow.flight.protocol.FlightService/DoGet"),
    ] {
        let (status, _body) = call(&app, request(method, path, None, Body::empty())).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "a worker answered {method} {path}");
    }
    let (status, _body) = call(&app, request("GET", HEALTH_PATH, None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "a worker's liveness is not served");

    let (status, _body) = call(&app, request("POST", EXECUTE_TASK, None, Body::empty())).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a caller with no node certificate ran a task"
    );
    let mut certified = request("POST", EXECUTE_TASK, None, Body::empty());
    let _previous = certified.extensions_mut().insert(NodeCertified);
    let (status, _body) = call(&app, certified).await;
    assert_eq!(status, StatusCode::OK, "a node's call did not reach the worker service");
}

#[tokio::test]
async fn a_worker_counts_a_node_call_in_flight_until_its_response_stream_is_dropped() {
    let streaming = tower::service_fn(|_request: axum::extract::Request| async move {
        Ok::<_, std::convert::Infallible>(axum::response::Response::new(Body::from_stream(
            futures_util::stream::pending::<Result<axum::body::Bytes, std::convert::Infallible>>(),
        )))
    });
    let app = super::worker_router(&settings_with(""), &WorkerMount::new(streaming)).expect("the worker router builds");

    let mut certified = request("POST", EXECUTE_TASK, None, Body::empty());
    let _previous = certified.extensions_mut().insert(NodeCertified);
    let answered = tower::ServiceExt::oneshot(app.clone(), certified)
        .await
        .expect("the router answers");
    let (_status, during) = call(&app, request("GET", "/metrics", None, Body::empty())).await;
    assert!(
        during.contains("\nsutura_worker_requests_in_flight 1\n"),
        "a call whose stream is still open is not counted: {during}"
    );

    drop(answered);
    let (_status, after) = call(&app, request("GET", "/metrics", None, Body::empty())).await;
    assert!(
        after.contains("\nsutura_worker_requests_in_flight 0\n"),
        "a dropped stream is still counted: {after}"
    );
}
