//! The `ci-cube-tier` arm of the real QA aggregate shell, run through `aggregate_shell`'s harness;
//! its own file for `openmetadata_aggregate.rs`'s reason.

use super::aggregate_shell::run_matrix_suite;

#[test]
fn a_selected_but_skipped_cube_tier_is_red() {
    let (ok, text) = run_matrix_suite("ci-cube-tier", "skipped", true);
    assert!(!ok, "a selected Cube tier cannot skip: {text}");
    assert!(
        text.contains("integration matrix must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_cube_tier_is_red() {
    let (ok, text) = run_matrix_suite("ci-cube-tier", "failure", false);
    assert!(!ok, "a failed Cube tier cannot be waived: {text}");
    assert!(
        text.contains("integration matrix reported"),
        "the failed leg must be named: {text}"
    );
}
