//! The surface, driven end to end through the assembled router.
//!
//! Through the router and not a spawned process: the real `axum::Router` from real `Settings` over
//! a fake behind the [`Surface`] port, called with `tower`'s `oneshot`. That reaches the layer
//! stack, token gate, limiter, extractors and response shapes with no socket, catalog directory or
//! data system. The peer address is the one thing the real server supplies and this does not -
//! `crate::testing::request` inserts it, and that pair lives there because four test modules had
//! grown four copies of it.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use sutura_config::Environment;
use tower::ServiceExt as _;

use crate::surface::LocalService;
use crate::testing::{
    bundle, call, catalog_of, fake_warehouse, request, sink, two_source_bundle, unanchored_bundle, warehouse_pretending_to_be,
    warehouse_that_answers_past_the_row_cap, warehouse_that_can_be_held,
};

mod fixtures;
use fixtures::*;

/// Whether the catalog route honours the prose setting it was started with.
///
/// Its own file rather than another section here, and the reason is mechanical: this one is at 1000
/// lines, which `cargo xtask max-lines` refuses. The helpers it needs are this module's, reached
/// through `super`.
mod catalog_prose;

// -------------------------------------------------------------------- health ----

#[tokio::test]
async fn health_answers_without_a_token_and_says_nothing_about_the_deployment() {
    // Liveness has to work for a probe with no credential, and it has to be worth nothing to an
    // attacker. Both halves in one assertion, against a deployment that DOES require a token
    // everywhere else.
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));
    let (status, body) = call(&app, request("GET", "/health", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"status":"ok"}"#);
}

// ------------------------------------------------------------- the token gate ----
//
// The three questions about a deployment that configures `security.access_token`. Its own file
// because this one is at the `max-lines` bound.

#[cfg(test)]
mod token;

// ------------------------------------------------------------------- queries ----

#[tokio::test]
async fn a_certified_question_is_answered_with_its_provenance() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""outcome":"answer""#), "{body}");
    assert!(body.contains(r#""definition_version":"test-1""#), "{body}");
}

// ------------------------------------------------------- refusals, per status ----
//
// **THE contract of this surface, and it changed.** A refusal used to be `200` with
// `outcome: refusal`, argued from "an error status invites a client library to retry" - a premise
// that does not survive checking (`crate::wire::refusal` has the citations). The `200` made a
// governance refusal indistinguishable from an answer to anything reading a status and not a body,
// e.g. an error-rate alert or a generated client whose success branch is `2xx`.
//
// One test per status, asserting all three things a refused caller gets: the status, the `code`,
// and a non-empty sentence. `wire::refusal` unit-tests the per-variant mapping; what these add is
// that the status survives the REAL router, which a handler-computed mapping the router flattens
// to `200` would not.

#[tokio::test]
async fn an_unknown_metric_is_a_404_naming_the_snapshot_that_does_not_define_it() {
    let app = app(settings(Environment::Development, ""));
    let (status, code, detail) = refusal(
        &app,
        r#"{"metric":"gross_margin","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(code, "metric_unknown");
    assert!(detail.contains("gross_margin"), "{detail}");
}

#[tokio::test]
async fn a_question_that_is_well_formed_and_out_of_bounds_is_a_422() {
    // Four codes on one status, which is the grouping rather than a shortage: each is a well formed
    // question outside a declared bound, the caller's move is the same in all four - narrow it - and
    // the `code` says which bound. `422` is documented as the status a client should NOT expect to
    // succeed on repetition, exactly the refusal's own claim.
    let app = app(settings(Environment::Development, ""));
    let range = r#""range":{"start":"2026-06-01","end":"2026-07-01"}"#;

    let (status, code, detail) = refusal(&app, &format!(r#"{{"metric":"revenue","grain":"year",{range}}}"#)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code, "grain_not_supported");
    assert!(detail.contains("year"), "{detail}");

    let (status, code, detail) = refusal(
        &app,
        r#"{"metric":"revenue","grain":"month","range":{"start":"0001-01-01","end":"9999-12-31"}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code, "time_range_too_long");
    // EACH NUMBER IN ITS OWN ROLE: `contains("3653")` passed with the two swapped. The general rule
    // is in `.agents/skills/engineering/rust`; this file had applied it one line over and not here.
    let max_days = sutura_domain::query::MAX_RANGE_DAYS;
    assert!(detail.contains("the period spans "), "{detail}");
    assert!(detail.contains(&format!("and the maximum is {max_days}")), "{detail}");

    let (status, code, _) = refusal(
        &app,
        &format!(r#"{{"metric":"revenue","grain":"month",{range},"dimensions":["region","region"]}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code, "duplicate_dimension");

    let (status, code, detail) = refusal(
        &app,
        &format!(r#"{{"metric":"revenue","grain":"month",{range},"dimensions":["one","two","three","four","five"]}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code, "too_many_dimensions");
    // BOTH HALVES of what `wire/refusal.rs` promises. `contains('4')` held only the bound, so
    // dropping `{requested}` from the renderer passed 168 of 168; reading the cap fails the cell
    // when it moves and the sentence does not.
    let max_dims = sutura_domain::query::MAX_DIMENSIONS;
    assert!(detail.contains("5 group-by keys were asked for"), "{detail}");
    assert!(detail.contains(&format!("and the maximum is {max_dims}")), "{detail}");
}

#[tokio::test]
async fn asking_outside_what_the_catalog_permits_is_a_403() {
    // The governance statuses: the catalog's answer to "may this be asked of this metric", which is
    // what 403 says - NOT a statement about a credential, since this surface has no per-caller
    // identity and no token widens a metric's dimension set. The sentence names the metric and the
    // dimension so nobody reads it as "get a better token".
    let app = app(settings(Environment::Development, ""));
    let range = r#""range":{"start":"2026-06-01","end":"2026-07-01"}"#;

    let (status, code, detail) = refusal(
        &app,
        &format!(r#"{{"metric":"revenue","grain":"month",{range},"dimensions":["channel"]}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(code, "dimension_not_permitted");
    assert!(detail.contains("channel"), "{detail}");

    let (status, code, detail) = refusal(
        &app,
        &format!(r#"{{"metric":"revenue","grain":"month",{range},"filters":[{{"dimension":"region","value":"east"}}]}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(code, "dimension_value_not_allowed");
    // The rejected value is NOT echoed back. The domain's variant does not carry it, and this is the
    // assertion that the status change did not put it back on the way out.
    assert!(!detail.contains("east"), "the rejected value reached the response: {detail}");
    assert!(detail.contains("region"), "{detail}");
}

#[tokio::test]
async fn a_question_that_spans_two_data_systems_is_refused_until_a_leg_executes() {
    // **Still green, and about the FAKE rather than the shipped set.** It runs over
    // `fake_warehouse()`, which takes the port's defaulted `EXECUTES_LEGS`, so it pins the status a
    // build whose adapter cannot run a leg gives: a 409 from `answer`'s own gate, not an adapter's
    // typed refusal surfacing as the 503 reserved for a retryable outage.
    // `sutura-exec-datafusion` declares the constant now, so the shipped engine no longer reaches
    // this branch; `sutura-exec-bigquery` and any adapter taking the default still do.
    let app = over(two_source_bundle(), fake_warehouse(), settings(Environment::Development, ""));
    let (status, code, detail) = refusal(
        &app,
        r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{detail}");
    assert_eq!(code, "federation_not_executable", "{detail}");
}

#[tokio::test]
async fn an_answer_past_the_row_cap_is_a_413_that_says_it_was_not_truncated() {
    // **The case this whole change was asked for.** A result over the cap is refused rather than cut
    // down to fit: a partial total under a certified name is wrong in the one way nothing downstream
    // can detect, and a caller has to tell that from the response alone.
    //
    // `413` is shared with the request-body limit on this same route, so the assertion covers the
    // code and the sentence too: `code` tells "the answer was too big" from "your request was too
    // big", and only this body carries `outcome`.
    let app = over(
        unanchored_bundle(),
        warehouse_that_answers_past_the_row_cap(),
        settings(Environment::Development, ""),
    );
    let (status, code, detail) = refusal(&app, crate::testing::A_QUESTION).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(code, "result_too_large");
    assert!(detail.contains("10000"), "the sentence does not name the cap: {detail}");
    assert!(
        detail.contains("NOT truncated"),
        "the sentence does not say nothing was cut: {detail}"
    );
    assert!(detail.contains("narrow"), "the sentence does not say what to do: {detail}");
}

#[tokio::test]
async fn an_answer_the_data_system_would_not_return_at_once_is_the_same_413_and_not_a_503() {
    // **The defect this bound was added for, where a caller reads it.** A result INSIDE the row cap
    // that a data system will not return in one piece used to leave `sutura_app::answer` as
    // `ServiceError::Warehouse`, which this surface answers `503 unavailable` - a dead data system's
    // status, and `docs/adr/0005`'s one retryable refusal. This is not an outage and the retry
    // returns the same reply. Both halves are asserted because either alone passes on the wrong
    // grouping: the status is the row cap's own, and the code is `result_too_large` rather than a
    // second code an operator would learn for the same remedy.
    let app = over(
        unanchored_bundle(),
        crate::testing::WarehouseThatWillNotPage::new(crate::testing::source()),
        settings(Environment::Development, ""),
    );
    let (status, code, detail) = refusal(&app, crate::testing::A_QUESTION).await;
    assert_ne!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "a bound that fires again in the same place was reported as an outage: {detail}"
    );
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(code, "result_too_large");
    // No figure, because there is none this deployment was told - the bound is the data system's own.
    // A digit here would be a certified-looking number for a bound nobody measured.
    assert!(
        !detail.chars().any(char::is_numeric),
        "the sentence names a bound nobody measured: {detail}"
    );
    assert!(
        detail.contains("NOT truncated"),
        "the sentence does not say nothing was cut: {detail}"
    );
    assert!(detail.contains("narrow"), "the sentence does not say what to do: {detail}");
}

#[tokio::test]
async fn an_execute_failure_that_is_not_a_size_bound_is_503_not_413() {
    // The control for the test above, which only a sibling fake can show: an `execute` failure whose
    // predicate answers `false` must leave as the `503` an outage produces, not a size bound's `413`
    // `result_too_large`. Retrying an outage is reasonable; telling a caller to narrow a question
    // because a data system was briefly unwell is the wrong instruction.
    // `WarehouseThatFailsToExecute` is `WarehouseThatWillNotPage`'s reach (a `dry_run` that accepts,
    // an `execute` that fails) with the port's default `false` predicate.
    let app = over(
        unanchored_bundle(),
        crate::testing::WarehouseThatFailsToExecute::new(crate::testing::source()),
        settings(Environment::Development, ""),
    );
    // A `ServiceError::Warehouse` is a FAILURE, not a refusal - it is the problem body the transport
    // reserves for an outage, with no `outcome` field - which is exactly the distinction this test is
    // about. So it is read with `call` rather than `refusal`, and asserted by status and code.
    let (status, body) = call(
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains(r#""code":"unavailable""#), "{body}");
}

#[tokio::test]
async fn a_data_system_the_plan_names_and_this_process_did_not_open_is_a_503() {
    // The one refusal where retrying is reasonable, sharing `503` with two failures that are not
    // refusals - `unavailable` and `at_capacity`. The code separates the three, and the body shape
    // separates this one further: it carries `outcome`.
    //
    // **No `Retry-After`.** `Failure::retry_after` already sets this surface's rule - a number
    // already known, or no header - and nothing here knows when a data system comes back.
    let app = over(
        unanchored_bundle(),
        warehouse_pretending_to_be("elsewhere"),
        settings(Environment::Development, ""),
    );
    let response = app
        .clone()
        .oneshot(request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)))
        .await
        .expect("the router is infallible as a service");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        response.headers().get("retry-after").is_none(),
        "a refusal invented a retry hint"
    );

    let (status, code, detail) = refusal(&app, crate::testing::A_QUESTION).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(code, "source_unavailable");
    assert!(detail.contains("local"), "{detail}");
}

#[tokio::test]
async fn a_refusal_keeps_the_envelope_a_client_already_parses() {
    // The compatibility half, and why this is an ADDITIVE change rather than a body redesign: the
    // status is new, while `outcome`, `reason` and `code` are exactly what they were - so a client
    // written against the old surface still finds everything it read, now with a status that agrees
    // with the body.
    let app = app(settings(Environment::Development, ""));
    let unknown = r#"{"metric":"gross_margin","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;
    let (status, body) = call(&app, request("POST", "/v1/query", None, Body::from(unknown))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains(r#""outcome":"refusal""#), "{body}");
    assert!(body.contains(r#""code":"metric_unknown""#), "{body}");
    assert!(body.contains(r#""status":404"#), "{body}");
}

#[tokio::test]
async fn a_body_carrying_sql_is_a_bad_request_and_the_field_is_named() {
    // The governance boundary, through a real JSON parser. Without `deny_unknown_fields` this is a
    // `200` answering a different question than the caller believes they asked.
    let app = app(settings(Environment::Development, ""));
    let smuggled = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},
                       "sql":"select * from orders"}"#;
    let (status, body) = call(&app, request("POST", "/v1/query", None, Body::from(smuggled))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
    assert!(body.contains("sql"), "{body}");
}

#[tokio::test]
async fn a_body_larger_than_the_configured_bound_is_refused_before_it_is_parsed() {
    let bounded = app(settings(Environment::Development, "server:\n  max_body_bytes: 8\n"));
    let (status, body) = call(
        &bounded,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    )
    .await;
    // `413` and not `400`, which is the whole reason `routes::v1::query::rejected` branches on the
    // rejection's status. The body-limit layer makes the JSON extractor reject with a length-limit
    // error - a `JsonRejection` exactly like a malformed body - so mapping every rejection to `400`
    // made "you sent too much" indistinguishable from "you sent a typo".
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert!(body.contains(r#""code":"too_large""#), "{body}");
    // And through a router assembled the same way, a body that is merely wrong is still a `400` - so
    // the assertion above is about the size and not about the route.
    let unbounded = app(settings(Environment::Development, ""));
    let (status, body) = call(&unbounded, request("POST", "/v1/query", None, Body::from("{"))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
}

#[tokio::test]
async fn a_request_that_outruns_the_bound_carries_the_documented_failure_body() {
    // `tower-http` implements `TimeoutLayer::with_status_code` as `Response::new(B::default())` -
    // the status and an EMPTY body - while the generated document and `problem.rs` both promise
    // every `408` carries a `ProblemBody`. Right status, no body, which a client parsing one
    // failure shape cannot handle. `middleware::enforce_timeout` exists for this; the assertion is
    // that the assembled router actually uses it.
    //
    // One second is the smallest bound `RequestTimeout::parse` accepts. The port call is HELD rather
    // than made slow, and armed only after `start`, because `start` re-executes every anchor.
    let settings = settings(Environment::Development, "server:\n  request_timeout_seconds: 1\n");
    let (engine, held) = warehouse_that_can_be_held();
    let service = LocalService::start(&catalog_of(bundle()), engine, sink(), crate::testing::broker(), 1 << 30)
        .expect("the test bundle validates");
    let app = crate::router(&crate::testing::state_over(Arc::new(service), settings)).expect("the test router assembles");
    held.arm();

    let (status, body) = call(
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    )
    .await;
    // Released before the assertions, so a failing one does not also leave the blocking pool holding
    // the runtime open for the cap.
    held.release();
    assert_eq!(status, StatusCode::REQUEST_TIMEOUT, "{body}");
    assert!(
        body.contains(r#""code":"timeout""#),
        "the 408 carried no problem body: {body}"
    );
    assert!(body.contains(r#""status":408"#), "{body}");
}

// ------------------------------------------------------------- the limiter ----

#[tokio::test]
async fn a_wrong_token_attempt_costs_a_rate_limit_cell() {
    // **The ordering bug, asserted.** `Router::layer` wraps what is there, so the LAST layer added
    // is outermost. With the token gate outside the limiter it answered `401` without calling
    // `next.run`, so a wrong-token attempt never reached the limiter and cost nothing - an unlimited
    // guessing loop against a 32-character shared secret, the one thing a rate limiter in front of a
    // bearer token exists to bound. The burst is one, so the SECOND wrong-token attempt must be
    // `429` and not `401`: a `401` there would mean the attempt was free.
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\nrate_limit:\n  enabled: true\n  api_per_second: 1\n  api_burst: 1\n"),
    ));
    let wrong = Some("this-is-not-the-token-but-is-long-enough");
    let (first, _) = call(&app, request("GET", "/v1/catalog", wrong, Body::empty())).await;
    assert_eq!(first, StatusCode::UNAUTHORIZED);
    let (second, body) = call(&app, request("GET", "/v1/catalog", wrong, Body::empty())).await;
    assert_eq!(
        second,
        StatusCode::TOO_MANY_REQUESTS,
        "a wrong-token attempt did not consume quota, so guessing is free: {body}"
    );
}

#[tokio::test]
async fn the_documentation_subtree_charges_a_wrong_token_the_same_way() {
    // The same inversion was in the documentation router, and a description of the surface behind a
    // secret is a secret worth guessing at too. Same assertion, other subtree.
    let app = app(settings(
        Environment::Development,
        &format!(
            "security:\n  access_token: \"{TOKEN}\"\nrate_limit:\n  enabled: true\n  probe_per_second: 1\n  probe_burst: 1\n"
        ),
    ));
    let wrong = Some("this-is-not-the-token-but-is-long-enough");
    assert_eq!(
        call(&app, request("GET", "/openapi.json", wrong, Body::empty())).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, request("GET", "/openapi.json", wrong, Body::empty())).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn the_assembled_router_hands_back_the_tiers_something_has_to_sweep() {
    // The other half of the ordering fix - one change, not two: with the limiter outermost every
    // path an unauthenticated caller can reach creates a bucket, and `governor`'s keyed store sheds
    // nothing until `retain_recent` is called, so fixing the order alone turns a narrow leak into a
    // surface-wide one. `middleware`'s own tests cover the sweep and the sweeper thread; **only here
    // can it be asserted that the ROUTER produces handles at all** - the leak existed because
    // `GovernorLayer::new(Arc::new(config))` was the last anyone saw of the configuration.
    let assembled = crate::assemble(&crate::testing::state_over(
        Arc::new(
            LocalService::start(
                &catalog_of(bundle()),
                fake_warehouse(),
                sink(),
                crate::testing::broker(),
                1 << 30,
            )
            .expect("the test bundle validates"),
        ),
        settings(
            Environment::Development,
            &format!("security:\n  access_token: \"{TOKEN}\"\nrate_limit:\n  enabled: true\n"),
        ),
    ))
    .expect("the test router assembles");
    let tiers: Vec<&str> = assembled
        .limiters()
        .iter()
        .map(crate::middleware::LimiterHandle::tier)
        .collect();
    assert!(
        tiers.contains(&"general") && tiers.contains(&"public"),
        "a tier was installed with no handle kept, so nothing can sweep it: {tiers:?}"
    );
    for handle in assembled.limiters() {
        // Reachable at all is the property: a handle that cannot be swept is the leak.
        handle.reap();
        assert_eq!(handle.tracked(), 0);
    }
}

#[tokio::test]
async fn the_limiter_refuses_a_caller_past_its_burst() {
    // A limiter that does not limit is the failure mode this exists for, and it is invisible
    // without a test: the layer composes, the service answers, and nothing is bounded.
    let app = app(settings(
        Environment::Development,
        "rate_limit:\n  enabled: true\n  api_per_second: 1\n  api_burst: 1\n",
    ));
    let (first, _) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(first, StatusCode::OK);
    let (second, body) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(body.contains(r#""code":"rate_limited""#), "{body}");
}

#[tokio::test]
async fn the_disabled_limiter_does_not_limit() {
    // The other side of the same switch, so "off" is a real state rather than an untested branch.
    let app = app(settings(
        Environment::Development,
        "rate_limit:\n  enabled: false\n  api_per_second: 1\n  api_burst: 1\n",
    ));
    for _ in 0_u8..5 {
        let (status, _) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn liveness_and_the_api_are_limited_separately() {
    // Two tiers, so a probe hammering liveness cannot exhaust a caller's quota for the API. The
    // public tier is exhausted here and the general tier still answers.
    let app = app(settings(
        Environment::Development,
        "rate_limit:\n  enabled: true\n  probe_per_second: 1\n  probe_burst: 1\n  api_per_second: 5\n  api_burst: 5\n",
    ));
    assert_eq!(
        call(&app, request("GET", "/health", None, Body::empty())).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, request("GET", "/health", None, Body::empty())).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        call(&app, request("GET", "/v1/catalog", None, Body::empty())).await.0,
        StatusCode::OK
    );
}

// ------------------------------------------------------- the documentation ----

#[tokio::test]
async fn the_interface_description_is_served_in_development_and_not_in_production() {
    let development = app(settings(Environment::Development, ""));
    let (status, body) = call(&development, request("GET", "/openapi.json", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    // The paragraph an integrator must not be able to miss. It used to read "NO PER-CALLER IDENTITY",
    // which stopped being unconditionally true when leg 1 landed; what has to survive is the sentence
    // about what identity does NOT buy, because that is the one somebody acts on wrongly.
    assert!(body.contains("NEITHER IS PER-CALLER ACCESS"), "{body}");
    assert!(body.contains("no row-level"), "{body}");

    // Production, fully configured, description off by default: a map of the surface is something a
    // deployment turns on, not something it must remember to turn off. `tls_termination: ingress`
    // rather than an "expose me anyway" switch - an off-host bind is refused until the operator says
    // where TLS is terminated, and `ingress` is the ordinary answer (a controller in front,
    // plaintext on the pod network). That deployment must load rather than be refused.
    let production = app(settings(
        Environment::Production,
        &format!(
            "server:\n  host: \"0.0.0.0\"\nsecurity:\n  access_token: \"{TOKEN}\"\n  tls_termination: \"ingress\"\n  metrics_token: \"0123456789abcdef0123456789abcdf0\"\n"
        ),
    ));
    let (status, _) = call(&production, request("GET", "/openapi.json", Some(TOKEN), Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_interface_description_is_behind_the_token_when_one_is_configured() {
    // A description of the surface is a description of what this deployment measures.
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));
    assert_eq!(
        call(&app, request("GET", "/openapi.json", None, Body::empty())).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, request("GET", "/openapi.json", Some(TOKEN), Body::empty()))
            .await
            .0,
        StatusCode::OK
    );
}

// ------------------------------------------------------------------ the metrics ----
//
// The credential, the exact series set, the refusal-versus-fault split, the disclosure exclusions
// and the scrape-under-load cell. Its own file because this one is at the `max-lines` bound.

#[cfg(test)]
mod metrics;

/// The metrics credential, distinct from the API token, for the cases that configure one.
const METRICS_TOKEN: &str = "0123456789abcdef0123456789abcdf0";

/// Settings with both credentials and pinned process numbers.
///
/// The engine width and the execution bound are pinned rather than left to the machine, because one
/// of these tests asserts the rendered exposition exactly and a machine-dependent number would make
/// that snapshot a property of the runner.
fn metrics_settings(environment: Environment) -> Settings {
    settings(
        environment,
        &format!(
            "security:\n  access_token: \"{TOKEN}\"\n  tls_termination: \"sidecar\"\n  \
             metrics_token: \"{METRICS_TOKEN}\"\nserver:\n  host: \"0.0.0.0\"\nruntime:\n  \
             engine_worker_threads: 3\n  max_concurrent_queries: 4\n"
        ),
    )
}
// ------------------------------------------------------------------ the log ----
//
// **The defect these were written for.** `TraceLayer::new_for_http()` builds a `DefaultMakeSpan`,
// which `tower-http` seeds from `DEFAULT_MESSAGE_LEVEL` = `Level::DEBUG`, while `telemetry.filter`
// defaults to `info`. So the ONE production span was disabled in the shipped default:
// `JsonStorageLayer` had nothing to attach, every machine-readable line carried an empty span
// context, and the documentation promised the opposite.
//
// **Only testable as bytes:** no return value says "a span existed", and the property is about what
// a collector receives - so these drive the real router with a subscriber over a buffer and read
// the buffer. `SUTURA_TEST_LOG=1` also prints it, which makes a failure here readable. See
// `sutura_runtime::testing::Capture`.

#[cfg(test)]
mod logging;

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
fn captured(format: sutura_config::LogFormat, app: &Router, request: Request<Body>) -> (StatusCode, String, String) {
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
fn json_lines(rendered: &str) -> Vec<serde_json::Value> {
    rendered
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect()
}

#[cfg(test)]
mod logging_tests;
