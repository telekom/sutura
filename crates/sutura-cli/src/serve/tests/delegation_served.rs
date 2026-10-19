//! The delegation exchange behind the served HTTP surface: a caller verified by the REAL leg-1
//! gate, the exchange `build_broker` composes called with that caller's own token, and the
//! `BigQuery` adapter handing the exchanged token to its transport (`telekom/sutura#1230`).
//!
//! In-process rather than on a spawned binary, and that is forced: a served `bigquery` deployment
//! opens the ADBC driver at boot and this venue has none, so the transport here is a recording
//! fake behind the real `BigQueryWarehouse`. What reaches it is the adapter's `JobIdentity` - the
//! credential the ADBC transport turns into its workload-identity document
//! (`crates/sutura-exec-bigquery/src/adbc/subject.rs`, held by that module's own cells).
//!
//! **What this does not show:** the driver, Google's token service, or any pool accepting the
//! exchanged token. The identity provider is a loopback fake answering what it is scripted to.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use sutura_config::{Environment, Settings, Sources};
use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};
use sutura_domain::model::SourceName;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::ResultBatches;
use sutura_exec_bigquery::transport::{DatasetAddress, DryRunEstimate, HeldTables, JobIdentity, JobRequest, JobTransport};
use sutura_http_client::test_support::{FakeServer, Scripted};

use super::support::{accepted_by, bundle_with_an_unanchored_metric, catalog_of, direct_overlay};

/// The two verified callers - two, so a source handed the wrong caller's token is visible from here.
const CALLERS: [&str; 2] = ["analyst-a@example.com", "analyst-b@example.com"];

/// The audience the exchanged token is asked for and carries.
const POOL: &str = "pool-client-id";

/// What the identity provider issues `subject` for [`POOL`]: a signed token from an issuer this
/// deployment does not trust for leg 1, so it can never be mistaken for the caller's own.
fn exchanged(subject: &str) -> String {
    MockIssuer::generating("https://idp.example.com", POOL, "the-idp-key")
        .expect("a mock issuer generates a key pair")
        .mint(&Token::for_subject(subject))
        .expect("the identity provider signs a token")
}

/// The request bodies the identity provider answered, in order.
fn offered(idp: FakeServer) -> Vec<String> {
    idp.finish().iter().map(|request| request.body().to_owned()).collect()
}

/// The RFC 8693 token response carrying `token`.
fn issued(token: &str) -> serde_json::Value {
    serde_json::json!({
        "access_token": token,
        "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
        "token_type": "Bearer",
        "expires_in": 300,
    })
}

/// The client secret file, removed when the cell ends.
struct SecretFile(std::path::PathBuf);

impl Drop for SecretFile {
    fn drop(&mut self) {
        drop(std::fs::remove_file(&self.0));
    }
}

/// What one job handed the transport, as the transport saw it.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    /// The deployment's own identity.
    Transport,
    /// A subject's own credential.
    Subject { assertion: String },
    /// A verified address, through domain-wide delegation.
    Address { address: String },
}

/// Never reached by a caller: the transport records the job and declines it.
#[derive(Debug, thiserror::Error)]
#[error("the recording transport answers no job")]
struct Declined;

/// A `JobTransport` that sends the identity of every job it is handed to the cell, and answers none.
struct Recording(Sender<Seen>);

impl Recording {
    fn record(&self, request: &JobRequest<'_>) {
        let seen = match request.identity() {
            JobIdentity::Transport => Seen::Transport,
            JobIdentity::AsSubject { assertion } => {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "the cell asserts which credential reached the transport"
                )]
                let assertion = String::from(assertion.expose_secret());
                Seen::Subject { assertion }
            }
            JobIdentity::AsAddress { address } => Seen::Address {
                address: String::from(address.as_str()),
            },
        };
        self.0.send(seen).expect("the cell holds the receiving end");
    }
}

impl JobTransport for Recording {
    type Error = Declined;

    fn run(&self, request: &JobRequest<'_>) -> Result<ResultBatches, Self::Error> {
        self.record(request);
        Err(Declined)
    }

    fn validate(&self, request: &JobRequest<'_>) -> Result<DryRunEstimate, Self::Error> {
        self.record(request);
        Ok(None)
    }

    fn list_tables(&self, _at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
        Err(Declined)
    }

    fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(Declined)
    }
}

/// The served router over one delegating `bigquery` source, its identity provider at `idp`, and what reached
/// its transport.
struct Served {
    app: axum::Router,
    issuer: MockIssuer,
    transport: Receiver<Seen>,
    _published: PublishedKeySet,
    _secret: SecretFile,
}

fn served(case: &str, idp: &FakeServer) -> Served {
    let issuer = MockIssuer::generating("https://issuer.example.com", "https://sutura.example.com", "the-current-key")
        .expect("a mock issuer generates a key pair");
    let published = PublishedKeySet::of(&issuer, case).expect("the key set publishes");
    let secret = SecretFile(std::env::temp_dir().join(format!("sutura-{case}-client-secret-{}", std::process::id())));
    std::fs::write(&secret.0, format!("{CLIENT_SECRET}\n")).expect("the client secret file is writable");
    let overlay = format!(
        "{}  identity: \"multi-user\"\nsources:\n{}      delegation:\n        - token_endpoint: \"{}/token\"\n          client_id: \"sutura\"\n          \
         client_secret_file: \"{}\"\n          audience: \"{POOL}\"\n",
        direct_overlay(&issuer, &published.path().to_string_lossy()),
        super::bigquery_entry("warehouse", "impersonation-at-source", super::wif()),
        idp.endpoint(),
        secret.0.display(),
    );
    // `/mcp` beside `/v1` wherever the transport is linked, so a cell can ask over both.
    #[cfg(feature = "agent")]
    let overlay = format!("{overlay}server:\n  agent_surface:\n    enabled: true\n");
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay)).expect("the overlay loads");
    // The composition root's own broker, built from the settings it would read.
    let broker = super::super::broker::build_broker(settings.sources(), None).expect("a direct deployment admits a delegation");
    let (recording, transport) = channel();
    let warehouse = sutura_exec_bigquery::BigQueryWarehouse::new(
        SourceName::parse("warehouse").expect("a test source is a source"),
        SourcePosture::ImpersonationAtSource,
        sutura_exec_bigquery::JobBilling::CallerRun { caller_may_name: false },
        Recording(recording),
    );
    let service: Arc<dyn sutura_app::surface::Surface> = Arc::new(
        sutura_app::surface::LocalService::start(
            &catalog_of(bundle_with_an_unanchored_metric("warehouse")),
            sutura_app::Warehouses::of(warehouse),
            sutura_runtime::TracingAuditSink::new(),
            broker,
            sutura_domain::plan::RefusingCombiner,
            1 << 30,
        )
        .expect("the test bundle validates"),
    );
    let gate = sutura_http::InboundGate::from_declaration(settings.security().inbound().expect("a direct deployment"))
        .expect("a published key set builds a gate");
    let admission = sutura_runtime::Admission::from_settings(settings.runtime());
    let state = sutura_http::ServiceState::new(service, Arc::new(settings), admission);
    #[cfg(feature = "agent")]
    let state = {
        let mount = super::super::agent_mount(&state)
            .expect("the agent mount builds")
            .expect("the overlay enabled the agent surface");
        state.with_agent_surface(mount)
    };
    let state = state.with_inbound_identity(Arc::new(gate));
    Served {
        app: sutura_http::router(&state).expect("the test router assembles"),
        issuer,
        transport,
        _published: published,
        _secret: secret,
    }
}

/// One `/v1/query` for the bundle's metric, carrying `token`: the status and the body.
async fn ask(app: axum::Router, token: &str) -> (axum::http::StatusCode, String) {
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt as _;

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/query")
        .header(axum::http::header::HOST, "localhost")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            r#"{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
        ))
        .expect("a well-formed request builds");
    let response = app.oneshot(request).await.expect("a tower service's Error is Infallible");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the response body reads");
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn a_served_callers_own_token_is_exchanged_and_the_source_is_handed_what_came_back() {
    let exchanges = CALLERS.map(exchanged);
    let idp = FakeServer::start(exchanges.iter().map(|token| Scripted::ok(&issued(token))).collect());
    let Served {
        app, issuer, transport, ..
    } = served("delegation-served-exchanges", &idp);

    let mut tokens = Vec::new();
    for (subject, exchanged) in CALLERS.iter().zip(&exchanges) {
        let token = issuer.mint(&accepted_by(subject)).expect("the issuer signs a token");
        let (status, body) = ask(app.clone(), &token).await;
        let seen: Vec<Seen> = transport.try_iter().collect();
        assert!(
            !seen.is_empty(),
            "{subject}'s question must reach the source ({status}: {body})"
        );
        for job in &seen {
            assert_eq!(
                job,
                &Seen::Subject {
                    assertion: exchanged.clone()
                },
                "every job for {subject} must carry the token exchanged for {subject} - never another \
                 caller's, the caller's own token or the deployment's identity"
            );
        }
        tokens.push(token);
    }
    let offered = offered(idp);
    assert_eq!(offered.len(), CALLERS.len(), "one question is one exchange: {offered:?}");
    for (offer, token) in offered.iter().zip(&tokens) {
        assert!(
            offer.contains(&format!("subject_token={token}")),
            "each exchange must offer that caller's own verified token: {offer}"
        );
    }
}

#[tokio::test]
async fn a_refused_exchange_answers_identity_unavailable_and_reaches_no_source() {
    let idp = FakeServer::start(vec![Scripted::status(400, r#"{"error":"invalid_grant"}"#)]);
    let Served {
        app, issuer, transport, ..
    } = served("delegation-served-refused", &idp);
    let [subject, _] = CALLERS;
    let token = issuer.mint(&accepted_by(subject)).expect("the issuer signs a token");

    let (status, body) = ask(app, &token).await;

    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("identity_unavailable"), "{body}");
    assert!(
        !body.contains(&token),
        "the refusal must not carry the caller's token: {body}"
    );
    assert_eq!(
        transport.try_iter().collect::<Vec<Seen>>(),
        Vec::new(),
        "a refused exchange must reach the source under no identity - not the caller's, not the deployment's"
    );
    let offered = offered(idp);
    let [offer] = offered.as_slice() else {
        panic!("one question is one exchange: {offered:?}");
    };
    assert!(
        offer.contains(&format!("subject_token={token}")),
        "the identity provider must have been offered the caller's own verified token: {offer}"
    );
}

/// What [`logged`] writes at `trace` from the blocking pool, to prove the capture sees that level there.
const CAPTURE_PROBE: &str = "outbound-echo capture is live";

/// The delegation client's secret, as [`served`] writes it to the secret file: distinct enough to
/// find in any text, and long enough for [`RUN`].
const CLIENT_SECRET: &str = "e83b5d0c71f9a26d4c0be915";

/// The fewest characters of a secret in a row that count as finding it: the HTTP client's wire
/// trace writes 16 bytes a line, so a secret of 23 or more characters leaves this many in one line
/// wherever the rows cut it.
const RUN: usize = 12;

/// Every line this process logs from the first call on, at `trace`, from any thread, through
/// `tracing` or through `log` over the service's own bridge.
///
/// Global, unlike `sutura-http`'s token-echo capture, because the exchange runs on the blocking pool
/// and a subscriber scoped to the calling thread does not see that pool. Once per process: where
/// cells share one, each reads every other's lines too, which only adds to what an absence is
/// checked against. The subscriber reads `RUST_LOG` before `trace`, so the cell refuses to read a
/// capture its probe did not reach.
fn logged() -> &'static sutura_runtime::testing::Capture {
    static CAPTURE: std::sync::OnceLock<sutura_runtime::testing::Capture> = std::sync::OnceLock::new();
    CAPTURE.get_or_init(|| {
        let sink = sutura_runtime::testing::Capture::new();
        let telemetry = sutura_config::TelemetrySettings::new(
            sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
            sutura_config::LogFilter::parse("trace").expect("a test directive is a directive"),
            sutura_config::LogFormat::Bunyan,
            true,
        );
        let subscriber =
            sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
        tracing::subscriber::set_global_default(subscriber).expect("no other subscriber is installed in this process");
        sutura_runtime::testing::bridge_log();
        sink
    })
}

/// One `/v1/query` for the bundle's metric, carrying `token`: the whole response.
async fn respond(app: axum::Router, token: &str) -> axum::response::Response {
    use axum::body::Body;
    use tower::ServiceExt as _;

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/query")
        .header(axum::http::header::HOST, "localhost")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            r#"{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
        ))
        .expect("a well-formed request builds");
    app.oneshot(request).await.expect("a tower service's Error is Infallible")
}

/// The status line, every header as `name: value`, and the body of one `/v1/query`.
async fn everything_returned(app: axum::Router, token: &str) -> String {
    use core::fmt::Write as _;

    let (parts, body) = respond(app, token).await.into_parts();
    let mut received = format!("{}\n", parts.status);
    for (name, value) in &parts.headers {
        writeln!(received, "{name}: {}", String::from_utf8_lossy(value.as_bytes())).expect("writing to a String cannot fail");
    }
    let body = axum::body::to_bytes(body, usize::MAX).await.expect("the response body reads");
    received.push_str(&String::from_utf8_lossy(&body));
    received
}

/// The status line, every header and the body of an `initialize` and then the `/mcp` `ask_metric`
/// call [`respond`] asks over `/v1`.
#[cfg(feature = "agent")]
async fn everything_returned_over_mcp(app: axum::Router, token: &str) -> String {
    use core::fmt::Write as _;
    use tower::ServiceExt as _;

    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                   "clientInfo": {"name": "sutura-delegation-served", "version": "0.0.0"}}
    });
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "ask_metric", "arguments": {
            "metrics": ["recurring_revenue"], "grain": "month", "range": {"start": "2026-06-01", "end": "2026-07-01"}
        }}
    });
    let mut received = String::new();
    for message in [initialize, call] {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(axum::http::header::HOST, "localhost")
            .header(axum::http::header::ACCEPT, "application/json, text/event-stream")
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
            .body(axum::body::Body::from(message.to_string()))
            .expect("a well-formed request builds");
        let (parts, body) = app
            .clone()
            .oneshot(request)
            .await
            .expect("a tower service's Error is Infallible")
            .into_parts();
        writeln!(received, "{}", parts.status).expect("writing to a String cannot fail");
        for (name, value) in &parts.headers {
            writeln!(received, "{name}: {}", String::from_utf8_lossy(value.as_bytes())).expect("writing to a String cannot fail");
        }
        let body = axum::body::to_bytes(body, usize::MAX).await.expect("the response body reads");
        received.push_str(&String::from_utf8_lossy(&body));
    }
    received
}

/// Whether `text` holds [`RUN`] characters of `secret` in a row.
fn holds_a_run_of(text: &str, secret: &str) -> bool {
    (0..=secret.len().saturating_sub(RUN)).any(|at| text.contains(secret.get(at..at + RUN).unwrap_or(secret)))
}

#[tokio::test]
async fn no_outbound_credential_reaches_a_response_or_a_log_line() {
    let log = logged();
    sutura_runtime::spawn_carrying_span(|| tracing::trace!("{CAPTURE_PROBE}"))
        .await
        .expect("the probe ran");
    assert!(
        log.contents().contains(CAPTURE_PROBE),
        "the capture did not see a `trace` line from the blocking pool, so it is not a record of everything logged: is RUST_LOG set?"
    );
    let [answered, refused] = CALLERS;
    let exchanged = exchanged(answered);
    // Each surface asks twice, and each second answer quotes what it was sent, as an identity
    // provider's error text may.
    let surfaces = if cfg!(feature = "agent") { 2 } else { 1 };
    let idp = FakeServer::start(
        (0..surfaces)
            .flat_map(|_| {
                [
                    Scripted::ok(&issued(&exchanged)),
                    Scripted::status(
                        401,
                        &format!(r#"{{"error":"invalid_client","error_description":"{CLIENT_SECRET} is not {exchanged}"}}"#),
                    ),
                ]
            })
            .collect(),
    );
    let Served {
        app, issuer, transport, ..
    } = served("delegation-served-no-echo", &idp);
    let tokens = [answered, refused].map(|subject| issuer.mint(&accepted_by(subject)).expect("the issuer signs a token"));

    let mut received = String::new();
    for token in &tokens {
        received.push_str(&everything_returned(app.clone(), token).await);
    }
    #[cfg(feature = "agent")]
    for token in &tokens {
        received.push_str(&everything_returned_over_mcp(app.clone(), token).await);
    }

    assert!(
        transport
            .try_iter()
            .any(|seen| matches!(&seen, Seen::Subject { assertion } if *assertion == exchanged)),
        "the exchanged token must have reached the source, or its absence below proves nothing: {received}"
    );
    let offered = offered(idp);
    assert!(
        offered.len() == tokens.len() * surfaces
            && offered
                .iter()
                .all(|offer| offer.contains(&format!("client_secret={CLIENT_SECRET}"))),
        "the client secret must have been sent, or its absence below proves nothing: {offered:?}"
    );
    let logged = log.contents();
    assert!(
        logged.contains(r#""log.target":"ureq::run""#),
        "the capture holds no line the HTTP client logged through `log`, so a dependency's `log` records are outside it"
    );
    let secrets = [
        ("the client secret", CLIENT_SECRET),
        ("the exchanged token", exchanged.as_str()),
    ]
    .into_iter()
    .chain(tokens.iter().map(|token| ("a caller's token", token.as_str())));
    let leaked: Vec<&str> = secrets
        .filter(|&(_, secret)| holds_a_run_of(&received, secret) || holds_a_run_of(&logged, secret))
        .map(|(name, _)| name)
        .collect();
    assert!(
        leaked.is_empty(),
        "an outbound credential was sent back or logged: {leaked:?}"
    );
}
