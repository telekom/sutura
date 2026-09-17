//! Case 2's own answer-orchestration cells, plus its truncation refusal and negative control -
//! `github.com/telekom/sutura#777`. Split out of `super` for that file's own `max-lines` reason,
//! not for thematic tidiness, so `use super::*` reaches every fixture (`federated_plan`,
//! `federated_fact_rows`, `federated_lookup_rows`, `FEDERATED_BUDGET`, `answer_federated` itself)
//! exactly as these tests read them before the move. `#[cfg(test)]` at the declaration, for
//! `leg_refusal_test`'s own reason: a bare `mod top_test;` declares nothing the causality scan
//! reads as a test module, so a later diff dropping only that line back to base would orphan this
//! file rather than fail loud.

use super::*;

/// Case 1's shape, over `federated_plan()`'s own fixture data: `product_family` is the ONLY
/// answer key, fact-side, so `region` never reaches the answer at all and every fact row sharing
/// one `product_family` groups together regardless of which lookup row its link matched. Built by
/// hand rather than through the splitter, `topk::case_1_pushdown_is_exact_for_a_hand_built_plan`'s
/// own reason: case 1's precondition cannot be reached through a real question at all.
///
/// **What this proves that `topk` does not.** `topk` calls `FederatedPlan::combine` and
/// `FederatedPlan::rank` directly; this goes through the WHOLE `answer_federated` orchestration -
/// mint, pre-flight, execute, provenance - so `answer_federated`'s own case-1 branch (`plan.top()`
/// reading `None` and falling back to `plan.fact().fact_top()`) is exercised by something, not
/// only by the arithmetic underneath it.
fn case_1_plan() -> sutura_domain::plan::FederatedPlan {
    use sutura_domain::federation::Federation;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, TableName};
    use sutura_domain::plan::{
        AnswerKey, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel,
        StatementTables, labels,
    };
    use sutura_domain::query::{Top, TopBy, TopDirection, TopN};

    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let table = TableName::parse("fct_subscription_monthly").expect("a test table");
    let column = |n: &str| ColumnName::parse(n).expect("a test column");
    let tablecol = |n: &str| PlanColumn::new(table.clone(), column(n));
    let dimension = |n: &str| DimensionName::parse(n).expect("a test dimension");
    let key = |n: &str| PlanKey::new(ResultLabel::dimension(&dimension(n)), tablecol(n));
    let link = || PlanKey::new(ResultLabel::internal(InternalLabel::Link), tablecol("customer_key"));
    let bucket = |c: &str| PlanBucket::new(ResultLabel::bucket(), Grain::Month, PlanColumn::new(table.clone(), column(c)));

    let sum = Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))));
    let federation = Federation::of(&sum);
    let terms: Vec<LegTerm> = labels(&federation)
        .into_iter()
        .map(|label| {
            LegTerm::new(
                PlanTerm::CountIf {
                    column: tablecol("amount_cents"),
                },
                ResultLabel::internal(label),
            )
        })
        .collect();
    let top = Top::new(TopN::parse(1).expect("one is a row count"), TopBy::Metric, TopDirection::Desc);
    let fact = LegPlan::Fact {
        source: fact_source,
        metric: metric(),
        tables: StatementTables::only(table.clone()),
        bucket: bucket("month"),
        keys: vec![key("product_family"), link()],
        terms,
        bindings: PlanBindings::none(),
        range: june(),
        top: Some(sutura_domain::plan::FactTop::new(top, federation.ranking())),
    };
    let lookup = LegPlan::Lookup {
        source: lookup_source,
        table: table.clone().into(),
        keys: vec![link(), key("region")],
        bindings: PlanBindings::none(),
    };
    sutura_domain::plan::FederatedPlan::new(
        metric(),
        ResultLabel::measure(&metric()),
        bucket("month"),
        fact,
        lookup,
        true,
        federation,
        vec![AnswerKey::fact(ResultLabel::dimension(&dimension("product_family")))],
    )
    .expect("a fact-only answer key, a left join, is a valid case-1 plan")
}

/// Case 1 through the WHOLE orchestration, not only the arithmetic - see [`case_1_plan`]'s own
/// doc. `federated_fact_rows()` groups by `product_family` alone into `A` (100 + 200 = 300) and
/// `B` (50): top 1 must return `A`.
#[test]
fn a_federated_answer_takes_case_1_through_the_whole_orchestration() {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        lookup_source,
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");
    let broker = crate::tests_support::CountingBroker::default();
    let plan = case_1_plan();

    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a case-1 federated answer is not an error")
    .into_outcome();

    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("case 1 within the ceiling is answered, not {outcome:?}");
    };
    assert_eq!(rows.rows().len(), 1, "top 1 keeps exactly one row: {rows:?}");
    let family_at = rows.column_index("product_family").expect("product_family is projected");
    assert_eq!(
        rows.cell(0, family_at),
        Some(&Value::Text(String::from("A"))),
        "300 (A, across two link values) outranks 50 (B)"
    );
}

/// Case 2 - `github.com/telekom/sutura#777`: `region` is `AnswerKey::lookup`, so the rank happens
/// above the combine. `federated_plan()`'s own combined answer has two groups (`A`/`north` sums
/// to 300, `B`/`north` to 50), and asking for the top one must return `A` alone, ranked - not the
/// two groups `FederatedPlan::combine`'s own ascending-by-key sort would otherwise leave in place.
#[test]
fn a_federated_answer_ranks_the_combine_for_case_2_and_keeps_only_top_n() {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        lookup_source,
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");
    let broker = crate::tests_support::CountingBroker::default();
    let top = sutura_domain::query::Top::new(
        sutura_domain::query::TopN::parse(1).expect("one is a row count"),
        sutura_domain::query::TopBy::Metric,
        sutura_domain::query::TopDirection::Desc,
    );
    let plan = federated_plan().with_top(top);

    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a federated answer with a case-2 top is not an error")
    .into_outcome();

    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("a within-ceiling case-2 top is answered, not {outcome:?}");
    };
    assert_eq!(rows.rows().len(), 1, "top 1 keeps exactly one row: {rows:?}");
    let family_at = rows.column_index("product_family").expect("product_family is projected");
    assert_eq!(
        rows.cell(0, family_at),
        Some(&Value::Text(String::from("A"))),
        "300 outranks 50"
    );
}

/// Case 2's truncation refusal - `github.com/telekom/sutura#777`: the combined set (two groups)
/// already reaches a ceiling of one, so the top is refused rather than ranked over an arbitrary
/// slice. The predicate AND the refusal, not merely the predicate: the negative control right
/// after this proves the ceiling is what fires it, not the presence of `top` alone.
#[test]
fn a_combined_set_at_the_ceiling_refuses_a_case_2_top_rather_than_ranking_it() {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        lookup_source,
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");
    let broker = crate::tests_support::CountingBroker::default();
    let top = sutura_domain::query::Top::new(
        sutura_domain::query::TopN::parse(1).expect("one is a row count"),
        sutura_domain::query::TopBy::Metric,
        sutura_domain::query::TopDirection::Desc,
    );
    let plan = federated_plan().with_top(top);
    let ceiling = sutura_domain::plan::RowCeiling::parse(1).expect("one is a row ceiling");

    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        ceiling,
    )
    .expect("a refusal is not an error")
    .into_outcome();

    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::TopOverUncertifiedRows { ceiling: 1 }
            }
        ),
        "two combined groups over a ceiling of one must refuse naming it, not {outcome:?}"
    );
}

/// The negative control for the refusal above: the SAME plan and the SAME two combined groups,
/// with the ceiling widened to fit them, answers rather than refusing - so the cell above is
/// testing the ceiling comparison and not merely "a case-2 `top` always refuses".
#[test]
fn the_same_combined_set_under_a_wide_enough_ceiling_answers_instead() {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        lookup_source,
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");
    let broker = crate::tests_support::CountingBroker::default();
    let top = sutura_domain::query::Top::new(
        sutura_domain::query::TopN::parse(1).expect("one is a row count"),
        sutura_domain::query::TopBy::Metric,
        sutura_domain::query::TopDirection::Desc,
    );
    let plan = federated_plan().with_top(top);

    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("an answer is not an error")
    .into_outcome();

    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "the same two groups under the default ceiling must answer, not {outcome:?}"
    );
}
