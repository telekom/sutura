//! Tests for the declarations in [`super`] and for the consistency checks over them.
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape. The reason is
//! mechanical: the module it tests plus these cases is over the thousand-line limit
//! `cargo xtask max-lines` enforces, and the only way past that gate is to split the file. Same
//! arrangement as `xtask/src/boundaries/api_shape.rs`.

use std::collections::BTreeSet;

use super::{
    Audience, Definitions, Description, Dimension, DimensionValue, InconsistentDefinitions, InvalidViaChain,
    MAX_DEFINITIONS_BYTES, MAX_DESCRIPTION_BYTES, MAX_VALUES_PER_DIMENSION, Metric, Model, Relationship, TIME_BUCKET_LABEL,
    ViaChain,
};
use crate::measure::{AggregatedColumn, Measure, Term};
use crate::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, Qualification, QualifiedTable,
    RelationshipName, SourceName, TableName,
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

fn value(raw: &str) -> DimensionValue {
    DimensionValue::parse(raw).expect("a test value is a value")
}

/// A model on `source` holding exactly `columns`.
fn model(name: &str, source: &str, columns: &[&str]) -> Model {
    Model::new(
        model_name(name),
        SourceName::parse(source).expect("a test source is a source"),
        TableName::parse(name).expect("a test table is a table"),
        columns.iter().map(|c| column(c)).collect::<BTreeSet<_>>(),
        Description::default(),
    )
}

/// A model whose physical table is named separately from the model itself.
///
/// [`model`] names the table after the model, which is the ordinary case and what every other test
/// here wants. The label-collision and qualified-path cases need the two to differ, and a path
/// rather than a name because that is what a catalog document writes.
fn model_over(name: &str, source: &str, table_path: &str, columns: &[&str]) -> Model {
    Model::new(
        model_name(name),
        SourceName::parse(source).expect("a test source is a source"),
        QualifiedTable::parse(table_path).expect("a test table path is a path"),
        columns.iter().map(|c| column(c)).collect::<BTreeSet<_>>(),
        Description::default(),
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
///
/// It used to key `dimensions` into a map on the way in, which is the bypass #266's D4 is about: no
/// test in this file could express a duplicate, because the helper collapsed it first.
fn metric(name: &str, dimensions: Vec<Dimension>) -> Metric {
    declaring(name, dimensions).expect("these dimensions are distinct")
}

/// The same metric, with the duplicate check still to run.
fn declaring(name: &str, dimensions: Vec<Dimension>) -> Result<Metric, InconsistentDefinitions> {
    Metric::new(
        metric_name(name),
        model_name("orders"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        dimensions,
        None,
        Description::default(),
        Audience::Open,
    )
}

/// A dimension, with its hops given in declared order.
///
/// `&[]`-style slices rather than a count of `&str` arguments: most cases here are one hop, but the
/// chain cases pass two, and `Some(&[])` would be an empty chain, which `ViaChain::of` refuses - so
/// `None` stays the only no-hop spelling.
fn dimension(name: &str, col: &str, via: Option<&[&str]>, values: Option<&[&str]>) -> Dimension {
    Dimension::new(
        dimension_name(name),
        column(col),
        via.map(|hops| {
            ViaChain::of(hops.iter().map(|h| relationship_name(h)).collect::<Vec<_>>()).expect("a test chain has hops")
        }),
        values.map(|v| v.iter().map(|s| value(s)).collect::<BTreeSet<_>>()),
        Description::default(),
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
            dimension("region", "region_code", Some(&["orders_customer"]), Some(&["north"])),
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
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("not_a_column")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
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
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::new(),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
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
        vec![dimension("region", "region_code", Some(&["orders_customer"]), None)],
    );
    assert_eq!(
        Definitions::assemble(models, fanning, vec![m]).unwrap_err(),
        InconsistentDefinitions::JoinWouldDuplicateRows {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("orders_customer"),
            hop: 1,
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
        vec![dimension("region", "region_code", Some(&["customer_orders"]), None)],
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
        vec![dimension("region", "region_code", Some(&["orders_customer"]), Some(&[]))],
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

/// #266's D4, at the seam it moved to: the constructor, not an adapter.
///
/// `Metric::new` took a `BTreeMap`, so the pair was collapsed before the domain saw it and each
/// adapter answered the question for itself - the markdown one refused, the `DataHub` one kept the
/// last. Taking a `Vec` makes the collapse unrepresentable rather than forbidden: there is nowhere
/// earlier for a caller to key the dimensions, so both adapters now get this same value.
///
/// An exact repeat rather than the folded pair below, and the two are one scan: the comparison is
/// `IdentifierCase::COARSEST`, which is true of two identical spellings, so this variant exists to
/// say the more useful thing about the case an author hits most.
#[test]
fn a_metric_declaring_one_dimension_twice_is_refused_by_the_constructor() {
    let twice = vec![
        dimension("region", "region_code", None, None),
        dimension("region", "amount_cents", None, None),
    ];
    assert_eq!(
        declaring("revenue", twice).unwrap_err(),
        InconsistentDefinitions::DuplicateDimension {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
        }
    );
}

/// #266's D5: two dimension labels that fold together, which nothing compared.
///
/// A dimension was folded against the time bucket, against the measure and against every table, and
/// against another dimension by nothing at all - `dimensions` is keyed byte-wise, so `region` and
/// `Region` were two entries and two projected columns. `Metric::new`'s own note carries the
/// pinned `DuckDB` run that says what the engine does with the pair.
///
/// **The DECLARED order is what the refusal names**, and that is the reason the scan is in the
/// constructor rather than in `assemble`: `first` is `region` because the fixture writes `region`
/// first, where a scan over the keyed field would report `Region` first - `BTreeMap` order, which is
/// nothing an author wrote. The message tells them which of the two to rename only here.
#[test]
fn two_dimensions_that_differ_only_in_case_are_one_label_and_are_refused() {
    let folded = vec![
        dimension("region", "region_code", None, None),
        dimension("Region", "amount_cents", None, None),
    ];
    assert_eq!(
        declaring("revenue", folded).unwrap_err(),
        InconsistentDefinitions::TwoDimensionsOneLabel {
            metric: metric_name("revenue"),
            first: dimension_name("region"),
            second: dimension_name("Region"),
        }
    );
}

#[test]
fn a_label_may_not_be_spelled_the_same_as_a_table_the_statement_reads() {
    // **The live failure this closes, and it was a 400 rather than a wrong number this time.** A
    // `BigQuery` submission returned `invalidQuery` - "Cannot access field day on a value with type
    // INT64" - because the metric's label equalled the table name and `GoogleSQL` bound the qualifier
    // in `table.column` to the select-list alias instead of to the table. Refused at load, for every
    // dialect, because which of two things a qualifier binds to is not something to maintain per
    // target.
    let orders_named_revenue = || {
        vec![model_over(
            "orders",
            "local",
            "revenue",
            &["amount_cents", "order_date", "region"],
        )]
    };
    let metric_collides = metric("revenue", vec![]);
    assert_eq!(
        Definitions::assemble(orders_named_revenue(), vec![], vec![metric_collides]).unwrap_err(),
        InconsistentDefinitions::LabelShadowsTable {
            metric: metric_name("revenue"),
            label: String::from("revenue"),
            table: TableName::parse("revenue").expect("a test table is a table"),
        },
        "the metric's own label is the one the live run collided on"
    );

    // The time bucket, which is projected for every question - so a table named after it collides
    // with all of them and no dimension has to be involved.
    let orders_named_period = vec![model_over(
        "orders",
        "local",
        TIME_BUCKET_LABEL,
        &["amount_cents", "order_date", "region"],
    )];
    assert_eq!(
        Definitions::assemble(orders_named_period, vec![], vec![metric("revenue", vec![])]).unwrap_err(),
        InconsistentDefinitions::LabelShadowsTable {
            metric: metric_name("revenue"),
            label: String::from(TIME_BUCKET_LABEL),
            table: TableName::parse(TIME_BUCKET_LABEL).expect("a test table is a table"),
        }
    );

    // A dimension's label, against the metric's own table.
    let orders_named_region = || {
        vec![model_over(
            "orders",
            "local",
            "region",
            &["amount_cents", "order_date", "region"],
        )]
    };
    assert_eq!(
        Definitions::assemble(
            orders_named_region(),
            vec![],
            vec![metric("revenue", vec![dimension("region", "region", None, None)])]
        )
        .unwrap_err(),
        InconsistentDefinitions::LabelShadowsTable {
            metric: metric_name("revenue"),
            label: String::from("region"),
            table: TableName::parse("region").expect("a test table is a table"),
        }
    );

    // And against a JOINED table, which is the half `check_metric` alone cannot see: the collision is
    // with the second table in the statement rather than with the first.
    let joined = vec![
        model("orders", "local", &["amount_cents", "order_date", "customer_id"]),
        model_over("customers", "local", "revenue", &["id", "region_code"]),
    ];
    assert_eq!(
        Definitions::assemble(
            joined,
            vec![Relationship::new(
                relationship_name("orders_customer"),
                model_name("orders"),
                column("customer_id"),
                model_name("customers"),
                column("id"),
                JoinType::ManyToOne,
            )],
            vec![metric(
                "revenue",
                vec![dimension("region", "region_code", Some(&["orders_customer"]), None)]
            )]
        )
        .unwrap_err(),
        InconsistentDefinitions::LabelShadowsTable {
            metric: metric_name("revenue"),
            label: String::from("revenue"),
            table: TableName::parse("revenue").expect("a test table is a table"),
        }
    );

    // The positive half: distinct names still assemble. Without it the four assertions above would
    // pass on a check that refused everything.
    drop(
        Definitions::assemble(
            orders_named_region(),
            vec![],
            vec![metric("revenue", vec![dimension("segment", "region", None, None)])],
        )
        .expect("labels that collide with nothing still assemble"),
    );
}

#[test]
fn a_label_and_a_table_that_differ_only_in_case_still_collide() {
    // **The hole a review reproduced, and it was an equality check.** GoogleSQL's lexical reference
    // lists *aliases within a query* and *column names* as NOT case-sensitive (checked 2026-08-30),
    // while its table names ARE case-sensitive by default - so a table `Orders` is a distinct table
    // and the qualifier `Orders` still resolves to a select-list alias spelled `orders`. An equality
    // check let every such pair through, and then the statement collided exactly as the same-case one
    // did.
    //
    // Compared under `IdentifierCase::COARSEST` for that reason, which is dialect-agnostic on purpose:
    // a bundle is loaded without knowing which target will serve it, and refusing a pair Postgres
    // would have told apart costs an author a rename while accepting one BigQuery folds is a wrong
    // number under a certified name. `sutura_sql::Dialect::identifier_case` is where each target
    // declares its own, and a test there holds this assumption to being the coarsest.
    //
    // Three cases, because there are three sources of a projected label: the metric, the time bucket
    // and a dimension.
    let orders_named_revenue = vec![model_over(
        "orders",
        "local",
        "revenue",
        &["amount_cents", "order_date", "region"],
    )];
    assert_eq!(
        Definitions::assemble(orders_named_revenue, vec![], vec![metric("Revenue", vec![])]).unwrap_err(),
        InconsistentDefinitions::LabelShadowsTable {
            metric: metric_name("Revenue"),
            label: String::from("Revenue"),
            table: TableName::parse("revenue").expect("a test table is a table"),
        },
        "a metric label folds against the table name"
    );

    let orders_named_period = vec![model_over(
        "orders",
        "local",
        "Period",
        &["amount_cents", "order_date", "region"],
    )];
    assert_eq!(
        Definitions::assemble(orders_named_period, vec![], vec![metric("revenue", vec![])]).unwrap_err(),
        InconsistentDefinitions::LabelShadowsTable {
            metric: metric_name("revenue"),
            label: String::from(TIME_BUCKET_LABEL),
            table: TableName::parse("Period").expect("a test table is a table"),
        },
        "the time bucket's label folds too, and it is projected for every question"
    );

    let orders_named_region = vec![model_over(
        "orders",
        "local",
        "region",
        &["amount_cents", "order_date", "region"],
    )];
    assert_eq!(
        Definitions::assemble(
            orders_named_region,
            vec![],
            vec![metric("revenue", vec![dimension("Region", "region", None, None)])]
        )
        .unwrap_err(),
        InconsistentDefinitions::LabelShadowsTable {
            metric: metric_name("revenue"),
            label: String::from("Region"),
            table: TableName::parse("region").expect("a test table is a table"),
        },
        "a dimension label folds as well"
    );

    // The positive half, and it is what stops the three above passing on a check that refuses
    // everything: two names that fold to different things still assemble.
    drop(
        Definitions::assemble(
            vec![model_over(
                "orders",
                "local",
                "region_totals",
                &["amount_cents", "order_date", "region"],
            )],
            vec![],
            vec![metric("revenue", vec![dimension("Region", "region", None, None)])],
        )
        .expect("region_totals and Region are two identifiers under any rule"),
    );
}

#[test]
fn two_projected_labels_that_differ_only_in_case_are_one_result_column() {
    // The other half of *two result columns cannot share a label*, and it was case-sensitive for the
    // same reason: GoogleSQL documents a result column's NAME as not case-sensitive, so `Period`
    // beside `period` is one column there. Both existing refusals now fold, and neither needed a new
    // variant - what changed is the comparison.
    let one_model = || vec![model("orders", "local", &["amount_cents", "order_date", "region_code"])];
    let bucket = metric("revenue", vec![dimension("Period", "region_code", None, None)]);
    assert_eq!(
        Definitions::assemble(one_model(), vec![], vec![bucket]).unwrap_err(),
        InconsistentDefinitions::DimensionShadowsTimeBucket {
            metric: metric_name("revenue"),
            dimension: dimension_name("Period"),
        }
    );

    let measure = metric("revenue", vec![dimension("Revenue", "region_code", None, None)]);
    assert_eq!(
        Definitions::assemble(one_model(), vec![], vec![measure]).unwrap_err(),
        InconsistentDefinitions::DimensionShadowsMeasure {
            metric: metric_name("revenue"),
            dimension: dimension_name("Revenue"),
        }
    );
}

#[test]
fn a_model_may_name_a_table_in_another_dataset_and_the_label_check_reads_the_last_part() {
    // A qualified model loads, and the two readings of "the table" are both available: the path a
    // `FROM` names, and the bare name a column is qualified by. The label check reads the BARE name
    // deliberately - `FROM a.b.c` gives the reference an implicit alias of `c` in every dialect
    // rendered for here, so `c` is what a qualifier could bind to and `a` and `b` are not.
    let qualified = vec![model_over(
        "orders",
        "local",
        "analytics-prod.sales.orders",
        &["amount_cents", "order_date", "region"],
    )];
    let definitions = Definitions::assemble(qualified, vec![], vec![metric("revenue", vec![])])
        .expect("a qualified model with no label collision assembles");
    let model = definitions.models().get(&model_name("orders")).expect("the model is there");
    assert_eq!(model.table().to_string(), "analytics-prod.sales.orders");
    assert_eq!(model.table_name().as_str(), "orders");
    assert_eq!(model.table().qualification(), Qualification::ProjectAndDataset);

    // And the collision is still on the last part rather than on the dataset: a metric called
    // `sales` beside a table in a dataset called `sales` is fine, and one called `orders` is not.
    let in_sales = || {
        vec![model_over(
            "orders",
            "local",
            "analytics-prod.sales.orders",
            &["amount_cents", "order_date", "region"],
        )]
    };
    drop(
        Definitions::assemble(in_sales(), vec![], vec![metric("sales", vec![])])
            .expect("a metric named after the DATASET collides with nothing a qualifier binds to"),
    );
    assert_eq!(
        Definitions::assemble(in_sales(), vec![], vec![metric("orders", vec![])]).unwrap_err(),
        InconsistentDefinitions::LabelShadowsTable {
            metric: metric_name("orders"),
            label: String::from("orders"),
            table: TableName::parse("orders").expect("a test table is a table"),
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
        vec![dimension("region", "region_code", Some(&["no_such_join"]), None)],
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
        vec![dimension("region", "not_there", Some(&["orders_customer"]), None)],
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

/// `orders` → `customers` → `regions`, all on `local`: the fixture every multi-hop case walks.
///
/// `customers_regions` starts at `customers` - the target of hop 1 - so the two relationships join
/// up and the chain is a single path.
fn three_models() -> ModelsAndJoins {
    let (mut models, mut relationships) = two_models();
    models.push(model("regions", "local", &["code", "label"]));
    models[1] = model("customers", "local", &["id", "region_code"]);
    relationships.push(Relationship::new(
        relationship_name("customers_regions"),
        model_name("customers"),
        column("region_code"),
        model_name("regions"),
        column("code"),
        JoinType::ManyToOne,
    ));
    (models, relationships)
}

fn chain_metric() -> Metric {
    metric(
        "revenue",
        vec![dimension(
            "region",
            "code",
            Some(&["orders_customer", "customers_regions"]),
            Some(&["north"]),
        )],
    )
}

#[test]
fn a_dimension_through_a_chain_assembles_and_resolves_on_the_last_hops_model() {
    let (models, relationships) = three_models();
    let definitions = Definitions::assemble(models, relationships, vec![chain_metric()]).expect("a joined-up chain assembles");
    // The column is read on the LAST hop's target, not on `customers`: `code` is a column of
    // `regions`, and only `regions` has it.
    let dimension = definitions
        .metric(&metric_name("revenue"))
        .expect("the metric is there")
        .dimensions()
        .iter()
        .find(|d| d.1.name() == &dimension_name("region"))
        .expect("the dimension is there");
    assert_eq!(dimension.1.column(), &column("code"));
    assert_eq!(
        dimension.1.via(),
        Some(&[relationship_name("orders_customer"), relationship_name("customers_regions")][..])
    );
}

/// The resolution above is only observable through a refusal, so this is the arm that proves it.
///
/// `region_code` is a column of `customers` - hop 1's target - and NOT of `regions`, hop 2's
/// target. So a chain dimension naming it must be refused, and the refusal must name `regions`:
/// that is what distinguishes "resolved on the LAST hop's model" from "resolved on the first
/// hop's". The passing cell above asserts `column()` and `via()`, which are the values the
/// constructor was handed, so it would stay green if resolution moved to the wrong hop.
#[test]
fn a_chain_dimension_naming_an_earlier_hops_column_is_refused_naming_the_last_hops_model() {
    let (models, relationships) = three_models();
    let m = metric(
        "revenue",
        vec![dimension(
            "region",
            "region_code",
            Some(&["orders_customer", "customers_regions"]),
            None,
        )],
    );
    assert_eq!(
        Definitions::assemble(models, relationships, vec![m]).unwrap_err(),
        InconsistentDefinitions::UnknownDimensionColumn {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            model: model_name("regions"),
            column: column("region_code"),
        },
        "a chain resolves its column on the LAST hop's model, so an earlier hop's column is unknown"
    );
}

#[test]
fn a_chain_hop_that_could_duplicate_rows_is_refused_naming_the_hop() {
    let (models, mut relationships) = three_models();
    relationships[1] = Relationship::new(
        relationship_name("customers_regions"),
        model_name("customers"),
        column("region_code"),
        model_name("regions"),
        column("code"),
        JoinType::OneToMany,
    );
    let m = chain_metric();
    assert_eq!(
        Definitions::assemble(models, relationships, vec![m]).unwrap_err(),
        InconsistentDefinitions::JoinWouldDuplicateRows {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("customers_regions"),
            hop: 2,
        },
        "the second hop is what would multiply rows, so the report names 2"
    );
}

#[test]
fn a_chain_whose_hop_does_not_start_at_the_previous_target_is_refused() {
    // `customers_regions` starts at `regions` instead of at `customers`, so the chain is two
    // relationships rather than one path.
    let (models, mut relationships) = three_models();
    relationships[1] = Relationship::new(
        relationship_name("customers_regions"),
        model_name("regions"),
        column("code"),
        model_name("customers"),
        column("region_code"),
        JoinType::ManyToOne,
    );
    let m = chain_metric();
    assert_eq!(
        Definitions::assemble(models, relationships, vec![m]).unwrap_err(),
        InconsistentDefinitions::ChainDoesNotJoinUp {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            previous: relationship_name("orders_customer"),
            relationship: relationship_name("customers_regions"),
        }
    );
}

#[test]
fn a_chain_hop_onto_another_data_system_is_refused() {
    // `regions` sits on `elsewhere`, and hop 2 crossing there would put the chained join on another
    // data system's statement. Hop 1 is allowed to cross - that is the federated case - so the same
    // models with a single-hop dimension on `orders_customer` still assemble below.
    let (mut models, relationships) = three_models();
    models[2] = model("regions", "elsewhere", &["code", "label"]);
    let m = chain_metric();
    assert_eq!(
        Definitions::assemble(models.clone(), relationships.clone(), vec![m]).unwrap_err(),
        InconsistentDefinitions::HopCrossesSource {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("customers_regions"),
            own: SourceName::parse("local").expect("a test source is a source"),
            target_source: SourceName::parse("elsewhere").expect("a test source is a source"),
        }
    );

    let single = metric("revenue", vec![dimension("region", "id", Some(&["orders_customer"]), None)]);
    drop(
        Definitions::assemble(models, relationships, vec![single])
            .expect("hop 1 crossing is the federated case and still assembles"),
    );
}

#[test]
fn a_chain_that_crosses_a_data_system_and_comes_back_is_refused() {
    // The hole the target-only comparison left, and it was a WRONG ANSWER rather than an error:
    // `customers` sits on `elsewhere` and `regions` back on `local`, so hop 2's TARGET is local and
    // a check reading the target alone accepted the chain. `sutura_semantic` reads a chain's source
    // off its LAST hop, so this loaded as a purely local dimension and the whole-answer plan put
    // `elsewhere`'s table into one `local` statement - under a certified metric name, with no
    // refusal anywhere. Hop 2's ORIGIN is what gives it away, which is why both ends are compared.
    let (mut models, relationships) = three_models();
    models[1] = model("customers", "elsewhere", &["id", "region_code"]);
    assert_eq!(
        Definitions::assemble(models, relationships, vec![chain_metric()]).unwrap_err(),
        InconsistentDefinitions::HopCrossesSource {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            relationship: relationship_name("customers_regions"),
            own: SourceName::parse("local").expect("a test source is a source"),
            // `elsewhere` is where the hop STARTS here, not where it ends: the field names the
            // system that is not the metric's, at whichever end of the hop it turned up.
            target_source: SourceName::parse("elsewhere").expect("a test source is a source"),
        }
    );
}

#[test]
fn an_empty_chain_is_not_representable() {
    assert_eq!(
        ViaChain::of(vec![]).expect_err("an empty chain is refused"),
        InvalidViaChain::Empty
    );
}

#[test]
fn a_dimension_with_an_allowlist_permits_only_what_it_lists() {
    let d = dimension("region", "region_code", None, Some(&["north", "south"]));
    assert!(d.is_filterable());
    assert!(d.permits(&value("north")));
    assert!(!d.permits(&value("east")));
    // A dimension with no allowlist permits nothing, which is what turns a filter on it into
    // `DimensionNotFilterable` rather than into a query.
    let open = dimension("day_of", "order_date", None, None);
    assert!(!open.is_filterable());
    assert!(!open.permits(&value("anything")));
}

#[test]
fn a_dimension_declaring_more_values_than_the_bound_does_not_load() {
    // The count no per-value parse can see. Every one of these is a legal `DimensionValue`; the list
    // of them is what would be interpolated into one line of an agent-facing document.
    let over: Vec<String> = (0..=MAX_VALUES_PER_DIMENSION).map(|n| format!("v{n}")).collect();
    let listed: Vec<&str> = over.iter().map(String::as_str).collect();
    assert_eq!(
        assemble_one(metric(
            "revenue",
            vec![dimension("region", "region_code", Some(&["orders_customer"]), Some(&listed))],
        ))
        .expect_err("a dimension over the value bound does not assemble"),
        InconsistentDefinitions::TooManyValues {
            metric: metric_name("revenue"),
            dimension: dimension_name("region"),
            count: MAX_VALUES_PER_DIMENSION.saturating_add(1),
            limit: MAX_VALUES_PER_DIMENSION,
        }
    );
    // Both ends of the bound: exactly the limit assembles, so this is the number written down rather
    // than an off-by-one nobody would notice from the failing side alone.
    let at_limit: Vec<String> = (0..MAX_VALUES_PER_DIMENSION).map(|n| format!("v{n}")).collect();
    let listed: Vec<&str> = at_limit.iter().map(String::as_str).collect();
    drop(
        assemble_one(metric(
            "revenue",
            vec![dimension("region", "region_code", Some(&["orders_customer"]), Some(&listed))],
        ))
        .expect("exactly the limit assembles"),
    );
}

/// The aggregate cap that [`super::MAX_VALUES_PER_DIMENSION`]'s own note names as missing: nothing
/// bounds how many metrics a catalog holds, so N of them each individually inside every per-item
/// cap - here, [`super::MAX_DESCRIPTION_BYTES`] - are not individually a catalog nobody would read.
#[test]
fn enough_conforming_metrics_to_exceed_the_aggregate_cap_do_not_load() {
    let filler = Description::parse("y".repeat(MAX_DESCRIPTION_BYTES)).expect("exactly the description cap is a description");
    let too_many: Vec<Metric> = (0..33_u32)
        .map(|index| {
            Metric::new(
                metric_name(&format!("metric_{index}")),
                model_name("orders"),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
                Vec::new(),
                column("order_date"),
                BTreeSet::from([Grain::Month]),
                Vec::new(),
                None,
                filler.clone(),
                Audience::Open,
            )
            .expect("no dimensions to duplicate")
        })
        .collect();
    let (models, relationships) = two_models();
    let error = Definitions::assemble(models, relationships, too_many).expect_err("33 max descriptions exceed the cap");
    match error {
        InconsistentDefinitions::DefinitionsTooLarge { bytes, limit } => {
            assert_eq!(limit, MAX_DEFINITIONS_BYTES);
            assert!(bytes > MAX_DEFINITIONS_BYTES, "{bytes} must exceed {MAX_DEFINITIONS_BYTES}");
        }
        other => panic!("the aggregate cap must be what refuses this: {other:?}"),
    }
    // And the cap is not so tight that a realistic catalog trips it: 31 of the same descriptions are
    // under 128 KiB and load.
    let inside: Vec<Metric> = (0..31_u32)
        .map(|index| {
            Metric::new(
                metric_name(&format!("metric_{index}")),
                model_name("orders"),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
                Vec::new(),
                column("order_date"),
                BTreeSet::from([Grain::Month]),
                Vec::new(),
                None,
                filler.clone(),
                Audience::Open,
            )
            .expect("no dimensions to duplicate")
        })
        .collect();
    let (models, relationships) = two_models();
    assert_eq!(
        Definitions::assemble(models, relationships, inside)
            .expect("31 max descriptions is inside the cap")
            .metrics()
            .len(),
        31
    );
}

/// The check that keeps the two copies of one measurement from drifting.
///
/// `Description`'s caps and `NoteBody`'s are the same numbers because they are the same measurement -
/// the longest prose body in this repository's example catalog is a metric description - and they are
/// two constants only because `catalog` may not depend on `knowledge`. `crate::text` exists because a
/// load-bearing rule was written down twice and the copies drifted, so this is a check rather than a
/// comment asking the next author to update both. It fails whichever is edited alone.
#[test]
fn a_description_and_a_note_body_are_bounded_by_the_same_numbers() {
    assert_eq!(super::MAX_DESCRIPTION_BYTES, crate::knowledge::MAX_NOTE_BODY_BYTES);
    assert_eq!(super::MAX_DESCRIPTION_LINES, crate::knowledge::MAX_NOTE_LINES);
}
