//! A host that will not supply randomness, through the assembled router.
//!
//! Under `shared-service-user` a job runs as the deployment, and its `sutura_correlation` label is
//! the only thing that joins it to the audit record of who asked. So a request with no drawn id is
//! refused before the warehouse is asked, never answered with an unlabelled job.

use std::sync::Arc;

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;

use super::settings;
use crate::testing::{call, request};

/// The draw on a host whose random source answers nothing.
fn no_randomness(_bits: &mut [u8; 16]) -> Result<(), getrandom::Error> {
    Err(getrandom::Error::UNSUPPORTED)
}

#[tokio::test]
async fn a_host_with_no_randomness_refuses_the_question_before_any_job_runs() {
    let (warehouses, held) = crate::testing::warehouse_that_can_be_held();
    let service = crate::surface::LocalService::start(
        &crate::testing::catalog_of(crate::testing::bundle()),
        warehouses,
        crate::testing::sink(),
        crate::testing::broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    let state = crate::testing::state_over(Arc::new(service), settings(Environment::Development, "")).drawing_from(no_randomness);
    let app = crate::router(&state).expect("the test router assembles");
    let (status, body) = call(
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains(r#""code":"no_randomness""#), "{body}");
    assert_eq!(held.labelled(), None, "a job ran with no id to label it");
}
