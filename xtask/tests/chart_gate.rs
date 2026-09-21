//! The chart gate is ONE check reached three ways, and the three have to name the same one.
//!
//! `github.com/telekom/sutura#149`'s remaining half. `checks.helm-chart` (`nix/helm-chart.nix`)
//! already ran in CI and in `just validate`'s loop; what it had no route into was a LOCAL hook, so
//! a chart edit reached the remote unlinted and unrendered. The fix is one derivation with three
//! callers - the `chart` commit hook, `just chart` and the `Chart` step in
//! `.github/workflows/ci.yml` - and the hazard of three callers is the one `nix/lint-workflows.sh`
//! exists for: a hand-written second invocation that drifts, so a finding a developer sees is not
//! the finding CI sees.
//!
//! **So the check name is DERIVED from each caller rather than asserted against a constant three
//! times.** The attribute is read out of `nix/run-gate.sh`'s `chart)` arm and out of the workflow,
//! and the two are compared with each other. A gate that held each against a literal here would go
//! green over a rename in one caller and a matching rename of the literal, which is the same defect
//! one level up.
//!
//! # What this does NOT reach
//!
//! * **Whether the check is any good.** That is `nix/helm-chart.nix`'s own four legs, and its
//!   header states the one measured gap: `helm lint` alone does not catch a broken template,
//!   because it reports a template's `fail()` as an INFO line rather than a lint error. This reads
//!   citations, never a render.
//! * **Whether the hook is INSTALLED.** `prek install` is a property of a checkout, not of the
//!   declared config - the residue `xtask/src/hooks.rs`'s header names and does not close either.
//! * **The `files:` regex's reach.** `xtask` carries no regex engine (`hook_coverage`'s header
//!   gives the reason), so what is held is that the pattern mentions the chart directory. Which
//!   chart PATHS a coverage report can see is the `Helm chart` row in
//!   `xtask/src/hook_coverage/surfaces.rs`, and its own test asserts the globs match.

#![cfg(test)]

use std::path::{Path, PathBuf};

/// The repository root, from this test binary's manifest directory - `xtask/tests/dprint_config.rs`'s
/// idiom, and READ AT RUN TIME for the reason its header gives: `include_str!` resolves against the
/// FILTERED copy in a nix build, and none of the four files below is a Cargo input.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf()
}

fn read(name: &str) -> String {
    std::fs::read_to_string(root().join(name)).unwrap_or_else(|error| panic!("{name} is readable: {error}"))
}

/// The gate argument the hook and the task both hand to `nix/run-gate.sh`.
const GATE: &str = "chart";

/// The invocation both of them are, character for character.
const ENTRY: &str = "bash nix/run-gate.sh chart";

/// One `case` arm of `nix/run-gate.sh`: the lines between `<name>)` and its `;;`.
///
/// The same shape `xtask/src/hook_coverage/abstain.rs` reads that script with, and for the same
/// reason - a `case` is what decides a tiered hook, so the arm is the unit to read.
fn case_arm(script: &str, name: &str) -> String {
    let opener = format!("{name})");
    let mut lines = script.lines().skip_while(|line| line.trim() != opener);
    assert!(lines.next().is_some(), "nix/run-gate.sh declares no `{opener}` arm");
    lines.take_while(|line| line.trim() != ";;").collect::<Vec<&str>>().join("\n")
}

/// The flake check one line builds, if it builds one: the token after `nix_check`, or the
/// `checks.<system>.<name>` attribute path.
fn built_check(line: &str) -> Option<&str> {
    let mut found = None;
    let mut previous: Option<&str> = None;
    for word in line.split_whitespace() {
        if previous == Some("nix_check") {
            found = Some(word);
        } else if let Some((_, name)) = word.split_once("checks.x86_64-linux.") {
            found = Some(name.trim_end_matches('\\'));
        }
        previous = Some(word);
    }
    found
}

/// The body of one `just` recipe: the indented lines under `<name>:`.
fn recipe(justfile: &str, name: &str) -> String {
    let header = format!("{name}:");
    let mut lines = justfile.lines().skip_while(|line| line.trim_end() != header);
    assert!(lines.next().is_some(), "the justfile declares no `{header}` recipe");
    lines
        .take_while(|line| line.starts_with(char::is_whitespace) || line.is_empty())
        .collect::<Vec<&str>>()
        .join("\n")
}

#[test]
fn the_commit_hook_the_task_and_ci_all_build_the_one_chart_check() {
    // 1. THE LOCAL HALF EXISTS AT ALL, which is #149's whole remaining ask. Before this the chart
    //    had a CI step and a `just validate` leg and no hook, so `git commit` over
    //    `charts/sutura/templates/` linted nothing and rendered nothing.
    let hooks = read(".pre-commit-config.yaml");
    assert!(
        hooks.contains(&format!("      - id: {GATE}\n")),
        ".pre-commit-config.yaml declares no `{GATE}` hook"
    );
    assert!(
        hooks.contains(&format!("entry: {ENTRY}")),
        "the `{GATE}` hook's entry is not `{ENTRY}`"
    );

    // 2. IT IS SCOPED TO THE CHART, not `always_run`. A hook with no filter runs on every diff, and
    //    `hook_coverage`'s surface table refuses a row claiming one of those - the claim could
    //    never report a gap. This reads the mention rather than the regex's reach; the module
    //    header says why, and the `Helm chart` row's own test holds the globs.
    let id_line = format!("- id: {GATE}");
    let scoped = hooks
        .lines()
        .skip_while(|line| line.trim() != id_line)
        .skip(1)
        .take_while(|line| !line.trim().starts_with("- id:"))
        .any(|line| line.trim_start().starts_with("files:") && line.contains("charts/"));
    assert!(scoped, "the `{GATE}` hook declares no `files:` naming `charts/`");

    // 3. THE HOOK AND CI BUILD THE SAME ATTRIBUTE, derived from each rather than compared to a
    //    literal: the arm names one flake check, the workflow's `Chart` step names one, and they
    //    are equal or the two halves have drifted. Nothing else in this file decides what runs.
    let script = read("nix/run-gate.sh");
    let arm = case_arm(&script, GATE);
    let local: Vec<&str> = arm.lines().filter_map(built_check).collect();
    assert_eq!(
        local.len(),
        1,
        "the `{GATE})` arm builds {local:?} - it has to build exactly one check"
    );
    let workflow = read(".github/workflows/ci.yml");
    let chart_step = workflow
        .lines()
        .skip_while(|line| line.trim() != "- name: Chart")
        .find_map(built_check);
    assert_eq!(
        chart_step,
        local.first().copied(),
        "the `Chart` step in ci.yml and the `{GATE})` arm of nix/run-gate.sh build different checks"
    );

    // 4. AND THE ARM TAKES NO SECOND ROUTE. A local `helm` would be an unpinned verdict, which is
    //    what `nix/helm-chart.nix` exists to prevent - it prints `helm version --short` from inside
    //    the derivation rather than trusting a caller's PATH.
    for line in arm.lines().filter(|line| !line.trim_start().starts_with('#')) {
        for tool in ["helm", "kubeconform"] {
            assert!(
                !line.split_whitespace().any(|word| word == tool),
                "the `{GATE})` arm invokes `{tool}` directly, which is a second and unpinned verdict: {line}"
            );
        }
    }

    // 5. THE CITABLE TASK IS THE SAME INVOCATION. `AGENTS.md` requires a `just` task rather than a
    //    command line, and `check-guidance` fails a citation of a task that does not exist - so
    //    every page that will cite this cites one recipe, and that recipe is not a third spelling.
    let justfile = read("justfile");
    assert!(
        recipe(&justfile, GATE).lines().any(|line| line.trim() == ENTRY),
        "`just {GATE}` does not run `{ENTRY}`"
    );
}
