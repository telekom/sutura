#![forbid(unsafe_code)]
//! The commit stage's tier policy and the CRAP and fuzz rows, driven through the real binary.
//!
//! `check-hook-tiers` refuses a commit hook that runs the suite, the doctests, the CRAP score or the
//! fuzz replay, and `hook-coverage --surface-tasks` names `crap` for a diff that reaches the scored
//! crate and `fuzz-smoke` for one that touches the fuzzed tree, which is the only thing that makes
//! `just ship-check` run them. These rules live in files that also hold their own unit tests, which
//! the causality gate cannot run against base; a separate target can be.

#![cfg(test)]

#[path = "scratch_tree/mod.rs"]
#[expect(
    dead_code,
    reason = "the module's own tests use `seal` and `Fixture`; this integration test does not"
)]
#[cfg(test)]
mod scratch_tree;

use std::path::Path;
use std::process::{Command, Output};

/// One fixture file, as `scratch_tree::Tree::of` takes it.
type File<'a> = (&'a str, &'a [u8]);

const NOTHING: &[u8] = b"";
const WORKSPACE: &[u8] = b"[workspace]\n";
const BEFORE: &[u8] = b"fn before() {}\n";

/// A config that passes every other rule, with one more hook at `stage` running `entry`.
fn config(entry: &str, stage: &str) -> String {
    [
        "default_install_hook_types: [pre-commit, pre-push, commit-msg]",
        "default_stages: [pre-commit]",
        "repos:",
        "  - repo: local",
        "    hooks:",
        "      - id: secret-sweep",
        "        name: secret-sweep",
        "        entry: bash nix/run-gate.sh secrets",
        "        stages: [pre-push]",
        "      - id: cargo-deny",
        "        name: cargo-deny",
        "        entry: bash nix/run-gate.sh supply-chain",
        "        stages: [pre-push]",
        "      - id: probe",
        "        name: probe",
        &format!("        entry: {entry}"),
        &format!("        stages: [{stage}]"),
        "",
    ]
    .join("\n")
}

fn tree(case: &str, config: &str, extra: &[File<'_>]) -> scratch_tree::Tree {
    let mut files: Vec<File<'_>> = vec![
        ("flake.nix", NOTHING),
        ("Cargo.toml", WORKSPACE),
        (".pre-commit-config.yaml", config.as_bytes()),
    ];
    files.extend_from_slice(extra);
    scratch_tree::Tree::of(case, &files)
}

fn xtask(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .expect("execute the real xtask binary")
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed in {}", root.display());
}

#[test]
fn a_commit_stage_that_runs_the_suite_crap_or_the_fuzz_replay_is_refused() {
    let entries = [
        "bash -c 'exec devenv shell bash nix/run-gate.sh tests'",
        "bash -c 'exec cargo test --doc --workspace --all-features'",
        "bash nix/run-gate.sh crap",
        "bash nix/run-fuzz.sh smoke",
    ];
    for (index, entry) in entries.iter().enumerate() {
        let commit = tree(&format!("hook-tiers-commit-{index}"), &config(entry, "pre-commit"), &[]);
        let refused = xtask(commit.root(), &["check-hook-tiers"]);
        let stderr = String::from_utf8(refused.stderr.clone()).expect("gate diagnostics");
        assert_eq!(refused.status.code(), Some(1), "{entry}: {stderr}");
        assert!(
            stderr.contains("the pre-commit stage runs the suite, CRAP or the fuzz replay") && stderr.contains("`probe`"),
            "the refusal must say what ran and which hook: {stderr}"
        );
        // The same hook declared on no commit stage is nobody's commit cost.
        let manual = tree(&format!("hook-tiers-manual-{index}"), &config(entry, "manual"), &[]);
        let allowed = xtask(manual.root(), &["check-hook-tiers"]);
        let stderr = String::from_utf8(allowed.stderr.clone()).expect("gate diagnostics");
        assert_eq!(allowed.status.code(), Some(0), "{entry}: {stderr}");
    }
}

/// The tasks `ship-check` would run for a diff that edits `path`, which `--surface-tasks` prints.
fn surface_tasks(case: &str, path: &str) -> Vec<String> {
    let fixture = tree(case, &config("echo ok", "pre-commit"), &[(path, BEFORE)]);
    let root = fixture.root();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    std::fs::write(root.join(path), b"fn after() {}\n").expect("edit the fixture file");
    let output = xtask(root, &["hook-coverage", "--since", "HEAD", "--surface-tasks"]);
    let stderr = String::from_utf8(output.stderr.clone()).expect("gate diagnostics");
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    String::from_utf8(output.stdout)
        .expect("task names")
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn a_diff_reaching_the_scored_crate_surfaces_the_crap_task_and_one_elsewhere_does_not() {
    let scored = surface_tasks("hook-surface-scored", "crates/sutura-domain/src/lib.rs");
    assert!(scored.iter().any(|task| task == "crap"), "the scored crate: {scored:?}");
    // A crate outside the scored scope pays for no coverage build; the row is not a blanket.
    let elsewhere = surface_tasks("hook-surface-elsewhere", "crates/sutura-http/src/lib.rs");
    assert!(!elsewhere.iter().any(|task| task == "crap"), "another crate: {elsewhere:?}");
}

#[test]
fn a_diff_reaching_the_fuzzed_tree_surfaces_the_replay_and_one_outside_it_skips_it() {
    let inside = surface_tasks("hook-surface-fuzzed", "crates/sutura-sql/src/lib.rs");
    assert!(inside.iter().any(|task| task == "fuzz-smoke"), "a fuzzed crate: {inside:?}");
    let harness = surface_tasks("hook-surface-harness", "fuzz/fuzz_targets/probe.rs");
    assert!(
        harness.iter().any(|task| task == "fuzz-smoke"),
        "the harness tree: {harness:?}"
    );
    // The replay is git-delta: a crate no target imports pays for no fuzz build.
    let outside = surface_tasks("hook-surface-unfuzzed", "crates/sutura-cli/src/lib.rs");
    assert!(
        !outside.iter().any(|task| task == "fuzz-smoke"),
        "an unfuzzed crate: {outside:?}"
    );
}
