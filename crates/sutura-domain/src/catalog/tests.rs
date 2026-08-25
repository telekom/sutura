//! Tests for the declarations in [`super`] and for the consistency checks over them.
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape. The reason is
//! mechanical: the module it tests plus these cases is over the thousand-line limit
//! `cargo xtask max-lines` enforces, and the only way past that gate is to split the file. Same
//! arrangement as `xtask/src/boundaries/api_shape.rs`.

use std::collections::{BTreeMap, BTreeSet};

use super::{Definitions, Dimension, InconsistentDefinitions, Metric, Model, Relationship, TIME_BUCKET_LABEL};
use crate::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, Measure, MetricName, ModelName, RelationshipName, SourceName,
    TableName,
};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn model_name(raw: &str) -> ModelName {
    ModelName::parse(raw).expect("a test model is a model")
}

fn metric_name(raw: &str) -> MetricName {
    MetricName::parse(raw).expect("a test metric is a metric")
}

fn dimension_name(raw: &str) -> DimensionName {
    DimensionName::parse(raw).expect("a test dimension is a dimension")
}

fn relationship_name(raw: &str) -> RelationshipName {
    RelationshipName::parse(raw).expect("a test relationship is a relationship")
}

/// A model on `source` holding exactly `columns`.
fn model(name: &str, source: &str, columns: &[&str]) -> Model {
    Model::new(
        model_name(name),
        SourceName::parse(source).expect("a test source is a source"),
        TableName::parse(name).expect("a test table is a table"),
        columns.iter().map(|c| column(c)).collect::<BTreeSet<_>>(),
        String::new(),
    )
}

/// The models and relationships a catalog is assembled from.
///
/// A named pair rather than an inline tuple, because the complexity threshold in `clippy.toml` is
/// set low enough to catch one and it is right to: a bare two-vector tuple says nothing about which
/// vector is which.
type ModelsAndJoins = (Vec<Model>, Vec<Relationship>);

/// `orders` and `customers`, both local, joined many-to-one on `customer_id`.
fn two_models() -> ModelsAndJoins {
    (
        vec![
            model("orders", "local", &["amount_cents", "order_date", "customer_id"]),
            model("customers", "local", &["id", "region_code"]),
        ],
        vec![Relationship::new(
            relationship_name("orders_customer"),
            model_name("orders"),
            column("customer_id"),
            model_name("customers"),
            column("id"),
            JoinType::ManyToOne,
        )],
    )
}

/// A `sum(amount_cents)` metric over `orders`, with the given dimensions.
fn metric(name: &str, dimensions: Vec<Dimension>) -> Metric {
    Metric::new(
        metric_name(name),
        model_name("orders"),
        Measure::new(Aggregate::Sum, column("amount_cents")),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        dimensions
            .into_iter()
            .map(|d| (d.name().clone(), d))
            .collect::<BTreeMap<_, _>>(),
        None,
        String::new(),
    )
}

fn dimension(name: &str, col: &str, via: Option<&str>, values: Option<&[&str]>) -> Dimension {
    Dimension::new(
        dimension_name(name),
        column(col),
        via.map(relationship_name),
        values.map(|v| v.iter().map(|s| String::from(*s)).collect::<BTreeSet<_>>()),
        String::new(),
    )
}

/// Assembles with the two standard models and one metric.
fn assemble_one(m: Metric) -> Result<Definitions, InconsistentDefinitions> {
    let (models, relationships) = two_models();
    Definitions::assemble(models, relationships, vec![m])
}

#[test]
fn a_consistent_catalog_assembles_and_is_addressable_by_name() {
    let definitions = assemble_one(metric(
        "revenue",
        vec![
            dimension("region", "region_code", Some("orders_customer"), Some(&["north"])),
            dimension("day_of", "order_date", None, None),
        ],
    ))
    .expect("this catalog holds together");
    assert!(definitions.metric(&metric_name("revenue")).is_some());
    assert!(definitions.model(&model_name("customers")).is_some());
    assert!(definitions.relationship(&relationship_name("orders_customer")).is_some());
    assert_eq!(definitions.metrics().len(), 1);
}

#[test]
fn a_duplicated_declaration_is_refused_rather_than_letting_the_second_win() {
    // Why `assemble` takes vectors and not maps: a caller that built a map first has already
    // dropped one of the pair, and "the second definition of revenue won" is not something to
    // discover from a number.
    let (models, relationships) = two_models();
    assert_eq!(
        Definitions::assemble(
            models,
            relationships,
            vec![metric("revenue", vec![]), metric("revenue", vec![])]
        )
        .unwrap_err(),
        InconsistentDefinitions::DuplicateMetric {
            metric: metric_name("revenue")
        }
    );
    let mut twice = two_models().0;
    twice.push(model("orders", "local", &["amount_cents", "order_date", "customer_id"]));
    assert_eq!(
        Definitions::assemble(twice, vec![], vec![]).unwrap_err(),
        InconsistentDefinitions::DuplicateModel {
            model: model_name("orders")
        }
    );
}

#[test]
fn a_metric_naming_a_column_its_model_does_not_have_is_refused() {
    // The check that lets the resolver assume a column exists. Without it the failure arrives
    // from the data system at query time, as a message about SQL rather than about a catalog.
    let broken = Metric::new(
        metric_name("revenue"),
        model_name("orders"),
        Measure::new(Aggregate::Sum, column("not_a_column")),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        BTreeMap::new(),
        None,
        String::new(),
    );
    assert_eq!(
        assemble_one(broken).unwrap_err(),
        InconsistentDefinitions::UnknownMeasureColumn {
            metric: metric_name("revenue"),
            model: model_name("orders"),
            column: column("not_a_column"),
        }
    );
}

#[test]
fn a_metric_with_no_grain_is_refused_because_no_question_could_resolve() {
    let grainless = Metric::new(
        metric_name("revenue"),
        model_name("orders"),
        Measure::new(Aggregate::Sum, column("amount_cents")),
        column("order_date"),
        BTreeSet::new(),
        BTreeMap::new(),
        None,
        String::new(),
    );
    assert_eq!(
        assemble_one(grainless).unwrap_err(),
        InconsistentDefinitions::NoGrains {
            metric: metric_name("revenue")
        }
    );
}

#[test]
fn a_join_that_could_duplicate_rows_is_refused_rather_than_optimised() {
    // The important one. A one-to-many join multiplies the fact rows, so `sum` returns a larger
    // number with no error anywhere. Refusing at load is the only place this is visible before
    // somebody acts on the number.
    let (models, _) = two_models();
    let fanning = vec![Relationship::new(
        relationship_name("orders_customer"),
        model_name("orders"),
        column("customer_id"),
        model_name("customers"),
        column("id"),
        JoinType::OneToMany,
    )];
    let m = metric(
        "revenue",
        vec![dimension("region", "region_code", Some("orders_customer"), None)],
    );
    assert_eq!(
        Definitions::assemble(models, fanning, vec![m]).unwrap_err(),
        InconsistentDefinitions::JoinWouldDuplicateRows {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("orders_customer"),
        }
    );
}

#[test]
fn a_dimension_reached_through_a_relationship_that_starts_elsewhere_is_refused() {
    // A relationship from `customers` cannot be used to reach a column from a metric on
    // `orders`: the join would have no column in common with the FROM clause.
    let (models, _) = two_models();
    let backwards = vec![Relationship::new(
        relationship_name("customer_orders"),
        model_name("customers"),
        column("id"),
        model_name("orders"),
        column("customer_id"),
        JoinType::ManyToOne,
    )];
    let m = metric(
        "revenue",
        vec![dimension("region", "region_code", Some("customer_orders"), None)],
    );
    assert_eq!(
        Definitions::assemble(models, backwards, vec![m]).unwrap_err(),
        InconsistentDefinitions::RelationshipNotFromMetricModel {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("customer_orders"),
            model: model_name("orders"),
        }
    );
}

#[test]
fn an_empty_value_allowlist_is_refused_rather_than_meaning_nothing() {
    // An empty list reads as filterable and permits no value, so every filter on it would be
    // refused for a reason that describes the question rather than the catalog.
    let m = metric(
        "revenue",
        vec![dimension("region", "region_code", Some("orders_customer"), Some(&[]))],
    );
    assert_eq!(
        assemble_one(m).unwrap_err(),
        InconsistentDefinitions::EmptyAllowlist {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
        }
    );
}

#[test]
fn a_dimension_may_not_take_a_label_the_projection_already_uses() {
    // Two columns with one label is a result a caller cannot read by name: they get whichever
    // the data system listed first. Caught at load, because the caller did not choose it.
    let one_model = || vec![model("orders", "local", &["amount_cents", "order_date", "region_code"])];
    let bucket = metric("revenue", vec![dimension(TIME_BUCKET_LABEL, "region_code", None, None)]);
    assert_eq!(
        Definitions::assemble(one_model(), vec![], vec![bucket]).unwrap_err(),
        InconsistentDefinitions::DimensionShadowsTimeBucket {
            metric: metric_name("revenue"),
            dimension: dimension_name(TIME_BUCKET_LABEL),
        }
    );

    let measure = metric("revenue", vec![dimension("revenue", "region_code", None, None)]);
    assert_eq!(
        Definitions::assemble(one_model(), vec![], vec![measure]).unwrap_err(),
        InconsistentDefinitions::DimensionShadowsMeasure {
            metric: metric_name("revenue"),
            dimension: dimension_name("revenue"),
        }
    );
}

#[test]
fn a_relationship_naming_a_column_that_does_not_exist_is_refused() {
    let (models, _) = two_models();
    let broken = vec![Relationship::new(
        relationship_name("orders_customer"),
        model_name("orders"),
        column("nope"),
        model_name("customers"),
        column("id"),
        JoinType::ManyToOne,
    )];
    assert_eq!(
        Definitions::assemble(models, broken, vec![]).unwrap_err(),
        InconsistentDefinitions::RelationshipUnknownColumn {
            relationship: relationship_name("orders_customer"),
            model: model_name("orders"),
            column: column("nope"),
        }
    );
}

#[test]
fn a_dimension_naming_a_relationship_that_does_not_exist_is_refused() {
    let m = metric(
        "revenue",
        vec![dimension("region", "region_code", Some("no_such_join"), None)],
    );
    assert_eq!(
        assemble_one(m).unwrap_err(),
        InconsistentDefinitions::UnknownRelationship {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("no_such_join"),
        }
    );
}

#[test]
fn a_dimension_naming_a_column_the_joined_model_does_not_have_is_refused() {
    let m = metric(
        "revenue",
        vec![dimension("region", "not_there", Some("orders_customer"), None)],
    );
    assert_eq!(
        assemble_one(m).unwrap_err(),
        InconsistentDefinitions::UnknownDimensionColumn {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            model: model_name("customers"),
            column: column("not_there"),
        }
    );
}

#[test]
fn a_dimension_with_an_allowlist_permits_only_what_it_lists() {
    let d = dimension("region", "region_code", None, Some(&["north", "south"]));
    assert!(d.is_filterable());
    assert!(d.permits("north"));
    assert!(!d.permits("east"));
    // A dimension with no allowlist permits nothing, which is what turns a filter on it into
    // `DimensionNotFilterable` rather than into a query.
    let open = dimension("day_of", "order_date", None, None);
    assert!(!open.is_filterable());
    assert!(!open.permits("anything"));
}
