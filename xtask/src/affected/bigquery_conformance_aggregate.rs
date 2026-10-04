//! The `bigquery-conformance` arm of the real `ci-aggregate` shell, run through
//! `aggregate_shell`'s harness. Its own file because `affected.rs` sits near the 1000-line cap.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_required_but_skipped_bigquery_conformance_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("BQC_RESULT", "skipped"),
        ("BQC_REQUIRED", "true"),
    ]);
    assert!(!ok, "a required live bigquery row cannot skip: {text}");
    assert!(
        text.contains("bigquery-conformance must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unrequired_bigquery_conformance_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("BQC_RESULT", "failure"),
        ("BQC_REQUIRED", "false"),
    ]);
    assert!(!ok, "a failed live bigquery row cannot be waived: {text}");
    assert!(
        text.contains("bigquery-conformance reported"),
        "the failed leg must be named: {text}"
    );
}
