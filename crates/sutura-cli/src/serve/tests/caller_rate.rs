//! Signed callers through the composed catalog, MCP and Flight routes. The empty Flight stream
//! reaches authentication, capability checks and framing; no remote leg executes. Fresh peer IPs
//! isolate the caller quota here; valid callers behind one IP still share the outer network cap.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, Version};
use axum::response::Response;
use sutura_config::{Environment, Settings, Sources};
use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};
use tower::ServiceExt as _;

use crate::serve::refresh::tests::FakeSurface;

fn direct_entry(issuer: &MockIssuer, keys: &PublishedKeySet, id: &str, path: &str) -> String {
    format!(
        "    - id: '{id}'\n      path: '{path}'\n      mode: direct\n      \
         resource: '{}'\n      authorization_server: '{}'\n      \
         key_set_file: '{}'\n      algorithms: [ES256]\n",
        issuer.audience(),
        issuer.issuer(),
        keys.path().display(),
    )
}

fn request(path: &str, token: &str, peer: u8) -> axum::http::request::Builder {
    Request::builder()
        .uri(path)
        .header("host", "sutura.example.com")
        .header("authorization", format!("Bearer {token}"))
        .extension(ConnectInfo(std::net::SocketAddr::from(([203, 0, 113, peer], 40000))))
}

fn initialize(path: &str, token: &str, peer: u8) -> Request<Body> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "sutura-caller-rate-test", "version": "0.0.0"}
        }
    });
    request(path, token, peer)
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&body).expect("a fixture initialize serializes"),
        ))
        .expect("a fixture MCP request builds")
}

fn flight(token: &str, peer: u8) -> Request<Body> {
    request("/arrow.flight.protocol.FlightService/DoExchange", token, peer)
        .method("POST")
        .version(Version::HTTP_2)
        .header("content-type", "application/grpc")
        .body(Body::empty())
        .expect("a fixture Flight request builds")
}

async fn send(app: &axum::Router, request: Request<Body>) -> Response {
    app.clone().oneshot(request).await.expect("the composed router is infallible")
}

async fn json(response: Response) -> serde_json::Value {
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1 << 16)
        .await
        .expect("a bounded fixture response reads");
    serde_json::from_slice(&body).expect("the real transport answered JSON")
}

#[tokio::test]
async fn signed_verifier_subject_quotas_span_catalog_mcp_and_flight() {
    let first = MockIssuer::generating("https://first.example.com", "https://sutura.example.com/a/mcp", "first-key")
        .expect("the first issuer generates a key");
    let second = MockIssuer::generating("https://second.example.com", "https://sutura.example.com/b/mcp", "second-key")
        .expect("the second issuer generates a key");
    let first_keys = PublishedKeySet::of(&first, "caller-rate-first").expect("the first key set publishes");
    let second_keys = PublishedKeySet::of(&second, "caller-rate-second").expect("the second key set publishes");
    let overlay = format!(
        "server:\n  agent_surface:\n    enabled: true\n  flight_surface:\n    enabled: true\n\
         rate_limit:\n  enabled: true\n  api_per_second: 1\n  api_burst: 2\n\
         security:\n  inbound:\n{}{}",
        direct_entry(&first, &first_keys, "first", "/a/mcp"),
        direct_entry(&second, &second_keys, "second", "/b/mcp"),
    );
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect("the two named direct verifiers and both transports load");
    let gate = super::super::inbound_gate(&settings)
        .expect("the production constructor reads both published key sets")
        .expect("the fixture declares inbound identity");
    let admission = sutura_runtime::Admission::from_settings(settings.runtime());
    let surface = FakeSurface::serving(super::support::bundle_over(&[]), false);
    let mut state = sutura_http::ServiceState::new(surface, Arc::new(settings), admission).with_inbound_identity(Arc::new(gate));
    for mount in super::super::agent_mounts(&state).expect("the composition root builds both real MCP mounts") {
        state = state.with_agent_surface(mount);
    }
    let flight_mount = sutura_http::flight::mount(&state);
    let app = sutura_http::router(&state.with_flight_surface(flight_mount)).expect("one governed router assembles");
    let scopes = sutura_app::Capability::every()
        .map(sutura_app::Capability::scope)
        .collect::<Vec<&str>>()
        .join(" ");
    let claims = Token::for_subject("user@example.com")
        .claiming("email", serde_json::json!("user@example.com"))
        .claiming("email_verified", serde_json::json!(true))
        .granting(&scopes);
    let first_token = first
        .mint(&claims)
        .expect("the first issuer signs the shared subject and address");
    let second_token = second
        .mint(&claims)
        .expect("the second issuer signs the same subject and address");

    // All setup and signing precede governor's real-clock sequence: one pair has two admissions.
    let catalog = request("/v1/catalog", &first_token, 1)
        .body(Body::empty())
        .expect("a fixture catalog request builds");
    let catalog = json(send(&app, catalog).await).await;
    assert_eq!(catalog["provenance"]["definition_version"], "test-1");
    assert_eq!(catalog["metrics"], serde_json::json!([]));
    let initialized = json(send(&app, initialize("/a/mcp", &first_token, 2)).await).await;
    assert_eq!(initialized["jsonrpc"], "2.0");
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    assert!(initialized.get("error").is_none(), "{initialized}");
    let exhausted = send(&app, flight(&first_token, 3)).await;
    assert_eq!(exhausted.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        to_bytes(exhausted.into_body(), 1 << 16)
            .await
            .expect("the Flight refusal reads")
            .is_empty()
    );
    assert_eq!(
        send(&app, initialize("/a/mcp", &first_token, 4)).await.status(),
        StatusCode::TOO_MANY_REQUESTS,
    );

    let initialized = json(send(&app, initialize("/b/mcp", &second_token, 5)).await).await;
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    assert!(initialized.get("error").is_none(), "{initialized}");
    let admitted = send(&app, flight(&second_token, 6)).await;
    assert_eq!(admitted.status(), StatusCode::OK);
    assert_eq!(
        admitted
            .headers()
            .get("grpc-status")
            .expect("the real Flight service answers a status"),
        "3"
    );
    assert_eq!(
        admitted
            .headers()
            .get("grpc-message")
            .expect("the framing error names its cause"),
        "the%20remote%20leg%20request%20is%20empty",
    );
    let catalog = request("/v1/catalog", &second_token, 7)
        .body(Body::empty())
        .expect("a fixture catalog request builds");
    assert_eq!(send(&app, catalog).await.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        send(&app, initialize("/b/mcp", &second_token, 8)).await.status(),
        StatusCode::TOO_MANY_REQUESTS,
    );
}
