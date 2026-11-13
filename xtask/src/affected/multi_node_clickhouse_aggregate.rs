//! The ClickHouse fleet arm of the real aggregate shell, through its existing harness.

use super::aggregate_shell::run_aggregator;

#[test]
fn a_selected_but_skipped_multi_node_clickhouse_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("MNCH_RESULT", "skipped"),
        ("MNCH_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected ClickHouse fleet test cannot skip: {text}");
    assert!(
        text.contains("multi-node-clickhouse must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_multi_node_clickhouse_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("MNCH_RESULT", "failure"),
        ("MNCH_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed ClickHouse fleet test cannot be waived: {text}");
    assert!(
        text.contains("multi-node-clickhouse reported"),
        "the failed leg must be named: {text}"
    );
}
