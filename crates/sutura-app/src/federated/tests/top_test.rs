//! Case 2's own answer-orchestration cells, plus its truncation refusal and negative control -
//! `github.com/telekom/sutura#777`. Split out of `super` for that file's own `max-lines` reason,
//! not for thematic tidiness, so `use super::*` reaches every fixture (`federated_plan`,
//! `federated_fact_rows`, `federated_lookup_rows`, `FEDERATED_BUDGET`, `answer_federated` itself)
//! exactly as these tests read them before the move. `#[cfg(test)]` at the declaration, for
//! `leg_refusal_test`'s own reason: a bare `mod top_test;` declares nothing the causality scan
//! reads as a test module, so a later diff dropping only that line back to base would orphan this
//! file rather than fail loud.

use super::*;

/// `federated_plan()` asking for its top one group by the metric.
pub(super) fn top_one_plan() -> sutura_domain::plan::FederatedPlan {
    let top = sutura_domain::query::Top::new(
        sutura_domain::query::TopN::parse(1).expect("one is a row count"),
        sutura_domain::query::TopBy::Metric,
        sutura_domain::query::TopDirection::Desc,
    );
    federated_plan().with_top(top)
}

/// One case-2 answer, over the two leg-executing fakes and a chosen `top` row ceiling.
fn case_two_outcome(row_ceiling: sutura_domain::plan::RowCeiling) -> ToolOutcome {
    outcome_with(
        &top_one_plan(),
        sutura_domain::plan::Ceilings::new(
            row_ceiling,
            sutura_domain::plan::FederatedRowCeiling::DEFAULT,
            sutura_domain::plan::PlanCeilings::DEFAULT,
        ),
    )
}

/// One answer to `plan`, over the two leg-executing fakes (the shared two-group fixtures) and
/// chosen row ceilings.
///
/// **One builder rather than copies, and `check-jscpd` is why**: the cells that use it differ in the
/// plan, the ceilings and what they assert, and the setup around them was byte-identical once the
/// combiner became an argument. The ceilings are the only knob, because they are the only thing
/// `github.com/telekom/sutura#777`'s and `#828`'s refusals read.
pub(super) fn outcome_with(plan: &sutura_domain::plan::FederatedPlan, ceilings: sutura_domain::plan::Ceilings) -> ToolOutcome {
    outcome_over(plan, ceilings, federated_fact_rows(), federated_lookup_rows())
}

/// [`outcome_with`] over chosen leg results, for a cell whose combined answer must be wider than
/// the shared fixtures'.
pub(super) fn outcome_over(
    plan: &sutura_domain::plan::FederatedPlan,
    ceilings: sutura_domain::plan::Ceilings,
    fact_rows: RowSet,
    lookup_rows: RowSet,
) -> ToolOutcome {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        fact_source,
        shared.clone(),
        fact_rows,
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        lookup_source,
        shared,
        lookup_rows,
    ))
    .expect("two sources, one registry");
    let broker = crate::tests_support::CountingBroker::default();
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    answer_federated(
        (&bundle(), sutura_domain::warehouse::schema::PinnedSchemas::default().digest()),
        plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        &combiner,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        ceilings,
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

/// A combiner that answers one fixed combined answer, whatever its legs returned - so a cell can
/// hand the rank a measure column of a type no leg fake's `Value` rows can build.
struct Combined(ResultBatches);

impl sutura_domain::plan::FederationCombiner for Combined {
    type Error = core::fmt::Error;

    fn combine(
        &self,
        _plan: &sutura_domain::plan::FederatedPlan,
        _legs: sutura_domain::plan::federated::Legs<'_>,
        _working_set_bytes: u64,
    ) -> Result<ResultBatches, Self::Error> {
        Ok(self.0.clone())
    }

    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }

    fn answer_not_well_formed(&self, _error: &Self::Error) -> Option<sutura_domain::plan::federated::FederatedAnswerRefusal> {
        None
    }

    fn workers_not_ready(&self, _error: &Self::Error) -> Option<sutura_domain::plan::WorkersShort> {
        None
    }

    fn node_lost(&self, _error: &Self::Error) -> bool {
        false
    }
}

/// One family's row: its key, and its measure in hundredths, or `None` for a null measure.
type Family<'a> = (&'a str, Option<i128>);

/// A combined answer in the combine's own column order - keys, bucket, measure - whose measure is
/// a `Decimal128(38, 2)`: what `sum` over a scale-2 `DECIMAL` answers. One row per family, in key
/// order, each measure given as its unscaled value in hundredths.
fn decimal_answer(families: &[Family<'_>]) -> ResultBatches {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Decimal128Array, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema};

    let every_row = |value: &str| -> ArrayRef { Arc::new(StringArray::from(vec![value; families.len()])) };
    let schema = Arc::new(Schema::new(vec![
        Field::new("product_family", DataType::Utf8, false),
        Field::new("region", DataType::Utf8, false),
        Field::new(sutura_domain::catalog::TIME_BUCKET_LABEL, DataType::Utf8, false),
        Field::new("revenue", DataType::Decimal128(38, 2), true),
    ]));
    let measure = Decimal128Array::from(families.iter().map(|&(_, cents)| cents).collect::<Vec<_>>())
        .with_precision_and_scale(38, 2)
        .expect("38, 2 is a decimal type");
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(StringArray::from(
                families.iter().map(|&(family, _)| family).collect::<Vec<_>>(),
            )),
            every_row("north"),
            every_row("2026-06"),
            Arc::new(measure),
        ],
    )
    .expect("four columns for four fields");
    let budget = sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::MAX);
    let mut accumulating = sutura_domain::warehouse::Accumulating::announcing(schema, families.len(), budget);
    accumulating.push(batch).expect("the batch carries the schema it announces");
    accumulating.finish()
}

/// The ranked answer to a case-2 `top` over `combined`, as each row's family and measure cell.
fn ranked_over(combined: ResultBatches, n: u32, direction: sutura_domain::query::TopDirection) -> Vec<(Value, Value)> {
    let top = sutura_domain::query::Top::new(
        sutura_domain::query::TopN::parse(n).expect("a row count"),
        sutura_domain::query::TopBy::Metric,
        direction,
    );
    let source = |name| SourceName::parse(name).expect("a test source");
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        source("facts"),
        shared(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        source("geo"),
        shared(),
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");
    let outcome = answer_federated(
        (&bundle(), PinnedSchemas::default().digest()),
        &federated_plan().with_top(top),
        &asked_by_a_person(),
        &crate::tests_support::CountingBroker::default(),
        &warehouses,
        &Combined(combined),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect("a case-2 top is answered or refused, never an error")
    .into_outcome();
    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("a within-ceiling case-2 top is answered, not {outcome:?}");
    };
    let family = rows.column_index("product_family").expect("product_family is projected");
    let measure = rows.column_index("revenue").expect("the measure is projected");
    rows.rows()
        .iter()
        .map(|row| {
            (
                row.get(family).cloned().unwrap_or(Value::Null),
                row.get(measure).cloned().unwrap_or(Value::Null),
            )
        })
        .collect()
}

fn text(value: &str) -> Value {
    Value::Text(String::from(value))
}

/// A decimal measure ranks by its number: 1000.00 outranks 999.00. Ranked over the decoded rows,
/// both are text and `"999.00"` sorts after `"1000.00"` by bytes, so the top one was 999.00.
#[test]
fn a_federated_top_ranks_a_decimal_measure_by_its_number_not_its_text() {
    let ranked = ranked_over(
        decimal_answer(&[("A", Some(99_900)), ("B", Some(100_000))]),
        1,
        sutura_domain::query::TopDirection::Desc,
    );
    assert_eq!(ranked, vec![(text("B"), text("1000.00"))], "1000.00 outranks 999.00");
}

/// Ascending over negative values and differing digit counts, with a null: by number, nulls last.
/// By bytes `"-1.50"` sorts before `"-10.00"` and `"10.00"` before `"2.50"`.
#[test]
fn a_federated_top_ascending_orders_negative_decimals_by_value_and_nulls_last() {
    let ranked = ranked_over(
        decimal_answer(&[
            ("A", Some(-150)),
            ("B", Some(-1_000)),
            ("C", Some(250)),
            ("D", Some(1_000)),
            ("E", None),
        ]),
        5,
        sutura_domain::query::TopDirection::Asc,
    );
    assert_eq!(
        ranked,
        vec![
            (text("B"), text("-10.00")),
            (text("A"), text("-1.50")),
            (text("C"), text("2.50")),
            (text("D"), text("10.00")),
            (text("E"), Value::Null),
        ],
        "ascending by value, the null after every value"
    );
}

/// Descending over differing digit counts, a tie and a null: the widest number first, the tie in
/// its key order, the null last. By bytes `"999.00"` and `"99.50"` both outrank `"1000.00"`.
#[test]
fn a_federated_top_descending_puts_the_largest_decimal_first_ties_in_key_order_and_nulls_last() {
    let ranked = ranked_over(
        decimal_answer(&[
            ("A", Some(9_950)),
            ("B", Some(100_000)),
            ("C", None),
            ("D", Some(99_900)),
            ("E", Some(100_000)),
        ]),
        4,
        sutura_domain::query::TopDirection::Desc,
    );
    assert_eq!(
        ranked,
        vec![
            (text("B"), text("1000.00")),
            (text("E"), text("1000.00")),
            (text("D"), text("999.00")),
            (text("A"), text("99.50")),
        ],
        "descending by value, a tie stable in key order, and top 4 drops the null"
    );
}
