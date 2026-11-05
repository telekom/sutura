//! The `multi-node-postgres` arm of the real `ci-aggregate` shell, run through `aggregate_shell`'s
//! harness. Its own file because `affected.rs` sits near the 1000-line cap.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_selected_but_skipped_multi_node_postgres_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("MNPG_RESULT", "skipped"),
        ("MNPG_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected PostgreSQL multi-node test cannot skip: {text}");
    assert!(
        text.contains("multi-node-postgres must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_multi_node_postgres_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("MNPG_RESULT", "failure"),
        ("MNPG_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed PostgreSQL multi-node test cannot be waived: {text}");
    assert!(
        text.contains("multi-node-postgres reported"),
        "the failed leg must be named: {text}"
    );
}
