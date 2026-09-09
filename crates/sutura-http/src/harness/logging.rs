//! Deployment-token and request-timeout warnings carry the bounded route template, never the raw request path.

use super::*;

fn warning(rendered: &str, message: &str) -> serde_json::Value {
    json_lines(rendered)
        .into_iter()
        .find(|line| line["msg"].as_str().is_some_and(|value| value.contains(message)))
        .unwrap_or_else(|| panic!("the `{message}` warning was not written: {rendered}"))
}

#[test]
fn a_rejected_document_path_does_not_reach_the_warning() {
    const SENTINEL: &str = "CALLER-SECRET";
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));
    let path = format!("/docs/{SENTINEL}");
    let (status, body, rendered) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("GET", &path, None, Body::empty()),
    );

    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let rejected = warning(&rendered, "rejected a request with no valid bearer token");
    assert_eq!(rejected["presented"], false, "{rejected}");
    assert_eq!(rejected["route"], "/docs/{*rest}", "{rejected}");
    assert!(rejected.get("path").is_none(), "the warning carried the raw path: {rejected}");
    assert!(
        !rendered.contains(SENTINEL),
        "a caller-chosen path reached the log: {rendered}"
    );
}

#[test]
fn a_timed_out_request_does_not_put_its_raw_path_on_the_warning() {
    let configured = settings(Environment::Development, "server:\n  request_timeout_seconds: 1\n");
    let (engine, held) = warehouse_that_can_be_held();
    let service = LocalService::start(&catalog_of(bundle()), engine, sink(), crate::testing::broker(), 1 << 30)
        .expect("the test bundle validates");
    let app = crate::router(&crate::testing::state_over(Arc::new(service), configured)).expect("the test router assembles");
    held.arm();

    // Kept open until the hold is released: dropping a Tokio runtime waits for its blocking pool.
    let capture = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, capture.clone()).expect("a valid directive builds a subscriber");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime builds");
    let (status, body) = tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(call(
            &app,
            request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
        ))
    });
    held.release();
    drop(runtime);
    let rendered = capture.contents();

    assert_eq!(status, StatusCode::REQUEST_TIMEOUT, "{body}");
    let timed_out = warning(&rendered, "gave up on a request that exceeded the configured bound");
    assert_eq!(timed_out["timeout_seconds"], 1, "{timed_out}");
    assert_eq!(timed_out["route"], "/v1/query", "{timed_out}");
    assert!(
        timed_out.get("path").is_none(),
        "the warning carried the raw path: {timed_out}"
    );
}
