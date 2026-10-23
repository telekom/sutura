//! Fan-in: three lookup legs, one joined onto another, each on its own link.

use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::plan::leg::LegPlan;
use sutura_domain::plan::{
    AnswerKey, FederatedAnswerRefusal, FederatedPlan, FederatedPlanError, InternalLabel, LinkKind, LookupLeg, LookupLegs,
    PlanBindings, PlanColumn, PlanKey, ResultLabel,
};
use sutura_domain::warehouse::{Real, ResultBatches, Value};

use super::fixtures::{FACT, UNBOUNDED, batches_of, column, dimension, fan_in_plan, leaf, try_combine_every, two_fact_plan};
use crate::combine::DataFusionCombiner;

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

fn link(leg: usize) -> String {
    InternalLabel::link(leg).label()
}

fn named(name: &str) -> String {
    String::from(name)
}

/// The fact leg and the three lookup legs, `crm` given `crm_rows`.
fn legs(crm_rows: Vec<Vec<Value>>) -> Vec<sutura_domain::warehouse::ResultBatches> {
    let fact = batches_of(
        vec![link(0), link(2), named(TIME_BUCKET_LABEL), leaf(0)],
        ["c1", "c2", "c3"]
            .into_iter()
            .zip([10, 20, 40])
            .map(|(customer, cents)| vec![text(customer), text(customer), text("2026-06"), Value::Integer(cents)])
            .collect(),
    );
    let geo = batches_of(
        vec![link(0), link(1), named("region")],
        vec![
            vec![text("c1"), text("r1"), text("north")],
            vec![text("c2"), text("r2"), text("south")],
            vec![text("c3"), text("r9"), text("west")],
        ],
    );
    let crm = batches_of(vec![link(1), named("area")], crm_rows);
    let ops = batches_of(vec![link(2), named("tier")], vec![vec![text("c1"), text("gold")]]);
    vec![fact, geo, crm, ops]
}

/// `crm` joins onto `geo` on `region_code`, INNER, so `c3`, whose region code `crm` lacks, is
/// dropped; `ops` joins onto the fact, LEFT, so `c2`, which `ops` lacks, stays with no tier.
#[test]
fn three_lookup_legs_and_a_chain_combine_on_their_own_links() {
    let crm = vec![vec![text("r1"), text("emea")], vec![text("r2"), text("apac")]];
    let answered = try_combine_every(&fan_in_plan(), &legs(crm), UNBOUNDED)
        .expect("a fan-in combine answers")
        .to_rows()
        .expect("a combined answer decodes");
    assert_eq!(answered.columns(), &["region", "area", "tier", "period", "revenue"]);
    assert_eq!(
        answered.rows(),
        &[
            vec![text("north"), text("emea"), text("gold"), text("2026-06"), Value::Integer(10)],
            vec![text("south"), text("apac"), Value::Null, text("2026-06"), Value::Integer(20)],
        ]
    );
}

/// A leg that joins onto an earlier lookup leg is checked on its own link: two `crm` rows for one
/// region code would double every measure joined through it.
#[test]
fn a_second_lookup_leg_with_two_rows_for_one_link_value_is_ambiguous() {
    let crm = vec![vec![text("r1"), text("emea")], vec![text("r1"), text("apac")]];
    let refused = try_combine_every(&fan_in_plan(), &legs(crm), UNBOUNDED).expect_err("an ambiguous link refuses");
    assert_eq!(
        sutura_domain::plan::FederationCombiner::answer_not_well_formed(
            &DataFusionCombiner::new().expect("a combiner builds"),
            &refused
        ),
        Some(FederatedAnswerRefusal::AmbiguousLink)
    );
}

/// A leg that joins onto an earlier lookup leg is typed on its own link: an integer `crm` link
/// against `geo`'s text link is refused, not joined under coercion.
#[test]
fn a_second_lookup_leg_whose_link_kind_disagrees_with_its_onto_leg_is_refused() {
    let crm = vec![vec![Value::Integer(1), text("emea")]];
    let refused = try_combine_every(&fan_in_plan(), &legs(crm), UNBOUNDED).expect_err("a link kind mismatch refuses");
    assert_eq!(
        sutura_domain::plan::FederationCombiner::answer_not_well_formed(
            &DataFusionCombiner::new().expect("a combiner builds"),
            &refused
        ),
        Some(FederatedAnswerRefusal::LinkTypeMismatch)
    );
}

/// A declaration names the first link only. Here the first link is an integer fact key against
/// `geo`'s text key, which the declaration casts; `crm`'s link against `geo`'s is two text keys,
/// which a declaration that reached it would refuse for want of an anchor, and which compare
/// exactly as they always did.
#[test]
fn a_later_lookup_leg_is_compared_exactly_under_a_declaration() {
    let mut every = legs(vec![vec![text("r1"), text("emea")], vec![text("r2"), text("apac")]]);
    every[0] = batches_of(
        vec![link(0), link(2), named(TIME_BUCKET_LABEL), leaf(0)],
        ["c1", "c2", "c3"]
            .into_iter()
            .zip([1, 2, 3])
            .zip([10, 20, 40])
            .map(|((customer, key), cents)| vec![Value::Integer(key), text(customer), text("2026-06"), Value::Integer(cents)])
            .collect(),
    );
    every[1] = batches_of(
        vec![link(0), link(1), named("region")],
        vec![
            vec![text("1"), text("r1"), text("north")],
            vec![text("2"), text("r2"), text("south")],
            vec![text("3"), text("r9"), text("west")],
        ],
    );
    let plan = fan_in_plan().with_link_comparison(Some(LinkKind::ExactInteger));
    let answered = try_combine_every(&plan, &every, UNBOUNDED)
        .expect("only the first link is declared")
        .to_rows()
        .expect("a combined answer decodes");
    assert_eq!(
        answered.rows(),
        &[
            vec![text("north"), text("emea"), text("gold"), text("2026-06"), Value::Integer(10)],
            vec![text("south"), text("apac"), Value::Null, text("2026-06"), Value::Integer(20)],
        ]
    );
}

fn quotient(value: f64) -> Value {
    Value::Real(Real::parse(value).expect("a test real is finite"))
}

/// One fact leg row: `(customer, product, leaf value)`, in June.
type RatioRow<'a> = (Option<&'a str>, &'a str, i64);

/// One fact leg's rows under leaf `at`.
fn ratio_fact(at: usize, rows: &[RatioRow<'_>]) -> ResultBatches {
    batches_of(
        vec![link(0), link(1), named(TIME_BUCKET_LABEL), leaf(at)],
        rows.iter()
            .map(|&(customer, product, value)| {
                vec![
                    customer.map_or(Value::Null, text),
                    text(product),
                    text("2026-06"),
                    Value::Integer(value),
                ]
            })
            .collect(),
    )
}

/// The four legs of the ratio over two lookups, in plan order: the fact, the second fact, `geo`
/// (customer to region) and `crm` (product to segment).
fn ratio_legs(revenue: ResultBatches, orders: ResultBatches) -> Vec<ResultBatches> {
    let geo = batches_of(
        vec![link(0), named("region")],
        vec![vec![text("c1"), text("north")], vec![text("c2"), text("south")]],
    );
    let crm = batches_of(
        vec![link(1), named("segment")],
        vec![vec![text("p1"), text("gold")], vec![text("p2"), text("silver")]],
    );
    vec![revenue, orders, geo, crm]
}

fn combined_rows(plan: &FederatedPlan, legs: &[ResultBatches]) -> Vec<Vec<Value>> {
    try_combine_every(plan, legs, UNBOUNDED)
        .expect("a ratio over two lookup legs answers")
        .to_rows()
        .expect("a combined answer decodes")
        .rows()
        .to_vec()
}

fn fact_table() -> TableName {
    TableName::parse(FACT).expect("a test table is a table")
}

fn lookup_tree(legs: Vec<LookupLeg>) -> LookupLegs {
    LookupLegs::parse(legs).expect("two lookup legs are a tree")
}

/// [`two_fact_plan`] over two lookup legs, `geo` (customer to region) and `crm` (product to segment).
///
/// With `chained` unset both legs join onto the fact and both fact legs group by both links and the
/// bucket. With it set `crm` joins onto `geo`, which projects the product link beside its own, and
/// the fact legs group by the customer link alone. Derived from the one-lookup plan so the builder
/// reads nothing the fixtures keep private.
fn ratio_plan(chained: bool) -> Result<FederatedPlan, FederatedPlanError> {
    let flat = two_fact_plan();
    let fact_table = fact_table();
    let linked = |leg: usize, name: &str| {
        PlanKey::new(
            ResultLabel::internal(InternalLabel::link(leg)),
            PlanColumn::new(fact_table.clone(), column(name)),
        )
    };
    let dimension_key = |name: &str| {
        PlanKey::new(
            ResultLabel::dimension(&dimension(name)),
            PlanColumn::new(fact_table.clone(), column(name)),
        )
    };
    let lookup = |on: &str, keys: Vec<PlanKey>| LegPlan::Lookup {
        source: SourceName::parse(on).expect("a test source is a source"),
        metric: sutura_domain::model::MetricName::parse("revenue").expect("a test metric is a metric"),
        table: fact_table.clone().into(),
        keys,
        bindings: PlanBindings::none(),
    };
    let fact_keys = if chained {
        vec![linked(0, "customer_key")]
    } else {
        vec![linked(0, "customer_key"), linked(1, "product_key")]
    };
    let with_fact_keys = |leg: &LegPlan| match leg.clone() {
        LegPlan::Fact {
            source,
            metric,
            tables,
            bucket,
            terms,
            bindings,
            range,
            ..
        } => LegPlan::Fact {
            source,
            metric,
            tables,
            bucket,
            keys: fact_keys.clone(),
            terms,
            bindings,
            range,
        },
        other @ LegPlan::Lookup { .. } => other,
    };
    let geo_keys = if chained {
        vec![linked(0, "customer_key"), linked(1, "product_key"), dimension_key("region")]
    } else {
        vec![linked(0, "customer_key"), dimension_key("region")]
    };
    let crm = lookup("crm", vec![linked(1, "product_key"), dimension_key("segment")]);
    let lookups = lookup_tree(vec![
        LookupLeg::onto_fact(lookup("geo", geo_keys), true),
        if chained {
            LookupLeg::onto_lookup(crm, 0, true)
        } else {
            LookupLeg::onto_fact(crm, true)
        },
    ]);
    FederatedPlan::new(
        flat.metric().clone(),
        ResultLabel::measure(flat.metric()),
        flat.bucket().clone(),
        with_fact_keys(flat.fact()),
        flat.second_fact().map(with_fact_keys),
        lookups,
        flat.federation().clone(),
        vec![
            AnswerKey::lookup(0, ResultLabel::dimension(&dimension("region"))),
            AnswerKey::lookup(1, ResultLabel::dimension(&dimension("segment"))),
        ],
    )
}

/// [`ratio_plan`], unchained, for the cells that need it to build.
fn ratio_over_two_lookups_plan() -> FederatedPlan {
    ratio_plan(false).expect("a ratio over two lookup legs is a plan")
}

/// Revenue is the fact's; orders the second fact's. `(c1, p1)` and `(c1, p2)` share a customer, so a
/// join on the customer link alone would fan each row out across the other's product.
fn revenue_and_orders() -> (ResultBatches, ResultBatches) {
    (
        ratio_fact(0, &[(Some("c1"), "p1", 100), (Some("c1"), "p2", 50), (Some("c2"), "p1", 30)]),
        ratio_fact(1, &[(Some("c1"), "p1", 2), (Some("c1"), "p2", 1), (Some("c2"), "p2", 4)]),
    )
}

/// Both facts meet each lookup on its own link, and a pair only one fact reached counts on its own
/// side: `(south, gold)` has revenue and no orders, `(south, silver)` orders and no revenue.
#[test]
fn a_ratio_over_two_lookup_legs_is_the_hand_worked_figure() {
    let (revenue, orders) = revenue_and_orders();
    assert_eq!(
        combined_rows(&ratio_over_two_lookups_plan(), &ratio_legs(revenue, orders)),
        vec![
            vec![text("north"), text("gold"), text("2026-06"), quotient(50.0)],
            vec![text("north"), text("silver"), text("2026-06"), quotient(50.0)],
            vec![text("south"), text("gold"), text("2026-06"), Value::Null],
            vec![text("south"), text("silver"), text("2026-06"), quotient(0.0)],
        ]
    );
}

/// Two second-fact rows for one pair of links would double every revenue row under them.
#[test]
fn a_second_fact_with_two_rows_for_one_pair_of_links_is_ambiguous() {
    let (revenue, _) = revenue_and_orders();
    let orders = ratio_fact(1, &[(Some("c1"), "p1", 2), (Some("c1"), "p1", 3)]);
    let refused = try_combine_every(&ratio_over_two_lookups_plan(), &ratio_legs(revenue, orders), UNBOUNDED)
        .expect_err("a repeated pair of links refuses");
    assert_eq!(
        sutura_domain::plan::FederationCombiner::answer_not_well_formed(
            &DataFusionCombiner::new().expect("a combiner builds"),
            &refused
        ),
        Some(FederatedAnswerRefusal::AmbiguousLink)
    );
}

/// A row with no customer joins no row of the other fact, and the two stay apart; both land in the
/// group of an unmatched customer, where their leaves are summed.
#[test]
fn a_ratio_row_with_a_null_first_link_still_counts_in_its_group() {
    let revenue = ratio_fact(0, &[(None, "p1", 40), (Some("c1"), "p1", 100)]);
    let orders = ratio_fact(1, &[(None, "p1", 2), (Some("c1"), "p1", 2)]);
    assert_eq!(
        combined_rows(&ratio_over_two_lookups_plan(), &ratio_legs(revenue, orders)),
        vec![
            vec![text("north"), text("gold"), text("2026-06"), quotient(50.0)],
            vec![Value::Null, text("gold"), text("2026-06"), quotient(20.0)],
        ]
    );
}

/// The second fact's product link is typed on its own: an integer one against the fact's text one is
/// refused, though the customer link agrees.
#[test]
fn a_second_fact_whose_second_link_kind_differs_is_refused() {
    let (revenue, _) = revenue_and_orders();
    let orders = batches_of(
        vec![link(0), link(1), named(TIME_BUCKET_LABEL), leaf(1)],
        vec![vec![text("c1"), Value::Integer(1), text("2026-06"), Value::Integer(2)]],
    );
    let refused = try_combine_every(&ratio_over_two_lookups_plan(), &ratio_legs(revenue, orders), UNBOUNDED)
        .expect_err("a link kind mismatch on the second link refuses");
    assert_eq!(
        sutura_domain::plan::FederationCombiner::answer_not_well_formed(
            &DataFusionCombiner::new().expect("a combiner builds"),
            &refused
        ),
        Some(FederatedAnswerRefusal::LinkTypeMismatch)
    );
}

/// A lookup leg that joins onto another is no link of the facts: both carry the customer link alone,
/// and `crm` meets `geo` on the product link inside the lookup side.
#[test]
fn a_ratio_over_a_chain_joins_the_facts_on_the_fact_parented_link_alone() {
    let fact = |at: usize, rows: [(&str, i64); 2]| {
        batches_of(
            vec![link(0), named(TIME_BUCKET_LABEL), leaf(at)],
            rows.iter()
                .map(|&(customer, value)| vec![text(customer), text("2026-06"), Value::Integer(value)])
                .collect(),
        )
    };
    let geo = batches_of(
        vec![link(0), link(1), named("region")],
        vec![
            vec![text("c1"), text("p1"), text("north")],
            vec![text("c2"), text("p2"), text("south")],
        ],
    );
    let crm = batches_of(
        vec![link(1), named("segment")],
        vec![vec![text("p1"), text("gold")], vec![text("p2"), text("silver")]],
    );
    let legs = [fact(0, [("c1", 100), ("c2", 30)]), fact(1, [("c1", 2), ("c2", 4)]), geo, crm];
    let plan = ratio_plan(true);
    assert!(plan.is_ok(), "a ratio over a chain is a plan: {:?}", plan.as_ref().err());
    let Ok(plan) = plan else { return };
    assert_eq!(
        combined_rows(&plan, &legs),
        vec![
            vec![text("north"), text("gold"), text("2026-06"), quotient(50.0)],
            vec![text("south"), text("silver"), text("2026-06"), quotient(7.5)],
        ]
    );
}

/// A cast can make two first-fact groups one key, and the probe that refuses them reads every fact
/// link and the bucket: `("1", p1)` and `("01", p2)` cast to one customer but are two groups on the
/// product link, so they are no repeated tuple.
#[test]
fn a_cast_first_fact_is_probed_for_a_repeated_tuple_of_every_link() {
    let plan = ratio_over_two_lookups_plan().with_link_comparison(Some(LinkKind::ExactInteger));
    let revenue = ratio_fact(0, &[(Some("1"), "p1", 100), (Some("01"), "p2", 50)]);
    let orders = ratio_fact(1, &[(Some("1"), "p1", 2), (Some("1"), "p2", 1)]);
    let geo = batches_of(vec![link(0), named("region")], vec![vec![Value::Integer(1), text("north")]]);
    let crm = batches_of(
        vec![link(1), named("segment")],
        vec![vec![text("p1"), text("gold")], vec![text("p2"), text("silver")]],
    );
    let answered = try_combine_every(&plan, &[revenue, orders, geo, crm], UNBOUNDED);
    assert!(
        answered.is_ok(),
        "two groups on two products are no repeated tuple: {:?}",
        answered.as_ref().err()
    );
    let rows = answered
        .ok()
        .and_then(|answer| answer.to_rows().ok())
        .map(|rows| rows.rows().to_vec());
    assert_eq!(
        rows,
        Some(vec![
            vec![text("north"), text("gold"), text("2026-06"), quotient(50.0)],
            vec![text("north"), text("silver"), text("2026-06"), quotient(50.0)],
        ])
    );
}
