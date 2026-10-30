//! The `multi-node` arm of the real `ci-aggregate` shell, run through `aggregate_shell`'s
//! harness. Its own file because `affected.rs` sits near the 1000-line cap.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_selected_but_skipped_multi_node_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("MN_RESULT", "skipped"),
        ("MN_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected multi-node test cannot skip: {text}");
    assert!(text.contains("multi-node must run"), "the missing leg must be named: {text}");
}

#[test]
fn a_failed_unselected_multi_node_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("MN_RESULT", "failure"),
        ("MN_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed multi-node test cannot be waived: {text}");
    assert!(text.contains("multi-node reported"), "the failed leg must be named: {text}");
}
