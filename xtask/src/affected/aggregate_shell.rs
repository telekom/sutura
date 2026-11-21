//! Run the workflow's actual aggregate steps against fixture job results.

fn shell(workflow: &str, job: &str, step: &str) -> String {
    let root = crate::repo::root().expect("repository root");
    let path = root.join(".github/workflows").join(workflow);
    let yaml = std::fs::read_to_string(path).expect("read workflow");
    let step = crate::workflows::step::named_step(&yaml, job, step).expect("aggregate step");
    crate::workflows::step::shell(&step)
        .into_iter()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
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
        ("QA_RESULT", "success"),
        ("RELEASE_PUSH", "false"),
        ("RELEASE_RESULT", "skipped"),
        ("CAUS_RESULT", "skipped"),
        ("CAUS_REQUIRED", "false"),
        ("PGD_RESULT", "skipped"),
        ("PGD_REQUIRED", "false"),
        ("SHIP_RESULT", "skipped"),
        ("SHIP_REQUIRED", "false"),
    ];
    run(
        &shell("ci.yml", "ci-aggregate", "Aggregate the required checks"),
        &defaults,
        envs,
    )
}

pub(super) fn run_qa(envs: &[(&str, &str)]) -> (bool, String) {
    let defaults = [
        ("PLAN_RESULT", "success"),
        ("CORE_RESULT", "skipped"),
        ("CORE_REQUIRED", "false"),
        ("MATRIX_COUNT", "0"),
        ("MATRIX_RESULT", "skipped"),
        ("DEMO_REQUIRED", "false"),
        ("DEMO_RESULT", "skipped"),
        ("CROSS_REQUIRED", "false"),
        ("CROSS_RESULT", "skipped"),
        ("DATAHUB_REQUIRED", "false"),
        ("DATAHUB_RESULT", "skipped"),
        ("BQ_REQUIRED", "false"),
        ("BQ_RESULT", "skipped"),
        ("LIVE_ELIGIBLE", "true"),
    ];
    run(
        &shell("qa.yml", "integration-verdict", "Require every selected suite"),
        &defaults,
        envs,
    )
}

pub(super) fn run_matrix_suite(suite: &str, outcome: &str, selected: bool) -> (bool, String) {
    let root = crate::repo::root().expect("repository root");
    let ids = crate::changes::ci_suites::ids(&root).expect("read suite manifest");
    assert!(ids.iter().any(|id| id == suite), "manifest is missing suite {suite}");
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
