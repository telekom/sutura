//! The cross-model ratio's own cells, split out of `super` (the `federated::tests` module) for
//! that file's own `max-lines` reason - the same reason `deadline_test` and `top_test` were. `use
//! super::*` reaches every fixture (`federated_plan`, `FEDERATED_BUDGET`, `answer_federated`,
//! `shared`, `bundle`, `asked_by_a_person`, `test_deadline`, `metric`, `june`) exactly as the
//! parent's own cells read them.

use sutura_domain::measure::ZeroDenominator;
use sutura_domain::plan::FederatedAnswerRefusal;

use super::*;

/// A two-fact `FederatedPlan` over three sources, the shape the splitter emits for a cross-model
/// ratio whose numerator and denominator read different fact models. The fact leg (source
/// "facts") carries the numerator leaf under the metric's own model; the second fact leg (source
/// "orders") carries the denominator leaf under a model it names; the lookup leg (source "geo")
/// decorates the shared link with a region. Both fact legs group by the link alone, which is the
/// only key `FederatedPlan::new` admits beside a second fact. The `Federation` is an
/// `Above::Quotient` of two leaves, so `labels(&federation)` returns leaf 0 (numerator, model
/// `None`) and leaf 1 (denominator, model `Some(orders)`), and D9 splits the fact legs' terms along
/// exactly that seam.
fn two_fact_plan_of(
    numerator: sutura_domain::model::Aggregate,
    zero_denominator: ZeroDenominator,
) -> sutura_domain::plan::FederatedPlan {
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, ModelName, TableName};
    use sutura_domain::plan::{
        AnswerKey, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel,
        StatementTables, labels,
    };

    let fact_table = TableName::parse("fct_subscription_monthly").expect("a test table");
    let orders_table = TableName::parse("dim_orders").expect("a test table");
    let column = |n: &str| ColumnName::parse(n).expect("a test column");
    let link = |table: &TableName| {
        PlanKey::new(
            ResultLabel::internal(InternalLabel::Link),
            PlanColumn::new(table.clone(), column("customer_key")),
        )
    };
    // Both fact legs bucket through ONE shared calendar column, which `FederatedPlan::new` holds.
    let calendar = PlanBucket::new(
        ResultLabel::bucket(),
        Grain::Month,
        PlanColumn::new(TableName::parse("dim_calendar").expect("a test table"), column("month")),
    );
    let measure = Measure::Ratio {
        numerator: Term::Aggregate(AggregatedColumn::new(numerator, column("amount_cents"))),
        denominator: Term::Aggregate(AggregatedColumn::on_model(
            Aggregate::Count,
            column("customer_key"),
            ModelName::parse("orders").expect("a test model"),
        )),
        zero_denominator,
    };
    let federation = sutura_domain::federation::Federation::of(&measure);

    // One placeholder `LegTerm` per leaf, labelled exactly as `labels` pairs the carried leaves -
    // the same zip the production splitter runs - and split by the model each leaf names.
    let terms = |second: bool, table: &TableName| -> Vec<LegTerm> {
        federation
            .carried()
            .iter()
            .zip(labels(&federation))
            .filter(|(leaf, _)| leaf.model().is_some() == second)
            .map(|(_, label)| {
                LegTerm::new(
                    PlanTerm::CountIf {
                        column: PlanColumn::new(table.clone(), column("amount_cents")),
                    },
                    ResultLabel::internal(label),
                )
            })
            .collect()
    };
    let fact_leg = |source: &str, table: &TableName, second: bool| LegPlan::Fact {
        source: SourceName::parse(source).expect("a test source"),
        metric: metric(),
        tables: StatementTables::only(table.clone()),
        bucket: calendar.clone(),
        keys: vec![link(table)],
        terms: terms(second, table),
        bindings: PlanBindings::none(),
        range: june(),
    };
    let fact = fact_leg("facts", &fact_table, false);
    let second_fact = fact_leg("orders", &orders_table, true);
    let lookup = LegPlan::Lookup {
        source: SourceName::parse("geo").expect("a test source"),
        table: fact_table.clone().into(),
        keys: vec![
            link(&fact_table),
            PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension")),
                PlanColumn::new(fact_table.clone(), column("region")),
            ),
        ],
        bindings: PlanBindings::none(),
    };
    sutura_domain::plan::FederatedPlan::new(
        metric(),
        ResultLabel::measure(&metric()),
        calendar,
        fact,
        Some(second_fact),
        lookup,
        true,
        federation,
        vec![AnswerKey::lookup(ResultLabel::dimension(
            &DimensionName::parse("region").expect("a test dimension"),
        ))],
    )
    .expect("a valid three-source two-fact plan")
}

fn two_fact_plan() -> sutura_domain::plan::FederatedPlan {
    two_fact_plan_of(sutura_domain::model::Aggregate::Sum, ZeroDenominator::Null)
}

/// One leg row: `(customer, period, leaf value)`.
type Dated<'a> = (&'a str, &'a str, i64);

/// One fact leg's rows under the link, the bucket and leaf `leaf`.
fn fact_rows(leaf: usize, rows: &[Dated<'_>]) -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![
            InternalLabel::Link.label(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(leaf).label(),
        ],
        rows.iter()
            .map(|&(link, period, value)| vec![Value::Text(link.into()), Value::Text(period.into()), Value::Integer(value)])
            .collect(),
    )
    .expect("well-formed fact rows")
}

/// The lookup leg's rows: `(customer, region)`.
fn region_rows(rows: &[(&str, &str)]) -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![InternalLabel::Link.label(), String::from("region")],
        rows.iter()
            .map(|&(link, region)| vec![Value::Text(link.into()), Value::Text(region.into())])
            .collect(),
    )
    .expect("well-formed lookup rows")
}

/// The fixture most cells read: c1 and c2 each with revenue and one visit in June, both in the north.
fn revenue_rows() -> RowSet {
    fact_rows(0, &[("c1", "2026-06", 150), ("c2", "2026-06", 200)])
}

fn visit_rows() -> RowSet {
    fact_rows(1, &[("c1", "2026-06", 1), ("c2", "2026-06", 1)])
}

fn north_rows() -> RowSet {
    region_rows(&[("c1", "north"), ("c2", "north")])
}

/// Three legs on three sources, answered through the real combiner by a broker that grants the
/// shared identity.
fn answered(plan: &sutura_domain::plan::FederatedPlan, fact: RowSet, second: RowSet, lookup: RowSet) -> ToolOutcome {
    let source = |name: &str| SourceName::parse(name).expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        source("facts"),
        shared.clone(),
        fact,
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        source("orders"),
        shared.clone(),
        second,
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(source("geo"), shared, lookup))
    .expect("three sources, one registry");
    answer_federated(
        &bundle(),
        plan,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .expect("a cross-model ratio is not an error")
    .into_outcome()
}

/// `(customer, value)` pairs as leaf `leaf`'s rows, all in June.
fn in_june(leaf: usize, rows: &[(&str, i64)]) -> RowSet {
    let dated: Vec<Dated<'_>> = rows.iter().map(|&(link, value)| (link, "2026-06", value)).collect();
    fact_rows(leaf, &dated)
}

/// The answer's rows over one fact leg's `(customer, revenue)`, the second's `(customer, visits)`
/// and the lookup's `(customer, region)`, all in June.
fn drilled_across(fact: &[(&str, i64)], second: &[(&str, i64)], regions: &[(&str, &str)]) -> Vec<Vec<Value>> {
    let outcome = answered(&two_fact_plan(), in_june(0, fact), in_june(1, second), region_rows(regions));
    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("a cross-model ratio whose three legs execute is answered, not {outcome:?}");
    };
    rows.rows().to_vec()
}

/// One real cell, the shape a ratio's quotient lands in.
fn real_cell(value: f64) -> Value {
    Value::Real(sutura_domain::warehouse::Real::parse(value).expect("a test real is finite"))
}

/// The rows for `region`, in period order.
fn in_region<'r>(rows: &'r [Vec<Value>], region: &str) -> Vec<&'r Vec<Value>> {
    rows.iter()
        .filter(|row| matches!(row.first(), Some(Value::Text(t)) if t == region))
        .collect()
}

#[test]
fn a_cross_model_ratio_answer_is_one_certified_number() {
    // Three sources, one warehouse each: the fact leg answers the numerator, the second fact leg
    // answers the denominator, the lookup leg decorates the join. The combiner joins both facts
    // FULL on the link and the bucket and the lookup LEFT, then groups by (region, bucket) and
    // divides the re-aggregated sums once, above the legs: north is (150 + 200) / (1 + 1) = 175.
    let outcome = answered(&two_fact_plan(), revenue_rows(), visit_rows(), north_rows());
    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("a cross-model ratio whose three legs execute is answered, not {outcome:?}");
    };
    assert_eq!(
        rows.rows(),
        [vec![
            Value::Text("north".into()),
            Value::Text("2026-06".into()),
            real_cell(175.0)
        ]],
        "one certified number for the one group"
    );
}

#[test]
fn a_cross_model_ratio_with_a_zero_denominator_subgroup_emits_null() {
    // The zero-denominator guard is applied to the FINAL denominator, above every leg: c1 alone in
    // the north counts 0 visits, so 100/0 -> null under `ZeroDenominator::Null`, and c2 alone in the
    // south is 200/1 = 200.0, so the null survives rather than being summed away by a sibling.
    let rows = drilled_across(
        &[("c1", 100), ("c2", 200)],
        &[("c1", 0), ("c2", 1)],
        &[("c1", "north"), ("c2", "south")],
    );
    assert_eq!(
        in_region(&rows, "north")[0].last(),
        Some(&Value::Null),
        "north divides by zero: {rows:?}"
    );
    assert_eq!(
        in_region(&rows, "south")[0].last(),
        Some(&real_cell(200.0)),
        "south is 200/1: {rows:?}"
    );
}

#[test]
fn a_two_fact_answer_records_all_three_sources_in_executed_as() {
    // `docs/adr/0040`'s disclosure for three legs: the two fact legs run under two postures and the
    // lookup under a third registration, and `executed_as` names all three sources in source order.
    // A cross-posture set is what makes the per-leg record the control the boot-time
    // acknowledgement is - and a three-leg shape is the one `telekom/sutura#780` adds the second
    // fact leg for.
    let facts = SourceName::parse("facts").expect("a test source");
    let orders = SourceName::parse("orders").expect("a test source");
    let geo = SourceName::parse("geo").expect("a test source");
    let postures = [
        (facts.clone(), shared()),
        (orders.clone(), sutura_domain::source::SourcePosture::ImpersonationAtSource),
        (geo.clone(), shared()),
    ];
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        facts,
        postures[0].1.clone(),
        revenue_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        orders,
        postures[1].1.clone(),
        visit_rows(),
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(
        geo,
        postures[2].1.clone(),
        north_rows(),
    ))
    .expect("three sources, one registry");

    let broker = crate::tests_support::AcknowledgingBroker::over(&postures);
    let plan = two_fact_plan();
    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .expect("a three-leg cross-posture answer is an Ok")
    .into_outcome();
    let ToolOutcome::Answer { provenance, .. } = outcome else {
        panic!("three legs under mixed postures are disclosed per leg, not refused: {outcome:?}");
    };
    assert_eq!(
        provenance
            .executed_as()
            .legs()
            .map(|(source, posture)| (source.as_str(), posture.as_str()))
            .collect::<Vec<(&str, &str)>>(),
        vec![
            ("facts", "shared-service-user"),
            ("geo", "shared-service-user"),
            ("orders", "impersonation-at-source"),
        ],
        "provenance records ALL three sources a two-fact answer ran as, each under its OWN posture"
    );
}

#[test]
fn a_two_fact_answer_whose_second_source_lacks_a_warehouse_is_refused() {
    // A two-fact plan needs a warehouse for its second fact's source, and the orchestrator looks
    // it up BEFORE any credential is minted - so a registry that omits it declines with
    // `SourceUnavailable` naming the missing source, the same refusal a missing fact or lookup
    // source already produces. This is the control that holds a three-leg plan to its three
    // registrations rather than half-answering over two.
    let facts = SourceName::parse("facts").expect("a test source");
    let geo = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        facts,
        shared.clone(),
        revenue_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(geo, shared, north_rows()))
    .expect("two sources, one registry - the second fact source is missing");

    let refused = answer_federated(
        &bundle(),
        &two_fact_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .expect("a refusal is an Ok")
    .into_outcome();
    assert!(
        matches!(
            refused,
            ToolOutcome::Refusal { reason: RefusalReason::SourceUnavailable { ref source } }
                if source.as_str() == "orders"
        ),
        "a second fact source with no warehouse is refused before minting, naming the source: {refused:?}"
    );
}

#[test]
fn a_second_fact_whose_credential_disagrees_with_its_adapters_posture_never_executes() {
    // The federated path's own posture check, asked of the third leg: the broker grants the
    // deployment's identity to every source, and the second fact's adapter declares
    // `impersonation-at-source`. Answering would record the leg as the asker while it ran as the
    // process, so the answer fails as a wiring error naming that leg.
    let facts = SourceName::parse("facts").expect("a test source");
    let orders = SourceName::parse("orders").expect("a test source");
    let geo = SourceName::parse("geo").expect("a test source");
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        facts,
        shared(),
        revenue_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        orders.clone(),
        sutura_domain::source::SourcePosture::ImpersonationAtSource,
        visit_rows(),
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(geo, shared(), north_rows()))
    .expect("three sources, one registry");

    let failure = answer_federated(
        &bundle(),
        &two_fact_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .expect_err("a leg that is not its posture's shape is a wiring failure, not an answer");
    assert!(
        matches!(
            failure,
            crate::ServiceError::Posture {
                cause: sutura_domain::identity::PresentedDisagreesWithPosture::ShapeIsNotThePosture { ref at, .. },
            } if *at == orders
        ),
        "the second fact's posture disagreement is refused, naming its source: {failure:?}"
    );
}

#[test]
fn a_second_fact_with_two_rows_for_one_link_and_period_is_refused() {
    // The second fact is joined on the link and the bucket, so two of its rows for c1 in June
    // would meet every first-fact c1 row twice and double the numerator under it. Refused as the
    // lookup side's own duplicate is, rather than answered as a wrong number.
    let duplicated = fact_rows(1, &[("c1", "2026-06", 1), ("c1", "2026-06", 1), ("c2", "2026-06", 1)]);
    let outcome = answered(&two_fact_plan(), revenue_rows(), duplicated, north_rows());
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::FederatedAnswerNotWellFormed {
                    federated: FederatedAnswerRefusal::AmbiguousLink
                }
            }
        ),
        "a duplicated second-fact row is refused, not answered: {outcome:?}"
    );
}

#[test]
fn a_second_fact_with_two_periods_is_joined_per_period_rather_than_fanned_out() {
    // One customer over two months on BOTH facts. Joined on the link alone, each first-fact row
    // would meet both second-fact rows: June (100 + 100) / (1 + 3) = 50 and July (300 + 300) /
    // (1 + 3) = 150. Joined on the link and the bucket, each month divides its own rows once:
    // 100 / 1 and 300 / 3, both 100.
    let fact = fact_rows(0, &[("c1", "2026-06", 100), ("c1", "2026-07", 300)]);
    let second = fact_rows(1, &[("c1", "2026-06", 1), ("c1", "2026-07", 3)]);
    let ToolOutcome::Answer { rows, .. } = answered(&two_fact_plan(), fact, second, north_rows()) else {
        panic!("a two-period cross-model ratio is answered");
    };
    let ratios: Vec<&[Value]> = rows.rows().iter().map(|row| &row[1..]).collect();
    assert_eq!(
        ratios,
        vec![
            &[Value::Text("2026-06".into()), real_cell(100.0)][..],
            &[Value::Text("2026-07".into()), real_cell(100.0)][..],
        ],
        "each month divides its own rows once: {rows:?}"
    );
}

/// Review probe C1, as a golden: c2 has revenue and no visit, and its revenue still counts. The
/// ratio of the region's totals is (100 + 200) / 1 = 300; joined INNER, c2 left the numerator and
/// the certified number was 100.
#[test]
fn a_customer_only_the_first_fact_reached_still_counts_in_the_numerator() {
    let rows = drilled_across(&[("c1", 100), ("c2", 200)], &[("c1", 1)], &[("c1", "north"), ("c2", "north")]);
    assert_eq!(
        in_region(&rows, "north")[0].last(),
        Some(&real_cell(300.0)),
        "north is (100 + 200) / 1: {rows:?}"
    );
}

/// The owner's empty-set rule, numerator side: a region only the revenue fact reached reads its
/// visits as a count over no rows, 0, so it divides by zero - and `Null` answers null for it,
/// never a missing row (review probe C2).
#[test]
fn a_group_only_the_numerator_reached_divides_by_an_empty_count() {
    let rows = drilled_across(&[("c1", 100), ("c2", 200)], &[("c1", 1)], &[("c1", "north"), ("c2", "south")]);
    assert_eq!(
        in_region(&rows, "south")[0].last(),
        Some(&Value::Null),
        "south is 200/0 under Null: {rows:?}"
    );
    assert_eq!(in_region(&rows, "north")[0].last(), Some(&real_cell(100.0)));
}

/// The same group under `Fail`: an empty count is a zero, and a zero denominator is the fault the
/// definition declared - refused exactly as an explicit zero row is, never answered as null.
#[test]
fn a_group_only_the_numerator_reached_is_refused_under_fail() {
    let outcome = answered(
        &two_fact_plan_of(sutura_domain::model::Aggregate::Sum, ZeroDenominator::Fail),
        fact_rows(0, &[("c1", "2026-06", 100), ("c2", "2026-06", 200)]),
        fact_rows(1, &[("c1", "2026-06", 1)]),
        region_rows(&[("c1", "north"), ("c2", "south")]),
    );
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::FederatedAnswerNotWellFormed {
                    federated: FederatedAnswerRefusal::NonFinite
                }
            }
        ),
        "200/0 under Fail is refused, as an explicit zero row is: {outcome:?}"
    );
}

/// The empty-set rule, denominator side: a region only the visits fact reached reads its revenue as
/// a sum over no rows, 0, so its ratio is 0/2 = 0 - a figure, not a null and not a missing row.
#[test]
fn a_group_only_the_denominator_reached_is_a_zero_ratio() {
    let rows = drilled_across(&[("c1", 100)], &[("c1", 1), ("c2", 2)], &[("c1", "north"), ("c2", "south")]);
    assert_eq!(
        in_region(&rows, "south").into_iter().map(|row| &row[1..]).collect::<Vec<_>>(),
        [&[Value::Text("2026-06".into()), real_cell(0.0)][..]],
        "south is 0/2 in June: {rows:?}"
    );
}

/// The coalesced bucket keeps a customer only the second fact reached apart per period: c2's June
/// and July visits are two rows, each under its own period, never one row with a null period.
#[test]
fn a_customer_only_the_second_fact_reached_keeps_each_period_apart() {
    let outcome = answered(
        &two_fact_plan(),
        fact_rows(0, &[("c1", "2026-06", 100)]),
        fact_rows(1, &[("c1", "2026-06", 1), ("c2", "2026-06", 1), ("c2", "2026-07", 1)]),
        region_rows(&[("c1", "north"), ("c2", "south")]),
    );
    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("a cross-model ratio is answered, not {outcome:?}");
    };
    let south: Vec<&Value> = in_region(rows.rows(), "south").into_iter().map(|row| &row[1]).collect();
    assert_eq!(
        south,
        [&Value::Text("2026-06".into()), &Value::Text("2026-07".into())],
        "two periods, two rows: {rows:?}"
    );
}

/// A minimum has no value over no rows, so the combine refuses a two-fact plan carrying one rather
/// than reading an absent side as 0. `Definitions::assemble` refuses such a definition first; this
/// hand-built plan is the only way past it, and the cell is the witness for the combiner's own guard.
#[test]
fn a_two_fact_plan_with_a_minimum_leaf_is_not_combined() {
    let source = |name: &str| SourceName::parse(name).expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        source("facts"),
        shared.clone(),
        revenue_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        source("orders"),
        shared.clone(),
        visit_rows(),
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(
        source("geo"),
        shared,
        north_rows(),
    ))
    .expect("three sources, one registry");
    let failure = answer_federated(
        &bundle(),
        &two_fact_plan_of(sutura_domain::model::Aggregate::Min, ZeroDenominator::Null),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .err();
    assert!(
        matches!(
            failure,
            Some(crate::ServiceError::Combine {
                cause: sutura_exec_datafusion::CombineError::UnsupportedAggregate {
                    aggregate: sutura_domain::model::Aggregate::Min
                }
            })
        ),
        "a minimum leaf in a two-fact plan is refused by the combiner: {failure:?}"
    );
}
