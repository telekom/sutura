//! The OAuth client a verified token names reaches every audit line, through the real router.
//!
//! Observed on the line `sutura_runtime::TracingAuditSink` writes, because that line is what a
//! deployment's collector reads.

use std::sync::Arc;

use axum::body::Body;
use axum::http::StatusCode;
use sutura_dev::issuer::MockIssuer;

use super::gate_over;
use crate::testing::{
    accepted_by, an_issuer, asked, broker, bundle, call, catalog_of, declared_inbound, direct_overlay, fake_warehouse, request,
    settings_with, state_over, unanchored_bundle, warehouse_that_answers_past_the_row_cap,
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
async fn an_unreadable_azp_is_audited_as_unreadable_and_still_answered() {
    let issuer = an_issuer();
    let (app, capture) = audited_router(&issuer, bundle(), fake_warehouse());
    for azp in [serde_json::Value::from(42), serde_json::Value::from("agent\u{7}app")] {
        let token = a_token_naming(&issuer, &[("azp", azp)]);
        assert_eq!(asked(&app, "POST", "/v1/query", Some(&token)).await.status, StatusCode::OK);
    }
    assert_eq!(
        client_on_each(&capture),
        [
            audit_line("answered", "unreadable", ""),
            audit_line("answered", "unreadable", "")
        ]
    );
}
