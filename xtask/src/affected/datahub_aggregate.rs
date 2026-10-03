//! The `ci-datahub-tier` arm of the real `ci-aggregate` shell, run through
//! `aggregate_shell`'s harness. Its own file because `affected.rs` sits at the 1000-line
//! cap - the same split `oracle_aggregate.rs` took for its leg.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_selected_but_skipped_datahub_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("ORACLE_RESULT", "skipped"),
        ("ORACLE_SELECTED", "false"),
        ("DH_RESULT", "skipped"),
        ("DH_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected DataHub tier cannot skip: {text}");
    assert!(
        text.contains("ci-datahub-tier must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_datahub_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("ORACLE_RESULT", "skipped"),
        ("ORACLE_SELECTED", "false"),
        ("DH_RESULT", "failure"),
        ("DH_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed DataHub tier cannot be waived: {text}");
    assert!(
        text.contains("ci-datahub-tier reported"),
        "the failed leg must be named: {text}"
    );
}

#[test]
fn a_clean_datahub_tier_runs_green() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("ORACLE_RESULT", "skipped"),
        ("ORACLE_SELECTED", "false"),
        ("DH_RESULT", "success"),
        ("DH_SELECTED", "true"),
    ]);
    assert!(ok, "a selected DataHub tier that runs must aggregate GREEN: {text}");
}
