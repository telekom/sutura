//! A bearer token a caller presents is never sent back or written down: not in a response body, not
//! in a response header, and not in a log line at any level.
//!
//! Each cell presents tokens that carry a canary through the refusal paths a deployment can reach -
//! the wrong deployment token, and for a caller identity a token that is malformed, unknown-keyed,
//! forged, expired, for another audience or of another class - then reads back everything the
//! router returned and everything it logged while answering. The log is captured at `trace`, which
//! is the strongest statement a test can make: a line from any crate at any level counts. A canary
//! and not only the token, because the places a token would leak into are the places a *part* of it
//! (a `kid`, a `sub`, a scheme) is more likely to.
//!
//! **What this does not reach.** The router is driven with `oneshot`, so there is no hyper
//! connection and none of that stack's own logging. The capture is scoped to the calling thread, so
//! a line written from the blocking pool (the query execution path) is not read - a refusal at a gate
//! happens before that pool is reached. And the agent route is a fake mount: a verified token reaches
//! a transport that echoes nothing by construction, so what is held for it is leg 1's refusals.

use core::fmt::Write as _;

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode};
use sutura_config::Environment;
use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};

use super::{TOKEN, app, settings};
use crate::testing::{
    A_QUESTION, accepted_by, an_issuer, broker, bundle, declared_inbound, direct_overlay, fake_warehouse, request, settings_with,
};

/// Distinctive enough that nothing else in a response or a log can contain it by chance.
const CANARY: &str = "echo-canary-9c41d7e2";

/// What one request was answered with, and what was logged while it was.
struct Observed {
    status: StatusCode,
    /// The status line, every header as `name: value`, and the body.
    received: String,
    logged: String,
}

impl Observed {
    /// Every string of `secrets` that is somewhere in what was sent back or logged.
    fn leaks<'a>(&self, secrets: &'a [String]) -> Vec<&'a str> {
        secrets
            .iter()
            .map(String::as_str)
            .filter(|secret| self.received.contains(secret) || self.logged.contains(secret))
            .collect()
    }
}

/// One request through `app`, with a subscriber over a buffer at `trace` installed for it.
///
/// A `Runtime` built here for the reason `harness::fixtures::captured` gives: `with_default` is scoped
/// to the calling thread, and `block_on` drives the future on it. The router is built by the caller,
/// outside the scope, so startup announcements are not in the buffer.
fn observe(app: &Router, request: Request) -> Observed {
    use tower::ServiceExt as _;

    let sink = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("trace").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime builds");
    let (status, received) = tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            let response = app
                .clone()
                .oneshot(request)
                .await
                .expect("the router is infallible as a service");
            let (parts, body) = response.into_parts();
            let bytes = axum::body::to_bytes(body, 1 << 20)
                .await
                .expect("the test response body is readable");
            let mut received = format!("{}\n", parts.status);
            for (name, value) in &parts.headers {
                writeln!(received, "{name}: {}", String::from_utf8_lossy(value.as_bytes()))
                    .expect("writing to a String cannot fail");
            }
            received.push_str(&String::from_utf8_lossy(&bytes));
            (parts.status, received)
        })
    });
    Observed {
        status,
        received,
        logged: sink.contents(),
    }
}

/// `request` with `authorization` set to `value`, whatever scheme that is.
fn presenting(mut request: Request, value: &str) -> Request {
    let _previous = request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(value).expect("a test credential is a header value"),
    );
    request
}

fn get(path: &str) -> Request {
    request("GET", path, None, Body::empty())
}

fn ask() -> Request {
    request("POST", "/v1/query", None, Body::from(A_QUESTION))
}

/// A token and the strings that must not come back: the whole of it, and each part of it a reader
/// could lift out, because the three segments of a compact JWT are each worth a log line on their own.
fn secrets_of(token: &str, canary: bool) -> Vec<String> {
    let mut secrets = vec![String::from(token)];
    secrets.extend(token.split('.').filter(|part| part.len() >= 10).map(String::from));
    if canary {
        secrets.push(String::from(CANARY));
    }
    secrets
}

// ------------------------------------------------------------ the deployment token ----

/// The routes a deployment token is asked for, and the two it is not.
fn gated_by_the_deployment_token() -> [(&'static str, Request); 3] {
    [
        ("GET /v1/catalog", get("/v1/catalog")),
        ("POST /v1/query", ask()),
        ("GET /openapi.json", get("/openapi.json")),
    ]
}

fn open_to_anybody() -> [(&'static str, Request); 2] {
    [("GET /health", get("/health")), ("GET /metrics", get("/metrics"))]
}

#[test]
fn a_deployment_token_is_not_echoed_whether_it_is_refused_or_accepted() {
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));
    let wrong = format!("{CANARY}-{}", "w".repeat(32));
    for (token, gated_status) in [(wrong.as_str(), StatusCode::UNAUTHORIZED), (TOKEN, StatusCode::OK)] {
        let secrets = secrets_of(token, token == wrong);
        let header = format!("Bearer {token}");
        for (route, probe) in gated_by_the_deployment_token() {
            let seen = observe(&app, presenting(probe, &header));
            assert_eq!(seen.status, gated_status, "{route}: {}", seen.received);
            assert!(
                seen.leaks(&secrets).is_empty(),
                "{route} sent back or logged the token it was presented: {:?}\n{}\n{}",
                seen.leaks(&secrets),
                seen.received,
                seen.logged
            );
            if token == wrong {
                // The capture is live: the refusal wrote its line, so an absent token is a finding
                // about what that line says and not about a buffer nothing reached.
                assert!(
                    seen.logged.contains("rejected a request with no valid bearer token"),
                    "{route} refused a wrong token and wrote nothing, so the absence proves nothing: {}",
                    seen.logged
                );
            }
        }
        // A token handed to a route that does not ask for one must not come back either.
        for (route, probe) in open_to_anybody() {
            let seen = observe(&app, presenting(probe, &header));
            assert_eq!(seen.status, StatusCode::OK, "{route}: {}", seen.received);
            assert!(
                seen.leaks(&secrets).is_empty(),
                "{route} sent back or logged the token it was presented: {:?}\n{}\n{}",
                seen.leaks(&secrets),
                seen.received,
                seen.logged
            );
        }
    }
}

// ------------------------------------------------------------- a caller's own token ----

/// A deployment that verifies `issuer`'s tokens, with the agent route mounted over a fake transport
/// when the build carries one.
fn deployment_verifying(issuer: &MockIssuer, published: &PublishedKeySet) -> Router {
    let settings = settings_with(&direct_overlay(issuer, &published.path().to_string_lossy()));
    let gate = crate::InboundGate::from_declaration(&declared_inbound(&settings)).expect("a published key set builds a gate");
    let service = crate::surface::LocalService::start(
        &crate::testing::catalog_of(bundle()),
        fake_warehouse(),
        crate::testing::sink(),
        broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    let state =
        crate::testing::state_over(std::sync::Arc::new(service), settings).with_inbound_identity(std::sync::Arc::new(gate));
    #[cfg(feature = "agent")]
    let state = state.with_agent_surface(crate::state::AgentMount::new(
        tower::service_fn(|request: axum::http::Request<Body>| {
            let _request = request;
            async { Ok::<_, std::convert::Infallible>(axum::response::Response::new(Body::empty())) }
        }),
        crate::state::SpendHeadroomPush::NoCeilingConfigured,
    ));
    crate::router(&state).expect("a leg-one deployment assembles")
}

/// What a case is called, the `Authorization` value it sends, and the strings that must not come back.
type Presented = (&'static str, String, Vec<String>);

/// Every route a caller's token is read on: the two versioned operations and, where the build has
/// one, the agent route.
fn routes_reading_a_caller() -> Vec<(&'static str, Request)> {
    let mut routes = vec![("GET /v1/catalog", get("/v1/catalog")), ("POST /v1/query", ask())];
    #[cfg(feature = "agent")]
    routes.push((
        "POST /mcp",
        request("POST", crate::constants::AGENT_MOUNT_PATH, None, Body::empty()),
    ));
    routes
}

#[test]
fn a_caller_token_this_deployment_refuses_is_not_echoed_on_any_route() {
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "token-echo").expect("the key set publishes");
    let app = deployment_verifying(&issuer, &published);
    // The same names under a key id that is the canary and a key this deployment does not hold:
    // what it refuses is a key it has never seen, by a name the caller chose.
    let unknown_key =
        MockIssuer::generating(issuer.issuer(), issuer.audience(), CANARY).expect("a mock issuer generates a key pair");
    let by_canary = || Token::for_subject(CANARY);
    let mut presented: Vec<Presented> = Vec::new();
    let mut jwt = |what: &'static str, token: String| {
        let secrets = secrets_of(&token, true);
        presented.push((what, format!("Bearer {token}"), secrets));
    };
    jwt(
        "a key id this deployment does not hold",
        unknown_key.mint(&by_canary()).expect("the issuer signs a token"),
    );
    jwt(
        "a signature from somebody else",
        issuer
            .mint_signed_by_a_stranger(&by_canary())
            .expect("the stranger signs a token"),
    );
    jwt(
        "an expired token",
        issuer
            .mint(&by_canary().expired_since(3600))
            .expect("the issuer signs a token"),
    );
    jwt(
        "another audience",
        issuer
            .mint(&by_canary().for_audience("https://another.example.com"))
            .expect("the issuer signs a token"),
    );
    jwt(
        "an ID token",
        issuer
            .mint(&by_canary().classed(Token::ID_TOKEN))
            .expect("the issuer signs a token"),
    );
    jwt(
        "no signature at all",
        issuer.mint_unsigned(&by_canary()).expect("the unsigned token assembles"),
    );
    let garbage = format!("{CANARY}.{CANARY}.{CANARY}");
    presented.push((
        "a value that is not a JWT",
        format!("Bearer {garbage}"),
        secrets_of(&garbage, true),
    ));
    presented.push(("another scheme", format!("Basic {CANARY}"), vec![String::from(CANARY)]));

    for (what, header, secrets) in &presented {
        for (route, probe) in routes_reading_a_caller() {
            let seen = observe(&app, presenting(probe, header));
            assert_eq!(
                seen.status,
                StatusCode::UNAUTHORIZED,
                "{route} accepted {what}: {}",
                seen.received
            );
            assert!(
                seen.leaks(secrets).is_empty(),
                "{route} sent back or logged {what}: {:?}\n{}\n{}",
                seen.leaks(secrets),
                seen.received,
                seen.logged
            );
            assert!(
                seen.logged.contains("no verified caller"),
                "{route} refused {what} and wrote nothing, so the absence proves nothing: {}",
                seen.logged
            );
        }
        // And a route that asks for no token, handed the same one.
        let seen = observe(&app, presenting(get("/health"), header));
        assert!(
            seen.leaks(secrets).is_empty(),
            "GET /health sent back or logged {what}: {:?}\n{}\n{}",
            seen.leaks(secrets),
            seen.received,
            seen.logged
        );
    }
}

#[test]
fn a_caller_token_this_deployment_accepts_is_not_echoed_either() {
    // A verified caller with no scope is refused the operation it asked for - `403`, not `401` - and
    // a verified caller with every scope is answered. Neither response nor either log line may carry
    // the token that established them. No canary in `sub` here: a verified subject is written to
    // the audit record on purpose, and only the token itself is what must stay out.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "token-echo-accepted").expect("the key set publishes");
    let app = deployment_verifying(&issuer, &published);
    let scopeless = issuer
        .mint(&Token::for_subject("reader@example.com"))
        .expect("the issuer signs a token");
    let granted = issuer
        .mint(&accepted_by("asker@example.com"))
        .expect("the issuer signs a token");

    for (what, token, expected) in [
        ("a caller with no scope", scopeless, StatusCode::FORBIDDEN),
        ("a caller with every scope", granted, StatusCode::OK),
    ] {
        let secrets = secrets_of(&token, false);
        let seen = observe(&app, presenting(get("/v1/catalog"), &format!("Bearer {token}")));
        assert_eq!(seen.status, expected, "{what}: {}", seen.received);
        assert!(
            seen.leaks(&secrets).is_empty(),
            "GET /v1/catalog sent back or logged the token of {what}: {:?}\n{}\n{}",
            seen.leaks(&secrets),
            seen.received,
            seen.logged
        );
    }
}
