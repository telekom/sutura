//! `GET /v1/catalog` under a REAL `Scope::Granted` caller, through the assembled router.
//!
//! [`super::catalog_model`] and [`super::catalog_metric`] only ever narrow under
//! `Scope::Everything` - no inbound gate at all, so every metric this test crate defines is visible
//! to begin with. These three close the same narrowing and refusal properties under a real
//! `docs/adr/0028` audience mapping: a `groups` claim mapped to `finance`, verified through a real
//! `InboundGate`, over a model that owns no open metric at all - the one shape `catalog_model`'s own
//! `customers` fixture (an open model with no metric) cannot stand in for.

use axum::body::Body;
use axum::http::StatusCode;
use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, AudienceGrant, Definitions, Metric, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, AudienceId, Grain, MetricName, ModelName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use crate::testing::{
    Answered, an_issuer, answer, broker, column, declared_inbound, description, direct_overlay, every_scope, fake_warehouse,
    metric_name, request, serving, settings_with, source,
};

/// A model whose only metric is [`restricted_only_metric_name`] - unlike `orders`, which keeps its
/// open `revenue`, this model owns no open metric, so an outsider's `?model=` on it must read as
/// unknown rather than merely empty.
fn restricted_only_model_name() -> ModelName {
    ModelName::parse("forecasts").expect("a test model is a model")
}

/// The one metric [`restricted_only_model_name`] owns, restricted to `finance`.
fn restricted_only_metric_name() -> MetricName {
    MetricName::parse("forecast_total").expect("a test metric is a metric")
}

/// A metric restricted to `finance`, owned by `orders` alongside its open `revenue` - unlike
/// [`restricted_only_metric_name`], this model also keeps an open metric, so a caller outside
/// `finance` reads `?model=orders` as present with a narrower metrics list, never as unknown.
fn finance_metric_name() -> MetricName {
    MetricName::parse("finance_only").expect("a test metric is a metric")
}

/// Two models - `orders` (open `revenue`, finance-restricted `finance_only`) and `forecasts`
/// (finance-restricted `forecast_total` only) - so a finance-mapped caller's `?model=` narrowing can
/// be told apart from a model-level restriction.
fn bundle_with_a_restricted_only_model() -> PinnedDefinitions {
    let orders = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        std::collections::BTreeSet::from([column("amount_cents"), column("order_date")]),
        description("Orders, one row per order."),
    );
    let restricted_to_finance = || {
        Audience::Restricted(
            AudienceGrant::parse(std::collections::BTreeSet::from([
                AudienceId::parse("finance").expect("a test audience id is one")
            ]))
            .expect("one id grants"),
        )
    };
    let revenue = Metric::new(
        metric_name(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        std::collections::BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        description("Revenue, in minor units."),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let finance_only = Metric::new(
        finance_metric_name(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        std::collections::BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        description("Only a finance-granted caller may see this - its model also owns an open metric."),
        restricted_to_finance(),
    )
    .expect("no dimensions to duplicate");
    let forecasts = Model::new(
        restricted_only_model_name(),
        source(),
        TableName::parse("forecasts").expect("a test table is a table"),
        std::collections::BTreeSet::from([column("period"), column("forecast_cents")]),
        description("Forecasts, one row per period - this catalog's only model with no open metric."),
    );
    let forecast_total = Metric::new(
        restricted_only_metric_name(),
        restricted_only_model_name(),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Sum,
            column("forecast_cents"),
        ))),
        Vec::new(),
        column("period"),
        std::collections::BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        description("A forecast only a finance-granted caller may see; its model owns nothing else."),
        restricted_to_finance(),
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![orders, forecasts], vec![], vec![revenue, finance_only, forecast_total])
        .expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            source(),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}

/// [`crate::testing::direct_overlay`], plus `docs/adr/0028`'s mapping: a `groups` claim value of
/// `finance-team` grants the `finance` audience.
fn direct_overlay_granting_finance(issuer: &MockIssuer, key_set_path: &str) -> String {
    format!(
        "{}  audience_mapping:\n    finance-team: [\"finance\"]\n",
        direct_overlay(issuer, key_set_path)
    )
}

/// A router serving [`bundle_with_a_restricted_only_model`], behind a real leg-1 gate that maps a
/// `groups` claim of `finance-team` onto the `finance` audience.
fn app(issuer: &MockIssuer, published: &PublishedKeySet) -> axum::Router {
    let settings = settings_with(&direct_overlay_granting_finance(issuer, &published.path().to_string_lossy()));
    let gate = crate::InboundGate::from_declaration(&declared_inbound(&settings)).expect("a published key set builds a gate");
    serving(
        bundle_with_a_restricted_only_model(),
        fake_warehouse(),
        broker(),
        settings,
        Some(gate),
    )
}

/// A token for a caller whose `groups` claim grants the `finance` audience, with every scope.
fn a_finance_caller(issuer: &MockIssuer) -> String {
    issuer
        .mint(
            &Token::for_subject("finance-caller@example.com")
                .granting(&every_scope())
                .claiming("groups", serde_json::json!(["finance-team"])),
        )
        .expect("the issuer signs a token")
}

/// `GET /v1/catalog?<query>`, carrying `token`. Returns the status, every header and the whole
/// body - a status-and-body pair alone cannot prove two responses are identical down to a header a
/// caller's own client would also see.
async fn catalog_full(app: &axum::Router, token: &str, query: &str) -> Answered {
    answer(
        app,
        request("GET", &format!("/v1/catalog?{query}"), Some(token), Body::empty()),
    )
    .await
}

/// A name no catalog declares, the same length as `real` - so a byte-for-byte comparison against a
/// real, hidden name is not upset by `Content-Length` alone.
fn same_length_absent_name(real: &str) -> String {
    "z".repeat(real.len())
}

#[tokio::test]
async fn a_restricted_only_model_is_refused_byte_identically_to_an_unknown_one() {
    // `forecasts` owns one metric and it is restricted - unlike `orders`, which an outsider can
    // still name because its own `revenue` stays open. An outsider's `?model=forecasts` has to read
    // as `model_unknown`, with the same status, body and EVERY header a model this catalog never
    // declared at all would produce.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "restricted-model-byte-identical").expect("the key set publishes");
    let app = app(&issuer, &published);
    let outsider = issuer
        .mint(&Token::for_subject("outsider@example.com").granting(&every_scope()))
        .expect("the issuer signs a token");
    let absent = same_length_absent_name(restricted_only_model_name().as_str());

    let restricted = catalog_full(&app, &outsider, &format!("model={}", restricted_only_model_name())).await;
    let unknown = catalog_full(&app, &outsider, &format!("model={absent}")).await;

    assert_eq!(restricted.status, StatusCode::NOT_FOUND, "{}", restricted.body);
    assert_eq!(restricted.status, unknown.status);
    assert_eq!(
        restricted.headers, unknown.headers,
        "a restricted-only model must read as unknown, header for header"
    );

    let restricted_json: serde_json::Value = serde_json::from_str(&restricted.body).expect("a refusal is JSON");
    let unknown_json: serde_json::Value = serde_json::from_str(&unknown.body).expect("a refusal is JSON");
    assert_eq!(restricted_json["code"], "model_unknown", "{}", restricted.body);
    // Only the echoed model name may differ between the two bodies.
    let restricted_detail = restricted_json["detail"]
        .as_str()
        .expect("a refusal carries a detail")
        .replace(restricted_only_model_name().as_str(), &absent);
    assert_eq!(
        restricted_detail,
        unknown_json["detail"].as_str().expect("a refusal carries a detail"),
        "a restricted-only model and an unknown one must read as the same refusal"
    );
}

#[tokio::test]
async fn a_restricted_metric_is_refused_byte_identically_to_an_unknown_one_over_catalog() {
    // The `/v1/catalog` twin of `crate::identity_e2e`'s own `/v1/query` property - same shape, this
    // route's `UnknownCatalogNameBody` instead, and every header compared too. `orders` keeps its
    // open `revenue`, so this is the metric-level restriction and not the model-level one above.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "restricted-metric-byte-identical-catalog").expect("the key set publishes");
    let app = app(&issuer, &published);
    let outsider = issuer
        .mint(&Token::for_subject("outsider@example.com").granting(&every_scope()))
        .expect("the issuer signs a token");
    let absent = same_length_absent_name(finance_metric_name().as_str());

    let restricted = catalog_full(&app, &outsider, &format!("metric={}", finance_metric_name())).await;
    let unknown = catalog_full(&app, &outsider, &format!("metric={absent}")).await;

    assert_eq!(restricted.status, StatusCode::NOT_FOUND, "{}", restricted.body);
    assert_eq!(restricted.status, unknown.status);
    assert_eq!(
        restricted.headers, unknown.headers,
        "a restricted metric must read as unknown, header for header"
    );

    let restricted_json: serde_json::Value = serde_json::from_str(&restricted.body).expect("a refusal is JSON");
    let unknown_json: serde_json::Value = serde_json::from_str(&unknown.body).expect("a refusal is JSON");
    assert_eq!(restricted_json["code"], "catalog_metric_unknown", "{}", restricted.body);
    let restricted_detail = restricted_json["detail"]
        .as_str()
        .expect("a refusal carries a detail")
        .replace(finance_metric_name().as_str(), &absent);
    assert_eq!(
        restricted_detail,
        unknown_json["detail"].as_str().expect("a refusal carries a detail"),
        "a restricted metric and an unknown one must read as the same refusal"
    );
}

#[tokio::test]
async fn a_granted_callers_model_filter_narrows_to_that_models_own_metrics() {
    // `super::catalog_model`'s narrowing test only ever runs unauthenticated, i.e.
    // `Scope::Everything` - this is the same property under a real `Scope::Granted` view: a
    // finance-mapped caller sees both `orders` and `forecasts`, and `?model=forecasts` must not leak
    // `orders`' own `revenue` in with it.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "granted-model-scoping").expect("the key set publishes");
    let app = app(&issuer, &published);
    let finance = a_finance_caller(&issuer);

    let answered = catalog_full(&app, &finance, &format!("model={}", restricted_only_model_name())).await;
    let body = answered.body;
    assert_eq!(answered.status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("a catalog body is JSON");
    let metrics = parsed["metrics"].as_array().expect("metrics is an array");
    assert_eq!(metrics.len(), 1, "{body}");
    assert_eq!(metrics[0]["name"], restricted_only_metric_name().as_str(), "{body}");
}
