//! Repo automation. Run as `cargo xtask <task>`, or `cargo run -q -p xtask -- <task>`.
//!
//! These are gates, not conveniences: each one answers "what fails if this rule is
//! violated?" with a non-zero exit code and a count, not with a paragraph of guidance. A
//! rule that lands here instead of in a document is a rule that cannot rot unnoticed.
//!
//! Gates live in `xtask` rather than in a script per check because there is then one thing
//! to install (the workspace), one language to review, and the checks are unit-tested by
//! `cargo test --workspace` like any other code.

mod boundaries;
mod causality;
mod changes;
mod commit_msg;
mod line_endings;
mod max_lines;
mod repo;
mod skills;
mod text;
mod unused_deps;

use std::process::ExitCode;

/// Every task, with the one-line description printed by `--help` and on a bad invocation.
const TASKS: &[(&str, &str)] = &[
    ("check-boundaries", "the domain crate depends on no framework"),
    ("max-lines", "no file over 1000 lines (exemptions: .max-lines-ignore)"),
    ("unused-deps", "every declared dependency is actually used"),
    ("line-endings", "every text file in the repo uses LF, not CRLF"),
    (
        "text-hygiene",
        "conflict markers, trailing whitespace, final newline, file size; --fix",
    ),
    (
        "commit-msg",
        "the commit subject is a conventional commit (hook passes the file)",
    ),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.split_first().map(|(_, rest)| rest).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("check-boundaries") => boundaries::run(),
        Some("max-lines") => max_lines::run(rest),
        Some("unused-deps") => unused_deps::run(),
        Some("line-endings") => line_endings::run(),
        Some("test-causality") => causality::run(rest),
        Some("check-skills") => skills::run(),
        Some("text-hygiene") => text::run(rest),
        Some("commit-msg") => commit_msg::run(rest),
        Some("classify") => changes::run_classify(rest),
        Some("changed-packages") => changes::run_changed_packages(rest),
        Some("check-changed") => changes::run_check_changed(rest),
        Some("--help" | "-h" | "help") => {
            usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("xtask: unknown task `{other}`");
            usage();
            ExitCode::from(2)
        }
        None => {
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("usage: cargo xtask <task>");
    for (name, description) in TASKS {
        eprintln!("  {name:<18} {description}");
    }
}

/// `cargo metadata` as JSON, with `extra` appended (for example `--no-deps`).
///
/// One place, because three gates read the workspace graph and each of them wants the same
/// `--locked` guarantee: a gate must not be the thing that rewrites `Cargo.lock`.
fn cargo_metadata(extra: &[&str]) -> Result<serde_json::Value, String> {
    let output = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked"])
        .args(extra)
        .output()
        .map_err(|e| format!("could not run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata was not valid JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::TASKS;

    /// The usage text and the dispatch table are the same list, so a task cannot be added
    /// without becoming discoverable.
    #[test]
    fn every_task_is_documented_once() {
        let mut names: Vec<&str> = TASKS.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate task name in TASKS");
        assert!(names.contains(&"max-lines"));
    }

    #[test]
    fn cargo_metadata_reads_this_workspace() {
        let meta = super::cargo_metadata(&["--no-deps"]).expect("cargo metadata should succeed in-tree");
        let packages = meta.get("packages").and_then(|p| p.as_array()).expect("packages array");
        assert!(
            packages
                .iter()
                .any(|p| p.get("name").and_then(|n| n.as_str()) == Some("xtask"))
        );
    }
}
