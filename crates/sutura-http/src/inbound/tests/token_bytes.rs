//! Configured token bounds through the real versioned router and the key-source port.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use sutura_config::{Environment, Settings, Sources};
use tower::ServiceExt as _;

use super::fixtures::{cache_over, claims, gate_over, jwks, key_pair, signed};
use crate::inbound::{InboundGate, TokenRejected};
use crate::testing::{ISSUER, KID, RESOURCE, broker, bundle, declared_inbound, fake_warehouse, serving};

const SENTINEL: &str = "token-byte-leak-sentinel";

fn overlay(mode: &str, limit: Option<usize>) -> String {
    let mode = match mode {
        "direct" => format!("    mode: direct\n    resource: '{RESOURCE}'\n    authorization_server: '{ISSUER}'\n"),
        _ => format!(
            "    mode: behind-gateway\n    transit_header: X-Transit-Proof\n    transit_issuer: '{ISSUER}'\n    transit_audience: '{RESOURCE}'\n    transit_token_type: at+jwt\n"
        ),
    };
    let limit = limit.map_or_else(String::new, |limit| format!("    max_token_bytes: {limit}\n"));
    format!("security:\n  inbound:\n{mode}{limit}    algorithms: [ES256]\n    key_set_file: /unread.json\n")
}

fn settings(mode: &str, limit: Option<usize>) -> Settings {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay(mode, limit)))
        .expect("a declared token bound loads")
}

fn app(mode: &str, limit: Option<usize>, document: &str) -> axum::Router {
    let settings = settings(mode, limit);
    let gate = gate_over(&declared_inbound(&settings), document);
    serving(bundle(), fake_warehouse(), broker(), settings, Some(gate))
}

async fn call(app: axum::Router, mode: &str, token: &str) -> (StatusCode, Option<String>, serde_json::Value) {
    let mut request = Request::builder().uri("/v1/catalog").header("host", "localhost");
    request = if mode == "direct" {
        request.header("authorization", format!("Bearer {token}"))
    } else {
        request.header("x-transit-proof", token)
    };
    let response = app
        .oneshot(request.body(Body::empty()).expect("a request builds"))
        .await
        .expect("the router is infallible");
    let status = response.status();
    let challenge = response
        .headers()
        .get("www-authenticate")
        .map(|value| value.to_str().expect("a challenge is ASCII").to_owned());
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("the response reads");
    let body = serde_json::from_slice(&bytes).expect("the route returns JSON");
    (status, challenge, body)
}

#[tokio::test]
async fn configured_token_bytes_admit_under_and_at_and_refuse_over_on_the_versioned_route() {
    let pair = key_pair();
    let document = jwks(KID, &pair);
    let now = super::fixtures::expiry_in(0);
    let extra = serde_json::json!({"scope": "sutura:catalog.read", "iat": now, "exp": now + 30,
        "padding": "p".repeat(6800)});
    let token = signed(&pair, KID, &claims("user@example.com", RESOURCE, ISSUER, &extra.to_string()));
    assert!(token.len() > 8192 && token.len() < 12288);
    for mode in ["direct", "behind-gateway"] {
        for limit in [token.len() + 1, token.len(), token.len() - 1] {
            let (status, challenge, body) = call(app(mode, Some(limit), &document), mode, &token).await;
            if token.len() <= limit {
                assert_eq!(status, StatusCode::OK, "{mode}: {body}");
            } else {
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{mode}: {body}");
                assert!(
                    challenge
                        .expect("oversize carries a challenge")
                        .contains("error=\"invalid_token\"")
                );
                assert_eq!(
                    body["detail"],
                    format!("token is {} bytes; security.inbound.max_token_bytes is {limit}", token.len())
                );
            }
        }
    }
}

#[tokio::test]
async fn absent_token_byte_setting_retains_8192_and_reports_only_sizes() {
    let pair = key_pair();
    let document = jwks(KID, &pair);
    let token = format!("{SENTINEL}{}", "x".repeat(8193 - SENTINEL.len()));
    for mode in ["direct", "behind-gateway"] {
        let (status, challenge, body) = call(app(mode, None, &document), mode, &token).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body["detail"],
            "token is 8193 bytes; security.inbound.max_token_bytes is 8192"
        );
        assert!(
            challenge
                .expect("oversize carries a challenge")
                .contains("error=\"invalid_token\"")
        );
        assert!(!body.to_string().contains(SENTINEL));
    }
}

#[test]
fn token_byte_configuration_is_nonzero_and_capped_in_both_modes() {
    for mode in ["direct", "behind-gateway"] {
        for limit in [1, 12288] {
            let _settings = settings(mode, Some(limit));
        }
        for limit in [0, 12289] {
            let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay(mode, Some(limit))))
                .expect_err("an invalid token bound cannot start");
            let sutura_config::SettingsError::InboundValue { cause } = error.reason() else {
                panic!("expected a parsed bound refusal, got {error:?}");
            };
            let error = cause.to_string();
            assert!(error.contains("security.inbound.max_token_bytes"), "{error}");
            assert!(error.contains("1 to 12288"), "{error}");
            assert!(error.contains(&limit.to_string()), "{error}");
        }
    }
}

#[tokio::test]
async fn a_configured_oversize_unknown_key_token_does_not_refresh_the_key_source() {
    let pair = key_pair();
    let token = signed(&pair, "unknown-key", &claims("user@example.com", RESOURCE, ISSUER, ""));
    let limit = token.len() - 1;
    for mode in ["direct", "behind-gateway"] {
        let declaration = declared_inbound(&settings(mode, Some(limit)));
        let now = Instant::now();
        let (keys, source) = cache_over(&[jwks(KID, &pair)], Duration::from_secs(1), now);
        let gate = InboundGate::over(&declaration, keys);
        let headers = if mode == "direct" {
            super::fixtures::bearer(&token)
        } else {
            super::fixtures::transit(&token)
        };
        let error = gate
            .establish(&headers, now + Duration::from_secs(2))
            .await
            .expect_err("oversize is refused first");
        assert!(matches!(error, TokenRejected::TooLong { .. }), "{error:?}");
        assert_eq!(source.calls.load(Ordering::SeqCst), 1, "only startup may read keys");
    }
}

#[test]
fn an_oversize_warning_contains_byte_counts_and_no_token_material() {
    let pair = key_pair();
    let app = app("direct", Some(100), &jwks(KID, &pair));
    let token = format!("{SENTINEL}{}", "z".repeat(101 - SENTINEL.len()));
    let sink = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a service name"),
        sutura_config::LogFilter::parse("info").expect("a filter"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber = sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a subscriber");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a test runtime builds");
    let (status, _, _) = tracing::subscriber::with_default(subscriber, || runtime.block_on(call(app, "direct", &token)));
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let log = sink.contents();
    assert!(
        log.contains("token is 101 bytes; security.inbound.max_token_bytes is 100"),
        "{log}"
    );
    assert!(!log.contains(SENTINEL), "{log}");
    assert!(!log.contains(&token), "{log}");
}

#[cfg(any(feature = "flight", feature = "worker"))]
async fn wire_call(app: axum::Router, mode: &str, token: &str, http2: bool) -> StatusCode {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use sutura_runtime::{Shutdown, ShutdownReason};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback listener");
    let address = listener.local_addr().expect("the bound address");
    let shutdown = Shutdown::with_grace(Duration::from_secs(1));
    let server = tokio::spawn(crate::server::run(listener, app, shutdown.clone()));
    let io = TokioIo::new(
        tokio::net::TcpStream::connect(address)
            .await
            .expect("connect to the listener"),
    );
    let mut request = Request::builder()
        .uri(format!("http://{address}/v1/catalog"))
        .header("host", "localhost");
    request = if mode == "direct" {
        request.header("authorization", format!("Bearer {token}"))
    } else {
        request.header("x-transit-proof", token)
    };
    let request = request.body(Body::empty()).expect("the wire request builds");
    let (response, connection) = if http2 {
        let (mut sender, connection) = hyper::client::conn::http2::handshake(TokioExecutor::new(), io)
            .await
            .expect("HTTP/2 handshake");
        let connection = tokio::spawn(connection);
        (sender.send_request(request).await.expect("HTTP/2 answers"), connection)
    } else {
        let (mut sender, connection) = hyper::client::conn::http1::handshake(io).await.expect("HTTP/1 handshake");
        let connection = tokio::spawn(connection);
        (sender.send_request(request).await.expect("HTTP/1 answers"), connection)
    };
    let status = response.status();
    let _body = to_bytes(Body::new(response.into_body()), 1 << 20)
        .await
        .expect("the wire response reads");
    connection.abort();
    shutdown.trigger(ShutdownReason::Terminate);
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .expect("the server drains")
        .expect("the server task returns")
        .expect("the listener stops cleanly");
    status
}

#[cfg(any(feature = "flight", feature = "worker"))]
#[tokio::test]
async fn a_larger_declared_token_bound_reaches_authentication_over_http1_and_http2() {
    let pair = key_pair();
    let now = super::fixtures::expiry_in(0);
    let extra = serde_json::json!({"scope": "sutura:catalog.read", "iat": now, "exp": now + 30,
        "padding": "p".repeat(6800)});
    let token = signed(&pair, KID, &claims("user@example.com", RESOURCE, ISSUER, &extra.to_string()));
    assert!(token.len() > 8192 && token.len() < 12288);
    let document = jwks(KID, &pair);
    for mode in ["direct", "behind-gateway"] {
        for http2 in [false, true] {
            let status = wire_call(app(mode, Some(12288), &document), mode, &token, http2).await;
            assert_eq!(status, StatusCode::OK, "{mode}, http2={http2}");
            let status = wire_call(app(mode, Some(12288), &document), mode, &"x".repeat(12289), http2).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{mode}, http2={http2}");
        }
    }
}
