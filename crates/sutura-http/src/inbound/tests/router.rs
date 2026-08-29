//! Leg 1 through the assembled router, rather than through the gate.
//!
//! What these three assert is that the LAYER IS INSTALLED - that a question with no token is refused
//! before a handler sees it, that one with a verified caller is answered, and that a deployment
//! declaring an identity with no gate attached does not get a router at all. Whether the validator is
//! correct is `super`'s job and `super::review`'s.
//!
//! A file of its own because `super`'s reached the thousand-line limit `cargo xtask max-lines`
//! enforces, and this is the group that stands on its own: it needs the real `Settings`, the real
//! `ServiceState` and `tower`'s `oneshot`, none of which anything else here touches.

use std::sync::Arc;

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::{Environment, Settings, Sources};
use tower::ServiceExt as _;

use crate::state::ServiceState;
use crate::surface::LocalService;
use crate::testing::{bundle, catalog_of, fake_warehouse, sink};

use super::{ISSUER, KID, RESOURCE, a_token, claims, gate_over, jwks, key_pair, signed};
/// A router for a deployment whose declaration is `declared`, with a gate over one key set document.
fn app(declared: &str, document: Option<&str>) -> axum::Router {
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(declared)).expect("the test settings load");
    let service = LocalService::start(&catalog_of(bundle()), fake_warehouse(), sink()).expect("the test bundle validates");
    let mut state = ServiceState::new(Arc::new(service), Arc::new(settings.clone()));
    if let Some(document) = document {
        let declaration = settings
            .security()
            .inbound()
            .expect("this overlay declares an inbound identity")
            .clone();
        state = state.with_inbound_identity(Arc::new(gate_over(&declaration, document)));
    }
    crate::router(&state).expect("the test router assembles")
}

/// The `direct` overlay every router test below uses. The key set path is never read - the gate is
/// built over a document.
const DIRECT_OVERLAY: &str = "security:\n  inbound:\n    mode: \"direct\"\n    \
                              resource: \"https://sutura.example.com\"\n    \
                              authorization_server: \"https://issuer.example.com\"\n    \
                              key_set_file: \"/unread.json\"\n    algorithms: [\"ES256\"]\n";

/// One request through the real router, with a peer address the limiter can key on.
async fn call(app: &axum::Router, token: Option<&str>) -> (StatusCode, Option<String>) {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/query")
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let mut request = builder
        .body(Body::from(
            r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
        ))
        .expect("the test request is well formed");
    let peer: std::net::SocketAddr = "203.0.113.7:44444".parse().expect("a test peer address is an address");
    let _previous_peer = request.extensions_mut().insert(axum::extract::ConnectInfo(peer));
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible as a service");
    let status = response.status();
    let challenge = response
        .headers()
        .get(axum::http::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(String::from);
    (status, challenge)
}

#[tokio::test]
async fn the_versioned_surface_needs_a_verified_caller_when_one_is_declared() {
    // Through the REAL router, so what is asserted is that the layer is installed rather than that the
    // gate works. A question with no token is a 401 with a challenge; the same question with a token
    // the issuer signed is answered.
    let pair = key_pair();
    let app = app(DIRECT_OVERLAY, Some(&jwks(KID, &pair)));
    let (status, challenge) = call(&app, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let challenge = challenge.expect("a refused caller is told where to look");
    assert!(challenge.starts_with("Bearer "), "{challenge}");
    assert!(
        challenge.contains(RESOURCE),
        "the realm is this deployment's own: {challenge}"
    );
    // And what it must NOT say: which check failed. There is no `error_description`.
    assert!(!challenge.contains("error_description"), "{challenge}");

    let (status, _) = call(&app, Some(&a_token(&pair))).await;
    assert_eq!(status, StatusCode::OK, "a verified caller's question is answered");

    // A forged signature is the same 401 as no token at all, which is the point: nothing in the
    // response distinguishes the two.
    let attacker = key_pair();
    let forged = signed(&attacker, KID, &claims("admin@example.com", RESOURCE, ISSUER, ""));
    let (status, _) = call(&app, Some(&forged)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_deployment_that_declares_no_inbound_identity_is_unaffected() {
    // The shape that ships today, and the assertion that leg 1 is opt-in: no declaration, no layer, and
    // a question is answered as `Subject::TheDeploymentItself` - which is every deployment that existed
    // before this change.
    let app = app("", None);
    let (status, challenge) = call(&app, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(challenge.is_none(), "nothing challenges a caller here");
}

#[test]
fn a_declared_inbound_identity_with_no_gate_attached_assembles_no_router() {
    // The mechanism that makes attaching leg 1 unforgettable. Without it, a composition root that built
    // the settings and skipped the gate would serve, answer every question as the deployment, and log a
    // startup line saying it establishes a caller identity.
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(DIRECT_OVERLAY))
        .expect("the test settings load");
    let service = LocalService::start(&catalog_of(bundle()), fake_warehouse(), sink()).expect("the test bundle validates");
    let state = ServiceState::new(Arc::new(service), Arc::new(settings));
    let refused = crate::router(&state).expect_err("a declaration with no gate assembles no router");
    assert!(
        matches!(refused, crate::RouterNotBuilt::InboundIdentityNotAttached { mode: "direct" }),
        "{refused:?}"
    );
    assert!(refused.to_string().contains("with_inbound_identity"), "{refused}");
}
