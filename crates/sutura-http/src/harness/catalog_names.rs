//! `GET /v1/catalog?names=true`, through the assembled router - the four refusals
//! `routes/v1/catalog.rs` added alongside the index, and a paged walk past the byte cap.
//!
//! Its own file for `catalog_prose`'s reason: the helpers it needs are this module's.

use std::collections::BTreeSet;

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Definitions, Metric, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, Grain, MAX_IDENTIFIER_LEN, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use super::{app, over, settings};
use crate::testing::{call, column, description, fake_warehouse, request};

/// `metric_count` metrics on one model, every name padded to [`MAX_IDENTIFIER_LEN`] - the
/// worst-case-cursor bundle a paged walk past the byte cap needs, large enough to force several
/// pages at `MAX_CATALOG_LISTING_BYTES`.
fn bundle_with_many_metrics_at_max_name_len(metric_count: usize) -> PinnedDefinitions {
    fn padded(prefix: &str, index: usize) -> String {
        let core = format!("{prefix}{index:05}");
        let fill = "x".repeat(MAX_IDENTIFIER_LEN - core.len());
        format!("{core}{fill}")
    }
    let owner = ModelName::parse("orders").expect("a test model name is a name");
    let model = Model::new(
        owner.clone(),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table name is a name"),
        BTreeSet::from([column("amount_cents")]),
        description("Orders, one row per order."),
    );
    let metrics = (0..metric_count)
        .map(|index| {
            let name = padded("x", index);
            Metric::new(
                MetricName::parse(&name).expect("a generated test metric name is a name"),
                owner.clone(),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
                Vec::new(),
                column("amount_cents"),
                BTreeSet::from([Grain::Day]),
                Vec::new(),
                None,
                description("x"),
                Audience::Open,
            )
            .expect("no dimensions to duplicate")
        })
        .collect();
    let definitions = Definitions::assemble(vec![model], vec![], metrics).expect("the generated test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}

#[tokio::test]
async fn names_given_with_model_is_refused_as_not_a_question() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(
        &app,
        request("GET", "/v1/catalog?names=true&model=orders", None, Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
    assert!(body.contains("names") && body.contains("model"), "{body}");
}

#[tokio::test]
async fn names_given_with_metric_is_refused_as_not_a_question() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(
        &app,
        request("GET", "/v1/catalog?names=true&metric=revenue", None, Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
}

#[tokio::test]
async fn model_prefix_given_with_model_is_refused_as_not_a_question() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(
        &app,
        request("GET", "/v1/catalog?model_prefix=ord&model=orders", None, Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
    assert!(body.contains("model_prefix"), "{body}");
}

#[tokio::test]
async fn model_prefix_given_with_metric_is_refused_as_not_a_question() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(
        &app,
        request("GET", "/v1/catalog?model_prefix=ord&metric=revenue", None, Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
}

#[tokio::test]
async fn after_without_names_is_refused_as_not_a_question() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(&app, request("GET", "/v1/catalog?after=orders", None, Body::empty())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
    assert!(body.contains("after") && body.contains("names"), "{body}");
}

#[tokio::test]
async fn empty_model_prefix_is_refused_as_not_a_question() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(
        &app,
        request("GET", "/v1/catalog?names=true&model_prefix=", None, Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains(r#""code":"not_a_question""#), "{body}");
    assert!(body.contains("model_prefix"), "{body}");
}

#[tokio::test]
async fn walking_names_after_the_route_reaches_every_metric_past_the_cap() {
    const METRIC_COUNT: usize = 3000;
    let app = over(
        bundle_with_many_metrics_at_max_name_len(METRIC_COUNT),
        fake_warehouse(),
        settings(Environment::Development, ""),
    );

    let mut seen = std::collections::BTreeSet::new();
    let mut after: Option<String> = None;
    let mut pages = 0_u32;
    loop {
        pages += 1;
        assert!(pages < 10_000, "the index never terminated - a cursor is not advancing");
        let path = after.as_ref().map_or_else(
            || "/v1/catalog?names=true".to_owned(),
            |cursor| format!("/v1/catalog?names=true&after={cursor}"),
        );
        let (status, body) = call(&app, request("GET", &path, None, Body::empty())).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("a names index body is JSON");
        assert!(
            body.len() <= sutura_app::catalog_budget::MAX_CATALOG_LISTING_BYTES,
            "page {pages} overshot the byte bound: {} bytes",
            body.len()
        );
        for entry in parsed["metric_names"].as_array().expect("metric_names is a list") {
            seen.insert(entry["name"].as_str().expect("a name is a string").to_owned());
        }
        match parsed.get("next").and_then(serde_json::Value::as_str) {
            Some(cursor) => after = Some(cursor.to_owned()),
            None => break,
        }
    }
    assert!(pages > 2, "the fixture must be large enough to force more than one page");
    assert_eq!(seen.len(), METRIC_COUNT, "every metric must be reached exactly once");
}
