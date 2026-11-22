//! The Oracle arms of the real QA aggregate shell, run through `aggregate_shell`'s
//! harness. Its own file because `affected.rs` sits at the 1000-line cap.

use super::aggregate_shell::{run_matrix_suite, suite_manifest};
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

    let suites = suite_manifest();
    for category in ["data_source_postgres", "data_source_bigquery"] {
        assert!(
            suites.iter().any(|suite| suite["id"] == "bigquery-driver"
                && suite["categories"]
                    .as_array()
                    .is_some_and(|categories| { categories.iter().any(|entry| entry == category) })),
            "{category} must select the linked driver suite"
        );
    }
}

/// Every Oracle receipt names a matrix entry that runs its own venue.
#[test]
fn oracle_queue_reuse_requires_both_matrix_lanes() {
    let root = crate::repo::root().expect("repository root");
    let suites = suite_manifest();
    let probe = r#"
        nix() { printf '%s %s\n' "$SUTURA_ORACLE_ACCEPTANCE_SHARD" "$*"; }
        source nix/ci-suite.sh "$1"
    "#;
    for (suite, invocation) in [
        ("oracle-tier", "none run .#oracle-acceptance -- --no-capture\n"),
        ("oracle-native", "native run .#oracle-native-acceptance\n"),
        ("oracle-legacy", "legacy run .#oracle-native-acceptance\n"),
    ] {
        assert!(
            suites
                .iter()
                .any(|entry| entry["id"] == suite && entry["venue"] == "standard"),
            "{suite} must have its own matrix entry"
        );
        let output = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", probe, "oracle-qa", suite])
            .current_dir(&root)
            .env_remove("BASH_ENV")
            .env("SUTURA_ORACLE_ACCEPTANCE_SHARD", "none")
            .output()
            .expect("probe the Oracle QA suite runner");
        assert!(
            output.status.success(),
            "{suite}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), invocation, "{suite}");
    }
}
