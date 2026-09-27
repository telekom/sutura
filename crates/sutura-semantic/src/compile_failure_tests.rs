//! Test cells for `CompileFailure::Bundle` variants no test provokes through the public
//! [`compile`](crate::compile) entry point.
//!
//! `Definitions::assemble` checks every cross-reference at load, so no valid `PinnedDefinitions`
//! can carry a metric naming an absent model, a dimension chain naming an absent relationship, or
//! a relationship naming an absent target model. These variants exist so a future edit that breaks
//! one of those invariants surfaces as a typed error rather than a silent mis-plan. Each cell below
//! builds a broken `PinnedDefinitions` via `Definitions::from_parts` (behind the `test-fakes`
//! feature, which bypasses `assemble`), hands it to `compile` as a `ScopedView`, and asserts the
//! typed `BundleInconsistent` variant. The cell is a fake that simulates what a buggy catalog
//! adapter could produce; `compile`'s own resolve stage is the reader under test.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{
    Audience, Column, Definitions, Description, Dimension, JoinKey, JoinKeys, Metric, Model, Relationship, ViaChain,
};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::Query;

use crate::{BundleInconsistent, CompileFailure, compile};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn metric_name(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a test metric name is a name")
}

fn model_name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model name is a name")
}

fn dimension_name(raw: &str) -> DimensionName {
    DimensionName::parse(raw).expect("a test dimension name is a name")
}

fn relationship_name(raw: &str) -> RelationshipName {
    RelationshipName::parse(raw).expect("a test relationship name is a name")
}

fn source_name(raw: &str) -> SourceName {
    SourceName::parse(raw).expect("a test source name is a source name")
}

fn table_name(raw: &str) -> TableName {
    TableName::parse(raw).expect("a test table name is a table")
}

fn model(name: &str, source: &str, table: &str, columns: &[&str]) -> Model {
    Model::new(
        model_name(name),
        source_name(source),
        table_name(table),
        columns
            .iter()
            .map(|c| Column::new(column(c), None, Description::default(), None)),
        Description::default(),
    )
}

fn relationship(name: &str, from: (&str, &str), to: (&str, &str)) -> Relationship {
    Relationship::new(
        relationship_name(name),
        model_name(from.0),
        model_name(to.0),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: column(from.1),
            target: column(to.1),
        }])
        .expect("a test relationship declares one key"),
    )
}

fn dimension(name: &str, col: &str, via: Option<ViaChain>) -> Dimension {
    Dimension::new(dimension_name(name), column(col), via, None, Description::default())
}

/// A metric that sums `amount_cents` on the `orders` model, grouped by month, with one dimension
/// when `dim` is set.
fn revenue_metric(dim: Option<Dimension>) -> Metric {
    let dims = dim.into_iter().collect();
    Metric::new(
        metric_name("revenue"),
        model_name("orders"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        dims,
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("a test metric is a metric")
}

/// A `PinnedDefinitions` built from `Definitions::from_parts`, bypassing `assemble`'s
/// cross-reference checks. This is the fake: it simulates what a buggy catalog adapter could
/// produce - a bundle where the cross-references do not hold.
fn pin_broken(definitions: Definitions) -> PinnedDefinitions {
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-broken").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(source_name("local"), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the test definitions hash")
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

fn asking_revenue(by: Vec<DimensionName>) -> Query {
    Query::single(metric_name("revenue"), Grain::Month, june(), by, Vec::new())
}

/// `compile` returns `BundleInconsistent::NoSuchModel` when a metric names a model the pinned
/// bundle does not hold - a cross-reference `assemble` would have refused at load.
///
/// The metric "revenue" is in the definitions' metric map (so `view.metric` finds it), but its
/// model is "ghost", which is absent from the model map. `resolve` looks up the first metric's
/// model and reaches `BundleInconsistent::NoSuchModel`.
#[test]
fn a_metric_naming_an_absent_model_is_a_broken_bundle_not_a_refusal() {
    let orders = model("orders", "local", "orders", &["amount_cents", "order_date"]);
    // The metric names "ghost" but only "orders" is in the model map.
    let metric = Metric::new(
        metric_name("revenue"),
        model_name("ghost"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("a test metric is a metric");

    let mut metrics = BTreeMap::new();
    metrics.insert(metric_name("revenue"), metric);
    let mut models = BTreeMap::new();
    models.insert(model_name("orders"), orders);

    let broken = pin_broken(Definitions::from_parts(models, BTreeMap::new(), metrics));
    let view = ScopedView::everything(&broken);

    let Err(failure) = compile(&asking_revenue(Vec::new()), &view, RowCeiling::DEFAULT) else {
        panic!("a metric naming an absent model must be a broken bundle, not a plan");
    };
    assert!(
        matches!(failure, CompileFailure::Bundle(BundleInconsistent::NoSuchModel { ref metric, ref model }) if *metric == metric_name("revenue") && *model == model_name("ghost")),
        "expected NoSuchModel for revenue/ghost, got {failure:?}"
    );
}

/// `compile` returns `BundleInconsistent::RelationshipAbsent` when a dimension's via chain names a
/// relationship the pinned bundle does not hold - a cross-reference `assemble` would have refused
/// at load.
///
/// The metric "revenue" declares dimension "region" with via chain `["missing"]`, but no
/// relationship "missing" exists in the definitions. `resolve_dimension` looks up the relationship
/// and reaches `BundleInconsistent::RelationshipAbsent`.
#[test]
fn a_dimension_chain_naming_an_absent_relationship_is_a_broken_bundle() {
    let orders = model("orders", "local", "orders", &["amount_cents", "order_date", "region_key"]);
    let via = ViaChain::of(vec![relationship_name("missing")]).expect("a non-empty chain is a chain");
    let region = dimension("region", "label", Some(via));
    let metric = revenue_metric(Some(region));

    let mut models = BTreeMap::new();
    models.insert(model_name("orders"), orders);
    let mut metrics = BTreeMap::new();
    metrics.insert(metric_name("revenue"), metric);

    let broken = pin_broken(Definitions::from_parts(models, BTreeMap::new(), metrics));
    let view = ScopedView::everything(&broken);

    let Err(failure) = compile(&asking_revenue(vec![dimension_name("region")]), &view, RowCeiling::DEFAULT) else {
        panic!("a dimension chain naming an absent relationship must be a broken bundle");
    };
    assert!(
        matches!(failure, CompileFailure::Bundle(BundleInconsistent::RelationshipAbsent { ref dimension }) if *dimension == dimension_name("region")),
        "expected RelationshipAbsent for region, got {failure:?}"
    );
}

/// `compile` returns `BundleInconsistent::JoinTargetMissing` when a relationship names a target
/// model the pinned bundle does not hold - a cross-reference `assemble` would have refused at
/// load.
///
/// The metric "revenue" declares dimension "region" with via chain `["to_ghost"]`. The
/// relationship `to_ghost` exists in the definitions but its target model `ghost` does not.
/// `resolve_dimension` looks up the relationship (succeeds), then looks up the target model and
/// reaches `BundleInconsistent::JoinTargetMissing`.
#[test]
fn a_relationship_naming_an_absent_target_model_is_a_broken_bundle() {
    let orders = model("orders", "local", "orders", &["amount_cents", "order_date", "region_key"]);
    // The relationship names "ghost" as its target model, which is not in the model map.
    let to_ghost = relationship("to_ghost", ("orders", "region_key"), ("ghost", "region_key"));
    let via = ViaChain::of(vec![relationship_name("to_ghost")]).expect("a non-empty chain is a chain");
    let region = dimension("region", "label", Some(via));
    let metric = revenue_metric(Some(region));

    let mut models = BTreeMap::new();
    models.insert(model_name("orders"), orders);
    let mut rels = BTreeMap::new();
    rels.insert(relationship_name("to_ghost"), to_ghost);
    let mut metrics = BTreeMap::new();
    metrics.insert(metric_name("revenue"), metric);

    let broken = pin_broken(Definitions::from_parts(models, rels, metrics));
    let view = ScopedView::everything(&broken);

    let Err(failure) = compile(&asking_revenue(vec![dimension_name("region")]), &view, RowCeiling::DEFAULT) else {
        panic!("a relationship naming an absent target model must be a broken bundle");
    };
    assert!(
        matches!(failure, CompileFailure::Bundle(BundleInconsistent::JoinTargetMissing { ref model }) if *model == model_name("ghost")),
        "expected JoinTargetMissing for ghost, got {failure:?}"
    );
}
