//! Leg 1 through the assembled router, rather than through the gate.
//!
//! What these assert is that the LAYER IS INSTALLED - that a question with no token is refused before
//! a handler sees it, that one with a verified caller is answered, that scopes decide which routes a
//! caller reaches, and that a deployment declaring an identity with no gate attached does not get a
//! router at all. Whether the validator is correct is `super`'s job and `super::review`'s.
//!
//! A file of its own because `super`'s reached the thousand-line limit `cargo xtask max-lines`
//! enforces, and this is the group that stands on its own: it needs the real `Settings`, the real
//! `ServiceState` and a call through the router, none of which anything else there touches.
//!
//! **The tokens here come from `sutura_dev::issuer` and not from `super`'s fixtures**, which is the
//! line this crate now draws: a test that goes through the ROUTER mints from the shared mock issuer,
//! and `super` keeps its own in-place key pair and encoder for the gate-level unit tests.
//!
//! **Two reasons, and the second is the one that makes it a line rather than a migration nobody
//! finished.** The router tests are the ones a composed-binary harness will later re-point at a
//! socket, so a fixture private to one module could not travel with them. And `super`'s tests need a
//! rawer tool than a mock issuer should be: they sign claim sets with a `sub` carrying a newline, with
//! no `exp` at all, past the token size cap, and with a hand-built delegation chain. Widening the
//! issuer to emit arbitrary JSON would turn it into a signer of anything and cost it the property that
//! makes it worth having - that every token it mints is one an issuer could have minted.

use axum::http::StatusCode;
use sutura_dev::issuer::{MockIssuer, Token};

use crate::testing::{
    Answered, accepted_by, an_issuer, asked, broker, bundle, declared_inbound, direct_overlay, fake_warehouse, request, serving,
    settings_with,
};

use super::gate_over;

/// The `key_set_file` every declaration here names, and nothing reads.
///
/// The gate is built over a DOCUMENT through [`gate_over`], so the path is never opened - which is
/// what keeps these tests off a filesystem. `super::published` is the module that does read a file,
/// deliberately, because the rotation bound needs a source that changes.
const UNREAD: &str = "/unread.json";

const METADATA_PATH: &str = "/.well-known/oauth-protected-resource";

/// A router for a deployment that verifies `issuer`, over that issuer's own key set.
fn app_verifying(issuer: &MockIssuer) -> axum::Router {
    app(&direct_overlay(issuer, UNREAD), Some(&issuer.key_set()))
}

/// A gateway declaration over `issuer`, whose keys are handed to [`app`] rather than read from here.
fn gateway_overlay(issuer: &MockIssuer) -> String {
    format!(
        "security:\n  inbound:\n    mode: \"behind-gateway\"\n    transit_header: \"X-Transit-Proof\"\n    \
         transit_issuer: \"{}\"\n    transit_audience: \"{}\"\n    key_set_file: \"{UNREAD}\"\n    \
         algorithms: [\"ES256\"]\n    transit_token_type: \"at+jwt\"\n",
        issuer.issuer(),
        issuer.audience(),
    )
}

/// A router for a deployment whose declaration is `declared`, with a gate over one key set document.
fn app(declared: &str, document: Option<&str>) -> axum::Router {
    let settings = settings_with(declared);
    let gate = document.map(|document| gate_over(&declared_inbound(&settings), document));
    serving(bundle(), fake_warehouse(), broker(), settings, gate)
}

/// One question through the real router, carrying `token` if there is one.
async fn call(app: &axum::Router, token: Option<&str>) -> Answered {
    asked(app, "POST", "/v1/query", token).await
}

/// Reads the challenge, if routing one request-target reaches the inbound gate.
async fn challenge_for(app: &axum::Router, target: &str, host: Option<&str>) -> Option<String> {
    use tower::ServiceExt as _;

    let mut request = request("POST", target, None, axum::body::Body::from(crate::testing::A_QUESTION));
    if let Some(host) = host {
        let value = axum::http::HeaderValue::from_str(host).expect("a test Host is a header value");
        let _previous = request.headers_mut().insert(axum::http::header::HOST, value);
    }
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible as a service");
    response
        .headers()
        .get(axum::http::header::WWW_AUTHENTICATE)
        .map(|challenge| challenge.to_str().expect("the challenge is visible ASCII").into())
}

/// A token this issuer signed, carrying `scope` and nothing else.
///
/// **Every router test that expects an answer has to mint one**, and that is a behaviour of the
/// surface rather than a fixture detail: a verified caller is permitted exactly the capabilities its
/// scopes name, so a token with no `scope` claim reaches a handler for nothing.
fn granting(issuer: &MockIssuer, scope: &str) -> String {
    issuer
        .mint(&Token::for_subject("someone@example.com").granting(scope))
        .expect("the issuer signs a token")
}

#[tokio::test]
async fn the_versioned_surface_needs_a_verified_caller_when_one_is_declared() {
    // Through the REAL router, so what is asserted is that the layer is installed rather than that the
    // gate works. A question with no token is a 401 with a challenge; the same question with a token
    // the issuer signed is answered.
    let issuer = an_issuer();
    let app = app_verifying(&issuer);
    let refused = call(&app, None).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    let challenge = refused.challenge.expect("a refused caller is told where to look");
    assert!(challenge.starts_with("Bearer "), "{challenge}");
    assert!(
        challenge.contains(issuer.audience()),
        "the realm is this deployment's own: {challenge}"
    );
    // And what it must NOT say: which check failed. There is no `error_description`.
    assert!(!challenge.contains("error_description"), "{challenge}");

    // **The token has to carry the scope**, which is a property of the surface and not a fixture
    // detail: leg 1 says who is asking and the capability gate says what they may invoke. The same
    // token with no `scope` claim is the 403 asserted two tests below.
    let granted = issuer
        .mint(&accepted_by("someone@example.com"))
        .expect("the issuer signs a token");
    let answered = call(&app, Some(&granted)).await;
    assert_eq!(answered.status, StatusCode::OK, "a verified caller's question is answered");

    // A forged signature is the same 401 as no token at all, which is the point: nothing in the
    // response distinguishes the two. Signed by a key this issuer does not publish, and correct in
    // every other respect - so what is refused is the signature rather than the key id.
    let forged = issuer
        .mint_signed_by_a_stranger(&accepted_by("admin@example.com"))
        .expect("a stranger signs a token");
    let refused = call(&app, Some(&forged)).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);

    let metrics = asked(&app, "GET", "/metrics", None).await;
    assert_eq!(metrics.status, StatusCode::OK, "{}", metrics.body);
    assert!(
        metrics.body.contains("sutura_unauthorized_total 2"),
        "both verified-caller rejections are counted: {}",
        metrics.body
    );
}

#[tokio::test]
async fn an_unauthenticated_client_reads_direct_metadata() {
    let resource = "https://sutura.example.com/v1/query";
    let issuer = MockIssuer::generating(crate::testing::ISSUER, resource, crate::testing::KID)
        .expect("a mock issuer generates a key pair");
    let app = app_verifying(&issuer);
    let path = format!("{METADATA_PATH}/v1/query");
    let metadata = asked(&app, "GET", &path, None).await;
    assert_eq!(metadata.status, StatusCode::OK, "the metadata route is public");
    let document: serde_json::Value = serde_json::from_str(&metadata.body).expect("the metadata is JSON");
    assert_eq!(
        document,
        serde_json::json!({
            "resource": resource,
            "authorization_servers": [issuer.issuer()],
        })
    );

    assert_eq!(crate::capability_of(&axum::http::Method::GET, &path), None);
}

#[tokio::test]
async fn a_challenge_points_at_metadata_only_for_an_exact_origin_form_resource() {
    let resource = "https://sutura.example.com/v1/query";
    let issuer = MockIssuer::generating(crate::testing::ISSUER, resource, crate::testing::KID)
        .expect("a mock issuer generates a key pair");

    let app = app_verifying(&issuer);
    let bare = "Bearer realm=\"https://sutura.example.com/v1/query\", error=\"invalid_token\"";
    let challenge = challenge_for(&app, "/v1/query", Some("sutura.example.com"))
        .await
        .expect("an origin-form request for the resource reaches the inbound gate");
    assert_eq!(
        challenge,
        format!("{bare}, resource_metadata=\"https://sutura.example.com/.well-known/oauth-protected-resource/v1/query\"")
    );
    assert!(!challenge.contains("error_description"));

    for (target, host) in [
        ("https://sutura.example.com/v1/query", None),
        ("HTTPS://sutura.example.com/v1/query", None),
        ("http://sutura.example.com/v1/query", None),
        ("/v1/query", None),
        ("/v1/query", Some("Sutura.example.com")),
        ("/v1/query", Some("sutura.example.com:443")),
        ("/v1/query/", Some("sutura.example.com")),
        ("/v1/catalog", Some("sutura.example.com")),
        ("/v1/query", Some("other.example.com")),
    ] {
        let challenge = challenge_for(&app, target, host).await;
        assert!(
            challenge.as_deref().is_none_or(|challenge| challenge == bare),
            "{target} with Host {host:?} is not the configured resource spelling: {challenge:?}"
        );
    }

    let root = an_issuer();
    assert_eq!(
        challenge_for(&app_verifying(&root), "/v1/query", Some("sutura.example.com"))
            .await
            .as_deref(),
        Some("Bearer realm=\"https://sutura.example.com\", error=\"invalid_token\""),
        "root metadata is direct-discovery-only and cannot describe a child URL"
    );
}

#[tokio::test]
async fn direct_metadata_derives_the_rfc_9728_path_for_roots_and_paths() {
    for (resource, path) in [
        ("https://sutura.example.com", String::from(METADATA_PATH)),
        ("https://sutura.example.com/", String::from(METADATA_PATH)),
        ("https://sutura.example.com/tenant/", format!("{METADATA_PATH}/tenant/")),
        (
            "https://sutura.example.com/:tenant/*leaf",
            format!("{METADATA_PATH}/:tenant/*leaf"),
        ),
    ] {
        let issuer = MockIssuer::generating(crate::testing::ISSUER, resource, crate::testing::KID)
            .expect("a mock issuer generates a key pair");
        let metadata = asked(&app_verifying(&issuer), "GET", &path, None).await;
        assert_eq!(metadata.status, StatusCode::OK, "{resource} must be served at {path}");
        let document: serde_json::Value = serde_json::from_str(&metadata.body).expect("the metadata is JSON");
        assert_eq!(document["resource"], resource);
    }
}

#[tokio::test]
async fn protected_resource_metadata_exists_only_for_direct_inbound_identity() {
    let direct = an_issuer();
    assert_eq!(
        asked(&app_verifying(&direct), "GET", METADATA_PATH, None).await.status,
        StatusCode::OK
    );

    let gateway = an_issuer();
    let gateway_app = app(&gateway_overlay(&gateway), Some(&gateway.key_set()));
    assert_eq!(
        asked(&gateway_app, "GET", METADATA_PATH, None).await.status,
        StatusCode::NOT_FOUND
    );

    let single_player_app = app("", None);
    assert_eq!(
        asked(&single_player_app, "GET", METADATA_PATH, None).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn protected_resource_metadata_uses_the_public_probe_rate_limit() {
    let issuer = an_issuer();
    let overlay = format!(
        "{}rate_limit:\n  enabled: true\n  probe_per_second: 1\n  probe_burst: 1\n",
        direct_overlay(&issuer, UNREAD)
    );
    let app = app(&overlay, Some(&issuer.key_set()));
    assert_eq!(asked(&app, "GET", METADATA_PATH, None).await.status, StatusCode::OK);
    assert_eq!(
        asked(&app, "GET", METADATA_PATH, None).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

/// **THE property of the capability gate, through the real router.**
///
/// A verified caller granted one capability reaches that route and is refused the other, with a `403`
/// naming the scope it lacks. Both halves matter: without the second the gate is a control that is
/// never exercised, and without the first it is a control that refuses everybody.
#[tokio::test]
async fn a_verified_caller_reaches_only_the_routes_its_scopes_name() {
    let issuer = an_issuer();
    let app = app_verifying(&issuer);
    let catalog_only = granting(&issuer, sutura_app::Capability::DescribeCatalog.scope());

    let answered = asked(&app, "GET", "/v1/catalog", Some(&catalog_only)).await;
    assert_eq!(answered.status, StatusCode::OK, "the granted capability is reachable");

    let refused = call(&app, Some(&catalog_only)).await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "the capability this token does not name is refused"
    );

    // The other way round, so the pass above is about the grant rather than about the route.
    let ask_only = granting(&issuer, sutura_app::Capability::AskMetric.scope());
    assert_eq!(call(&app, Some(&ask_only)).await.status, StatusCode::OK);
    assert_eq!(
        asked(&app, "GET", "/v1/catalog", Some(&ask_only)).await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_catalog_response_is_private_and_not_stored() {
    use tower::ServiceExt as _;

    let issuer = an_issuer();
    let token = granting(&issuer, sutura_app::Capability::DescribeCatalog.scope());
    let response = app_verifying(&issuer)
        .oneshot(request("GET", "/v1/catalog", Some(&token), axum::body::Body::empty()))
        .await
        .expect("the router is infallible as a service");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(axum::http::header::CACHE_CONTROL),
        Some(&axum::http::HeaderValue::from_static("private, no-store"))
    );
}

/// Fail closed, and the refusal is what makes it survivable.
///
/// A token this deployment verified, carrying no capability scope, reaches nothing. That is the
/// operational trap leg 1 introduces - switch `security.inbound` on before authoring scopes and every
/// caller is switched off - so the response has to be diagnosable without a log: `403`,
/// `insufficient_scope`, and the exact scope string named in the detail.
#[tokio::test]
async fn a_verified_caller_whose_token_names_no_capability_scope_reaches_nothing() {
    let issuer = an_issuer();
    let app = app_verifying(&issuer);
    // The ordinary token with no `granting` call at all, which is the shape an operator produces by
    // switching the mode on before authoring any scope.
    let scopeless = issuer
        .mint(&Token::for_subject("someone@example.com"))
        .expect("the issuer signs a token");

    let refused = call(&app, Some(&scopeless)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(refused.body.contains(r#""code":"insufficient_scope""#), "{}", refused.body);
    assert!(
        refused.body.contains(sutura_app::Capability::AskMetric.scope()),
        "{}",
        refused.body
    );
    // And it is not a `401`: re-authenticating would hand back the same token forever.
    assert_ne!(refused.status, StatusCode::UNAUTHORIZED);

    let refused = asked(&app, "GET", "/v1/catalog", Some(&scopeless)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(
        refused.body.contains(sutura_app::Capability::DescribeCatalog.scope()),
        "{}",
        refused.body
    );
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
    let answered = call(&app, None).await;
    assert_eq!(answered.status, StatusCode::OK);
    assert!(answered.challenge.is_none(), "nothing challenges a caller here");
    let answered = asked(&app, "GET", "/v1/catalog", None).await;
    assert_eq!(
        answered.status,
        StatusCode::OK,
        "the catalog is not gated where nobody is identified"
    );
}

#[test]
fn a_declared_inbound_identity_with_no_gate_attached_assembles_no_router() {
    // The mechanism that makes attaching leg 1 unforgettable. Without it, a composition root that built
    // the settings and skipped the gate would serve, answer every question as the deployment, and log a
    // startup line saying it establishes a caller identity.
    // `serving` cannot express this: it would attach nothing and then `expect` the router, which is
    // the one thing this test needs to fail. So the two statements are written out here.
    let settings = settings_with(&direct_overlay(&an_issuer(), UNREAD));
    let service = crate::surface::LocalService::start(
        &crate::testing::catalog_of(bundle()),
        fake_warehouse(),
        crate::testing::sink(),
        broker(),
        1 << 30,
    )
    .expect("the test bundle validates");
    let state = crate::testing::state_over(std::sync::Arc::new(service), settings);
    let refused = crate::router(&state).expect_err("a declaration with no gate assembles no router");
    assert!(
        matches!(refused, crate::RouterNotBuilt::InboundIdentityNotAttached { mode: "direct" }),
        "{refused:?}"
    );
    assert!(refused.to_string().contains("with_inbound_identity"), "{refused}");
}
