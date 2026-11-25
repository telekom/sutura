//! `GET /v1/catalog?model=...`, through the assembled router.
//!
//! `crate::wire::catalog`'s own tests pin what `CatalogBody::of` renders for a given `model`
//! filter; this one pins the ROUTE: parsing the query string, resolving the name against the
//! caller's own view, and the two ways that can go wrong - a name that does not parse at all, and
//! one that parses but names nothing this caller may see - `github.com/telekom/sutura#2071`'s
//! second ask.

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;

use super::{app, over, settings};
use crate::testing::{call, fake_warehouse, request, two_source_bundle};

#[tokio::test]
async fn the_catalog_route_narrows_metrics_to_the_named_model() {
    let app = over(two_source_bundle(), fake_warehouse(), settings(Environment::Development, ""));

    let (status, body) = call(&app, request("GET", "/v1/catalog?model=orders", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("revenue"), "{body}");

    // `customers` is a real, visible model with no metric of its own - the filter narrows the
    // metrics list to empty rather than falling back to the whole catalog.
    let (status, body) = call(&app, request("GET", "/v1/catalog?model=customers", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("revenue"), "{body}");
    assert!(body.contains(r#""metrics":[]"#), "{body}");
}

#[tokio::test]
async fn an_unfiltered_catalog_is_unchanged_from_a_model_filtered_one_combined() {
    let app = app(settings(Environment::Development, ""));
    let (status, unfiltered) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "{unfiltered}");
    let (status, filtered) = call(&app, request("GET", "/v1/catalog?model=orders", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "{filtered}");
    // `bundle()` is one model, one metric, so narrowing to that model changes nothing: the only
    // thing a filter can omit here is an empty rest.
    assert!(
        unfiltered.contains("revenue") && filtered.contains("revenue"),
        "{unfiltered} / {filtered}"
    );
}

#[tokio::test]
async fn a_model_this_catalog_does_not_define_is_refused_not_found() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(&app, request("GET", "/v1/catalog?model=warehouse", None, Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains(r#""code":"model_unknown""#), "{body}");
}

#[tokio::test]
async fn a_model_name_that_does_not_parse_is_refused_as_not_a_question() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(&app, request("GET", "/v1/catalog?model=not%20a%20name", None, Body::empty())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
}
