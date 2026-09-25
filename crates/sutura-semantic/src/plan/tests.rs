//! Tests for the declarations in [`super`].
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape. The reason is
//! mechanical: the module it tests plus these cases is over the thousand-line limit
//! `cargo xtask max-lines` enforces, and the only way past that gate is to split the file. Same
//! arrangement as `crates/sutura-domain/src/catalog/tests.rs`.

use std::collections::BTreeSet;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Audience, Description, Dimension, JoinKey, JoinKeys, Metric, Model, Relationship, ViaChain};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, ColumnName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::plan::{PlanPredicate, QueryPlan};
use sutura_domain::query::RefusalReason;

use super::{DimensionName, Plan, PlanError, plan};
use crate::resolve::{Resolution, ResolvedDimension, ResolvedFilter, ResolvedFilterValue, ResolvedJoin};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn dimension_name(raw: &str) -> DimensionName {
    DimensionName::parse(raw).expect("a test dimension is a dimension")
}

/// A model whose physical table is `dim_{name}`, so no `ON` clause below can pass by naming a
/// model where it should name a table.
fn model(name: &str, on: &str, columns: &[&str]) -> Model {
    Model::new(
        ModelName::parse(name).expect("a test model is a model"),
        SourceName::parse(on).expect("a test source is a source"),
        TableName::parse(format!("dim_{name}")).expect("a test table is a table"),
        columns.iter().map(|c| column(c)),
        Description::default(),
    )
}

fn relationship(name: &str, from: (&str, &str), to: (&str, &str)) -> Relationship {
    Relationship::new(
        RelationshipName::parse(name).expect("a test relationship is a relationship"),
        ModelName::parse(from.0).expect("a test model is a model"),
        ModelName::parse(to.0).expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: column(from.1),
            target: column(to.1),
        }])
        .expect("a test relationship declares one key"),
    )
}

/// `facts -> customers -> regions` beside `facts -> products`, with `customers` placed on the
/// source given.
///
/// Two chains that share NO hop, which is what the order cell needs: two chains sharing hop 1
/// dedup to the same list in either order, so a corpus built that way passes with the sort
/// removed - measured, and it is why `family` is reached through a relationship of its own.
///
/// `region_code` is hop 2's origin column and sits on `customers` only: `dim_facts` does not
/// declare it, which is what turns the hop-qualification defect into a binder error rather than
/// a silent regrouping in this venue.
struct Corpus {
    facts: Model,
    /// Every declared hop beside the model on its far side, which is what a resolution holds.
    reached: Vec<(Relationship, Model)>,
    held: Metric,
}

impl Corpus {
    fn with_customers_on(customers: &str) -> Self {
        Self {
            facts: model("facts", "local", &["amount_cents", "day", "customer_key", "product_key"]),
            reached: vec![
                (
                    relationship("facts_customer", ("facts", "customer_key"), ("customers", "customer_key")),
                    model("customers", customers, &["customer_key", "region_code"]),
                ),
                (
                    relationship("customers_region", ("customers", "region_code"), ("regions", "code")),
                    model("regions", "local", &["code", "label"]),
                ),
                (
                    relationship("facts_product", ("facts", "product_key"), ("products", "product_key")),
                    model("products", "local", &["product_key", "family"]),
                ),
            ],
            held: Metric::new(
                MetricName::parse("revenue").expect("a test metric is a metric"),
                ModelName::parse("facts").expect("a test model is a model"),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
                Vec::new(),
                column("day"),
                BTreeSet::from([Grain::Month]),
                vec![
                    declared("region", "label", &["facts_customer", "customers_region"]),
                    declared("family", "family", &["facts_product"]),
                ],
                None,
                Description::default(),
                Audience::Open,
            )
            .expect("these dimensions are distinct"),
        }
    }

    /// The dimension named, with each hop of its declared chain resolved by name.
    fn key(&self, name: &str) -> ResolvedDimension<'_> {
        let held = self
            .held
            .dimension(&dimension_name(name))
            .expect("the metric declares this dimension");
        ResolvedDimension {
            dimension: held,
            join: Some(
                held.via()
                    .unwrap_or_default()
                    .iter()
                    .map(|hop| {
                        let (relationship, model) = self
                            .reached
                            .iter()
                            .find(|(declared_hop, _)| declared_hop.name() == hop)
                            .expect("the corpus declares this hop");
                        ResolvedJoin { relationship, model }
                    })
                    .collect(),
            ),
        }
    }

    fn asking<'a>(&'a self, keys: Vec<ResolvedDimension<'a>>) -> Resolution<'a> {
        Resolution {
            metric: &self.held,
            metrics: vec![&self.held],
            model: &self.facts,
            grain: Grain::Month,
            range: TimeRange::new(
                Date::parse("2026-06-01").expect("a test date is a date"),
                Date::parse("2026-07-01").expect("a test date is a date"),
            )
            .expect("June is a range"),
            keys,
            filters: Vec::new(),
            top: None,
        }
    }
}

fn declared(name: &str, col: &str, via: &[&str]) -> Dimension {
    Dimension::new(
        dimension_name(name),
        column(col),
        Some(
            ViaChain::of(
                via.iter()
                    .map(|hop| RelationshipName::parse(hop).expect("a test relationship is a relationship"))
                    .collect(),
            )
            .expect("a test chain has hops"),
        ),
        None,
        Description::default(),
    )
}

fn mono(resolution: &Resolution<'_>) -> Box<QueryPlan> {
    match plan(resolution).expect("this resolution plans") {
        Plan::Mono(query) => query,
        Plan::Federated(_) => panic!("every model here is local, so the plan is one statement"),
    }
}

/// An `In` filter's values bind in placeholder order, right after the two range bounds - #968.
///
/// `predicates_and_params` binds `start`, `end`, then every requested filter's values in the order
/// `requested_predicate` pushes them, so a two-value `In` after the range bounds must land at
/// placeholders `2` and `3` in the order the question gave the values. Getting this wrong is a
/// values-arrive-out-of-order defect a golden SQL string cannot show, because the placeholders
/// (`?`, `?`) look identical regardless of which index each one binds.
#[test]
fn an_in_filter_binds_its_values_right_after_the_range_bounds() {
    let corpus = Corpus::with_customers_on("local");
    let resolution = Resolution {
        filters: vec![ResolvedFilter {
            dimension: corpus.key("region"),
            value: ResolvedFilterValue::In(
                sutura_domain::nonempty::NonEmpty::parse(vec![String::from("north"), String::from("south")])
                    .expect("two values is not empty"),
            ),
        }],
        ..corpus.asking(Vec::new())
    };
    let planned = mono(&resolution);
    let in_filter = planned
        .filters()
        .iter()
        .find(|filter| matches!(filter.predicate(), PlanPredicate::In { .. }))
        .expect("the requested In filter reaches the plan");
    let PlanPredicate::In { params, .. } = in_filter.predicate() else {
        panic!("just matched on PlanPredicate::In above");
    };
    assert_eq!(
        params.iter().copied().collect::<Vec<_>>(),
        vec![2, 3],
        "expected the In filter's two values at placeholders 2 and 3 (after start/end), got {:?}",
        in_filter.predicate()
    );
}

/// One `ON` clause per hop, each qualified by the table the hop actually starts at.
///
/// THE BUG THIS EXISTS FOR: every hop's origin was qualified by the metric's own table, so hop 2
/// rendered `ON dim_facts.region_code = dim_regions.code`. `region_code` is a column of
/// `dim_customers`; against `DuckDB` that is `Binder Error: Table "dim_facts" does not have
/// a column named "region_code"`, and on a fact table that happens to carry a column of the same
/// name it is not an error at all - it is a different grouping under a certified metric name.
#[test]
fn a_later_hop_joins_from_the_previous_hops_table() {
    let corpus = Corpus::with_customers_on("local");
    let planned = mono(&corpus.asking(vec![corpus.key("region")]));
    let clauses: Vec<(String, String)> = planned
        .joins()
        .iter()
        .map(|join| {
            let first = join.keys().first().expect("a hop declares at least one key");
            (first.origin().table().to_string(), first.target().table().to_string())
        })
        .collect();
    assert_eq!(
        clauses,
        vec![
            (String::from("dim_facts"), String::from("dim_customers")),
            (String::from("dim_customers"), String::from("dim_regions")),
        ],
        "hop 2 must join FROM the table hop 1 arrived at"
    );
}

/// The join order is a function of the plan, not of the order the caller listed dimensions.
///
/// Two chains sharing no hop, asked in both orders. LEFT joins commute, so a reordering costs no
/// wrong number - what it costs is the rendered text every golden in `sutura-app` pins.
#[test]
fn the_join_order_does_not_depend_on_the_order_the_dimensions_arrive() {
    let corpus = Corpus::with_customers_on("local");
    let order =
        |planned: &QueryPlan| -> Vec<String> { planned.joins().iter().map(|join| join.relationship().to_string()).collect() };
    assert_eq!(
        order(&mono(&corpus.asking(vec![corpus.key("region"), corpus.key("family")]))),
        order(&mono(&corpus.asking(vec![corpus.key("family"), corpus.key("region")]))),
        "the same two chains asked in two orders rendered two join orders"
    );
}

/// A chain that left its data system is refused HERE, not only at load.
///
/// `customers` on `elsewhere` with `regions` back on `local` is the shape the load check used to
/// accept: the last hop is local, so `is_remote` calls the whole chain local and the whole-answer
/// plan renders `elsewhere`'s table into one `local` statement. No bundle can be assembled that
/// way any more, which is why this resolution is built by hand.
#[test]
fn a_chain_that_leaves_its_source_is_refused_at_plan_time() {
    let corpus = Corpus::with_customers_on("elsewhere");
    let Err(refused) = plan(&corpus.asking(vec![corpus.key("region")])) else {
        panic!("a chain that crossed and came back must be refused");
    };
    assert!(
        matches!(
            refused,
            PlanError::ChainLeavesItsSource { ref dimension, hop: 2, .. } if *dimension == dimension_name("region")
        ),
        "expected the chain refusal naming hop 2, got {refused:?}"
    );
}

/// A relationship crossing into a remote source with MORE THAN ONE join key is refused by its
/// own name, `FederationLinkCompound` - not `FederationLinkAmbiguous`, which means two
/// RELATIONSHIPS crossing at once and would tell a caller something untrue about one correctly
/// declared compound key. Built by hand for the same reason every federated-splitter cell here
/// is: `Definitions::assemble` never puts two remote-crossing keys in front of a caller's
/// question on its own, so the splitter's own guard is the only venue that provokes this.
#[test]
fn a_compound_crossing_relationship_is_refused_by_its_own_name() {
    let facts = model("facts", "local", &["amount_cents", "day", "customer_key", "month"]);
    let customers = model("customers", "elsewhere", &["customer_key", "month", "region_code"]);
    let crossing = Relationship::new(
        RelationshipName::parse("facts_customer").expect("a test relationship is a relationship"),
        ModelName::parse("facts").expect("a test model is a model"),
        ModelName::parse("customers").expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![
            JoinKey::Equal {
                origin: column("customer_key"),
                target: column("customer_key"),
            },
            JoinKey::Equal {
                origin: column("month"),
                target: column("month"),
            },
        ])
        .expect("two keys is a non-empty set"),
    );
    let metric = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("facts").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("day"),
        BTreeSet::from([Grain::Month]),
        vec![declared("region", "region_code", &["facts_customer"])],
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("one dimension is distinct");
    let dimension = metric
        .dimension(&dimension_name("region"))
        .expect("the metric declares this dimension");
    let resolution = Resolution {
        metric: &metric,
        metrics: vec![&metric],
        model: &facts,
        grain: Grain::Month,
        range: TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range"),
        keys: vec![ResolvedDimension {
            dimension,
            join: Some(vec![ResolvedJoin {
                relationship: &crossing,
                model: &customers,
            }]),
        }],
        filters: Vec::new(),
        top: None,
    };
    let Err(refused) = plan(&resolution) else {
        panic!("a compound crossing relationship must be refused, not planned");
    };
    assert!(
        matches!(
            refused,
            PlanError::Refused(RefusalReason::FederationLinkCompound { ref relationship, .. })
                if relationship.as_str() == "facts_customer"
        ),
        "expected FederationLinkCompound naming facts_customer, got {refused:?}"
    );
}

/// A ratio side naming a model other than the metric's own - `telekom/sutura#780`'s vocabulary -
/// is refused before either plan shape is attempted. The MONO half: no `keys`, so `remote` is
/// empty here - a mutation gating on "no remote dimension" would leave this green, which is
/// why the sibling test below adds a remote one. Built by hand: the check reads only the
/// term's model, so no second model needs declaring for it to fire.
#[test]
fn a_ratio_term_naming_another_model_is_refused_before_either_plan_shape_is_tried() {
    let facts = model("facts", "local", &["amount_cents", "customer_key", "day"]);
    let metric = Metric::new(
        MetricName::parse("revenue_per_customer").expect("a test metric is a metric"),
        ModelName::parse("facts").expect("a test model is a model"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::CountDistinct,
                column("customer_key"),
                ModelName::parse("customers").expect("a test model is a model"),
            )),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        },
        Vec::new(),
        column("day"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let resolution = Resolution {
        metric: &metric,
        metrics: vec![&metric],
        model: &facts,
        grain: Grain::Month,
        range: TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range"),
        keys: Vec::new(),
        filters: Vec::new(),
        top: None,
    };
    let Err(refused) = plan(&resolution) else {
        panic!("a ratio term naming another model must be refused");
    };
    assert!(
        matches!(
            refused,
            PlanError::Refused(sutura_domain::query::RefusalReason::CrossModelRatioNotExecutable {
                ref metric,
                ref model,
            }) if *metric == MetricName::parse("revenue_per_customer").expect("a test metric is a metric")
                && *model == ModelName::parse("customers").expect("a test model is a model")
        ),
        "expected the cross-model ratio refusal naming `customers`, got {refused:?}"
    );
}

/// The FEDERATED half: one remote dimension, so `remote.len() == 1` and this would otherwise
/// dispatch to `federated_plan` - refused for the same reason the mono cell above is, because
/// the check runs before `remote` is computed at all. Gating it on "no remote dimension"
/// (`telekom/sutura#1014`'s review) plans this federated instead, resolving the denominator
/// against the metric's own fact table under a certified name. `customers` is both the
/// ratio's second model and the dimension's remote target, on purpose: the same model a
/// second fact leg would need is what makes this question plan federated at all.
#[test]
fn a_ratio_term_naming_another_model_is_refused_with_a_remote_dimension_too() {
    let facts = model("facts", "local", &["amount_cents", "customer_key", "day"]);
    let customers = model("customers", "remote", &["customer_key", "region_code"]);
    let facts_customer = relationship("facts_customer", ("facts", "customer_key"), ("customers", "customer_key"));
    let metric = Metric::new(
        MetricName::parse("revenue_per_customer").expect("a test metric is a metric"),
        ModelName::parse("facts").expect("a test model is a model"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Count,
                column("customer_key"),
                ModelName::parse("customers").expect("a test model is a model"),
            )),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        },
        Vec::new(),
        column("day"),
        BTreeSet::from([Grain::Month]),
        vec![declared("region", "region_code", &["facts_customer"])],
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let region = metric
        .dimension(&dimension_name("region"))
        .expect("the metric declares this dimension");
    let resolution = Resolution {
        metric: &metric,
        metrics: vec![&metric],
        model: &facts,
        grain: Grain::Month,
        range: TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range"),
        keys: vec![ResolvedDimension {
            dimension: region,
            join: Some(vec![ResolvedJoin {
                relationship: &facts_customer,
                model: &customers,
            }]),
        }],
        filters: Vec::new(),
        top: None,
    };
    let Err(refused) = plan(&resolution) else {
        panic!("a ratio term naming another model must be refused even with a remote dimension present");
    };
    assert!(
        matches!(
            refused,
            PlanError::Refused(sutura_domain::query::RefusalReason::CrossModelRatioNotExecutable {
                ref metric,
                ref model,
            }) if *metric == MetricName::parse("revenue_per_customer").expect("a test metric is a metric")
                && *model == ModelName::parse("customers").expect("a test model is a model")
        ),
        "expected the cross-model ratio refusal naming `customers`, got {refused:?}"
    );
}

/// The control for the cell above: a term that names the metric's OWN model explicitly plans
/// exactly as one naming none does, because both resolve their column against the metric's own
/// table. Without this, the check above could not tell "another model" from "any name at all".
#[test]
fn a_ratio_term_naming_the_metric_s_own_model_plans_like_one_naming_none() {
    let facts = model("facts", "local", &["amount_cents", "customer_key", "day"]);
    let metric = Metric::new(
        MetricName::parse("revenue_per_customer").expect("a test metric is a metric"),
        ModelName::parse("facts").expect("a test model is a model"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::CountDistinct,
                column("customer_key"),
                ModelName::parse("facts").expect("a test model is a model"),
            )),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        },
        Vec::new(),
        column("day"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let resolution = Resolution {
        metric: &metric,
        metrics: vec![&metric],
        model: &facts,
        grain: Grain::Month,
        range: TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range"),
        keys: Vec::new(),
        filters: Vec::new(),
        top: None,
    };
    let planned = mono(&resolution);
    assert!(
        matches!(planned.measure(), sutura_domain::plan::PlanMeasure::Ratio { .. }),
        "a same-model term must still plan the ratio"
    );
}
