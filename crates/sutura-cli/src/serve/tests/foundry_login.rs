//! A `foundry_login` source behind the served HTTP surface: a caller verified by the REAL leg-1
//! gate, the broker `build_broker` composes asking a loopback identity provider's broker token
//! endpoint with that caller's own token, and the Foundry adapter handing the token that came back
//! to its transport.
//!
//! In-process, over a recording fake of the Foundry transport port behind the real
//! `FoundryWarehouse`. Gated on `bigquery` as well as `foundry` only to reuse `support`'s leg-1
//! fixtures unchanged; `just test` links both.
//!
//! **What this does not show:** any Foundry tenant accepting a brokered token, or any identity
//! provider storing one. The identity provider is a loopback fake answering what it is scripted to.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use sutura_config::{Environment, Settings, Sources};
use sutura_dev::issuer::{MockIssuer, PublishedKeySet};
use sutura_domain::identity::Secret;
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_exec_foundry::{FoundryTransport, FoundryWarehouse, OntologyIdentifier, SqlRequest};
use sutura_http_client::test_support::{FakeServer, Scripted};

use super::support::{accepted_by, bundle_with_a_metric, catalog_of, direct_overlay};

const CALLERS: [&str; 2] = ["analyst-a@example.com", "analyst-b@example.com"];

/// The broker client's secret, distinct enough to find in any text.
const CLIENT_SECRET: &str = "f0c1d7a9e3b54c2a8d6e71f4";

/// The opaque Foundry token the identity provider stored for `subject`.
fn stored_for(subject: &str) -> String {
    format!("foundry-user-token.{}", subject.replace(['@', '.'], "-"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs()
}

/// The Identity Brokering API's stored token response, expiring at `expires`.
fn stored(token: &str, expires: u64) -> serde_json::Value {
    serde_json::json!({
        "access_token": token,
        "token_type": "Bearer",
        "accessTokenExpiration": expires,
        "refresh_token": "never-read-refresh-token",
    })
}

/// The client secret file, removed when the cell ends.
struct SecretFile(std::path::PathBuf);

impl Drop for SecretFile {
    fn drop(&mut self) {
        drop(std::fs::remove_file(&self.0));
    }
}

/// What the Foundry fake answers every request with.
#[derive(Debug, Clone, Copy)]
enum Answer {
    Rows,
    Status(u16),
}

#[derive(Debug, thiserror::Error)]
#[error("the fake Foundry endpoint answered {0}")]
struct Status(u16);

/// Sends the bearer of every request it is handed to the cell, then answers as scripted.
#[derive(Debug)]
struct Recording {
    answer: Answer,
    seen: Sender<String>,
}

impl FoundryTransport for Recording {
    type Error = Status;

    fn execute(&self, _request: &SqlRequest, token: &Secret, _deadline: Deadline) -> Result<Vec<u8>, Self::Error> {
        #[expect(clippy::disallowed_methods, reason = "the cell asserts which token reached the transport")]
        let token = String::from(token.expose_secret());
        drop(self.seen.send(token));
        match self.answer {
            Answer::Rows => Ok(one_row()),
            Answer::Status(status) => Err(Status(status)),
        }
    }

    /// `sutura_exec_foundry::http::HttpFoundry`'s rule.
    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, Status(401 | 403))
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }
}

/// One Arrow IPC row of the bundle's metric.
fn one_row() -> Vec<u8> {
    use arrow_array::{ArrayRef, Int64Array, RecordBatch};
    use arrow_schema::{DataType, Field, Schema};

    let schema = Arc::new(Schema::new(vec![Field::new("recurring_revenue", DataType::Int64, false)]));
    let values: ArrayRef = Arc::new(Int64Array::from(vec![7_i64]));
    let batch = RecordBatch::try_new(Arc::clone(&schema), vec![values]).expect("the fixture batch is rectangular");
    let mut bytes = Vec::new();
    let mut writer = arrow_ipc::writer::StreamWriter::try_new(&mut bytes, &schema).expect("the Arrow writer opens");
    writer.write(&batch).expect("the fixture batch writes");
    writer.finish().expect("the Arrow stream finishes");
    bytes
}

/// The `sources:` entry of one `foundry_login` source at `idp`, its secret in `secret`.
fn foundry_entry(idp: &FakeServer, secret: &std::path::Path) -> String {
    format!(
        "sources:\n  warehouse:\n    kind: \"foundry\"\n    endpoint: \"https://foundry.example.com\"\n    \
         ontology: \"sales_ontology\"\n    posture: \"impersonation-at-source\"\n    foundry_login:\n      \
         token_endpoint: \"{}/realms/example/broker/foundry/token\"\n      client_id: \"sutura\"\n      \
         client_secret_file: \"{}\"\n",
        idp.endpoint(),
        secret.display(),
    )
}

struct Served {
    app: axum::Router,
    issuer: MockIssuer,
    foundry: Receiver<String>,
    _published: PublishedKeySet,
    _secret: SecretFile,
}

fn served(case: &str, idp: &FakeServer, answer: Answer) -> Served {
    let issuer = MockIssuer::generating("https://issuer.example.com", "https://sutura.example.com", "the-current-key")
        .expect("a mock issuer generates a key pair");
    let published = PublishedKeySet::of(&issuer, case).expect("the key set publishes");
    let secret = SecretFile(std::env::temp_dir().join(format!("sutura-{case}-client-secret-{}", std::process::id())));
    std::fs::write(&secret.0, format!("{CLIENT_SECRET}\n")).expect("the client secret file is writable");
    let overlay = format!(
        "{}  identity: \"multi-user\"\n{}",
        direct_overlay(&issuer, &published.path().to_string_lossy()),
        foundry_entry(idp, &secret.0),
    );
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay)).expect("the overlay loads");
    let broker =
        super::super::broker::build_broker(settings.sources(), None).expect("a direct deployment admits a foundry_login");
    let (seen, foundry) = channel();
    let warehouse = FoundryWarehouse::each_caller(
        SourceName::parse("warehouse").expect("a test source is a source"),
        OntologyIdentifier::parse("sales_ontology").expect("the ontology parses"),
        Recording { answer, seen },
        sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::MAX),
    );
    let service: Arc<dyn sutura_app::surface::Surface> = Arc::new(
        sutura_app::surface::LocalService::start(
            // One object type, unqualified: Ontology SQL names an object type by its API name.
            &catalog_of(bundle_with_a_metric("warehouse", false, "Customer")),
            sutura_app::Warehouses::of(warehouse),
            sutura_runtime::TracingAuditSink::new(),
            broker,
            sutura_domain::plan::RefusingCombiner,
            1 << 30,
        )
        .expect("the test bundle validates"),
    );
    let gate = sutura_http::InboundGate::from_declaration(
        settings
            .security()
            .inbound()
            .first()
            .and_then(sutura_config::InboundVerifier::identity)
            .expect("a direct deployment"),
    )
    .expect("a published key set builds a gate");
    let admission = sutura_runtime::Admission::from_settings(settings.runtime());
    let state = sutura_http::ServiceState::new(service, Arc::new(settings), admission).with_inbound_identity(Arc::new(gate));
    Served {
        app: sutura_http::router(&state).expect("the test router assembles"),
        issuer,
        foundry,
        _published: published,
        _secret: secret,
    }
}

/// One `/v1/query` for the bundle's metric, carrying `token` when there is one: the status and the body.
async fn ask(app: axum::Router, token: Option<&str>) -> (axum::http::StatusCode, String) {
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt as _;

    let mut request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/query")
        .header(axum::http::header::HOST, "localhost")
        .header(axum::http::header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = request
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
async fn each_caller_is_answered_by_foundry_under_their_own_brokered_token() {
    let expires = now() + 300;
    let idp = FakeServer::start(
        CALLERS
            .iter()
            .map(|subject| Scripted::ok(&stored(&stored_for(subject), expires)))
            .collect(),
    );
    let Served {
        app, issuer, foundry, ..
    } = served("foundry-login-each-caller", &idp, Answer::Rows);

    let mut tokens = Vec::new();
    for subject in CALLERS {
        let token = issuer.mint(&accepted_by(subject)).expect("the issuer signs a token");
        let (status, body) = ask(app.clone(), Some(&token)).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{subject}: {body}");
        assert!(
            body.contains(r#""executed_as":[{"source":"warehouse","posture":"impersonation-at-source"}]"#),
            "the answer must record that this leg ran as its caller: {body}"
        );
        let brokered = stored_for(subject);
        let seen: Vec<String> = foundry.try_iter().collect();
        assert!(!seen.is_empty(), "{subject}'s question must reach Foundry");
        assert!(
            seen.iter().all(|sent| *sent == brokered),
            "every Foundry request for {subject} must carry the token stored for {subject}, unchanged - \
             never another caller's, the caller's inbound token or a deployment's: {seen:?}"
        );
        for leaked in [token.as_str(), brokered.as_str(), CLIENT_SECRET] {
            assert!(!body.contains(leaked), "the answer must carry no credential: {body}");
        }
        tokens.push(token);
    }
    let offered: Vec<String> = idp.finish().iter().map(|request| request.body().to_owned()).collect();
    assert_eq!(
        offered.len(),
        CALLERS.len(),
        "one question is one broker request: {offered:?}"
    );
    for (offer, token) in offered.iter().zip(&tokens) {
        assert!(
            offer.contains(&format!("token={token}")) && offer.contains(&format!("client_secret={CLIENT_SECRET}")),
            "each broker request must offer that caller's own verified token: {offer}"
        );
    }
}

#[tokio::test]
async fn a_caller_with_no_linked_foundry_account_is_refused_and_reaches_no_source() {
    let idp = FakeServer::start(vec![Scripted::status(
        400,
        r#"{"error":"invalid_request","error_description":"User not associated to identity provider"}"#,
    )]);
    let Served {
        app, issuer, foundry, ..
    } = served("foundry-login-not-linked", &idp, Answer::Rows);
    let [subject, _] = CALLERS;
    let token = issuer.mint(&accepted_by(subject)).expect("the issuer signs a token");

    let (status, body) = ask(app, Some(&token)).await;

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("identity_not_linked"), "{body}");
    assert!(
        !body.contains(&token),
        "the refusal must not carry the caller's token: {body}"
    );
    assert_eq!(
        foundry.try_iter().count(),
        0,
        "an unlinked caller must reach Foundry under no identity - not theirs, not the deployment's"
    );
    assert_eq!(idp.finish().len(), 1);
}

#[tokio::test]
async fn an_already_expired_brokered_token_is_refused_before_any_foundry_request() {
    let stale = stored_for(CALLERS[0]);
    let idp = FakeServer::start(vec![Scripted::ok(&stored(&stale, now() - 1))]);
    let Served {
        app, issuer, foundry, ..
    } = served("foundry-login-expired", &idp, Answer::Rows);
    let token = issuer.mint(&accepted_by(CALLERS[0])).expect("the issuer signs a token");

    let (status, body) = ask(app, Some(&token)).await;

    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("identity_unavailable"), "{body}");
    assert!(
        !body.contains(&stale),
        "the refusal must not carry the brokered token: {body}"
    );
    assert_eq!(
        foundry.try_iter().count(),
        0,
        "sutura refreshes nothing and sends no expired token"
    );
    assert_eq!(idp.finish().len(), 1);
}

#[tokio::test]
async fn a_foundry_refusal_of_the_callers_token_is_a_typed_refusal_not_an_empty_answer() {
    let idp = FakeServer::start(vec![Scripted::ok(&stored(&stored_for(CALLERS[0]), now() + 300))]);
    let Served {
        app, issuer, foundry, ..
    } = served("foundry-login-forbidden", &idp, Answer::Status(403));
    let token = issuer.mint(&accepted_by(CALLERS[0])).expect("the issuer signs a token");

    let (status, body) = ask(app, Some(&token)).await;

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("source_refused"), "{body}");
    assert!(!body.contains("\"rows\""), "a refusal carries no rows: {body}");
    assert_eq!(foundry.try_iter().collect::<Vec<String>>(), [stored_for(CALLERS[0])]);
    drop(idp.finish());
}

#[tokio::test]
async fn an_anonymous_request_reaches_neither_the_identity_provider_nor_foundry() {
    let idp = FakeServer::start(Vec::new());
    let Served { app, foundry, .. } = served("foundry-login-anonymous", &idp, Answer::Rows);

    let (status, body) = ask(app, None).await;

    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(foundry.try_iter().count(), 0);
    assert_eq!(idp.finish().len(), 0, "no broker request may be made for nobody");
}

#[test]
fn an_anchor_on_a_foundry_login_source_does_not_boot() {
    let idp = FakeServer::start(Vec::new());
    let secret = std::path::Path::new("/run/secrets/idp-client-secret");
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"multi-user\"\n  inbound:\n    mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
         authorization_server: \"https://issuer.example.com\"\n    key_set_file: \"/etc/sutura/jwks.json\"\n    \
         algorithms: [\"ES256\"]\n{}",
        foundry_entry(&idp, secret)
    )))
    .expect("the overlay loads");
    let error = crate::serve::boot::refuse_unverifiable_anchors(&super::bundle_with_an_anchor("warehouse"), settings.sources())
        .expect_err("a source with no identity of its own has nothing to re-run a certified number as");
    assert!(error.contains("recurring_revenue") && error.contains("warehouse"), "{error}");
    assert_eq!(idp.finish().len(), 0);
}
