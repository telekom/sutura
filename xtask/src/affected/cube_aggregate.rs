//! The `ci-cube-tier` arm of the real `ci-aggregate` shell, run through `aggregate_shell`'s harness;
//! its own file for `openmetadata_aggregate.rs`'s reason.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_selected_but_skipped_cube_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("CUBE_RESULT", "skipped"),
        ("CUBE_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected Cube tier cannot skip: {text}");
    assert!(
        text.contains("ci-cube-tier must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_cube_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("CUBE_RESULT", "failure"),
        ("CUBE_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed Cube tier cannot be waived: {text}");
    assert!(text.contains("ci-cube-tier reported"), "the failed leg must be named: {text}");
}
