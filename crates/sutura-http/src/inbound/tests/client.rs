//! The OAuth client a verified token names reaches every audit line, through the real router.
//!
//! Observed on the line `sutura_runtime::TracingAuditSink` writes, because that line is what a
//! deployment's collector reads.

use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::StatusCode;
use base64::Engine as _;
use sutura_dev::issuer::MockIssuer;

use super::fixtures::{bearer, claims, direct, jwks, key_pair};
use super::gate_over;
use crate::inbound::token::{NotVerified, TokenRejected};
use crate::testing::{
    ISSUER, KID, RESOURCE, accepted_by, an_issuer, asked, broker, bundle, call, catalog_of, declared_inbound, direct_overlay,
    fake_warehouse, request, settings_with, state_over, unanchored_bundle, warehouse_that_answers_past_the_row_cap,
};

/// An audit sink that writes through the shipping sink into its own buffer, on whatever thread
/// records - the answer path records on the blocking pool, out of reach of a thread-local default.
struct AuditedLines {
    dispatch: tracing::Dispatch,
}

impl sutura_domain::audit::AuditSink for AuditedLines {
    fn record(&self, record: &sutura_domain::audit::CallRecord<'_>) {
        tracing::dispatcher::with_default(&self.dispatch, || sutura_runtime::TracingAuditSink::new().record(record));
    }
}

/// A router verifying `issuer` with `run_sql` on, whose audit lines land in the returned buffer.
fn audited_router(
    issuer: &MockIssuer,
    pinned: sutura_domain::pinned::PinnedDefinitions,
    warehouses: sutura_app::Warehouses<crate::testing::FakeWarehouse>,
) -> (axum::Router, sutura_runtime::testing::Capture) {
    let capture = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, capture.clone()).expect("a valid directive builds a subscriber");
    let sink = AuditedLines {
        dispatch: tracing::Dispatch::new(subscriber),
    };
    let settings = settings_with(&format!(
        "{}tools:\n  run_sql:\n    enabled: true\n",
        direct_overlay(issuer, "/unread.json")
    ));
    let gate = gate_over(&declared_inbound(&settings), &issuer.key_set());
    let service = crate::surface::LocalService::start(
        &catalog_of(pinned),
        warehouses,
        sink,
        broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    let state = state_over(Arc::new(service), settings).with_inbound_identity(Arc::new(gate));
    (crate::router(&state).expect("the test router assembles"), capture)
}

/// The `azp` and `client_id` of each audit line in `capture`, after the line's message.
fn client_on_each(capture: &sutura_runtime::testing::Capture) -> Vec<[String; 3]> {
    capture
        .contents()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| line.get("subject_established").is_some())
        .map(|line| {
            let field = |key: &str| line[key].as_str().unwrap_or("<absent>").to_owned();
            [field("msg"), field("azp"), field("client_id")]
        })
        .collect()
}

/// A token for every scope whose extra claims are `claims`.
fn a_token_naming(issuer: &MockIssuer, claims: &[(&str, serde_json::Value)]) -> String {
    let token = claims
        .iter()
        .fold(accepted_by("someone@example.com"), |token, (name, value)| {
            token.claiming(name, value.clone())
        });
    issuer.mint(&token).expect("the issuer signs a token")
}

/// One audit line's message, `azp` and `client_id`.
fn audit_line(message: &str, azp: &str, client_id: &str) -> [String; 3] {
    [message, azp, client_id].map(String::from)
}

#[tokio::test]
async fn the_client_a_token_names_is_on_every_audit_line() {
    let issuer = an_issuer();
    let token = a_token_naming(&issuer, &[("azp", "agent-app".into())]);
    let (app, capture) = audited_router(&issuer, bundle(), fake_warehouse());
    assert_eq!(asked(&app, "POST", "/v1/query", Some(&token)).await.status, StatusCode::OK);
    for statement in ["select 1", "refuse me"] {
        let body = Body::from(format!(r#"{{"statement":{statement:?}}}"#));
        drop(call(&app, request("POST", "/v1/sql/run", Some(&token), body)).await);
    }
    let (refusing, refused) = audited_router(&issuer, unanchored_bundle(), warehouse_that_answers_past_the_row_cap());
    drop(asked(&refusing, "POST", "/v1/query", Some(&token)).await);

    let mut lines = client_on_each(&capture);
    lines.extend(client_on_each(&refused));
    let expected = ["answered", "raw_answered", "raw_refused", "refused"].map(|message| audit_line(message, "agent-app", ""));
    assert_eq!(lines, expected);
}

#[tokio::test]
async fn a_client_id_claim_is_audited_and_two_that_disagree_are_both_kept() {
    let issuer = an_issuer();
    let (app, capture) = audited_router(&issuer, bundle(), fake_warehouse());
    for claims in [
        vec![("client_id", "agent-app".into())],
        vec![("azp", "agent-app".into()), ("client_id", "another-app".into())],
    ] {
        let token = a_token_naming(&issuer, &claims);
        assert_eq!(asked(&app, "POST", "/v1/query", Some(&token)).await.status, StatusCode::OK);
    }
    assert_eq!(
        client_on_each(&capture),
        [
            audit_line("answered", "", "agent-app"),
            audit_line("answered", "agent-app", "another-app")
        ]
    );
}

#[tokio::test]
async fn an_unreadable_client_claim_is_audited_as_unreadable_and_still_answered() {
    let issuer = an_issuer();
    let (app, capture) = audited_router(&issuer, bundle(), fake_warehouse());
    let unreadable = [
        ("azp", serde_json::Value::from(42)),
        ("azp", "agent\u{7}app".into()),
        ("azp", "a".repeat(257).into()),
        ("azp", "agent\u{202e}app".into()),
        ("client_id", 42.into()),
    ];
    for (claim, value) in unreadable {
        let token = a_token_naming(&issuer, &[(claim, value)]);
        assert_eq!(asked(&app, "POST", "/v1/query", Some(&token)).await.status, StatusCode::OK);
    }
    let on_azp = audit_line("answered", "unreadable", "");
    assert_eq!(
        client_on_each(&capture),
        [
            on_azp.clone(),
            on_azp.clone(),
            on_azp.clone(),
            on_azp,
            audit_line("answered", "", "unreadable")
        ]
    );
}

/// RFC 7519 section 4 lets a parser reject a claim set that repeats a name; this one does for
/// `client_id`, as it does for `azp`. Signed by hand, because a JSON object built here cannot repeat a key.
#[tokio::test]
async fn a_token_that_repeats_its_client_id_claim_is_refused() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let once = claims("someone@example.com", RESOURCE, ISSUER, "").to_string();
    let twice = format!(
        r#"{},"client_id":"agent-app","client_id":"another-app"}}"#,
        once.trim_end_matches('}')
    );
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some(KID.into());
    header.typ = Some(sutura_config::TokenType::ACCESS_TOKEN.into());
    let encoded = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let message = format!(
        "{}.{}",
        encoded(&serde_json::to_vec(&header).expect("a header serializes")),
        encoded(twice.as_bytes())
    );
    let key = jsonwebtoken::EncodingKey::from_ec_der(&pair.serialize_der());
    let signature =
        jsonwebtoken::crypto::sign(message.as_bytes(), &key, jsonwebtoken::Algorithm::ES256).expect("a test token signs");
    let result = gate
        .establish(&bearer(&format!("{message}.{signature}")), Instant::now())
        .await;
    assert!(
        matches!(result, Err(TokenRejected::NotVerified(NotVerified::Json))),
        "{result:?}"
    );
}
