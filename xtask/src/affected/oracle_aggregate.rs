//! The `oracle-tier` arm of the real `ci-aggregate` shell, run through `aggregate_shell`'s
//! harness. Its own file because `affected.rs` sits at the 1000-line cap.

use super::aggregate_shell::run_aggregator;
use super::tests::selected;

#[test]
fn a_selected_but_skipped_oracle_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("ORACLE_RESULT", "skipped"),
        ("ORACLE_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected Oracle tier cannot skip: {text}");
    assert!(text.contains("oracle-tier must run"), "the missing leg must be named: {text}");
}

#[test]
fn a_failed_unselected_oracle_tier_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("ORACLE_RESULT", "failure"),
        ("ORACLE_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed Oracle tier cannot be waived: {text}");
    assert!(text.contains("oracle-tier reported"), "the failed leg must be named: {text}");
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

    let root = crate::repo::root().expect("the repo root");
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read .github/workflows/ci.yml");
    let indent = |line: &str| line.chars().take_while(|c| *c == ' ').count();
    let driver_check_if: String = ci
        .lines()
        .skip_while(|line| line.trim_end() != "  bigquery-driver-check:")
        .skip(1)
        .find(|line| line.trim_start().starts_with("if:"))
        .expect("the bigquery-driver-check job has an if: line")
        .to_owned();
    assert!(
        driver_check_if.contains("needs.identity-classify.outputs.driver_check == 'true'"),
        "the driver check must be gated on the combined flag: {driver_check_if}"
    );
    let classify_outputs: String = ci
        .lines()
        .skip_while(|line| line.trim_end() != "  identity-classify:")
        .skip(1)
        .skip_while(|line| line.trim_end() != "    outputs:")
        .skip(1)
        .take_while(|line| line.trim().is_empty() || indent(line) > 4)
        .collect::<Vec<_>>()
        .join("\n");
    let flag = classify_outputs
        .lines()
        .find(|line| line.trim_start().starts_with("driver_check:"))
        .expect("identity-classify publishes the driver check's flag");
    assert!(
        flag.contains("steps.classify.outputs.data_source_postgres == 'true'")
            && flag.contains("steps.classify.outputs.data_source_bigquery == 'true'"),
        "a postgres-only diff must not skip the driver's server cells: {flag}"
    );
    let bq_selected: String = ci
        .lines()
        .find(|line| line.contains("BQ_SELECTED:"))
        .expect("ci-aggregate's BQ_SELECTED env line is present")
        .to_owned();
    assert!(
        bq_selected.contains("needs.ci.outputs.data_source_postgres == 'true'"),
        "the aggregator must see the postgres leg as selected: {bq_selected}"
    );
}
