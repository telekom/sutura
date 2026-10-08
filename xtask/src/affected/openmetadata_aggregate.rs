//! The `ci-openmetadata-tier` arm of the real `ci-aggregate` shell, run through
//! `aggregate_shell`'s harness; its own file because `affected.rs` sits at the 1000-line cap.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_selected_but_skipped_openmetadata_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("OM_RESULT", "skipped"),
        ("OM_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected OpenMetadata tier cannot skip: {text}");
    assert!(
        text.contains("ci-openmetadata-tier must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_openmetadata_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("OM_RESULT", "failure"),
        ("OM_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed OpenMetadata tier cannot be waived: {text}");
    assert!(
        text.contains("ci-openmetadata-tier reported"),
        "the failed leg must be named: {text}"
    );
}

#[test]
fn a_clean_openmetadata_tier_runs_green() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("OM_RESULT", "success"),
        ("OM_SELECTED", "true"),
    ]);
    assert!(ok, "a selected OpenMetadata tier that runs must aggregate GREEN: {text}");
    assert!(
        !text.contains("ci-openmetadata-tier"),
        "a green run names no OpenMetadata failure: {text}"
    );
}
