//! The `multi-node-postgres` arm of the real QA aggregate shell, run through `aggregate_shell`'s
//! harness. Its own file because `affected.rs` sits near the 1000-line cap.

use super::aggregate_shell::run_matrix_suite;

#[test]
fn a_selected_but_skipped_multi_node_postgres_is_red() {
    let (ok, text) = run_matrix_suite("multi-node-postgres", "skipped", true);
    assert!(!ok, "a selected PostgreSQL multi-node test cannot skip: {text}");
    assert!(
        text.contains("integration matrix must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_multi_node_postgres_is_red() {
    let (ok, text) = run_matrix_suite("multi-node-postgres", "failure", false);
    assert!(!ok, "a failed PostgreSQL multi-node test cannot be waived: {text}");
    assert!(
        text.contains("integration matrix reported"),
        "the failed leg must be named: {text}"
    );
}
