//! The cross-model ratio's own cells, split out of `super` (the `federated::tests` module) for
//! that file's own `max-lines` reason - the same reason `deadline_test` and `top_test` were. `use
//! super::*` reaches every fixture (`federated_plan`, `FEDERATED_BUDGET`, `answer_federated`,
//! `shared`, `bundle`, `asked_by_a_person`, `test_deadline`, `metric`, `june`) exactly as the
//! parent's own cells read them.

use super::*;

/// A two-fact `FederatedPlan` over three sources, the shape the splitter emits for a cross-model
/// ratio whose numerator and denominator read different fact models. The fact leg (source
/// "facts") carries the numerator leaf under the metric's own model; the second fact leg (source
/// "orders") carries the denominator leaf under a model it names; the lookup leg (source "geo")
/// decorates the shared link with a region. The `Federation` is an `Above::Quotient` of two leaves,
/// so `labels(&federation)` returns leaf 0 (numerator, model `None`) and leaf 1 (denominator,
/// model `Some(orders)`), and D9 splits the fact legs' terms along exactly that seam.
fn two_fact_plan() -> sutura_domain::plan::FederatedPlan {
    use sutura_domain::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, ModelName, TableName};
    use sutura_domain::plan::{
        AnswerKey, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel,
        StatementTables, labels,
    };

    let fact_source = SourceName::parse("facts").expect("a test source");
    let orders_source = SourceName::parse("orders").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let fact_table = TableName::parse("fct_subscription_monthly").expect("a test table");
    let orders_table = TableName::parse("dim_orders").expect("a test table");
    let column = |n: &str| ColumnName::parse(n).expect("a test column");
    let fact_col = |n: &str| PlanColumn::new(fact_table.clone(), column(n));
    let orders_col = |n: &str| PlanColumn::new(orders_table.clone(), column(n));
    let dimension = |n: &str| DimensionName::parse(n).expect("a test dimension");
    let fact_key = |n: &str| PlanKey::new(ResultLabel::dimension(&dimension(n)), fact_col(n));
    let link = |col: PlanColumn| PlanKey::new(ResultLabel::internal(InternalLabel::Link), col);
    let bucket = |c: &str, t: TableName| PlanBucket::new(ResultLabel::bucket(), Grain::Month, PlanColumn::new(t, column(c)));

    let orders_model = ModelName::parse("orders").expect("a test model");
    let measure = Measure::Ratio {
        numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
        denominator: Term::Aggregate(AggregatedColumn::on_model(
            Aggregate::Count,
            column("customer_key"),
            orders_model,
        )),
        zero_denominator: ZeroDenominator::Null,
    };
    let federation = sutura_domain::federation::Federation::of(&measure);

    // D9 splits a two-fact plan's terms by the model each leaf carries: the first fact leg takes
    // the `None`-model leaves (the numerator, leaf 0) and the second fact leg takes the rest (the
    // denominator, leaf 1). One placeholder `LegTerm` per leaf, labelled exactly as `labels` pairs
    // the carried leaves - the same zip the production splitter runs.
    let carried = federation.carried();
    let all_labels = labels(&federation);
    let fact_terms: Vec<LegTerm> = carried
        .iter()
        .zip(all_labels.iter())
        .filter(|(leaf, _)| leaf.model().is_none())
        .map(|(_, label)| {
            LegTerm::new(
                PlanTerm::CountIf {
                    column: fact_col("amount_cents"),
                },
                ResultLabel::internal(*label),
            )
        })
        .collect();
    let second_terms: Vec<LegTerm> = carried
        .iter()
        .zip(all_labels.iter())
        .filter(|(leaf, _)| leaf.model().is_some())
        .map(|(_, label)| {
            LegTerm::new(
                PlanTerm::CountIf {
                    column: orders_col("customer_key"),
                },
                ResultLabel::internal(*label),
            )
        })
        .collect();

    let fact = LegPlan::Fact {
        source: fact_source,
        metric: metric(),
        tables: StatementTables::only(fact_table.clone()),
        bucket: bucket("month", fact_table.clone()),
        keys: vec![fact_key("product_family"), link(fact_col("customer_key"))],
        terms: fact_terms,
        bindings: PlanBindings::none(),
        range: june(),
    };
    let second_fact = LegPlan::Fact {
        source: orders_source,
        metric: metric(),
        tables: StatementTables::only(orders_table.clone()),
        bucket: bucket("month", orders_table.clone()),
        keys: vec![link(orders_col("customer_key"))],
        terms: second_terms,
        bindings: PlanBindings::none(),
        range: june(),
    };
    let lookup = LegPlan::Lookup {
        source: lookup_source,
        table: fact_table.clone().into(),
        keys: vec![link(fact_col("customer_key")), fact_key("region")],
        bindings: PlanBindings::none(),
    };
    sutura_domain::plan::FederatedPlan::new(
        metric(),
        ResultLabel::measure(&metric()),
        bucket("month", fact_table.clone()),
        fact,
        Some(second_fact),
        lookup,
        true,
        federation,
        vec![
            AnswerKey::fact(ResultLabel::dimension(&dimension("product_family"))),
            AnswerKey::lookup(ResultLabel::dimension(&dimension("region"))),
        ],
    )
    .expect("a valid three-source two-fact plan")
}

/// The fact leg's rows: product family, link, bucket and the numerator leaf (leaf 0). The same
/// fixture the existing two-leg tests feed, so the numerator side is a known quantity.
fn two_fact_rows() -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![
            String::from("product_family"),
            InternalLabel::Link.label(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(0).label(),
        ],
        vec![
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
            vec![
                Value::Text("A".into()),
                Value::Text("c2".into()),
                Value::Text("2026-06".into()),
                Value::Integer(200),
            ],
            vec![
                Value::Text("B".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(50),
            ],
        ],
    )
    .expect("a well-formed two-fact fact result")
}

/// The second fact leg's rows: link, bucket and the denominator leaf (leaf 1). One count per
/// customer, the rows the denominator sums above the join.
fn second_fact_rows() -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![
            InternalLabel::Link.label(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(1).label(),
        ],
        vec![
            vec![Value::Text("c1".into()), Value::Text("2026-06".into()), Value::Integer(1)],
            vec![Value::Text("c2".into()), Value::Text("2026-06".into()), Value::Integer(1)],
        ],
    )
    .expect("a well-formed second-fact result")
}

/// The lookup leg's rows for the cross-model ratio: both customers in the north region, so the
/// combiner groups by (`product_family`, `region`) and the two fact rows under A re-aggregate together.
fn two_fact_lookup_rows() -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![InternalLabel::Link.label(), String::from("region")],
        vec![
            vec![Value::Text("c1".into()), Value::Text("north".into())],
            vec![Value::Text("c2".into()), Value::Text("north".into())],
        ],
    )
    .expect("a well-formed two-fact lookup result")
}

/// One real cell, the shape a ratio's quotient lands in.
fn real_cell(value: f64) -> Value {
    Value::Real(sutura_domain::warehouse::Real::parse(value).expect("a test real is finite"))
}
/// The fact leg's rows for the zero-denominator cell: only family A, so each region's group has
/// exactly one customer and the zero count is not summed away by a non-zero sibling.
fn zero_denominator_fact_rows() -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![
            String::from("product_family"),
            InternalLabel::Link.label(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(0).label(),
        ],
        vec![
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
            vec![
                Value::Text("A".into()),
                Value::Text("c2".into()),
                Value::Text("2026-06".into()),
                Value::Integer(200),
            ],
        ],
    )
    .expect("a well-formed two-fact fact result")
}

#[test]
fn a_cross_model_ratio_answer_is_one_certified_number() {
    // Three sources, one warehouse each: the fact leg answers the numerator, the second fact leg
    // answers the denominator, the lookup leg decorates the join. The combiner joins both facts
    // INNER on the link and the lookup LEFT, then groups by (product_family, region, bucket) and
    // divides the re-aggregated sums once, above the legs. A/north is (100 + 200) / (1 + 1) = 150
    // and B/north is 50 / 1 = 50, the two numbers this cell pins.
    let facts = SourceName::parse("facts").expect("a test source");
    let orders = SourceName::parse("orders").expect("a test source");
    let geo = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        facts,
        shared.clone(),
        two_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        orders,
        shared.clone(),
        second_fact_rows(),
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(
        geo,
        shared,
        two_fact_lookup_rows(),
    ))
    .expect("three sources, one registry");

    let plan = two_fact_plan();
    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &crate::tests_support::CountingBroker::default(),
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a three-source cross-model ratio is not an error")
    .into_outcome();
    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("a cross-model ratio whose three legs execute is answered, not {outcome:?}");
    };
    assert_eq!(rows.rows().len(), 2, "the two product families each form one group: {rows:?}");
    let by_family = |family: &str| {
        rows.rows()
            .iter()
            .find(|row| matches!(row.first(), Some(Value::Text(t)) if t == family))
            .unwrap_or_else(|| panic!("a row for {family} is present: {rows:?}"))
    };
    let a = by_family("A");
    let b = by_family("B");
    assert_eq!(a.last(), Some(&real_cell(150.0)), "A/north is (100+200)/(1+1) = 150.0: {a:?}");
    assert_eq!(b.last(), Some(&real_cell(50.0)), "B/north is 50/1 = 50.0: {b:?}");
}

#[test]
fn a_cross_model_ratio_with_a_zero_denominator_subgroup_emits_null() {
    // The zero-denominator guard is applied to the FINAL denominator, above every leg: a group
    // whose second-fact counts all sum to zero divides by zero, and `ZeroDenominator::Null` turns
    // that into a null cell rather than an error. Splitting the two customers across two regions
    // keeps the groups apart - c1 alone in the north (count 0, so 100/0 -> null) and c2 alone in
    // the south (count 1, so 200/1 = 200.0) - so the null survives the re-aggregation rather than
    // being summed away by a non-zero sibling in the same group.
    use sutura_domain::plan::InternalLabel;
    let second_fact = RowSet::new(
        vec![
            InternalLabel::Link.label(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(1).label(),
        ],
        vec![
            vec![Value::Text("c1".into()), Value::Text("2026-06".into()), Value::Integer(0)],
            vec![Value::Text("c2".into()), Value::Text("2026-06".into()), Value::Integer(1)],
        ],
    )
    .expect("a well-formed second-fact result");
    let lookup = RowSet::new(
        vec![InternalLabel::Link.label(), String::from("region")],
        vec![
            vec![Value::Text("c1".into()), Value::Text("north".into())],
            vec![Value::Text("c2".into()), Value::Text("south".into())],
        ],
    )
    .expect("a well-formed lookup result");

    let facts = SourceName::parse("facts").expect("a test source");
    let orders = SourceName::parse("orders").expect("a test source");
    let geo = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        facts,
        shared.clone(),
        zero_denominator_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        orders,
        shared.clone(),
        second_fact,
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(geo, shared, lookup))
    .expect("three sources, one registry");

    let plan = two_fact_plan();
    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a zero-denominator group is a governed answer")
    .into_outcome();
    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("a cross-model ratio with a zero-denominator group is answered with a null, not {outcome:?}");
    };
    assert_eq!(rows.rows().len(), 2, "one row per (family, region) group: {rows:?}");
    let north = rows
        .rows()
        .iter()
        .find(|row| matches!(row.get(1), Some(Value::Text(t)) if t == "north"))
        .unwrap_or_else(|| panic!("a north row is present: {rows:?}"));
    let south = rows
        .rows()
        .iter()
        .find(|row| matches!(row.get(1), Some(Value::Text(t)) if t == "south"))
        .unwrap_or_else(|| panic!("a south row is present: {rows:?}"));
    assert_eq!(
        north.last(),
        Some(&Value::Null),
        "A/north sums to 100 over a zero count, so null: {north:?}"
    );
    assert_eq!(south.last(), Some(&real_cell(200.0)), "A/south is 200/1 = 200.0: {south:?}");
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
        two_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        orders,
        postures[1].1.clone(),
        second_fact_rows(),
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(
        geo,
        postures[2].1.clone(),
        two_fact_lookup_rows(),
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
        sutura_domain::plan::RowCeiling::DEFAULT,
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
        two_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        geo,
        shared,
        two_fact_lookup_rows(),
    ))
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
        sutura_domain::plan::RowCeiling::DEFAULT,
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
        two_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        orders.clone(),
        sutura_domain::source::SourcePosture::ImpersonationAtSource,
        second_fact_rows(),
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(
        geo,
        shared(),
        two_fact_lookup_rows(),
    ))
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
        sutura_domain::plan::RowCeiling::DEFAULT,
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
    use sutura_domain::plan::InternalLabel;
    let duplicated = RowSet::new(
        vec![
            InternalLabel::Link.label(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(1).label(),
        ],
        vec![
            vec![Value::Text("c1".into()), Value::Text("2026-06".into()), Value::Integer(1)],
            vec![Value::Text("c1".into()), Value::Text("2026-06".into()), Value::Integer(1)],
            vec![Value::Text("c2".into()), Value::Text("2026-06".into()), Value::Integer(1)],
        ],
    )
    .expect("a well-formed second-fact result");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        two_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("orders").expect("a test source"),
        shared.clone(),
        duplicated,
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("geo").expect("a test source"),
        shared,
        two_fact_lookup_rows(),
    ))
    .expect("three sources, one registry");

    let outcome = answer_federated(
        &bundle(),
        &two_fact_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a deterministic combine failure is a refusal, not a `ServiceError`")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::FederatedAnswerNotWellFormed {
                    federated: sutura_domain::plan::FederatedAnswerRefusal::AmbiguousLink
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
    use sutura_domain::plan::InternalLabel;
    let bucket = String::from(sutura_domain::catalog::TIME_BUCKET_LABEL);
    let fact_rows = RowSet::new(
        vec![
            String::from("product_family"),
            InternalLabel::Link.label(),
            bucket.clone(),
            InternalLabel::Leaf(0).label(),
        ],
        vec![
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-07".into()),
                Value::Integer(300),
            ],
        ],
    )
    .expect("a well-formed two-period fact result");
    let second_rows = RowSet::new(
        vec![InternalLabel::Link.label(), bucket, InternalLabel::Leaf(1).label()],
        vec![
            vec![Value::Text("c1".into()), Value::Text("2026-06".into()), Value::Integer(1)],
            vec![Value::Text("c1".into()), Value::Text("2026-07".into()), Value::Integer(3)],
        ],
    )
    .expect("a well-formed two-period second-fact result");

    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        fact_rows,
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("orders").expect("a test source"),
        shared.clone(),
        second_rows,
    ))
    .expect("two sources so far")
    .and(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("geo").expect("a test source"),
        shared,
        two_fact_lookup_rows(),
    ))
    .expect("three sources, one registry");

    let outcome = answer_federated(
        &bundle(),
        &two_fact_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a two-period cross-model ratio is not an error")
    .into_outcome();
    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("a two-period cross-model ratio is answered, not {outcome:?}");
    };
    let ratios: Vec<&[Value]> = rows.rows().iter().map(|row| &row[2..]).collect();
    assert_eq!(
        ratios,
        vec![
            &[Value::Text("2026-06".into()), real_cell(100.0)][..],
            &[Value::Text("2026-07".into()), real_cell(100.0)][..],
        ],
        "each month divides its own rows once: {rows:?}"
    );
}
