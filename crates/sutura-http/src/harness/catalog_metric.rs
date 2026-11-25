//! `GET /v1/catalog?metric=...`, through the assembled router - the metric-pool twin of
//! [`super::catalog_model`], `github.com/telekom/sutura#2071`'s second ask.

use std::collections::BTreeSet;

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Definitions, Metric, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use super::{app, over, settings};
use crate::testing::{call, column, description, fake_warehouse, request, source, two_source_bundle};

/// Two models, each owning its own metric - never one metric alone, or asserting the OTHER one's
/// absence would restate that it never existed rather than prove the `metric` filter ran.
fn two_metric_bundle() -> PinnedDefinitions {
    #[expect(
        clippy::needless_collect,
        reason = "the BTreeSet is the fourth positional argument `Model::new` takes, not an intermediate \
                  this closure iterates again - there is nothing to fuse it with"
    )]
    let model = |name: &str, columns: &[&str], prose: &str| {
        Model::new(
            ModelName::parse(name).expect("a test model is a model"),
            source(),
            TableName::parse(name).expect("a test table is a table"),
            columns.iter().copied().map(column).collect::<BTreeSet<_>>(),
            description(prose),
        )
    };
    let metric = |name: &str, owner: &str, aggregate: Aggregate, column_name: &str, grains: &[Grain], prose: &str| {
        Metric::new(
            MetricName::parse(name).expect("a test metric is a metric"),
            ModelName::parse(owner).expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(aggregate, column(column_name)))),
            Vec::new(),
            column(column_name),
            grains.iter().copied().collect::<BTreeSet<_>>(),
            Vec::new(),
            None,
            description(prose),
            Audience::Open,
        )
        .expect("no dimensions to duplicate")
    };
    let models = vec![
        model("orders", &["amount_cents", "order_date"], "Orders, one row per order."),
        model("customers", &["customer_id"], "Customers, one row per customer."),
    ];
    let metrics = vec![
        metric(
            "revenue",
            "orders",
            Aggregate::Sum,
            "amount_cents",
            &[Grain::Day, Grain::Month],
            "Revenue, in minor units.",
        ),
        metric(
            "customer_count",
            "customers",
            Aggregate::Count,
            "customer_id",
            &[Grain::Day],
            "How many customers, counted once each.",
        ),
    ];
    let definitions = Definitions::assemble(models, vec![], metrics).expect("the test bundle is consistent");
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
async fn the_catalog_route_narrows_to_one_named_metric() {
    let app = over(two_metric_bundle(), fake_warehouse(), settings(Environment::Development, ""));

    let (status, body) = call(&app, request("GET", "/v1/catalog?metric=revenue", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("revenue"), "{body}");
    assert!(!body.contains("customer_count"), "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("a catalog body is JSON");
    assert_eq!(parsed["metrics"].as_array().expect("metrics is an array").len(), 1, "{body}");
}

#[tokio::test]
async fn a_metric_this_catalog_does_not_define_is_refused_not_found() {
    let app = app(settings(Environment::Development, ""));
    let (status, body) = call(&app, request("GET", "/v1/catalog?metric=does_not_exist", None, Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains(r#""code":"catalog_metric_unknown""#), "{body}");
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
