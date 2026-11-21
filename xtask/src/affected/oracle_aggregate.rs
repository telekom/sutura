//! The Oracle arms of the real `ci-aggregate` shell, run through `aggregate_shell`'s
//! harness. Its own file because `affected.rs` sits at the 1000-line cap.

use super::aggregate_shell::run_aggregator;
use super::tests::selected;

#[test]
fn a_selected_but_skipped_oracle_fleet_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("MNORA_RESULT", "skipped"),
        ("MNORA_SELECTED", "true"),
    ]);
    assert!(!ok, "a selected Oracle fleet test cannot skip: {text}");
    assert!(
        text.contains("multi-node-oracle must run"),
        "the missing leg must be named: {text}"
    );
}

#[test]
fn a_failed_unselected_oracle_fleet_is_red() {
    let (ok, text) = run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        ("MNORA_RESULT", "failure"),
        ("MNORA_SELECTED", "false"),
    ]);
    assert!(!ok, "a failed Oracle fleet test cannot be waived: {text}");
    assert!(
        text.contains("multi-node-oracle reported"),
        "the failed leg must be named: {text}"
    );
}

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

/// Queue reuse consumes successful job names after the unchanged API filters; exercise its actual
/// result loop so one Oracle matrix success cannot stand in for both venues.
#[test]
fn oracle_queue_reuse_requires_both_matrix_lanes() {
    let root = crate::repo::root().expect("repository root");
    let workflow = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("workflow");
    let step = crate::workflows::step::named_step(&workflow, "queue-validated", "Legs the merge queue ran green on this commit")
        .expect("queue lookup step");
    let body = crate::workflows::step::shell(&step)
        .into_iter()
        .skip_while(|line| !line.trim_start().starts_with("for leg in "))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    for (names, expected) in [
        ("oracle-tier (acceptance)\noracle-tier (native)", true),
        ("oracle-tier (acceptance)", false),
        ("oracle-tier (native)", false),
        ("", false),
        ("oracle-tier", false),
        ("oracle-tier (acceptance)\noracle-tier (native-extra)", false),
    ] {
        let output = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-euo", "pipefail", "-c", &body])
            .env_remove("BASH_ENV")
            .env("passed", names)
            .env("GITHUB_OUTPUT", "/dev/stdout")
            .output()
            .expect("queue result loop executes");
        let printed = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{names:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            printed.lines().any(|line| line == format!("oracle_tier={expected}")),
            "{names:?}: {printed}"
        );
    }
}
