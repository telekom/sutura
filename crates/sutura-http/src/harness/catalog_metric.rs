//! `GET /v1/catalog?metric=...`, through the assembled router - the metric-pool twin of
//! [`super::catalog_model`], `github.com/telekom/sutura#2071`'s second ask.

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;

use super::{app, over, settings};
use crate::testing::{call, fake_warehouse, request, two_source_bundle};

#[tokio::test]
async fn the_catalog_route_narrows_to_one_named_metric() {
    let app = over(two_source_bundle(), fake_warehouse(), settings(Environment::Development, ""));

    let (status, body) = call(&app, request("GET", "/v1/catalog?metric=revenue", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("revenue"), "{body}");
    assert!(
        !body.contains(r#""metrics":[]"#),
        "a visible metric must narrow to itself, not an empty list: {body}"
    );
}

#[tokio::test]
async fn a_metric_this_catalog_does_not_define_is_refused_not_found() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(&app, request("GET", "/v1/catalog?metric=does_not_exist", None, Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains(r#""code":"metric_unknown""#), "{body}");
}

#[tokio::test]
async fn a_metric_name_that_does_not_parse_is_refused_as_not_a_question() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(&app, request("GET", "/v1/catalog?metric=not%20a%20name", None, Body::empty())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
}

#[tokio::test]
async fn model_and_metric_given_together_is_refused_as_not_a_question() {
    let app = over(two_source_bundle(), fake_warehouse(), settings(Environment::Development, ""));
    let (status, body) = call(
        &app,
        request("GET", "/v1/catalog?model=orders&metric=revenue", None, Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
    assert!(body.contains("model") && body.contains("metric"), "{body}");
}
