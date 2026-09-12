//! `/metrics`, through the assembled router.
//!
//! Its own file for the same mechanical reason `catalog_prose` has one: `harness.rs` is at the
//! `max-lines` bound. These are the tests `docs/adr/0015` asks for - the separate credential, the
//! exact series set, the governance-versus-fault split, the disclosure exclusions, and a scrape that
//! does not block on work in flight - each driven through the real router rather than a bare
//! registry, because the wiring is the half a component test cannot see.

use std::time::Duration;

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;

use super::{app, metrics_settings, over};
use crate::testing::{
    A_QUESTION, WarehouseThatFailsToExecute, bundle, call, request, source, unanchored_bundle, warehouse_that_can_be_held,
};

/// The metrics credential, distinct from the API token, for the cases that configure one.
const METRICS_TOKEN: &str = "0123456789abcdef0123456789abcdf0";

fn metrics_settings_with(server: &str, additional: &str) -> sutura_config::Settings {
    super::settings(
        Environment::Development,
        &format!(
            "security:\n  access_token: \"{}\"\n  tls_termination: \"sidecar\"\n  \
             metrics_token: \"{METRICS_TOKEN}\"\nserver:\n  host: \"0.0.0.0\"\n{server}runtime:\n  \
             engine_worker_threads: 3\n  max_concurrent_queries: 4\n{additional}",
            super::TOKEN
        ),
    )
}

async fn scrape(app: &axum::Router) -> String {
    let (status, body) = call(app, request("GET", "/metrics", Some(METRICS_TOKEN), Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn sample(exposition: &str, name: &str) -> u64 {
    let prefix = format!("{name} ");
    exposition
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("the exposition has no sample named {name}: {exposition}"))
        .parse()
        .unwrap_or_else(|cause| panic!("{name} is not an integer sample: {cause}"))
}

fn question_total(exposition: &str) -> u64 {
    exposition
        .lines()
        .filter_map(|line| line.strip_prefix("sutura_questions_total{"))
        .map(|sample| {
            sample
                .split_once(' ')
                .unwrap_or_else(|| panic!("a question sample has no value: {sample}"))
                .1
                .parse::<u64>()
                .unwrap_or_else(|cause| panic!("a question sample is not an integer: {cause}"))
        })
        .sum()
}

/// The rendered exposition of a freshly assembled router, exactly.
///
/// **The snapshot the issue asks for, and its job is review.** Every series and every label this
/// deployment can ever export is one line in here, so an added label or family is a visible diff
/// rather than a silent cardinality or disclosure change. It is deterministic because the test
/// settings pin `runtime.engine_worker_threads` and a fresh router has observed nothing.
const EXPECTED_EXPOSITION: &str = r#"# TYPE sutura_questions_total counter
sutura_questions_total{code="answer"} 0
sutura_questions_total{code="at_capacity"} 0
sutura_questions_total{code="identity_unavailable"} 0
sutura_questions_total{code="insufficient_scope"} 0
sutura_questions_total{code="internal"} 0
sutura_questions_total{code="not_a_question"} 0
sutura_questions_total{code="rate_limited"} 0
sutura_questions_total{code="refused"} 0
sutura_questions_total{code="timeout"} 0
sutura_questions_total{code="too_large"} 0
sutura_questions_total{code="unauthorized"} 0
sutura_questions_total{code="unavailable"} 0
# TYPE sutura_question_duration_seconds histogram
sutura_question_duration_seconds_bucket{le="0.01"} 0
sutura_question_duration_seconds_bucket{le="0.05"} 0
sutura_question_duration_seconds_bucket{le="0.1"} 0
sutura_question_duration_seconds_bucket{le="0.5"} 0
sutura_question_duration_seconds_bucket{le="1"} 0
sutura_question_duration_seconds_bucket{le="2"} 0
sutura_question_duration_seconds_bucket{le="5"} 0
sutura_question_duration_seconds_bucket{le="10"} 0
sutura_question_duration_seconds_bucket{le="30"} 0
sutura_question_duration_seconds_bucket{le="60"} 0
sutura_question_duration_seconds_bucket{le="+Inf"} 0
sutura_question_duration_seconds_sum 0
sutura_question_duration_seconds_count 0
# TYPE sutura_execution_slots gauge
sutura_execution_slots 4
# TYPE sutura_execution_slots_in_use gauge
sutura_execution_slots_in_use 0
# TYPE sutura_admission_shed_total counter
sutura_admission_shed_total 0
# TYPE sutura_admission_wait_seconds histogram
sutura_admission_wait_seconds_bucket{le="0.001"} 0
sutura_admission_wait_seconds_bucket{le="0.01"} 0
sutura_admission_wait_seconds_bucket{le="0.1"} 0
sutura_admission_wait_seconds_bucket{le="1"} 0
sutura_admission_wait_seconds_bucket{le="5"} 0
sutura_admission_wait_seconds_bucket{le="30"} 0
sutura_admission_wait_seconds_bucket{le="+Inf"} 0
sutura_admission_wait_seconds_sum 0
sutura_admission_wait_seconds_count 0
# TYPE sutura_rate_limited_total counter
sutura_rate_limited_total{tier="public"} 0
sutura_rate_limited_total{tier="general"} 0
sutura_rate_limited_total{tier="metrics"} 0
# TYPE sutura_rate_limit_buckets gauge
sutura_rate_limit_buckets{tier="public"} 0
sutura_rate_limit_buckets{tier="general"} 0
sutura_rate_limit_buckets{tier="metrics"} 0
# TYPE sutura_unauthorized_total counter
sutura_unauthorized_total 0
# TYPE sutura_answer_rows histogram
sutura_answer_rows_bucket{le="1"} 0
sutura_answer_rows_bucket{le="10"} 0
sutura_answer_rows_bucket{le="100"} 0
sutura_answer_rows_bucket{le="1000"} 0
sutura_answer_rows_bucket{le="10000"} 0
sutura_answer_rows_bucket{le="100000"} 0
sutura_answer_rows_bucket{le="+Inf"} 0
sutura_answer_rows_sum 0
sutura_answer_rows_count 0
# TYPE sutura_engine_worker_threads gauge
sutura_engine_worker_threads 3
# TYPE sutura_catalog_metrics gauge
sutura_catalog_metrics 1
"#;

#[tokio::test]
async fn the_metrics_token_is_separate_from_the_api_token() {
    let app = app(metrics_settings(Environment::Development));
    // No credential is refused.
    assert_eq!(
        call(&app, request("GET", "/metrics", None, Body::empty())).await.0,
        StatusCode::UNAUTHORIZED
    );
    // The ORDINARY API token is refused - this is the whole separation `docs/adr/0015` Decision 1
    // exists for. A holder of the API token can ask any question, so reusing it for a scrape is the
    // privilege escalation the separate credential prevents.
    assert_eq!(
        call(&app, request("GET", "/metrics", Some(super::TOKEN), Body::empty()))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    // A wrong credential is refused.
    assert_eq!(
        call(
            &app,
            request("GET", "/metrics", Some("0123456789abcdef0123456789abcdfF"), Body::empty())
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    // The metrics credential succeeds.
    assert_eq!(
        call(&app, request("GET", "/metrics", Some(METRICS_TOKEN), Body::empty()))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn metrics_and_non_question_routes_never_move_the_question_family() {
    let app = app(metrics_settings(Environment::Development));
    assert_eq!(
        call(&app, request("GET", "/metrics", None, Body::empty())).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, request("GET", "/v1/catalog", Some(super::TOKEN), Body::empty()))
            .await
            .0,
        StatusCode::OK
    );

    let exposition = scrape(&app).await;
    assert_eq!(question_total(&exposition), 0, "{exposition}");
    assert_eq!(
        sample(&exposition, "sutura_question_duration_seconds_count"),
        0,
        "{exposition}"
    );
    assert_eq!(sample(&exposition, "sutura_unauthorized_total"), 1, "{exposition}");
}

#[tokio::test]
async fn a_non_question_rate_limit_moves_only_the_surface_counter() {
    let app = app(metrics_settings_with(
        "",
        "rate_limit:\n  enabled: true\n  api_per_second: 1\n  api_burst: 1\n",
    ));
    assert_eq!(
        call(&app, request("GET", "/v1/catalog", Some(super::TOKEN), Body::empty()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, request("GET", "/v1/catalog", Some(super::TOKEN), Body::empty()))
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );

    let exposition = scrape(&app).await;
    assert_eq!(question_total(&exposition), 0, "{exposition}");
    assert_eq!(
        sample(&exposition, "sutura_question_duration_seconds_count"),
        0,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, r#"sutura_rate_limited_total{tier="general"}"#),
        1,
        "{exposition}"
    );
}

#[tokio::test]
async fn every_terminal_question_moves_one_code_and_one_duration() {
    let app = app(metrics_settings(Environment::Development));

    assert_eq!(
        call(&app, request("POST", "/v1/query", None, Body::from(A_QUESTION))).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, request("POST", "/v1/query", Some(super::TOKEN), Body::from("{")))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, request("POST", "/v1/query", Some(super::TOKEN), Body::from(A_QUESTION)))
            .await
            .0,
        StatusCode::OK
    );

    let exposition = scrape(&app).await;
    assert_eq!(question_total(&exposition), 3, "{exposition}");
    assert_eq!(
        sample(&exposition, r#"sutura_questions_total{code="unauthorized"}"#),
        1,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, r#"sutura_questions_total{code="not_a_question"}"#),
        1,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, r#"sutura_questions_total{code="answer"}"#),
        1,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, "sutura_question_duration_seconds_count"),
        3,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, "sutura_admission_wait_seconds_count"),
        1,
        "only the answered request reaches admission: {exposition}"
    );
    assert_eq!(sample(&exposition, "sutura_answer_rows_count"), 1, "{exposition}");
}

#[tokio::test]
async fn a_query_rejected_by_the_body_bound_is_counted_once() {
    let app = app(metrics_settings_with("  max_body_bytes: 8\n", ""));
    assert_eq!(
        call(&app, request("POST", "/v1/query", Some(super::TOKEN), Body::from(A_QUESTION)))
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );

    let exposition = scrape(&app).await;
    assert_eq!(question_total(&exposition), 1, "{exposition}");
    assert_eq!(
        sample(&exposition, r#"sutura_questions_total{code="too_large"}"#),
        1,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, "sutura_question_duration_seconds_count"),
        1,
        "{exposition}"
    );
}

#[tokio::test]
async fn a_rate_limited_query_moves_both_the_surface_and_question_counters() {
    let app = app(metrics_settings_with(
        "",
        "rate_limit:\n  enabled: true\n  api_per_second: 1\n  api_burst: 1\n",
    ));
    assert_eq!(
        call(&app, request("POST", "/v1/query", Some(super::TOKEN), Body::from(A_QUESTION)))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, request("POST", "/v1/query", Some(super::TOKEN), Body::from(A_QUESTION)))
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );

    let exposition = scrape(&app).await;
    assert_eq!(question_total(&exposition), 2, "{exposition}");
    assert_eq!(
        sample(&exposition, r#"sutura_questions_total{code="rate_limited"}"#),
        1,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, r#"sutura_rate_limited_total{tier="general"}"#),
        1,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, "sutura_question_duration_seconds_count"),
        2,
        "{exposition}"
    );
}

#[tokio::test]
async fn a_timed_out_query_is_counted_once() {
    let (engine, held) = warehouse_that_can_be_held();
    let app = over(bundle(), engine, metrics_settings_with("  request_timeout_seconds: 1\n", ""));
    held.arm();
    let (status, _) = call(&app, request("POST", "/v1/query", Some(super::TOKEN), Body::from(A_QUESTION))).await;
    held.release();
    assert_eq!(status, StatusCode::REQUEST_TIMEOUT);

    let exposition = scrape(&app).await;
    assert_eq!(question_total(&exposition), 1, "{exposition}");
    assert_eq!(
        sample(&exposition, r#"sutura_questions_total{code="timeout"}"#),
        1,
        "{exposition}"
    );
    assert_eq!(
        sample(&exposition, "sutura_question_duration_seconds_count"),
        1,
        "{exposition}"
    );
}

#[tokio::test]
async fn the_rendered_exposition_is_exactly_these_series() {
    // The exact-set test `docs/adr/0015` Decision 5 asks for as the second layer: the label TYPE
    // stops caller text, and this stops an author adding a family or a label dimension. A scrape of
    // the real router, so the deployment-wide gauges `ServiceState::new` registers are in it too.
    let app = app(metrics_settings(Environment::Development));
    let (status, body) = call(&app, request("GET", "/metrics", Some(METRICS_TOKEN), Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, EXPECTED_EXPOSITION);
}

#[tokio::test]
async fn a_refusal_and_a_fault_are_different_series() {
    // **The paging decision, and the reason this whole issue exists.** A governance refusal is a
    // correct outcome and must not page anybody; a fault must. Both are counted on the same family,
    // separated by `code`, so an operator can alert on the fault codes without silence around them.
    let app = app(metrics_settings(Environment::Development));
    // `dimension_not_permitted` is a `ToolOutcome::Refusal`: the catalog governed it.
    let (status, refused) = call(
        &app,
        request(
            "POST",
            "/v1/query",
            Some(super::TOKEN),
            Body::from(
                r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["channel"]}"#,
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert!(refused.contains(r#""outcome":"refusal""#), "{refused}");
    assert!(refused.contains(r#""code":"dimension_not_permitted""#), "{refused}");

    let faulting = over(
        unanchored_bundle(),
        WarehouseThatFailsToExecute::new(source()),
        metrics_settings(Environment::Development),
    );
    let (status, body) = call(
        &faulting,
        request("POST", "/v1/query", Some(super::TOKEN), Body::from(A_QUESTION)),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");

    let (_, body) = call(&app, request("GET", "/metrics", Some(METRICS_TOKEN), Body::empty())).await;
    assert!(body.contains(r#"sutura_questions_total{code="refused"} 1"#), "{body}");
    // A refusal did NOT page: no fault code moved, and no unauthorized attempt was invented.
    assert!(body.contains(r#"sutura_questions_total{code="unavailable"} 0"#), "{body}");
    assert!(body.contains(r#"sutura_questions_total{code="internal"} 0"#), "{body}");

    let (_, body) = call(&faulting, request("GET", "/metrics", Some(METRICS_TOKEN), Body::empty())).await;
    assert!(body.contains(r#"sutura_questions_total{code="unavailable"} 1"#), "{body}");
    assert!(body.contains(r#"sutura_questions_total{code="refused"} 0"#), "{body}");
}

#[tokio::test]
async fn no_series_names_a_source_or_a_question() {
    // `docs/adr/0015`'s disclosure exclusions, asserted over the rendered output rather than
    // assumed: no question text, no metric or dimension name, no source name, no caller address and
    // no credential. The question carries a sentinel no series could contain.
    let app = app(metrics_settings(Environment::Development));
    let sentinel = "sentinel_q7x9";
    let asked = format!(r#"{{"metric":"{sentinel}","grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}}}}"#);
    let (status, _) = call(&app, request("POST", "/v1/query", Some(super::TOKEN), Body::from(asked))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = call(&app, request("GET", "/metrics", Some(METRICS_TOKEN), Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    for forbidden in [
        sentinel,
        // A metric the served catalog defines: a label must never be a question or a catalog name.
        "revenue",
        // The source name the fake bundle declares.
        "local",
        // The credentials, in any form.
        super::TOKEN,
        METRICS_TOKEN,
        // A caller address. `testing::request` inserts a peer address, so this is the address the
        // scrape could have exported as a label and must not.
        "203.0.113",
    ] {
        assert!(!body.contains(forbidden), "the exposition carried `{forbidden}`: {body}");
    }
}

#[tokio::test]
async fn a_scrape_answers_while_a_question_holds_its_slot() {
    // **"A scrape must not make the service work", at the strongest point.** A slot is held until
    // the WORK finishes, and the metrics route is deliberately outside the admission bound, so a
    // scrape during the busiest possible moment answers immediately and OBSERVES the in-flight work
    // rather than blocking on it. The held warehouse is what makes the question stay in flight.
    let (engine, held) = warehouse_that_can_be_held();
    let router = over(bundle(), engine, metrics_settings(Environment::Development));
    held.arm();

    let asking = tokio::spawn({
        let router = router.clone();
        async move {
            call(
                &router,
                request("POST", "/v1/query", Some(super::TOKEN), Body::from(A_QUESTION)),
            )
            .await
        }
    });

    // Poll rather than sleep: the assertion is that the slot is counted while the work is in
    // flight, and a fixed sleep either makes the test slow or flakes on a loaded machine.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let observed = loop {
        let (status, body) = call(&router, request("GET", "/metrics", Some(METRICS_TOKEN), Body::empty())).await;
        assert_eq!(status, StatusCode::OK);
        if body.contains("sutura_execution_slots_in_use 1") {
            break body;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the in-flight slot was never observed: {body}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    // The scrape saw the work that is holding a slot, which a request counter cannot show.
    assert!(observed.contains("sutura_execution_slots_in_use 1"), "{observed}");

    held.release();
    asking.await.expect("the question task did not panic");
}
