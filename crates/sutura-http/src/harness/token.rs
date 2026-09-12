//! The bearer-token gate, through the assembled router.
//!
//! Split out of `harness.rs` by the `max-lines` gate. The tests are unchanged: the same three
//! questions about a deployment that configures `security.access_token` - the versioned API needs
//! it, a path that matched nothing does not, and a loopback development service with none is open
//! on purpose.

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;

use super::{TOKEN, app, settings};
use crate::testing::{call, request};

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
