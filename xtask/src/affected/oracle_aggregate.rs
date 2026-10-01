//! The `oracle-tier` arm of the real `ci-aggregate` shell, run through `tests::shell_simulation`'s
//! harness. Its own file because `affected.rs` sits at the 1000-line cap.

use super::tests::shell_simulation::run_aggregator;

#[test]
fn a_selected_but_skipped_oracle_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("ORACLE_RESULT", "skipped"),
        ("ORACLE_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected Oracle tier cannot skip: {text}");
    assert!(text.contains("oracle-tier must run"), "the missing leg must be named: {text}");
}

#[test]
fn a_failed_unselected_oracle_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("ORACLE_RESULT", "failure"),
        ("ORACLE_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed Oracle tier cannot be waived: {text}");
    assert!(text.contains("oracle-tier reported"), "the failed leg must be named: {text}");
}
