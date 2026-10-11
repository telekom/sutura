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
use sutura_exec_bigquery::transport::{
    DatasetAddress, DatasetId, DryRunEstimate, HeldTables, JobIdentity, JobRequest, JobTransport, ProjectId,
};
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
    std::fs::write(&secret.0, "idp-client-secret\n").expect("the client secret file is writable");
    let overlay = format!(
        "{}  identity: \"multi-user\"\nsources:\n{}      delegation:\n        token_endpoint: \"{}/token\"\n        client_id: \"sutura\"\n        \
         client_secret_file: \"{}\"\n        audience: \"{POOL}\"\n",
        direct_overlay(&issuer, &published.path().to_string_lossy()),
        super::bigquery_entry("warehouse", "impersonation-at-source", super::wif()),
        idp.endpoint(),
        secret.0.display(),
    );
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay)).expect("the overlay loads");
    // The composition root's own broker, built from the settings it would read.
    let broker = super::super::broker::build_broker(settings.sources(), None).expect("a direct deployment admits a delegation");
    let (recording, transport) = channel();
    let warehouse = sutura_exec_bigquery::BigQueryWarehouse::new(
        SourceName::parse("warehouse").expect("a test source is a source"),
        SourcePosture::ImpersonationAtSource,
        ProjectId::parse("acme-analytics").expect("a test project is a project"),
        DatasetId::parse("warehouse").expect("a test dataset is a dataset"),
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
    let state = sutura_http::ServiceState::new(service, Arc::new(settings), admission).with_inbound_identity(Arc::new(gate));
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
