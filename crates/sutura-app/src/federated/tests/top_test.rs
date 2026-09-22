//! Case 2's own answer-orchestration cells, plus its truncation refusal and negative control -
//! `github.com/telekom/sutura#777`. Split out of `super` for that file's own `max-lines` reason,
//! not for thematic tidiness, so `use super::*` reaches every fixture (`federated_plan`,
//! `federated_fact_rows`, `federated_lookup_rows`, `FEDERATED_BUDGET`, `answer_federated` itself)
//! exactly as these tests read them before the move. `#[cfg(test)]` at the declaration, for
//! `leg_refusal_test`'s own reason: a bare `mod top_test;` declares nothing the causality scan
//! reads as a test module, so a later diff dropping only that line back to base would orphan this
//! file rather than fail loud.

use super::*;

/// One case-2 answer, over the two leg-executing fakes and a chosen `top` row ceiling.
///
/// **One builder rather than three copies, and `check-jscpd` is why**: the three cells below differ
/// in the ceiling and in what they assert, and the setup around them was byte-identical once the
/// combiner became an argument. `row_ceiling` is the only knob, because it is the only thing
/// `github.com/telekom/sutura#777`'s refusal reads.
fn case_two_outcome(row_ceiling: sutura_domain::plan::RowCeiling) -> ToolOutcome {
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
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        &combiner,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        row_ceiling,
    )
    .expect("a case-2 top is answered or refused, never an error")
    .into_outcome()
}

/// Case 2 - `github.com/telekom/sutura#777`: `region` is `AnswerKey::lookup`, so the rank happens
/// above the combine. `federated_plan()`'s own combined answer has two groups (`A`/`north` sums
/// to 300, `B`/`north` to 50), and asking for the top one must return `A` alone, ranked - not the
/// two groups `FederatedPlan::combine`'s own ascending-by-key sort would otherwise leave in place.
#[test]
fn a_federated_answer_ranks_the_combine_for_case_2_and_keeps_only_top_n() {
    let outcome = case_two_outcome(sutura_domain::plan::RowCeiling::DEFAULT);

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
    let ceiling = sutura_domain::plan::RowCeiling::parse(1).expect("one is a row ceiling");
    let outcome = case_two_outcome(ceiling);

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
    let outcome = case_two_outcome(sutura_domain::plan::RowCeiling::DEFAULT);

    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "the same two groups under the default ceiling must answer, not {outcome:?}"
    );
}
