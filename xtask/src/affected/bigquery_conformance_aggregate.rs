//! The `bigquery-conformance` arm of the real CI aggregate shell, run through
//! `aggregate_shell`'s harness. Its own file because `affected.rs` sits near the 1000-line cap.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_required_but_skipped_bigquery_conformance_is_red() {
    let (ok, text) = run_aggregator(&[("BQ_RESULT", "skipped"), ("BQ_REQUIRED", "true")]);
    assert!(!ok, "a required live bigquery row cannot skip: {text}");
    assert!(
        text.contains("bigquery-conformance must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unrequired_bigquery_conformance_is_red() {
    let (ok, text) = run_aggregator(&[("BQ_RESULT", "failure"), ("BQ_REQUIRED", "false")]);
    assert!(!ok, "a failed live bigquery row cannot be waived: {text}");
    assert!(
        text.contains("bigquery-conformance reported"),
        "the failed leg must be named: {text}"
    );
}
