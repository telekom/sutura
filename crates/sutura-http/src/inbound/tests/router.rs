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
    let service = LocalService::start(&catalog_of(bundle()), fake_warehouse(), sink(), crate::testing::broker())
        .expect("the test bundle validates");
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

/// A token this issuer signed, carrying `scope`.
///
/// **Every router test that expects an answer now has to mint one**, and that is the behaviour change
/// this branch makes rather than a test-fixture detail: a verified caller is permitted exactly the
/// capabilities its scopes name, so a token with no `scope` claim reaches a handler for nothing.
fn a_token_granting(pair: &rcgen::KeyPair, scope: &str) -> String {
    let extra = serde_json::json!({ "scope": scope }).to_string();
    signed(pair, KID, &claims("someone@example.com", RESOURCE, ISSUER, &extra))
}

/// A token granting every capability this surface has, space-delimited per RFC 6749.
fn a_fully_granted_token(pair: &rcgen::KeyPair) -> String {
    let scope = sutura_app::Capability::every()
        .map(sutura_app::Capability::scope)
        .collect::<Vec<&str>>()
        .join(" ");
    a_token_granting(pair, &scope)
}

/// One request through the real router, with a peer address the limiter can key on.
async fn call(app: &axum::Router, token: Option<&str>) -> (StatusCode, Option<String>) {
    ask(app, token, "POST", "/v1/query").await
}

/// The same, for any method and path, so the catalog route can be reached too.
async fn ask(app: &axum::Router, token: Option<&str>, method: &str, uri: &str) -> (StatusCode, Option<String>) {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
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

    // **The token now has to carry the scope**, which is this branch's behaviour change and not a
    // fixture detail: leg 1 says who is asking and the capability gate says what they may invoke.
    // `a_token` - the same token, with no `scope` claim - is the 403 asserted two tests below.
    let (status, _) = call(&app, Some(&a_fully_granted_token(&pair))).await;
    assert_eq!(status, StatusCode::OK, "a verified caller's question is answered");

    // A forged signature is the same 401 as no token at all, which is the point: nothing in the
    // response distinguishes the two.
    let attacker = key_pair();
    let forged = signed(&attacker, KID, &claims("admin@example.com", RESOURCE, ISSUER, ""));
    let (status, _) = call(&app, Some(&forged)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// **THE property of the capability gate, through the real router.**
///
/// A verified caller granted one capability reaches that route and is refused the other, with a `403`
/// naming the scope it lacks. Both halves matter: without the second the gate is a control that is
/// never exercised, and without the first it is a control that refuses everybody.
#[tokio::test]
async fn a_verified_caller_reaches_only_the_routes_its_scopes_name() {
    let pair = key_pair();
    let app = app(DIRECT_OVERLAY, Some(&jwks(KID, &pair)));
    let catalog_only = a_token_granting(&pair, sutura_app::Capability::DescribeCatalog.scope());

    let (status, _) = ask(&app, Some(&catalog_only), "GET", "/v1/catalog").await;
    assert_eq!(status, StatusCode::OK, "the granted capability is reachable");

    let (status, _) = call(&app, Some(&catalog_only)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the capability this token does not name is refused"
    );

    // The other way round, so the pass above is about the grant rather than about the route.
    let ask_only = a_token_granting(&pair, sutura_app::Capability::AskMetric.scope());
    let (status, _) = call(&app, Some(&ask_only)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = ask(&app, Some(&ask_only), "GET", "/v1/catalog").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Fail closed, and the refusal is what makes it survivable.
///
/// A token this deployment verified, carrying no capability scope, reaches nothing. That is the
/// operational trap this branch introduces - switch `security.inbound` on before authoring scopes and
/// every caller is switched off - so the response has to be diagnosable without a log: `403`,
/// `insufficient_scope`, and the exact scope string named in the detail.
#[tokio::test]
async fn a_verified_caller_whose_token_names_no_capability_scope_reaches_nothing() {
    let pair = key_pair();
    let app = app(DIRECT_OVERLAY, Some(&jwks(KID, &pair)));
    // `a_token` is the same token as everywhere else in this file, and it carries no `scope`.
    let scopeless = a_token(&pair);
    let (status, body) = answered(&app, &scopeless, "POST", "/v1/query").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains(r#""code":"insufficient_scope""#), "{body}");
    assert!(body.contains(sutura_app::Capability::AskMetric.scope()), "{body}");
    // And it is not a `401`: re-authenticating would hand back the same token forever.
    assert_ne!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = answered(&app, &scopeless, "GET", "/v1/catalog").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains(sutura_app::Capability::DescribeCatalog.scope()), "{body}");
}

/// One request, with the body read back, for the two tests that assert on `code` and on the scope.
async fn answered(app: &axum::Router, token: &str, method: &str, uri: &str) -> (StatusCode, String) {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
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
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("the test response body is small");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn a_deployment_that_declares_no_inbound_identity_is_unaffected() {
    // The shape that ships today, and the assertion that leg 1 is opt-in: no declaration, no layer, and
    // a question is answered as `Subject::TheDeploymentItself` - which is every deployment that existed
    // before this change.
    //
    // **And it is the assertion that the capability gate is opt-in too**, which is the half worth
    // pinning here: with no verified caller there is no claim to narrow by, so `Permitted` is every
    // capability and both routes answer. A filter over an unverified claim would look like a control
    // and be none.
    let app = app("", None);
    let (status, challenge) = call(&app, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(challenge.is_none(), "nothing challenges a caller here");
    let (status, _) = ask(&app, None, "GET", "/v1/catalog").await;
    assert_eq!(status, StatusCode::OK, "the catalog is not gated where nobody is identified");
}

#[test]
fn a_declared_inbound_identity_with_no_gate_attached_assembles_no_router() {
    // The mechanism that makes attaching leg 1 unforgettable. Without it, a composition root that built
    // the settings and skipped the gate would serve, answer every question as the deployment, and log a
    // startup line saying it establishes a caller identity.
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(DIRECT_OVERLAY))
        .expect("the test settings load");
    let service = LocalService::start(&catalog_of(bundle()), fake_warehouse(), sink(), crate::testing::broker())
        .expect("the test bundle validates");
    let state = ServiceState::new(Arc::new(service), Arc::new(settings));
    let refused = crate::router(&state).expect_err("a declaration with no gate assembles no router");
    assert!(
        matches!(refused, crate::RouterNotBuilt::InboundIdentityNotAttached { mode: "direct" }),
        "{refused:?}"
    );
    assert!(refused.to_string().contains("with_inbound_identity"), "{refused}");
}
