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
        ("https://sutura.example.com/v1/query", Some("sutura.example.com")),
        ("HTTPS://sutura.example.com/v1/query", Some("sutura.example.com")),
        ("http://sutura.example.com/v1/query", Some("sutura.example.com")),
        ("https://other.example.com/v1/query", Some("sutura.example.com")),
        ("/v1/query", None),
        ("/v1/query", Some("Sutura.example.com")),
        ("/v1/query", Some("sutura.example.com:443")),
        ("/v1/query?x=1", Some("sutura.example.com")),
        ("/v1/query/", Some("sutura.example.com")),
        ("/v1/catalog", Some("sutura.example.com")),
        ("/v1/query", Some("other.example.com")),
        // Authority-form (RFC 9112 section 3.2.3, what a CONNECT target looks like): an authority
        // with no scheme and no path, which is the only way `Uri::authority()` is `Some` while
        // `Uri::scheme()` is `None` - the one disjunct of `describes()`'s guard the rows above never
        // exercise alone, since every scheme-carrying target here carries an authority too.
        ("sutura.example.com:443", Some("sutura.example.com")),
    ] {
        let challenge = challenge_for(&app, target, host).await;
        assert!(
            challenge.as_deref().is_none_or(|challenge| challenge == bare),
            "{target} with Host {host:?} is not the configured resource spelling: {challenge:?}"
        );
    }

    let spelled_resource = "https://Sutura.Example.com:443/v1/query";
    let spelled_issuer = MockIssuer::generating(crate::testing::ISSUER, spelled_resource, crate::testing::KID)
        .expect("a mock issuer generates a key pair");
    let spelled_app = app_verifying(&spelled_issuer);
    let spelled_bare = format!("Bearer realm=\"{spelled_resource}\", error=\"invalid_token\"");
    assert_eq!(
        challenge_for(&spelled_app, "/v1/query", Some("Sutura.Example.com:443")).await,
        Some(format!(
            "{spelled_bare}, resource_metadata=\"https://Sutura.Example.com:443/.well-known/oauth-protected-resource/v1/query\""
        )),
        "the configured authority is preserved byte for byte"
    );
    assert_eq!(
        challenge_for(&spelled_app, "/v1/query", Some("sutura.example.com")).await,
        Some(spelled_bare),
        "a normalized Host is not the configured resource spelling"
    );
    let metadata = asked(&spelled_app, "GET", "/.well-known/oauth-protected-resource/v1/query", None).await;
    assert_eq!(metadata.status, StatusCode::OK);
    let document: serde_json::Value = serde_json::from_str(&metadata.body).expect("the metadata is JSON");
    assert_eq!(document["resource"], spelled_resource);

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
        sutura_domain::plan::RefusingCombiner,
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

/// Leg 1 stands in front of the AGENT surface too: a POST to `/mcp` with no bearer gets the same
/// `401` challenge every forgery on the versioned surface gets, before the transport is reached.
///
/// The mount's own transport is never invoked by this test - the fake service below is a
/// placeholder for `sutura_mcp::http::service`; what is under test is the LAYERING (`establish_asked`
/// behind `inbound_layered`) that a composition root stacking the mount directly onto its router
/// would have to re-derive. Asserting the challenge is byte-identical to the versioned API's is what
/// keeps a caller able to tell "this route is unguarded" from "this route is guarded the same way".
#[cfg(feature = "agent")]
#[tokio::test]
async fn the_agent_route_refuses_an_unverified_caller_with_the_same_challenge_every_forgery_gets() {
    let issuer = an_issuer();
    let settings = settings_with(&direct_overlay(&issuer, UNREAD));
    let gate = super::gate_over(&crate::testing::declared_inbound(&settings), &issuer.key_set());
    let service = crate::surface::LocalService::start(
        &crate::testing::catalog_of(bundle()),
        fake_warehouse(),
        crate::testing::sink(),
        broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    let mut state = crate::testing::state_over(std::sync::Arc::new(service), settings);
    state = state.with_inbound_identity(std::sync::Arc::new(gate));
    // This deployment configures no `governance.per_replica_spend_ceiling`, so there is no
    // `sutura_spend_headroom_bytes` series for the mount to push onto and it says so. A mount that
    // claimed a gauge on this state would be refused by `agent_subtree`.
    let mount = crate::state::AgentMount::new(
        tower::service_fn(|request: axum::http::Request<axum::body::Body>| {
            let _request = request;
            async { Ok::<_, std::convert::Infallible>(axum::response::Response::new(axum::body::Body::empty())) }
        }),
        crate::state::SpendHeadroomPush::NoCeilingConfigured,
    );
    state = state.with_agent_surface(mount);
    let router = crate::router(&state).expect("a leg-one deployment with an agent mount assembles");

    let mcp = no_bearer(&router, crate::constants::AGENT_MOUNT_PATH).await;
    let query_path = format!("{}{}", crate::constants::API_V1_PREFIX, crate::constants::base_paths::QUERY);
    let api = no_bearer(&router, &query_path).await;

    assert_eq!(
        mcp.0,
        StatusCode::UNAUTHORIZED,
        "the agent route refuses an unverified caller: {}",
        mcp.1
    );
    assert_eq!(mcp.1["code"], "unauthorized", "{}", mcp.1);
    let mcp_challenge = mcp.2.expect("the agent route's refusal carries a challenge");
    let api_challenge = api.2.expect("the versioned route's refusal carries a challenge");
    assert_eq!(
        api.0,
        StatusCode::UNAUTHORIZED,
        "the versioned route refuses an unverified caller: {}",
        api.1
    );
    assert_eq!(
        mcp_challenge, api_challenge,
        "the agent surface and the versioned surface refuse alike"
    );
}

/// One no-bearer POST through the assembled router: the status, the JSON body and the challenge.
#[cfg(feature = "agent")]
async fn no_bearer(router: &axum::Router, path: &str) -> (StatusCode, serde_json::Value, Option<String>) {
    use tower::ServiceExt as _;
    let request = crate::testing::request("POST", path, None, axum::body::Body::empty());
    let response = router.clone().oneshot(request).await.expect("the router answers");
    let status = response.status();
    let challenge = response
        .headers()
        .get(axum::http::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(String::from);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body reads");
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, value, challenge)
}

/// The deployment token these cells configure where they configure one.
const DEPLOYMENT_TOKEN: &str = "a-deployment-token-0123456789abcdef";

/// A router over `overlay` with `issuer`'s gate and an agent mount that answers every request `200`.
///
/// The mount stands in for `sutura_mcp::http::service`, so a refusal these cells read comes from a
/// layer `crate::router` put in front of it.
#[cfg(feature = "agent")]
fn with_agent_mount(overlay: &str, issuer: &MockIssuer) -> axum::Router {
    let settings = settings_with(overlay);
    let gate = gate_over(&declared_inbound(&settings), &issuer.key_set());
    let service = crate::surface::LocalService::start(
        &crate::testing::catalog_of(bundle()),
        fake_warehouse(),
        crate::testing::sink(),
        broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    let state = crate::testing::state_over(std::sync::Arc::new(service), settings)
        .with_inbound_identity(std::sync::Arc::new(gate))
        .with_agent_surface(crate::state::AgentMount::new(
            tower::service_fn(|request: axum::http::Request<axum::body::Body>| {
                let _request = request;
                async { Ok::<_, std::convert::Infallible>(axum::response::Response::new(axum::body::Body::empty())) }
            }),
            crate::state::SpendHeadroomPush::NoCeilingConfigured,
        ));
    crate::router(&state).expect("a leg-one deployment with an agent mount assembles")
}

/// The status of one request through the real router, carrying each credential that is given.
async fn status_of(
    app: &axum::Router,
    (method, path): (&str, &str),
    bearer: Option<&str>,
    assertion: Option<&str>,
    host: Option<&str>,
) -> StatusCode {
    use tower::ServiceExt as _;

    let mut request = request(method, path, bearer, axum::body::Body::from(crate::testing::A_QUESTION));
    for (name, value) in [("x-transit-proof", assertion), ("host", host)] {
        if let Some(value) = value {
            let value = axum::http::HeaderValue::from_str(value).expect("a test header value is a header value");
            let _previous = request.headers_mut().insert(name, value);
        }
    }
    app.clone()
        .oneshot(request)
        .await
        .expect("the router is infallible as a service")
        .status()
}

/// A short-lived gateway assertion for `subject`, carrying every scope.
fn assertion_from(issuer: &MockIssuer, subject: &str) -> String {
    issuer
        .mint(&accepted_by(subject).living_for(60))
        .expect("the issuer signs an assertion")
}

/// `/mcp` spends the same rate limit tier `/v1` does: a burst past it is a `429` on both.
#[cfg(feature = "agent")]
#[tokio::test]
async fn the_agent_route_spends_the_same_rate_limit_as_the_versioned_surface() {
    let issuer = an_issuer();
    let overlay = format!(
        "{}rate_limit:\n  enabled: true\n  api_per_second: 1\n  api_burst: 1\n",
        direct_overlay(&issuer, UNREAD)
    );
    let app = with_agent_mount(&overlay, &issuer);
    let query = format!("{}{}", crate::constants::API_V1_PREFIX, crate::constants::base_paths::QUERY);
    for path in [query.as_str(), crate::constants::AGENT_MOUNT_PATH] {
        let mut statuses = Vec::new();
        for _ in 0..3 {
            statuses.push(status_of(&app, ("POST", path), None, None, None).await);
        }
        assert_eq!(
            statuses.last(),
            Some(&StatusCode::TOO_MANY_REQUESTS),
            "{path} past a burst of one: {statuses:?}"
        );
    }
}

/// Behind a gateway with a deployment token, `/mcp` needs the token wherever `/v1` does: a verified
/// assertion alone is a `401` on both, and the two together reach both.
#[cfg(feature = "agent")]
#[tokio::test]
async fn the_agent_route_needs_the_deployment_token_wherever_the_versioned_surface_does() {
    let issuer = an_issuer();
    let overlay = format!("{}  access_token: \"{DEPLOYMENT_TOKEN}\"\n", gateway_overlay(&issuer));
    let app = with_agent_mount(&overlay, &issuer);
    let assertion = assertion_from(&issuer, "someone@example.com");
    for route in [("GET", "/v1/catalog"), ("POST", crate::constants::AGENT_MOUNT_PATH)] {
        assert_eq!(
            status_of(&app, route, None, Some(&assertion), None).await,
            StatusCode::UNAUTHORIZED,
            "{route:?} with an assertion and no deployment token"
        );
        assert_eq!(
            status_of(&app, route, Some(DEPLOYMENT_TOKEN), Some(&assertion), None).await,
            StatusCode::OK,
            "{route:?} with both"
        );
    }
}

/// With no credential configured the loopback bind is the whole perimeter, so `/v1/*`, `/docs` and
/// `/openapi.json` answer only a loopback `Host` - the names `/mcp`'s transport used to accept alone.
#[tokio::test]
async fn a_deployment_with_no_credential_answers_only_a_loopback_host() {
    let app = app("", None);
    for path in [
        "/v1/catalog",
        crate::constants::OPENAPI_JSON_PATH,
        crate::constants::SWAGGER_UI_PATH,
    ] {
        for host in ["other.example.com", "other.example.com:8080", "127.0.0.1.example.com"] {
            assert_eq!(
                status_of(&app, ("GET", path), None, None, Some(host)).await,
                StatusCode::FORBIDDEN,
                "{path} with Host {host}"
            );
        }
        for host in [
            "localhost",
            "localhost:8080",
            "LOCALHOST:8080",
            "127.0.0.1:8080",
            "[::1]:8080",
        ] {
            assert_ne!(
                status_of(&app, ("GET", path), None, None, Some(host)).await,
                StatusCode::FORBIDDEN,
                "{path} with Host {host}"
            );
        }
    }
    let refused = asked_with_host(&app, "/v1/catalog", "other.example.com").await;
    assert!(refused.contains(r#""code":"host_not_allowed""#), "{refused}");
}

/// A request that names no host at all is answered, not refused: an HTTP/1.1 client always sends a
/// `Host`, so the check is for a client that names one.
#[tokio::test]
async fn a_request_with_no_host_is_answered() {
    let app = app("", None);
    assert_eq!(
        status_of(&app, ("GET", "/v1/catalog"), None, None, None).await,
        StatusCode::OK
    );
    for path in [crate::constants::OPENAPI_JSON_PATH, crate::constants::SWAGGER_UI_PATH] {
        assert_ne!(
            status_of(&app, ("GET", path), None, None, None).await,
            StatusCode::FORBIDDEN,
            "{path} with no Host"
        );
    }
}

/// The body of one GET carrying `host`.
async fn asked_with_host(app: &axum::Router, path: &str, host: &str) -> String {
    use tower::ServiceExt as _;

    let mut request = request("GET", path, None, axum::body::Body::empty());
    let _previous = request.headers_mut().insert(
        axum::http::header::HOST,
        axum::http::HeaderValue::from_str(host).expect("a test Host is a header value"),
    );
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible as a service");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body reads");
    String::from_utf8_lossy(&body).into_owned()
}

/// `/mcp` answers the hosts `/v1` does on a loopback deployment: a foreign name is a `403` on both.
#[cfg(feature = "agent")]
#[tokio::test]
async fn the_agent_route_answers_only_a_loopback_host_where_the_versioned_surface_does() {
    let issuer = an_issuer();
    let app = with_agent_mount(&direct_overlay(&issuer, UNREAD), &issuer);
    let token = granting(&issuer, "sutura:catalog.read");
    for route in [("GET", "/v1/catalog"), ("POST", crate::constants::AGENT_MOUNT_PATH)] {
        assert_eq!(
            status_of(&app, route, Some(&token), None, Some("other.example.com")).await,
            StatusCode::FORBIDDEN,
            "{route:?} with a foreign Host"
        );
        assert_eq!(
            status_of(&app, route, Some(&token), None, Some("localhost:8080")).await,
            StatusCode::OK,
            "{route:?} with a loopback Host"
        );
    }
}

/// The settings overlay for a gateway deployment bound off the loopback, with `allowed_hosts` as
/// given (a YAML flow list, or nothing).
fn off_host(issuer: &MockIssuer, allowed_hosts: Option<&str>) -> String {
    let declared = allowed_hosts.map_or_else(String::new, |hosts| format!("  allowed_hosts: {hosts}\n"));
    format!(
        "server:\n  host: \"0.0.0.0\"\n{declared}{}  tls_termination: \"ingress\"\n  \
         metrics_token: \"0123456789abcdef0123456789abcdf0\"\nrate_limit:\n  enabled: true\n",
        gateway_overlay(issuer)
    )
}

/// A deployment off the loopback that declares the host its operator put in front of it answers that
/// host on `/v1` and on `/mcp`, and refuses a name nothing declared on both.
#[cfg(feature = "agent")]
#[tokio::test]
async fn a_declared_host_is_answered_on_every_route_and_an_undeclared_one_is_not() {
    let issuer = an_issuer();
    let app = with_agent_mount(&off_host(&issuer, Some("[\"declared.example.org\"]")), &issuer);
    let assertion = assertion_from(&issuer, "someone@example.com");
    for route in [("GET", "/v1/catalog"), ("POST", crate::constants::AGENT_MOUNT_PATH)] {
        for host in ["declared.example.org", "Declared.Example.org:8443"] {
            assert_eq!(
                status_of(&app, route, None, Some(&assertion), Some(host)).await,
                StatusCode::OK,
                "{route:?} with Host {host}"
            );
        }
        assert_eq!(
            status_of(&app, route, None, Some(&assertion), Some("undeclared.example.org")).await,
            StatusCode::FORBIDDEN,
            "{route:?} with an undeclared Host"
        );
    }
}

/// Off the loopback with nothing declared, the deployment answers every `Host` and says so at boot,
/// naming the key that would restrict it: its callers already present a credential.
#[cfg(feature = "agent")]
#[tokio::test]
async fn an_off_host_deployment_declaring_no_host_warns_and_answers_every_host() {
    let issuer = an_issuer();
    let sink = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
    let app = tracing::subscriber::with_default(subscriber, || with_agent_mount(&off_host(&issuer, None), &issuer));
    let rendered = sink.contents();
    let warning = rendered
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|line| line["level"] == 40 && line.to_string().contains("server.allowed_hosts"))
        .unwrap_or_else(|| panic!("no boot warning names `server.allowed_hosts`: {rendered}"));
    assert!(warning.to_string().contains("answers every Host"), "{warning}");
    let assertion = assertion_from(&issuer, "someone@example.com");
    for route in [("GET", "/v1/catalog"), ("POST", crate::constants::AGENT_MOUNT_PATH)] {
        assert_eq!(
            status_of(&app, route, None, Some(&assertion), Some("anything.example.net")).await,
            StatusCode::OK,
            "{route:?} with an arbitrary Host"
        );
    }
}
