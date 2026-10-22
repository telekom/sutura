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
use sutura_domain::plan::leg::LegPlan;
use sutura_domain::plan::{PlanPredicate, QueryPlan};
use sutura_domain::query::RefusalReason;

use super::{DimensionName, Plan, PlanError, plan};
use crate::resolve::{Resolution, ResolvedDimension, ResolvedFilter, ResolvedFilterValue, ResolvedJoin};
use crate::{CompileFailure, Compiled};

mod corpus;
mod drill_across;
mod federation;
mod link_types;
mod two_sources;
use corpus::{Corpus, federated_by, join_tables, pair, read_from};
use two_sources::{ONE_SOURCE, Placed, TwoSources, ask, federated, refusal, revenue_beside_a_ratio, two_facts};

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
        Plan::Federated(_) | Plan::DrillAcross(_) => panic!("every model here is local, so the plan is one statement"),
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
            let first = join.keys().first();
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

/// A chain that crosses onto one data system and on to a third is refused at plan time as well.
#[test]
fn a_chain_that_crosses_and_on_to_a_third_system_is_refused_at_plan_time() {
    let corpus = Corpus::placed("elsewhere", "third", "local");
    let got = plan(&corpus.asking(vec![corpus.key("region")])).err();
    assert!(matches!(got, Some(PlanError::ChainLeavesItsSource { hop: 2, .. })), "{got:?}");
}

/// A chain that crosses at its FIRST hop and carries on is one lookup statement.
///
/// `customers` and `regions` both on `elsewhere`: hop 1 is the link, so the fact leg joins nothing;
/// hop 2 is a join the LOOKUP leg makes from the table hop 1 landed on, and the dimension is read
/// from `regions`, so dropping the lookup's joins projects a column its statement does not read.
#[test]
fn a_chain_that_crosses_and_carries_on_joins_inside_the_lookup_leg() {
    let corpus = Corpus::placed("elsewhere", "elsewhere", "local");
    let planned = federated_by(&corpus, "region");
    let (fact, lookup) = (
        planned.fact(),
        planned.lookups().legs().first().expect("one lookup leg").leg(),
    );
    assert!(join_tables(fact).is_empty(), "hop 1 is the link and no fact join");
    assert_eq!(read_from(fact.keys().last()), pair("dim_facts", "customer_key"));
    assert_eq!(lookup.table_name().to_string(), "dim_customers");
    assert_eq!(join_tables(lookup), vec![pair("dim_customers", "dim_regions")]);
    assert_eq!(read_from(lookup.keys().first()), pair("dim_customers", "customer_key"));
    assert_eq!(read_from(lookup.keys().get(1)), pair("dim_regions", "label"));
}

/// A chain that crosses at a LATER hop links through the hop before it.
///
/// `brands` on `elsewhere`, `products` local: hop 1 is a fact-leg join and hop 2 is the link, so the
/// fact leg's link column belongs to `products`, the table hop 1 landed on, and not to `facts`. A
/// splitter that qualifies the link by the metric's table asks the fact table for `brand_key`.
#[test]
fn a_chain_that_crosses_at_a_later_hop_links_through_the_hop_before_it() {
    let corpus = Corpus::placed("local", "local", "elsewhere");
    let planned = federated_by(&corpus, "brand");
    let (fact, lookup) = (
        planned.fact(),
        planned.lookups().legs().first().expect("one lookup leg").leg(),
    );
    assert_eq!(join_tables(fact), vec![pair("dim_facts", "dim_products")]);
    assert_eq!(read_from(fact.keys().last()), pair("dim_products", "brand_key"));
    assert_eq!(lookup.table_name().to_string(), "dim_brands");
    assert!(join_tables(lookup).is_empty(), "the crossing hop is the last hop here");
    assert_eq!(read_from(lookup.keys().first()), pair("dim_brands", "brand_key"));
    assert_eq!(read_from(lookup.keys().get(1)), pair("dim_brands", "name"));
}

/// A filter on a dimension reached AFTER the crossing is bound on that dimension's own table.
#[test]
fn a_filter_after_the_crossing_binds_on_the_table_it_reaches() {
    let corpus = Corpus::placed("elsewhere", "elsewhere", "local");
    let resolution = Resolution {
        filters: vec![ResolvedFilter {
            dimension: corpus.key("region"),
            value: ResolvedFilterValue::Eq(String::from("north")),
        }],
        ..corpus.asking(vec![corpus.key("region")])
    };
    let Plan::Federated(planned) = plan(&resolution).expect("this resolution plans") else {
        panic!("a dimension on another data system is a federated plan");
    };
    let filter = planned
        .lookups()
        .legs()
        .first()
        .expect("one lookup leg")
        .leg()
        .filters()
        .first()
        .expect("the lookup leg carries the filter");
    let PlanPredicate::Equals { column, .. } = filter.predicate() else {
        panic!("an equality filter plans as Equals, got {:?}", filter.predicate());
    };
    assert_eq!(
        pair(&column.table().to_string(), &column.column().to_string()),
        pair("dim_regions", "label")
    );
}

/// Two tables of one lookup statement that answer to one identifier are refused, not aliased.
///
/// `regions` carried on a table named like `customers`: the lookup leg would render
/// `FROM dim_customers LEFT JOIN dim_customers`, which binds either column to the wrong table. The
/// fact leg's own tables take the same refusal.
#[test]
fn two_tables_of_a_lookup_leg_under_one_name_are_refused() {
    let mut corpus = Corpus::placed("elsewhere", "elsewhere", "local");
    corpus.reached[1].1 = Model::new(
        ModelName::parse("regions").expect("a test model is a model"),
        SourceName::parse("elsewhere").expect("a test source is a source"),
        TableName::parse("dim_customers").expect("a test table is a table"),
        ["code", "label"].iter().map(|c| column(c)),
        Description::default(),
    );
    let Err(refused) = plan(&corpus.asking(vec![corpus.key("region")])) else {
        panic!("two tables under one identifier must be refused");
    };
    assert!(
        matches!(
            refused,
            PlanError::Refused(RefusalReason::PlanTablesShareAnIdentifier { ref table }) if table.as_str() == "dim_customers"
        ),
        "expected the shared-identifier refusal, got {refused:?}"
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
        cross: None,
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
