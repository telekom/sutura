//! `POST /v1/sql/run` through the assembled router - `docs/adr/0013`'s tool, off by default.
//!
//! **`#666`'s review, finding 2.** The only existing coverage of the deployment switch and the
//! scope gate was over `crate::capability::permitted_for` and `crate::capability::governed`
//! directly, never over a REQUEST: a regression in `require_capability`'s own wiring - the one
//! place a deployment's `tools.run_sql.enabled` actually reaches a response - reddened nothing.
//! These go through the real router, the way every other status in this suite is proved.

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;

use super::{over, settings};
use crate::testing::{bundle, call, fake_warehouse, request};

fn run_sql_body(statement: &str) -> String {
    format!(r#"{{"statement":{statement:?}}}"#)
}

#[tokio::test]
async fn a_default_deployment_refuses_run_sql_as_not_enabled_rather_than_as_a_missing_scope() {
    // Default settings: no `tools:` key, which `docs/adr/0013` calls the normal case. No
    // `security.inbound` either, so this caller holds every OTHER capability - and still may not
    // reach this one, which is the property a per-caller scope alone cannot provide.
    let app = over(bundle(), fake_warehouse(), settings(Environment::Development, ""));
    let (status, body) = call(
        &app,
        request("POST", "/v1/sql/run", None, Body::from(run_sql_body("select 1"))),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains(r#""code":"tool_not_enabled""#), "{body}");
    // NOT the scope-refusal code: obtaining `sutura:sql.run` would not help here, and the body must
    // not tell a caller to go get it.
    assert!(!body.contains("insufficient_scope"), "{body}");
}

#[tokio::test]
async fn a_deployment_that_turns_run_sql_on_answers_it() {
    let app = over(
        bundle(),
        fake_warehouse(),
        settings(Environment::Development, "tools:\n  run_sql:\n    enabled: true\n"),
    );
    let (status, body) = call(
        &app,
        request("POST", "/v1/sql/run", None, Body::from(run_sql_body("select 1"))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""outcome":"raw_rows""#), "{body}");
    // Never a certified shape, however this call answers: `#666`'s review found this claim
    // overstated as "shares no field name" - `columns`/`rows` ARE shared - so what is asserted
    // here is the property that actually holds.
    assert!(!body.contains("provenance"), "{body}");
    assert!(!body.contains("definition_digest"), "{body}");
}

#[tokio::test]
async fn a_result_over_the_row_cap_is_a_413_through_the_real_router() {
    // `FakeWarehouse::execute_raw`'s own magic string, past `sutura_domain::plan::MAX_ROWS` by one
    // row - the boundary the plan's own row cap names, reached this time through `run_sql` rather
    // than through a typed `RawRefusalReason::TooManyRows` literal.
    let app = over(
        bundle(),
        fake_warehouse(),
        settings(Environment::Development, "tools:\n  run_sql:\n    enabled: true\n"),
    );
    let (status, body) = call(
        &app,
        request("POST", "/v1/sql/run", None, Body::from(run_sql_body("too many rows"))),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert!(body.contains(r#""outcome":"raw_refusal""#), "{body}");
    assert!(body.contains(r#""code":"too_many_rows""#), "{body}");
}

#[tokio::test]
async fn a_statement_the_data_system_refuses_is_a_403_carrying_the_domain_shape() {
    // The OTHER `403` on this route - `outcome: refusal`, not `code` with no `outcome` - so a
    // client cannot confuse the data system's own refusal with the deployment switch or the scope
    // gate above; the body shape is the only thing that tells the three apart at one status.
    let app = over(
        bundle(),
        fake_warehouse(),
        settings(Environment::Development, "tools:\n  run_sql:\n    enabled: true\n"),
    );
    let (status, body) = call(
        &app,
        request("POST", "/v1/sql/run", None, Body::from(run_sql_body("refuse me"))),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains(r#""outcome":"raw_refusal""#), "{body}");
    assert!(body.contains(r#""code":"source_refused""#), "{body}");
}
