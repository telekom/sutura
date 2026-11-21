//! The Oracle arms of the real QA aggregate shell, run through `aggregate_shell`'s
//! harness. Its own file because `affected.rs` sits at the 1000-line cap.

use super::aggregate_shell::run_matrix_suite;
use super::tests::selected;

#[test]
fn a_selected_but_skipped_oracle_fleet_is_red() {
    let (ok, text) = run_matrix_suite("multi-node-oracle", "skipped", true);
    assert!(!ok, "a selected Oracle fleet test cannot skip: {text}");
    assert!(
        text.contains("integration matrix must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_oracle_fleet_is_red() {
    let (ok, text) = run_matrix_suite("multi-node-oracle", "failure", false);
    assert!(!ok, "a failed Oracle fleet test cannot be waived: {text}");
    assert!(
        text.contains("integration matrix reported"),
        "the failed leg must be named: {text}"
    );
}

#[test]
fn a_selected_but_skipped_oracle_tier_is_red() {
    let (ok, text) = run_matrix_suite("oracle-tier", "skipped", true);
    assert!(!ok, "a selected Oracle tier cannot skip: {text}");
    assert!(
        text.contains("integration matrix must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_oracle_tier_is_red() {
    let (ok, text) = run_matrix_suite("oracle-tier", "failure", false);
    assert!(!ok, "a failed Oracle tier cannot be waived: {text}");
    assert!(
        text.contains("integration matrix reported"),
        "the failed leg must be named: {text}"
    );
}

/// A change confined to the linked PostgreSQL driver must still run
/// `bigquery-driver-check`: it is the only venue that runs that driver's libpq and its two
/// sign-in cells against their tier, so before `data_source_postgres` gated it a postgres-only
/// diff skipped them and only `conninfo.rs`'s string cells saw it.
#[test]
fn a_postgres_only_change_runs_the_leg_that_runs_its_server_cells() {
    let cats = selected(&[
        "crates/sutura-exec-postgres/src/adbc/conninfo.rs",
        "crates/sutura-exec-postgres/tests/kerberos.rs",
    ]);
    assert!(
        cats.needs("data_source_postgres"),
        "a postgres driver change selects its adapter: {cats:?}"
    );

    let root = crate::repo::root().expect("repository root");
    assert!(crate::changes::ci_suites::matches_category(
        &root,
        "bigquery-driver",
        "data_source_postgres"
    ));
    assert!(crate::changes::ci_suites::matches_category(
        &root,
        "bigquery-driver",
        "data_source_bigquery"
    ));
}

/// The acceptance verdict does not stand in for either of the native shards.
#[test]
fn oracle_queue_reuse_requires_both_matrix_lanes() {
    use crate::changes::ci_suites::passed_suites;

    let root = crate::repo::root().expect("repository root");
    let acceptance = "qa / suite (oracle-tier)";
    let native = "qa / suite (oracle-native)";
    let legacy = "qa / suite (oracle-legacy)";
    for (green, expected) in [
        (vec![acceptance], vec!["oracle-tier"]),
        (vec![acceptance, native], vec!["oracle-tier", "oracle-native"]),
        (vec![native, legacy], vec!["oracle-native", "oracle-legacy"]),
        (
            vec![acceptance, native, legacy],
            vec!["oracle-tier", "oracle-native", "oracle-legacy"],
        ),
        (vec!["qa / suite (oracle-native-extra)"], vec![]),
    ] {
        assert_eq!(passed_suites(&root, &green), expected);
    }
}
