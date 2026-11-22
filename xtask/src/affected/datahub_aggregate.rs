//! The `ci-datahub-tier` arm of the real QA aggregate shell, run through
//! `aggregate_shell`'s harness. Its own file because `affected.rs` sits at the 1000-line
//! cap - the same split `oracle_aggregate.rs` took for its leg.

use super::aggregate_shell::run_matrix_suite;

#[test]
fn a_selected_but_skipped_datahub_tier_is_red() {
    let (ok, text) = run_matrix_suite("datahub-tier", "skipped", true);
    assert!(!ok, "a selected DataHub tier cannot skip: {text}");
    assert!(
        text.contains("integration matrix must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_datahub_tier_is_red() {
    let (ok, text) = run_matrix_suite("datahub-tier", "failure", false);
    assert!(!ok, "a failed DataHub tier cannot be waived: {text}");
    assert!(
        text.contains("integration matrix reported"),
        "the failed leg must be named: {text}"
    );
}

#[test]
fn a_clean_datahub_tier_runs_green() {
    let (ok, text) = run_matrix_suite("datahub-tier", "success", true);
    assert!(ok, "a selected DataHub tier that runs must aggregate GREEN: {text}");
}
