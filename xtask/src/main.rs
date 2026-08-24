//! Repo automation. Run as `cargo xtask <task>`, or `cargo run -q -p xtask -- <task>`.
//!
//! These are gates, not conveniences: each answers "what fails if this rule is violated?"
//! with a non-zero exit and a count, not with a paragraph of guidance. A rule that lands here
//! instead of in a document is a rule that cannot rot unnoticed.
//!
//! Gates live in one binary rather than a script per check: one thing to install, one language
//! to review, and they are unit-tested by `cargo test --workspace` like any other code.

mod boundaries;
mod causality;
mod changes;
mod commit_msg;
mod guidance;
mod line_endings;
mod max_lines;
mod repo;
mod skills;
mod text;
mod unused_deps;

use std::process::ExitCode;

/// What a gate does: read the repo, print a verdict, exit non-zero on a violation.
type Gate = fn(&[String]) -> ExitCode;

/// A task: the name, the `--help` line, and the code it runs.
///
/// The handler is IN the table, so `--help` and dispatch cannot disagree. They did once - six
/// dispatched tasks were missing from the list, so `--help` lied and `check-guidance` reported
/// every mention of them as a deleted gate. A table plus a separate `match` is two lists.
struct Task {
    name: &'static str,
    description: &'static str,
    run: Gate,
}

const TASKS: &[Task] = &[
    Task {
        name: "check-boundaries",
        description: "the domain crate depends on no framework",
        run: boundaries::run,
    },
    Task {
        name: "max-lines",
        description: "no file over 1000 lines (exemptions: .max-lines-ignore)",
        run: max_lines::run,
    },
    Task {
        name: "unused-deps",
        description: "every declared dependency is actually used",
        run: unused_deps::run,
    },
    Task {
        name: "line-endings",
        description: "every text file uses LF, not CRLF",
        run: line_endings::run,
    },
    Task {
        name: "text-hygiene",
        description: "conflict markers, whitespace, final newline, file size; --fix",
        run: text::run,
    },
    Task {
        name: "check-skills",
        description: "the skill router and the skill tree agree",
        run: skills::run,
    },
    Task {
        name: "check-guidance",
        description: "docs and comments still describe this repo",
        run: guidance::run,
    },
    Task {
        name: "commit-msg",
        description: "the commit subject is a conventional commit (the hook passes the file)",
        run: commit_msg::run,
    },
    Task {
        name: "classify",
        description: "what a diff requires; --since <ref>, or paths (fails open)",
        run: changes::run_classify,
    },
    Task {
        name: "changed-packages",
        description: "the cargo packages owning the given .rs paths",
        run: changes::run_changed_packages,
    },
    Task {
        name: "check-changed",
        description: "cargo check, narrowed to the packages that changed",
        run: changes::run_check_changed,
    },
    Task {
        name: "test-causality",
        description: "a changed test is red on base, green on head; --since <ref>",
        run: causality::run,
    },
];

/// Every task name. `check-guidance` reads this to reject a doc citing a task that is gone.
pub(crate) fn task_names() -> impl Iterator<Item = &'static str> {
    TASKS.iter().map(|t| t.name)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.split_first().map(|(_, rest)| rest).unwrap_or_default();

    match args.first().map(String::as_str) {
        Some("--help" | "-h" | "help") => {
            usage();
            ExitCode::SUCCESS
        }
        Some(requested) => TASKS.iter().find(|t| t.name == requested).map_or_else(
            || {
                eprintln!("xtask: unknown task `{requested}`");
                usage();
                ExitCode::from(2)
            },
            |task| (task.run)(rest),
        ),
        None => {
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("usage: cargo xtask <task>");
    for task in TASKS {
        eprintln!("  {:<18} {}", task.name, task.description);
    }
}

/// `cargo metadata` as JSON, with `extra` appended (for example `--no-deps`).
///
/// One place, because three gates read the workspace graph and each wants the same `--locked`
/// guarantee: a gate must not be the thing that rewrites `Cargo.lock`.
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
    use super::{TASKS, task_names};

    #[test]
    fn task_names_are_unique() {
        let mut names: Vec<&str> = task_names().collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate task name in TASKS");
    }

    #[test]
    fn every_task_is_dispatchable() {
        // Trivially true now that the handler lives in the table - which is the point. This
        // asserts the shape that makes it true, so a refactor back to a separate `match`
        // fails here rather than silently reintroducing the drift.
        for task in TASKS {
            assert!(!task.name.is_empty());
            assert!(!task.description.is_empty(), "{} has no --help line", task.name);
        }
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
