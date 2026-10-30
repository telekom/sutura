//! The `k3s-autoscaling` arm of the real `ci-aggregate` shell, run through `aggregate_shell`'s
//! harness. Its own file for `multi_node_aggregate.rs`'s reason.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_selected_but_skipped_k3s_autoscaling_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("K3S_RESULT", "skipped"),
        ("K3S_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected k3s autoscaling test cannot skip: {text}");
    assert!(
        text.contains("k3s-autoscaling must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_k3s_autoscaling_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("K3S_RESULT", "failure"),
        ("K3S_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed k3s autoscaling test cannot be waived: {text}");
    assert!(
        text.contains("k3s-autoscaling reported"),
        "the failed leg must be named: {text}"
    );
}
