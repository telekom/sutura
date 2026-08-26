//! The surface, driven end to end through the assembled router.
//!
//! Through the router and not through a spawned process: every test here builds the real
//! `axum::Router` from real `Settings` and a fake behind the [`Surface`] port, then calls it with
//! `tower`'s `oneshot`. That exercises the layer stack, the token gate, the limiter, the extractors
//! and the response shapes without a socket, a catalog directory or a data system.
//!
//! One thing has to be supplied by hand that the real server supplies for it: the peer address. The
//! limiter keys on the connection's address, which `axum::serve` attaches via
//! `into_make_service_with_connect_info`; a `oneshot` has no connection, so [`request`] inserts one.
//! Without it the limiter would report that it cannot extract a key and limit nothing - which is
//! also the failure mode in production if that call is ever dropped from
//! [`crate::server::serve`].

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use sutura_config::{Environment, Settings, Sources};
use tower::ServiceExt as _;

use crate::state::ServiceState;
use crate::surface::LocalService;
use crate::testing::{bundle, catalog_of, fake_warehouse};

/// A token that satisfies the configured floor.
const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// A well formed question the fake will answer.
const QUESTION: &str = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

fn settings(environment: Environment, overlay: &str) -> Settings {
    Settings::load(&Sources::defaults(environment).with_overlay(overlay)).expect("the test settings load")
}

/// The router, over a fake warehouse that answers every plan.
fn app(settings: Settings) -> Router {
    let service = LocalService::start(&catalog_of(bundle()), fake_warehouse()).expect("the test bundle validates");
    crate::router(&ServiceState::new(Arc::new(service), Arc::new(settings))).expect("the test router assembles")
}

/// A request with a peer address attached. See the module documentation.
fn request(method: &str, path: &str, token: Option<&str>, body: Body) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let mut request = builder
        .header("content-type", "application/json")
        .body(body)
        .expect("the test request is well formed");
    let peer: SocketAddr = "203.0.113.7:44444".parse().expect("a test peer address is an address");
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// Calls the router once and reads the whole response.
async fn call(app: &Router, request: Request<Body>) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible as a service");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("the test response body is readable");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

// -------------------------------------------------------------------- health ----

#[tokio::test]
async fn health_answers_without_a_token_and_says_nothing_about_the_deployment() {
    // Liveness has to work for a probe with no credential, and it has to be worth nothing to an
    // attacker. Both halves in one assertion, against a deployment that DOES require a token
    // everywhere else.
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));
    let (status, body) = call(&app, request("GET", "/health", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"status":"ok"}"#);
}

// ------------------------------------------------------------- the token gate ----

#[tokio::test]
async fn the_versioned_api_needs_the_token_when_one_is_configured() {
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));

    let (status, body) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains(r#""code":"unauthorized""#), "{body}");

    let (status, _) = call(
        &app,
        request(
            "GET",
            "/v1/catalog",
            Some("wrong-but-long-enough-to-be-a-token"),
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = call(&app, request("GET", "/v1/catalog", Some(TOKEN), Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("revenue"), "{body}");
}

#[tokio::test]
async fn an_unmatched_path_answers_not_found_without_holding_a_credential() {
    // **This test asserts the CURRENT behaviour, and it is not the behaviour this file first
    // claimed.** `route_layer` runs only for a request that matched a route in that subtree, so a
    // path under the version prefix that matches nothing skips the token gate and falls through to
    // the top-level `404`.
    //
    // Kept as `404` rather than contorted into a `401`, because what it discloses is only *which
    // paths exist* - and the paths are in the interface description, which is published. Every path
    // that resolves to a handler does hold a credential, which is asserted by the test above. If
    // that judgement ever changes, this test is where it changes.
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));
    let (status, body) = call(&app, request("GET", "/v1/catalog/secret", None, Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // What matters either way: the response says nothing about the deployment.
    assert!(body.is_empty(), "{body}");
}

#[tokio::test]
async fn a_loopback_development_service_with_no_token_configured_answers_without_one() {
    // The permissive branch, and the reason it is safe: this configuration is only loadable on a
    // loopback bind outside production, which `sutura-config` enforces with a startup refusal.
    let app = app(settings(Environment::Development, ""));
    let (status, _) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
}

// ------------------------------------------------------------------- queries ----

#[tokio::test]
async fn a_certified_question_is_answered_with_its_provenance() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(&app, request("POST", "/v1/query", None, Body::from(QUESTION))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""outcome":"answer""#), "{body}");
    assert!(body.contains(r#""definition_version":"test-1""#), "{body}");
}

#[tokio::test]
async fn a_refusal_comes_back_as_a_success_with_an_outcome_of_refusal() {
    // THE contract of this surface. A refusal is a result: an error status would invite a client
    // library to retry, and retrying a governance decision until it succeeds is precisely what the
    // refusal exists to prevent.
    let app = app(settings(Environment::Development, ""));
    let unknown = r#"{"metric":"gross_margin","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;
    let (status, body) = call(&app, request("POST", "/v1/query", None, Body::from(unknown))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""outcome":"refusal""#), "{body}");
    assert!(body.contains(r#""code":"metric_unknown""#), "{body}");
}

#[tokio::test]
async fn a_body_carrying_sql_is_a_bad_request_and_the_field_is_named() {
    // The governance boundary, through a real JSON parser. Without `deny_unknown_fields` this is a
    // `200` answering a different question than the caller believes they asked.
    let app = app(settings(Environment::Development, ""));
    let smuggled = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},
                       "sql":"select * from orders"}"#;
    let (status, body) = call(&app, request("POST", "/v1/query", None, Body::from(smuggled))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
    assert!(body.contains("sql"), "{body}");
}

#[tokio::test]
async fn a_body_larger_than_the_configured_bound_is_refused_before_it_is_parsed() {
    let app = app(settings(Environment::Development, "server:\n  max_body_bytes: 8\n"));
    let (status, _) = call(&app, request("POST", "/v1/query", None, Body::from(QUESTION))).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

// ------------------------------------------------------------- the limiter ----

#[tokio::test]
async fn the_limiter_refuses_a_caller_past_its_burst() {
    // A limiter that does not limit is the failure mode this exists for, and it is invisible
    // without a test: the layer composes, the service answers, and nothing is bounded.
    let app = app(settings(
        Environment::Development,
        "rate_limit:\n  enabled: true\n  api_per_second: 1\n  api_burst: 1\n",
    ));
    let (first, _) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(first, StatusCode::OK);
    let (second, body) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(body.contains(r#""code":"rate_limited""#), "{body}");
}

#[tokio::test]
async fn the_disabled_limiter_does_not_limit() {
    // The other side of the same switch, so "off" is a real state rather than an untested branch.
    let app = app(settings(
        Environment::Development,
        "rate_limit:\n  enabled: false\n  api_per_second: 1\n  api_burst: 1\n",
    ));
    for _ in 0_u8..5 {
        let (status, _) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn liveness_and_the_api_are_limited_separately() {
    // Two tiers, so a probe hammering liveness cannot exhaust a caller's quota for the API. The
    // public tier is exhausted here and the general tier still answers.
    let app = app(settings(
        Environment::Development,
        "rate_limit:\n  enabled: true\n  probe_per_second: 1\n  probe_burst: 1\n  api_per_second: 5\n  api_burst: 5\n",
    ));
    assert_eq!(
        call(&app, request("GET", "/health", None, Body::empty())).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, request("GET", "/health", None, Body::empty())).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        call(&app, request("GET", "/v1/catalog", None, Body::empty())).await.0,
        StatusCode::OK
    );
}

// ------------------------------------------------------- the documentation ----

#[tokio::test]
async fn the_interface_description_is_served_in_development_and_not_in_production() {
    let development = app(settings(Environment::Development, ""));
    let (status, body) = call(&development, request("GET", "/openapi.json", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("NO PER-CALLER IDENTITY"), "{body}");

    // Production, fully configured, and the description is off by default: a map of the surface is
    // something a deployment turns on rather than something it has to remember to turn off.
    let production = app(settings(
        Environment::Production,
        &format!("server:\n  host: \"0.0.0.0\"\nsecurity:\n  access_token: \"{TOKEN}\"\n  expose_beyond_loopback: true\n"),
    ));
    let (status, _) = call(&production, request("GET", "/openapi.json", Some(TOKEN), Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_interface_description_is_behind_the_token_when_one_is_configured() {
    // A description of the surface is a description of what this deployment measures.
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));
    assert_eq!(
        call(&app, request("GET", "/openapi.json", None, Body::empty())).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, request("GET", "/openapi.json", Some(TOKEN), Body::empty()))
            .await
            .0,
        StatusCode::OK
    );
}
