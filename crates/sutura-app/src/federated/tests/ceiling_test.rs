//! The federated answer's row ceiling is a configured value, and one row over it is a refusal that
//! names it - `github.com/telekom/sutura#828`. `federated_plan()`'s combined answer has two groups,
//! so a ceiling of one is crossed and a ceiling of two is reached without being crossed. A `top`
//! answer is held to it too, over the combined set it is ranked from.

use super::top_test::{outcome_over, outcome_with, top_one_plan};
use super::*;
use sutura_domain::plan::{FederatedRowCeiling, RowCeiling, RowCeilings};

fn ceilings(rows: u32) -> RowCeilings {
    RowCeilings::new(
        RowCeiling::DEFAULT,
        FederatedRowCeiling::parse(rows).expect("a test ceiling is a ceiling"),
    )
}

fn under(rows: u32) -> ToolOutcome {
    outcome_with(&federated_plan(), ceilings(rows))
}

#[test]
fn a_combined_answer_over_the_configured_ceiling_is_refused_naming_it() {
    let outcome = under(1);
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    bound: ResultBound::Rows { limit: 1 }
                }
            }
        ),
        "two combined rows over a ceiling of one must refuse naming one, not {outcome:?}"
    );
}

#[test]
fn a_combined_answer_at_the_configured_ceiling_is_answered_whole() {
    let ToolOutcome::Answer { rows, .. } = under(2) else {
        panic!("two combined rows at a ceiling of two must be answered");
    };
    assert_eq!(
        rows.rows().len(),
        2,
        "the answer is the whole combined set, never a truncation"
    );
}

#[test]
fn the_default_ceiling_answers_what_it_always_answered() {
    let outcome = outcome_with(&federated_plan(), RowCeilings::DEFAULT);
    assert!(matches!(outcome, ToolOutcome::Answer { .. }), "{outcome:?}");
}

/// The configured ceiling is the bound, not the compiled `MAX_ROWS` under it: a ceiling above the
/// single-source cap answers a combined set the single-source cap would refuse. The one-more row
/// proves the answer is the whole set and the ceiling is not merely "unbounded".
#[test]
fn a_ceiling_above_the_single_source_cap_answers_a_wider_combined_set_in_full() {
    let over_the_cap = usize::try_from(sutura_domain::plan::MAX_ROWS).expect("the cap fits a usize") + 2;
    let fact_rows = RowSet::new(
        federated_fact_rows().columns().to_vec(),
        (0..over_the_cap)
            .map(|at| {
                vec![
                    Value::Text(format!("p{at}")),
                    Value::Text(format!("c{at}")),
                    Value::Text("2026-06".into()),
                    Value::Integer(100),
                ]
            })
            .collect(),
    )
    .expect("a well-formed wide fact result");
    let lookup_rows = RowSet::new(
        federated_lookup_rows().columns().to_vec(),
        (0..over_the_cap)
            .map(|at| vec![Value::Text(format!("c{at}")), Value::Text("north".into())])
            .collect(),
    )
    .expect("a well-formed wide lookup result");
    let ceilings = RowCeilings::new(
        RowCeiling::DEFAULT,
        FederatedRowCeiling::parse(sutura_domain::plan::MAX_ROWS * 2).expect("a ceiling above the cap"),
    );

    let outcome = outcome_over(&federated_plan(), ceilings, fact_rows, lookup_rows);

    let ToolOutcome::Answer { rows, .. } = outcome else {
        panic!("{over_the_cap} combined rows under a ceiling of twice the cap must be answered, not {outcome:?}");
    };
    assert_eq!(rows.rows().len(), over_the_cap, "the answer is the whole combined set");
}

#[test]
fn a_top_over_a_combined_set_past_the_federated_ceiling_is_refused_naming_it() {
    let outcome = outcome_with(&top_one_plan(), ceilings(1));
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    bound: ResultBound::Rows { limit: 1 }
                }
            }
        ),
        "two combined groups over a federated ceiling of one must refuse a top naming one, not {outcome:?}"
    );
}

#[test]
fn a_top_over_a_combined_set_at_the_federated_ceiling_is_ranked_and_answered() {
    let ToolOutcome::Answer { rows, .. } = outcome_with(&top_one_plan(), ceilings(2)) else {
        panic!("two combined groups at a federated ceiling of two must answer a top");
    };
    assert_eq!(rows.rows().len(), 1, "top 1 keeps exactly one ranked row");
}
