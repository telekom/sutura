//! The router, the settings and the log capture every assertion in this suite is driven through.
//!
//! **Holds no `#[test]` and no assertion**, for `just causality`'s reason - see the `federated`
//! suite's own fixtures module. It also asserts nothing about what it builds: `settings` takes its
//! overlay as raw YAML, so a mistyped key is an ABSENT setting rather than a refusal, and only the
//! parent's assertions say what was actually configured.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use sutura_config::{Environment, Settings, Sources};

use crate::testing::{bundle, call, fake_warehouse, request};

/// A token that satisfies the configured floor.
pub(super) const TOKEN: &str = "0123456789abcdef0123456789abcdef";

pub(super) fn settings(environment: Environment, overlay: &str) -> Settings {
    Settings::load(&Sources::defaults(environment).with_overlay(overlay)).expect("the test settings load")
}

/// The router, over a fake warehouse that answers every plan.
pub(super) fn app(settings: Settings) -> Router {
    over(bundle(), fake_warehouse(), settings)
}

/// The router over a named bundle and a named warehouse.
///
/// The refusal statuses need four fixtures the default pair cannot produce - a result past the row
/// cap, a result the data system will not return at once, a bundle whose models sit on two data
/// systems, and an adapter claiming to be somewhere else - and each is still driven through the REAL
/// router, which is the point of this file.
///
/// **Generic in the adapter rather than fixed to `FakeWarehouse`**, because the last of those four
/// needs an adapter whose ERROR TYPE is its own: what separates a size bound from an outage is the
/// port's predicate over that type, so a fake with a flag would let one code path pretend to be both.
/// `ServiceState` erases the surface behind a `dyn Surface`, so there is nothing downstream of here
/// for the parameter to reach.
pub(super) fn over<W>(
    pinned: sutura_domain::pinned::PinnedDefinitions,
    warehouses: sutura_app::Warehouses<W>,
    settings: Settings,
) -> Router
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    crate::testing::serving(pinned, warehouses, crate::testing::broker(), settings, None)
}

// -------------------------------------------------------------------- health ----

/// One question, and the three things a refused caller must be given.
///
/// Returns the status, the `code` and the `detail`, parsed out of the body rather than matched as a
/// substring: the assertion is about the contract, and a `detail` that happened to contain the word
/// `code` would satisfy a substring check.
pub(super) async fn refusal(app: &Router, question: &str) -> (StatusCode, String, String) {
    let (status, body) = call(app, request("POST", "/v1/query", None, Body::from(String::from(question)))).await;
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("a refusal is JSON");
    assert_eq!(parsed["outcome"], "refusal", "not a refusal: {body}");
    let code = parsed["reason"]["code"]
        .as_str()
        .expect("a refusal carries a code")
        .to_owned();
    let detail = parsed["reason"]["detail"]
        .as_str()
        .expect("a refusal carries a sentence")
        .to_owned();
    // The status is in the body as well as on the response, and the two must not disagree - a client
    // that logged only the body has nothing else to go on.
    assert_eq!(
        parsed["reason"]["status"].as_u64(),
        Some(u64::from(status.as_u16())),
        "{body}"
    );
    assert!(!detail.is_empty(), "{code} refused with no sentence");
    (status, code, detail)
}

/// Runs one request through `app` with a subscriber over a buffer, and returns what was written.
///
/// A `Runtime` built here rather than `#[tokio::test]`, and that is load-bearing:
/// `tracing::subscriber::with_default` is scoped to the calling thread, and `block_on` drives the
/// future on the calling thread - so the whole request is inside the scope. An ambient
/// `#[tokio::test]` runtime would leave the dispatcher and the future on two different threads.
///
/// The router is built by the CALLER, outside the scope, so the buffer holds the request's lines and
/// not the startup announcements. That is what lets the sentinel assertion below be about the whole
/// buffer.
pub(super) fn captured(format: sutura_config::LogFormat, app: &Router, request: Request<Body>) -> (StatusCode, String, String) {
    let sink = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        // The CONFIGURED default. Using `trace` here would enable the very span whose absence at
        // `info` is the defect, and the test would pass over the bug.
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        format,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime builds");
    let (status, body) = tracing::subscriber::with_default(subscriber, || runtime.block_on(call(app, request)));
    (status, body, sink.contents())
}

/// The lines of a captured buffer that are JSON objects, parsed.
pub(super) fn json_lines(rendered: &str) -> Vec<serde_json::Value> {
    rendered
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect()
}

/// Would this surface read its own correlation id back?
///
/// The span field is text by the time it is in the log, so this is the only way to check it is a
/// value the type would accept - which is what stops the span from carrying something the header
/// path would refuse.
pub(super) fn is_a_correlation_id(raw: &str) -> bool {
    crate::correlation::CorrelationId::parse(raw).is_ok()
}
