//! The `k3s-autoscaling` arm of the real QA aggregate shell, run through `aggregate_shell`'s
//! harness. Its own file for `multi_node_aggregate.rs`'s reason.

use super::aggregate_shell::run_matrix_suite;

#[test]
fn a_selected_but_skipped_k3s_autoscaling_is_red() {
    let (ok, text) = run_matrix_suite("k3s-autoscaling", "skipped", true);
    assert!(!ok, "a selected k3s autoscaling test cannot skip: {text}");
    assert!(
        text.contains("integration matrix must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_k3s_autoscaling_is_red() {
    let (ok, text) = run_matrix_suite("k3s-autoscaling", "failure", false);
    assert!(!ok, "a failed k3s autoscaling test cannot be waived: {text}");
    assert!(
        text.contains("integration matrix reported"),
        "the failed leg must be named: {text}"
    );
}
