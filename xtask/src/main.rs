//! Repo automation. Run as `cargo xtask <task>`, or `cargo run -q -p xtask -- <task>`.
//!
//! These are gates, not conveniences: each answers "what fails if this rule is violated?"
//! with a non-zero exit and a count, not with a paragraph of guidance. A rule that lands here
//! instead of in a document is a rule that cannot rot unnoticed.
//!
//! Gates live in one binary rather than a script per check: one thing to install, one language
//! to review, and they are unit-tested by `cargo nextest run --workspace` like any other code.

mod api_docs;
mod boundaries;
mod causality;
mod changes;
mod commit_msg;
mod docs;
mod fmt;
mod guidance;
mod line_endings;
mod max_lines;
mod pins;
mod repo;
mod skills;
mod text;
mod unused_deps;
mod workflows;

use std::process::ExitCode;

/// What a task concluded.
///
/// Not `ExitCode`: that type is opaque - it cannot be compared or read back - so a task that
/// runs other tasks could not tell whether they passed. `main` converts this to an `ExitCode`
/// once, at the process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Nothing to report.
    Pass,
    /// A violation. The task has already printed it.
    Fail,
    /// Invoked wrongly - bad or missing arguments. Distinct from a violation, because a
    /// mistyped command is not a repo problem.
    Usage,
}

impl Verdict {
    // Not const: `ExitCode::from` is not a const fn.
    fn exit_code(self) -> ExitCode {
        match self {
            Self::Pass => ExitCode::SUCCESS,
            Self::Fail => ExitCode::FAILURE,
            Self::Usage => ExitCode::from(2),
        }
    }
}

/// What a gate does: read the repo, print a verdict.
type Gate = fn(&[String]) -> Verdict;

/// Whether a task belongs to the `hygiene` sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Cheap, argument-free, judges the whole repo. Collected into `hygiene`.
    Hygiene,
    /// Everything else: takes arguments, changes files, or runs other tasks. Never collected,
    /// which is also what stops `hygiene` from recursing into itself.
    Standalone,
}

/// A task: the name, the `--help` line, whether it is a hygiene gate, and the code it runs.
///
/// The handler is IN the table, so `--help` and dispatch cannot disagree. They did once - six
/// dispatched tasks were missing from the list, so `--help` lied and `check-guidance` reported
/// every mention of them as a deleted gate. A table plus a separate `match` is two lists.
///
/// `kind` is here for the same reason. The hygiene list used to be hand-transcribed in the
/// justfile, twice in devenv.nix, in flake.nix and as eight separate hooks - and the order
/// differed in three of them. `check-guidance` verifies that a NAMED task exists, so it catches
/// a rename but is blind to an omission: adding a gate and forgetting one of five call sites
/// was invisible. Now there is one list and the callers ask for it by name.
struct Task {
    name: &'static str,
    description: &'static str,
    kind: Kind,
    run: Gate,
}

const TASKS: &[Task] = &[
    Task {
        name: "check-boundaries",
        description: "the domain crate depends on no framework",
        kind: Kind::Hygiene,
        run: boundaries::run,
    },
    Task {
        name: "max-lines",
        description: "no file over 1000 lines (exemptions: .max-lines-ignore)",
        kind: Kind::Hygiene,
        run: max_lines::run,
    },
    Task {
        name: "check-pins",
        description: "no tool is pinned by both nix and pixi",
        kind: Kind::Hygiene,
        run: pins::run,
    },
    Task {
        name: "unused-deps",
        description: "every declared dependency is actually used",
        kind: Kind::Hygiene,
        run: unused_deps::run,
    },
    Task {
        name: "line-endings",
        description: "every text file uses LF, not CRLF",
        kind: Kind::Hygiene,
        run: line_endings::run,
    },
    Task {
        name: "text-hygiene",
        description: "conflict markers, whitespace, final newline, file size; --fix",
        kind: Kind::Hygiene,
        run: text::run,
    },
    Task {
        name: "check-skills",
        description: "the skill router and the skill tree agree",
        kind: Kind::Hygiene,
        run: skills::run,
    },
    Task {
        name: "check-guidance",
        description: "docs and comments still describe this repo",
        kind: Kind::Hygiene,
        run: guidance::run,
    },
    Task {
        name: "check-workflows",
        description: "every flake output a workflow names exists",
        kind: Kind::Hygiene,
        run: workflows::run,
    },
    Task {
        name: "check-docs",
        description: "the nav in mkdocs.yml and the pages under docs/ agree",
        kind: Kind::Hygiene,
        run: docs::run,
    },
    Task {
        name: "commit-msg",
        description: "the commit subject is a conventional commit (the hook passes the file)",
        kind: Kind::Standalone,
        run: commit_msg::run,
    },
    Task {
        name: "classify",
        description: "what a diff requires; --since <ref>, or paths (fails open)",
        kind: Kind::Standalone,
        run: changes::run_classify,
    },
    Task {
        name: "changed-packages",
        description: "the cargo packages owning the given .rs paths",
        kind: Kind::Standalone,
        run: changes::run_changed_packages,
    },
    Task {
        name: "check-changed",
        description: "cargo check, narrowed to the packages that changed",
        kind: Kind::Standalone,
        run: changes::run_check_changed,
    },
    Task {
        // NOT `Kind::Hygiene`, and not by oversight. The hygiene sweep is cheap,
        // argument-free and runs everywhere a developer commits - including hosts and
        // sandboxes with no Rust nightly at all. This one COMPILES the library crates and
        // needs the nightly toolchain, because `--output-format json` is an unstable rustdoc
        // option. Collecting it would make the cheap sweep expensive and, worse, unrunnable
        // in the places it currently runs.
        name: "check-api-docs",
        description: "docs/api/*.md is what the generator produces (NIGHTLY; compiles)",
        kind: Kind::Standalone,
        run: api_docs::run,
    },
    Task {
        name: "fmt",
        description: "cargo fmt, scoped to our packages (--check to verify)",
        kind: Kind::Standalone,
        run: fmt::run,
    },
    Task {
        name: "hygiene",
        description: "every cheap structural gate, in order (the one list)",
        kind: Kind::Standalone,
        run: run_hygiene,
    },
    Task {
        name: "test-causality",
        description: "a changed test is red on base, green on head; --since <ref>",
        kind: Kind::Standalone,
        run: causality::run,
    },
];

/// Every task name. `check-guidance` reads this to reject a doc citing a task that is gone.
pub(crate) fn task_names() -> impl Iterator<Item = &'static str> {
    TASKS.iter().map(|t| t.name)
}

/// Run every hygiene gate, in declaration order, stopping at the first failure.
///
/// Stopping rather than collecting: these are ordered cheapest-first, so the first failure is
/// usually the cheapest to read, and a wall of output from eight gates is worse than one.
fn run_hygiene(_args: &[String]) -> Verdict {
    let gates: Vec<&Task> = TASKS.iter().filter(|t| t.kind == Kind::Hygiene).collect();
    for task in &gates {
        // No arguments: a hygiene gate takes none, which is what `Kind::Hygiene` asserts.
        match (task.run)(&[]) {
            Verdict::Pass => {}
            other => {
                eprintln!();
                eprintln!("xtask hygiene: stopped at `{}`", task.name);
                return other;
            }
        }
    }
    println!("xtask hygiene: ok - {} gate(s)", gates.len());
    Verdict::Pass
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
            |task| (task.run)(rest).exit_code(),
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
        // A leading dot marks a member of the `hygiene` sweep, so the set is readable here
        // rather than only in the source.
        let mark = if task.kind == Kind::Hygiene { "." } else { " " };
        eprintln!("{mark} {:<18} {}", task.name, task.description);
    }
    eprintln!();
    eprintln!(". = run together by `cargo xtask hygiene`");
}

/// `cargo metadata` as JSON, with `extra` appended (for example `--no-deps`).
///
/// One place, because three gates read the workspace graph and each wants the same `--locked`
/// guarantee: a gate must not be the thing that rewrites `Cargo.lock`.
fn cargo_metadata(extra: &[&str]) -> Result<serde_json::Value, String> {
    // `env!("CARGO")` looks equivalent and is not: it bakes cargo's absolute store path into
    // the binary at compile time, which put the whole cargo closure into the shipped package's
    // runtime closure and made the build non-reproducible. Cargo sets this variable whenever it
    // invokes us, so the runtime lookup is the same value with none of that.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));
    let output = std::process::Command::new(cargo)
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
    fn every_task_has_a_help_line() {
        for task in TASKS {
            assert!(!task.description.is_empty(), "{} has no --help line", task.name);
        }
    }

    #[test]
    fn the_hygiene_set_is_not_empty() {
        // An empty set would make `hygiene` a green no-op - the failure this whole change
        // exists to prevent, arrived at from the other direction.
        assert!(TASKS.iter().any(|t| t.kind == super::Kind::Hygiene));
    }

    #[test]
    fn hygiene_does_not_contain_itself() {
        // Marked `Standalone`, so `run_hygiene` cannot collect and re-enter itself.
        let me = TASKS.iter().find(|t| t.name == "hygiene").expect("hygiene is registered");
        assert_eq!(me.kind, super::Kind::Standalone);
    }

    #[test]
    fn no_hygiene_gate_needs_arguments() {
        // `run_hygiene` calls each with an empty slice. A gate that needs `--since` or a path
        // would silently do the wrong thing, so those stay `Standalone`.
        let needs_args = [
            "classify",
            "check-changed",
            "test-causality",
            "commit-msg",
            "changed-packages",
        ];
        for name in needs_args {
            let task = TASKS.iter().find(|t| t.name == name).expect("task is registered");
            assert_eq!(task.kind, super::Kind::Standalone, "{name} must not be in the hygiene set");
        }
    }

    #[test]
    fn the_api_docs_gate_is_not_collected_into_hygiene() {
        // It takes no arguments, so the test above would not catch this one. The reason is
        // COST and REACH: it compiles the library crates and needs the nightly toolchain for
        // rustdoc's unstable JSON output, while the hygiene sweep runs on every commit and
        // inside the Nix sandbox, neither of which has a nightly.
        let task = TASKS.iter().find(|t| t.name == "check-api-docs").expect("task is registered");
        assert_eq!(task.kind, super::Kind::Standalone);
    }

    #[test]
    fn a_verdict_maps_to_the_expected_exit_code() {
        use std::process::ExitCode;

        use super::Verdict;
        // `ExitCode` cannot be compared, so assert via the Debug form - the one thing it does
        // expose. Pass must be 0 or a failing gate would not fail the build.
        assert_eq!(format!("{:?}", Verdict::Pass.exit_code()), format!("{:?}", ExitCode::SUCCESS));
        assert_eq!(format!("{:?}", Verdict::Fail.exit_code()), format!("{:?}", ExitCode::FAILURE));
        assert_eq!(
            format!("{:?}", Verdict::Usage.exit_code()),
            format!("{:?}", ExitCode::from(2))
        );
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
