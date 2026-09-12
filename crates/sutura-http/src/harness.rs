//! The surface, driven end to end through the assembled router.
//!
//! Through the router and not a spawned process: the real `axum::Router` from real `Settings` over
//! a fake behind the [`Surface`] port, called with `tower`'s `oneshot`. That reaches the layer
//! stack, token gate, limiter, extractors and response shapes with no socket, catalog directory or
//! data system. The peer address is the one thing the real server supplies and this does not -
//! `crate::testing::request` inserts it, and that pair lives there because four test modules had
//! grown four copies of it.
//!
//! **This file reached the 1000-line cap, and what may leave is the HARNESS - not the assertions:**
//! `just causality` reverts a changed file to measure it, so a test moved into a new module loses
//! the `mod` declaration that compiles it and is never measured on base. Submodules reach the
//! builders through `use super::*`.

use std::sync::Arc;

use axum::body::Body;
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
mod catalog_prose;

// ------------------------------------------------------------- the token gate ----

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

#[tokio::test]
async fn the_versioned_api_needs_the_token_when_one_is_configured() {
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));

    let (status, body) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains(r#""code":"unauthorized""#), "{body}");

    let (status, _) = call(
        &app,
        request(
            "GET",
            "/v1/catalog",
            Some("wrong-but-long-enough-to-be-a-token"),
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = call(&app, request("GET", "/v1/catalog", Some(TOKEN), Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("revenue"), "{body}");
}

#[tokio::test]
async fn an_unmatched_path_answers_not_found_without_holding_a_credential() {
    // **Asserts the CURRENT behaviour, which is not what this file first claimed.** `route_layer`
    // runs only for a request that matched a route in that subtree, so a path under the version
    // prefix matching nothing skips the token gate and falls through to the top-level `404`.
    //
    // Kept as `404` rather than contorted into a `401`: it discloses only *which paths exist*, and
    // those are in the published interface description. Every path that resolves to a handler does
    // hold a credential - the test above asserts it. If that judgement changes, it changes here.
    let app = app(settings(
        Environment::Development,
        &format!("security:\n  access_token: \"{TOKEN}\"\n"),
    ));
    let (status, body) = call(&app, request("GET", "/v1/catalog/secret", None, Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // What matters either way: the response says nothing about the deployment.
    assert!(body.is_empty(), "{body}");
}

// ------------------------------------------------------------------- queries ----

#[tokio::test]
async fn a_loopback_development_service_with_no_token_configured_answers_without_one() {
    // The permissive branch, and the reason it is safe: this configuration is only loadable on a
    // loopback bind outside production, which `sutura-config` enforces with a startup refusal.
    let app = app(settings(Environment::Development, ""));
    let (status, _) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
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

// ------------------------------------------------------------- the limiter ----

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

// ------------------------------------------------------- the documentation ----

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
        &format!("server:\n  host: \"0.0.0.0\"\nsecurity:\n  access_token: \"{TOKEN}\"\n  tls_termination: \"ingress\"\n"),
    ));
    let (status, _) = call(&production, request("GET", "/openapi.json", Some(TOKEN), Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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

#[cfg(test)]
mod logging;

#[test]
fn a_request_produces_one_info_span_carrying_the_route_and_a_correlation_id() {
    // RED before the fix for two independent reasons: the span was `DEBUG`, so at the configured
    // `info` it did not exist, and `DefaultMakeSpan` names no route and mints no correlation.
    // Asserted on the bunyan rendering because a collector reads that one, and on the SPAN lines -
    // `tracing-bunyan-formatter` emits `[request - START]` and `[request - END]` for a span it can
    // see, so their presence is the span's existence.
    let app = app(settings(Environment::Development, ""));
    let (status, body, rendered) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    );
    assert_eq!(status, StatusCode::OK, "{body}");

    let lines = json_lines(&rendered);
    assert!(!lines.is_empty(), "nothing was logged at all: {rendered}");
    let span_lines: Vec<&serde_json::Value> = lines
        .iter()
        .filter(|line| {
            line["msg"]
                .as_str()
                .is_some_and(|msg| msg.contains("[REQUEST - START]") || msg.contains("[REQUEST - END]"))
        })
        .collect();
    assert!(
        !span_lines.is_empty(),
        "no request span reached the log at the configured level: {rendered}"
    );
    for line in &span_lines {
        // `level` 30 is bunyan's `info`. A `DEBUG` span filtered out at `info` is the defect; a
        // `DEBUG` span that somehow survived would still be the wrong level to promise.
        assert_eq!(line["level"], 30, "the request span is not at info: {line}");
        assert_eq!(line["route"], "/v1/query", "the span does not name the route: {line}");
        assert_eq!(line["method"], "POST", "the span does not name the method: {line}");
        let correlation = line["correlation"]
            .as_str()
            .unwrap_or_else(|| panic!("the span carries no correlation id: {line}"));
        assert!(
            is_a_correlation_id(correlation),
            "the correlation id is not one this surface would read back: {correlation}"
        );
    }
    // And the raw path is NOT what is logged. `/v1/query` happens to be its own route template, so
    // the assertion that means something is that no line carries a `path` field at all.
    for line in &lines {
        assert!(line.get("path").is_none(), "the raw request path reached the log: {line}");
    }

    // The same design point's other half, the one a caller controls: a path that matched nothing
    // must NOT appear anywhere in the log. It reaches the catch-all fallback, which the same layer
    // wraps, so there is still a span and its route is the constant. Otherwise the log's cardinality
    // is something a caller chooses by probing, and a probed path with a secret in it is a secret in
    // the log.
    let (status, _, rendered) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("GET", "/v1/probing-for-SECRETish-paths", None, Body::empty()),
    );
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        !rendered.contains("probing-for-SECRETish-paths"),
        "a path a caller invented reached the log: {rendered}"
    );
    assert!(
        rendered.contains(r#""route":"unmatched""#),
        "an unmatched request produced no span, or not the constant: {rendered}"
    );
    // So the assertion above cannot drift from the constant it is spelling out.
    assert_eq!(crate::router::UNMATCHED_ROUTE, "unmatched");
}

#[test]
fn a_body_that_states_its_own_subject_is_not_a_question() {
    // **The confused deputy, at the wire.** A caller that states its own identity does not have
    // one, so a body carrying a principal is refused rather than read, and the value must not reach
    // the log on the way out.
    //
    // **A REGRESSION GUARD, GREEN against the unmodified code** - `deny_unknown_fields` on
    // `QuestionBody` already refused an undeclared key, so it proves nothing about this change and
    // everything about the next. Like `a_filter_value_never_reaches_the_log`, the property is newly
    // load-bearing because a chain now exists for a caller to try to state, and the mistake would
    // be somebody adding the field to the wire type to be helpful.
    //
    // The other half - that the sink's chain is the transport's, reachable by no request parameter -
    // is `crate::surface::tests`' `a_chain_reaches_the_sink_through_no_field_a_caller_supplies`,
    // there rather than here because the record is written inside the blocking task and a
    // thread-scoped subscriber cannot see a pool thread's event (`sutura_runtime::testing` records
    // that; `crates/sutura-runtime/tests/blocking_span.rs` is the integration test that exists
    // because of it). **So this file can assert what a caller is TOLD, not what was recorded.**
    const IMPERSONATED: &str = "victim@example.com";

    let app = app(settings(Environment::Development, ""));

    // ONE: a body that tries to name a principal is not a question. `deny_unknown_fields` makes it a
    // parse error that NAMES the field, so the attempt is visible rather than ignored - and the
    // domain types carry no `Deserialize`, so even a wire shape accepting the key would have nothing
    // to turn it into.
    let claiming = format!(
        r#"{{"metric":"revenue","grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},"subject":"{IMPERSONATED}"}}"#
    );
    let (status, body, rendered) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("POST", "/v1/query", None, Body::from(claiming)),
    );
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a body naming a subject was accepted: {body}"
    );
    let problem: serde_json::Value = serde_json::from_str(&body).expect("a failure is JSON");
    let detail = problem["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("subject"),
        "the refusal does not name the field that was rejected: {body}"
    );

    // And the invented value does not reach the log on the way to being refused - the property
    // `a_filter_value_never_reaches_the_log` pins, applied to the field far worse to echo: an
    // identifier a record could later be read as attributing a call to. The RESPONSE names the
    // field, which is the point of `deny_unknown_fields`; the log is where the value must not land.
    assert!(
        !rendered.contains(IMPERSONATED),
        "an identifier the caller invented reached the log: {rendered}"
    );

    // TWO: the same body without the extra field IS answered, so the assertion above is about the
    // field rather than about the request being malformed some other way.
    let (status, body, _) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    );
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[test]
fn one_correlation_id_ties_the_span_the_question_and_the_answer_together() {
    // **The property an operator actually uses:** not "a span exists" but "these three lines are the
    // same request". RED before the fix twice over - no span at `info` for the handler's events to
    // sit inside, and no correlation id to tie them with.
    //
    // The caller's own header is used, so the assertion is about ONE known value rather than three
    // unknown ones agreeing - and it pins the ingress case, the reason for reading the header.
    let app = app(settings(Environment::Development, ""));
    let mut request = request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION));
    request
        .headers_mut()
        .insert(crate::correlation::HEADER, "Ingress-42_abc".parse().expect("a test header"));
    let (status, body, rendered) = captured(sutura_config::LogFormat::Bunyan, &app, request);
    assert_eq!(status, StatusCode::OK, "{body}");

    let lines = json_lines(&rendered);
    // `answered` was the second line here and is gone from this crate: the per-outcome line became
    // the audit record `Surface::answer` writes, emitted from the blocking task and so invisible to
    // the thread-scoped subscriber this helper installs - see
    // `a_body_that_states_its_own_subject_is_not_a_question`. `finished processing request` is
    // `tower_http`'s response line, carries the status, and is written on THIS thread, so it makes
    // the assertion "these lines are the same request" rather than "a line exists".
    for wanted in ["question received", "finished processing request"] {
        let line = lines
            .iter()
            .find(|line| line["msg"].as_str().is_some_and(|msg| msg.contains(wanted)))
            .unwrap_or_else(|| panic!("no `{wanted}` line was written: {rendered}"));
        assert_eq!(
            line["correlation"], "Ingress-42_abc",
            "the `{wanted}` line is not attributable to the request: {line}"
        );
        // The identifying fields moved onto the span, so they are on EVERY line of the request and
        // not only on the one that first knew them.
        assert_eq!(line["metric"], "revenue", "{line}");
        assert_eq!(line["grain"], "month", "{line}");
    }
    // And the span itself, so all three carry it rather than the two events agreeing with each
    // other and with nothing.
    assert!(
        lines
            .iter()
            .any(|line| line["msg"].as_str().is_some_and(|msg| msg.contains("[REQUEST - END]"))
                && line["correlation"] == "Ingress-42_abc"),
        "the span does not carry the id its events carry: {rendered}"
    );
}

#[test]
fn a_filter_value_never_reaches_the_log() {
    // **A REGRESSION GUARD, not a bug fix.** Nothing logs filter values today, so this is GREEN
    // against the unmodified code - it proves nothing about this change and everything about the
    // next. It is here because the discipline is stated in a comment at one call site and enforced
    // by nothing, and because moving fields onto a span would break it: a span field is copied onto
    // every line of the request, so a value put there by mistake leaks further than one on an event.
    // `sutura_domain::query`'s `a_rejected_filter_value_is_not_echoed_back` covers the refusal TYPE.
    // **Nothing covered the log.**
    const SENTINEL: &str = "SENTINEL-MUST-NOT-BE-LOGGED";

    let app = app(settings(Environment::Development, ""));
    let question = format!(
        r#"{{"metric":"revenue","grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},
            "filters":[{{"dimension":"region","value":"{SENTINEL}"}}]}}"#
    );
    // Both renderings, because they are two different formatters and only one of them is what a
    // collector reads. A leak in the other is still a leak.
    for format in [sutura_config::LogFormat::Bunyan, sutura_config::LogFormat::Pretty] {
        let (status, body, rendered) = captured(format, &app, request("POST", "/v1/query", None, Body::from(question.clone())));
        // The value is outside the declared allowlist, so this is the `403` - which is the path
        // where a value is most likely to be reflected somewhere, and the reason the fixture uses a
        // refused value rather than an accepted one.
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert!(!body.contains(SENTINEL), "the response echoed the value: {body}");
        assert!(
            !rendered.contains(SENTINEL),
            "a filter value reached the {format} log: {rendered}"
        );
        // A line about this request is still there, so the assertion above is not passing because
        // nothing was logged. It was the handler's own `refused` line, replaced by the audit record,
        // which is written from the blocking task and cannot reach a thread-scoped subscriber.
        // `tower_http`'s response line carries the status this refusal was given and is written on
        // this thread, so it is the honest stand-in. Matched by message rather than status field,
        // because this loop runs both renderings and only one writes fields as JSON.
        assert!(rendered.contains("finished processing request"), "{rendered}");
    }
}
