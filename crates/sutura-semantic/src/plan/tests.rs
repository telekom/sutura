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
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::plan::leg::LegPlan;
use sutura_domain::plan::{
    AnswerKey, InternalLabel, LegAt, LegCeiling, LookupLeg, LookupLegCeiling, PlanCeilings, PlanPredicate, QueryPlan,
    SourceCeiling,
};
use sutura_domain::query::RefusalReason;

use super::{Plan, PlanError};
use crate::resolve::{Resolution, ResolvedDimension, ResolvedFilter, ResolvedFilterValue, ResolvedJoin};
use crate::{CompileFailure, Compiled};

mod compound_link;
mod corpus;
mod drill_across;
mod federation;
mod link_types;
mod ratio_links;
mod two_sources;
use corpus::{Corpus, federated_by, join_tables, lookup_at, lookup_shape, pair, read_from};
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

/// [`super::plan`] under the default ceilings, which every cell but the ceilings' own plans under.
fn plan(resolution: &Resolution<'_>) -> Result<Plan, PlanError> {
    super::plan(resolution, PlanCeilings::DEFAULT)
}

/// The plan ceilings of a deployment that configured `sources`, `lookup_legs` and `legs`.
fn ceilings(sources: usize, lookup_legs: usize, legs: usize) -> PlanCeilings {
    PlanCeilings::new(
        SourceCeiling::parse(sources).expect("a test source ceiling parses"),
        LookupLegCeiling::parse(lookup_legs).expect("a test lookup leg ceiling parses"),
        LegCeiling::parse(legs).expect("a test leg ceiling parses"),
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

/// A chain that crosses and comes back is planned as federated legs, never as one statement.
///
/// `customers` on `elsewhere` with `regions` back on `local`: the crossing back is a second lookup
/// leg that joins onto the first, and the second crossing's link leaves the first leg at the column
/// it starts from.
#[test]
fn a_chain_that_crosses_and_comes_back_is_planned_as_two_federated_legs() {
    let corpus = Corpus::placed("elsewhere", "local", "local");
    let planned = federated_by(&corpus, "region");
    assert_eq!(
        lookup_shape(&planned),
        vec![
            (String::from("elsewhere"), LegAt::Fact),
            (String::from("local"), LegAt::Lookup(0))
        ]
    );
    let (first, second) = (lookup_at(&planned, 0), lookup_at(&planned, 1));
    let link = |at: usize| InternalLabel::link(at).label();
    assert_eq!(first.keys().get(1).map(|key| key.label().to_owned()), Some(link(1)));
    assert_eq!(read_from(first.keys().get(1)), pair("dim_customers", "region_code"));
    assert_eq!(read_from(second.keys().first()), pair("dim_regions", "code"));
    assert_eq!(read_from(second.keys().get(1)), pair("dim_regions", "label"));
    assert_eq!(planned.keys().first().map(AnswerKey::side), Some(LegAt::Lookup(1)));
}

/// A chain that crosses onto one data system and on to a third spends two lookup legs.
#[test]
fn a_chain_that_crosses_and_on_to_a_third_system_is_planned_as_two_federated_legs() {
    let corpus = Corpus::placed("elsewhere", "third", "local");
    let planned = federated_by(&corpus, "region");
    assert_eq!(
        lookup_shape(&planned),
        vec![
            (String::from("elsewhere"), LegAt::Fact),
            (String::from("third"), LegAt::Lookup(0))
        ]
    );
}

/// Two dimensions that cross onto two data systems are two lookup legs, each joined onto the fact.
///
/// The fact leg groups by the origin of BOTH links, the second of them on `products`, the table the
/// hop before the crossing landed on, and each answer key is read from the leg that holds it.
#[test]
fn dimensions_on_two_other_systems_are_planned_as_two_lookup_legs_on_the_fact() {
    let corpus = Corpus::placed("a", "a", "b");
    let Plan::Federated(planned) = plan(&corpus.asking(vec![corpus.key("region"), corpus.key("brand")])).expect("this plans")
    else {
        panic!("dimensions on other data systems are a federated plan");
    };
    assert_eq!(
        lookup_shape(&planned),
        vec![(String::from("a"), LegAt::Fact), (String::from("b"), LegAt::Fact)]
    );
    assert_eq!(read_from(planned.fact().keys().first()), pair("dim_facts", "customer_key"));
    assert_eq!(read_from(planned.fact().keys().get(1)), pair("dim_products", "brand_key"));
    assert_eq!(join_tables(planned.fact()), vec![pair("dim_facts", "dim_products")]);
    assert_eq!(
        planned.keys().iter().map(AnswerKey::side).collect::<Vec<_>>(),
        vec![LegAt::Lookup(0), LegAt::Lookup(1)]
    );
}

/// A filter makes ITS lookup leg an INNER join and leaves the others as they were.
#[test]
fn a_filter_on_one_lookup_leg_makes_only_that_leg_inner() {
    let corpus = Corpus::placed("a", "a", "b");
    let resolution = Resolution {
        filters: vec![ResolvedFilter {
            dimension: corpus.key("region"),
            value: ResolvedFilterValue::Eq(String::from("north")),
        }],
        ..corpus.asking(vec![corpus.key("region"), corpus.key("brand")])
    };
    let Plan::Federated(planned) = plan(&resolution).expect("this plans") else {
        panic!("dimensions on other data systems are a federated plan");
    };
    assert_eq!(
        planned
            .lookups()
            .legs()
            .iter()
            .map(LookupLeg::include_unmatched)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

/// Exactly three lookup legs are planned: the cap is a bound that admits its own count.
///
/// `region` crosses twice and `family` once, over three other data systems, so the plan carries a
/// chain's two legs and a leg of its own, and a cap that refused its own limit would refuse it.
#[test]
fn a_question_that_needs_exactly_three_lookup_legs_is_planned() {
    let corpus = Corpus::spread("a", "b", "c", "local");
    let Plan::Federated(planned) =
        plan(&corpus.asking(vec![corpus.key("region"), corpus.key("family")])).expect("three legs plan")
    else {
        panic!("dimensions on other data systems are a federated plan");
    };
    assert_eq!(
        lookup_shape(&planned),
        vec![
            (String::from("a"), LegAt::Fact),
            (String::from("b"), LegAt::Lookup(0)),
            (String::from("c"), LegAt::Fact)
        ]
    );
}

/// The legs, their link labels and the side each answer key is read from do not depend on the order
/// the caller listed the dimensions.
///
/// `brand` sorts after `region` by its crossing relationship, so a leg numbered by arrival would swap
/// the two links when `brand` is asked first.
#[test]
fn the_legs_and_their_links_do_not_depend_on_the_order_the_dimensions_are_asked() {
    let corpus = Corpus::placed("a", "a", "b");
    let planned =
        |by: [&str; 2]| match plan(&corpus.asking(by.iter().map(|name| corpus.key(name)).collect())).expect("this plans") {
            Plan::Federated(planned) => planned,
            Plan::Mono(_) | Plan::DrillAcross(_) => panic!("dimensions on other data systems are a federated plan"),
        };
    let (forward, reverse) = (planned(["region", "brand"]), planned(["brand", "region"]));
    assert_eq!(lookup_shape(&forward), lookup_shape(&reverse));
    let fact_keys = |plan: &sutura_domain::plan::FederatedPlan| {
        plan.fact()
            .keys()
            .iter()
            .map(|key| (key.label().to_owned(), read_from(Some(key))))
            .collect::<Vec<_>>()
    };
    assert_eq!(fact_keys(&forward), fact_keys(&reverse));
    let side_of = |plan: &sutura_domain::plan::FederatedPlan, label: &str| {
        plan.keys().iter().find(|key| key.label() == label).map(AnswerKey::side)
    };
    for label in ["region", "brand"] {
        assert_eq!(
            side_of(&forward, label),
            side_of(&reverse, label),
            "{label} is read from one leg either way"
        );
    }
}

/// One metric under a configured ceiling of three: three data systems plan, and a fourth - its fact
/// leg beside three lookup legs - is refused naming the configured number rather than the default.
#[test]
fn one_metric_reads_up_to_a_configured_ceiling_of_three_data_systems_and_not_four() {
    let three = ceilings(3, 3, 6);
    let within = Corpus::placed("a", "a", "b");
    let planned = super::plan(&within.asking(vec![within.key("region"), within.key("brand")]), three);
    assert!(
        matches!(planned, Ok(Plan::Federated(_))),
        "local, a and b are three data systems, got {:?}",
        planned.err()
    );
    let over = Corpus::spread("a", "b", "c", "local");
    let Err(PlanError::Refused(reason)) = super::plan(&over.asking(vec![over.key("region"), over.key("family")]), three) else {
        panic!("a fact leg and three lookup legs on other data systems are four, over a ceiling of three");
    };
    assert_eq!(reason, RefusalReason::PlanSpansTooManySources { sources: 4, limit: 3 });
}

/// One metric under a configured lookup leg ceiling, each side of a value below and above the default
/// three: at one, a single lookup leg plans and two are refused naming one; at four, the four lookup
/// legs the default refuses plan.
#[test]
fn one_metric_reads_up_to_a_configured_lookup_leg_ceiling_lower_or_higher_than_three() {
    let one = ceilings(4, 1, 6);
    let single = Corpus::placed("elsewhere", "elsewhere", "local");
    let planned = super::plan(&single.asking(vec![single.key("region")]), one);
    assert!(
        matches!(planned, Ok(Plan::Federated(_))),
        "one lookup leg plans under one, got {:?}",
        planned.err()
    );
    let two = Corpus::placed("a", "a", "b");
    let Err(PlanError::Refused(reason)) = super::plan(&two.asking(vec![two.key("region"), two.key("brand")]), one) else {
        panic!("two lookup legs are over a lookup leg ceiling of one");
    };
    assert_eq!(reason, RefusalReason::PlanNeedsTooManyLookupLegs { legs: 2, limit: 1 });
    let four = Corpus::spread("a", "b", "c", "d");
    let Ok(Plan::Federated(planned)) = super::plan(&four.asking(vec![four.key("region"), four.key("brand")]), ceilings(5, 4, 6))
    else {
        panic!("four lookup legs plan under a lookup leg ceiling of four");
    };
    assert_eq!(planned.lookups().legs().len(), 4);
}

/// One metric under a configured leg ceiling below the default six: its fact leg and three lookup legs
/// are four legs, which plan under four and are refused under three, naming three.
#[test]
fn one_metric_runs_up_to_a_configured_leg_ceiling_and_not_one_leg_more() {
    let corpus = Corpus::spread("a", "b", "c", "local");
    let asked = || corpus.asking(vec![corpus.key("region"), corpus.key("family")]);
    let planned = super::plan(&asked(), ceilings(4, 3, 4));
    assert!(
        matches!(planned, Ok(Plan::Federated(_))),
        "four legs plan under four, got {:?}",
        planned.err()
    );
    let Err(PlanError::Refused(reason)) = super::plan(&asked(), ceilings(4, 3, 3)) else {
        panic!("four legs are over a leg ceiling of three");
    };
    assert_eq!(reason, RefusalReason::QuestionNeedsTooManyLegs { legs: 4, limit: 3 });
}

/// A question that would read four lookup legs is refused by name, with the count and the cap.
#[test]
fn a_question_that_needs_four_lookup_legs_is_refused_with_the_count() {
    let corpus = Corpus::spread("a", "b", "c", "d");
    let Err(PlanError::Refused(reason)) = plan(&corpus.asking(vec![corpus.key("region"), corpus.key("brand")])) else {
        panic!("four lookup legs must be refused");
    };
    assert_eq!(reason, RefusalReason::PlanNeedsTooManyLookupLegs { legs: 4, limit: 3 });
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
