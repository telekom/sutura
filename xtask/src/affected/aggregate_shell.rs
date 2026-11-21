//! Run the workflow's actual aggregate steps against fixture job results.

fn shell(workflow: &str, job: &str, names: &[&str]) -> Option<String> {
    let root = crate::repo::root()?;
    let path = root.join(".github/workflows").join(workflow);
    let yaml = std::fs::read_to_string(path).ok()?;
    let step = names
        .iter()
        .find_map(|name| crate::workflows::step::named_step(&yaml, job, name))?;
    let mut lines = crate::workflows::step::shell(&step).into_iter();
    lines.next()?;
    Some(lines.collect::<Vec<_>>().join("\n"))
}

pub(super) fn suite_manifest() -> Vec<serde_json::Value> {
    let root = crate::repo::root().expect("repository root");
    let manifest = root.join("devco/ci-suites.json");
    match std::fs::read_to_string(manifest) {
        Ok(source) => serde_json::from_str(&source).expect("parse suite manifest"),
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(cause) => panic!("read suite manifest: {cause}"),
    }
}

fn run(script: &str, defaults: &[(&str, &str)], envs: &[(&str, &str)]) -> (bool, String) {
    let out = std::process::Command::new("bash")
        .args(["--noprofile", "--norc", "-c", script])
        .env_remove("BASH_ENV")
        .envs(defaults.iter().copied())
        .envs(envs.iter().copied())
        .output()
        .expect("bash runs the aggregate shell");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

pub(super) fn run_aggregator(envs: &[(&str, &str)]) -> (bool, String) {
    let defaults = [
        ("CI_RESULT", "success"),
        ("FAST_RESULT", "success"),
        ("PLAN_RESULT", "success"),
        ("QA_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", "false"),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", "false"),
        ("E2E_RESULT", "skipped"),
        ("E2E_REQUIRED", "false"),
        ("ORACLE_RESULT", "skipped"),
        ("ORACLE_SELECTED", "false"),
        ("MN_RESULT", "skipped"),
        ("MN_SELECTED", "false"),
        ("MNPG_RESULT", "skipped"),
        ("MNPG_SELECTED", "false"),
        ("MNORA_RESULT", "skipped"),
        ("MNORA_SELECTED", "false"),
        ("K3S_RESULT", "skipped"),
        ("K3S_SELECTED", "false"),
        ("DH_RESULT", "skipped"),
        ("DH_SELECTED", "false"),
        ("OM_RESULT", "skipped"),
        ("OM_SELECTED", "false"),
        ("CUBE_RESULT", "skipped"),
        ("CUBE_SELECTED", "false"),
        ("BQC_RESULT", "skipped"),
        ("BQC_REQUIRED", "false"),
        ("CROSS_RESULT", "skipped"),
        ("BQ_REQUIRED", "false"),
        ("DATAHUB_RESULT", "skipped"),
        ("DATAHUB_REQUIRED", "false"),
        ("RELEASE_PUSH", "false"),
        ("RELEASE_RESULT", "success"),
        ("CAUS_RESULT", "skipped"),
        ("CAUS_REQUIRED", "false"),
        ("PGD_RESULT", "skipped"),
        ("PGD_REQUIRED", "false"),
        ("SHIP_RESULT", "skipped"),
        ("SHIP_REQUIRED", "false"),
    ];
    let script = shell(
        "ci.yml",
        "ci-aggregate",
        &["Aggregate the required checks", "Aggregate the category-gated legs"],
    )
    .expect("CI aggregate shell");
    run(&script, &defaults, envs)
}

pub(super) fn run_qa(envs: &[(&str, &str)]) -> (bool, String) {
    let defaults = [
        ("PLAN_PRESENT", "true"),
        ("CORE_RESULT", "skipped"),
        ("CORE_REQUIRED", "false"),
        ("MATRIX_COUNT", "0"),
        ("MATRIX_RESULT", "skipped"),
        ("DEMO_REQUIRED", "false"),
        ("DEMO_RESULT", "skipped"),
        ("CROSS_REQUIRED", "false"),
        ("CROSS_RESULT", "skipped"),
    ];
    let Some(script) = shell("qa.yml", "integration-verdict", &["Require every selected suite"]) else {
        return (false, String::from("QA integration verdict is absent"));
    };
    run(&script, &defaults, envs)
}

pub(super) fn run_matrix_suite(suite: &str, outcome: &str, selected: bool) -> (bool, String) {
    if !suite_manifest().iter().any(|entry| entry["id"] == suite) {
        return (false, format!("QA manifest has no suite {suite}"));
    }
    run_qa(&[("MATRIX_COUNT", if selected { "1" } else { "0" }), ("MATRIX_RESULT", outcome)])
}

#[test]
fn a_failed_fast_check_keeps_the_aggregate_red_when_the_core_job_skips() {
    let (ok, text) = run_aggregator(&[("CI_RESULT", "skipped"), ("FAST_RESULT", "failure")]);
    assert!(!ok && text.contains("fast checks reported 'failure'"), "{text}");
}

#[test]
fn required_smoke_and_full_core_graph_cannot_skip() {
    for (envs, message) in [
        (&[("DEMO_REQUIRED", "true")][..], "demo-container must run"),
        (&[("MATRIX_COUNT", "1")][..], "integration matrix must run"),
        (&[("CORE_REQUIRED", "true")][..], "core graph must run"),
    ] {
        let (passed, output) = run_qa(envs);
        assert!(!passed && output.contains(message), "{output}");
    }
    let (passed, output) = run_qa(&[]);
    assert!(passed, "ordinary PR QA can finish without advisory smoke results: {output}");
}
