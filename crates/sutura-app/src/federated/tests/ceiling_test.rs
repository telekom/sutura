//! The federated answer's row ceiling is a configured value, and one row over it is a refusal that
//! names it - `github.com/telekom/sutura#828`. `federated_plan()`'s combined answer has two groups,
//! so a ceiling of one is crossed and a ceiling of two is reached without being crossed.

use super::top_test::outcome_with;
use super::*;
use sutura_domain::plan::{FederatedRowCeiling, RowCeiling, RowCeilings};

fn under(rows: u32) -> ToolOutcome {
    outcome_with(
        &federated_plan(),
        RowCeilings::new(
            RowCeiling::DEFAULT,
            FederatedRowCeiling::parse(rows).expect("a test ceiling is a ceiling"),
        ),
    )
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
