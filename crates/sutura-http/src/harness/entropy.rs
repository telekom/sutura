//! A host that will not supply randomness, through the assembled router.
//!
//! Under `shared-service-user` a job runs as the deployment, and its `sutura_correlation` label is
//! the only thing that joins it to the audit record of who asked. So a request with no drawn id is
//! refused before the warehouse is asked, never answered with an unlabelled job.

use axum::body::Body;
use axum::http::StatusCode;

use crate::testing::{call, request};

/// The draw on a host whose random source answers nothing.
fn no_randomness(_bits: &mut [u8; 16]) -> Result<(), getrandom::Error> {
    Err(getrandom::Error::UNSUPPORTED)
}

#[tokio::test]
async fn a_host_with_no_randomness_refuses_the_question_before_any_job_runs() {
    let (app, held) = super::fixtures::held_drawing_from(no_randomness);
    let (status, body) = call(
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains(r#""code":"no_randomness""#), "{body}");
    assert_eq!(held.labelled(), None, "a job ran with no id to label it");
}
