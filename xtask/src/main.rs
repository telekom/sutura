//! Repo automation. Run as `cargo xtask <task>`, or `cargo run -q -p xtask -- <task>`.
//!
//! These are gates, not conveniences: each answers "what fails if this rule is violated?"
//! with a non-zero exit and a count, not with a paragraph of guidance. A rule that lands here
//! instead of in a document is a rule that cannot rot unnoticed.
//!
//! Gates live in one binary rather than a script per check: one thing to install, one language
//! to review, and they are unit-tested by `cargo nextest run --workspace` like any other code.

mod action_shell;
mod api_docs;
mod api_links;
mod arrow_major;
mod attribution;
mod boot_order;
mod boundaries;
mod bounded_wait;
mod branches;
mod causality;
mod changes;
mod commit_msg;
mod compose;
mod conformance;
#[cfg(test)]
mod cpu_busy;
mod crap;
mod default_feature_tests;
mod default_features;
mod devenv_linter;
mod devenv_shell;
mod docs;
mod examples;
#[cfg(test)]
mod falsifier;
mod feature_remedies;
mod fmt;
mod fuzz;
mod gate_classification;
mod guidance;
mod hook_coverage;
mod hooks;
mod inconclusive;
mod jscpd;
mod line_endings;
mod markdown;
mod max_lines;
mod newtype_leaks;
mod nix_platform;
mod one_bound;
mod orphan_modules;
mod pins;
mod refusals;
mod registry;
mod release_provenance;
mod repo;
mod rust_source;
#[cfg(test)]
mod scratch_tree;
mod serde_parse;
mod shared_client;
mod shipped;
mod skills;
mod task_table;
mod tasks;
mod text;
mod threshold_expect;
mod unused_deps;
#[cfg(test)]
mod validated_base;
mod venues;
mod warm_start;
mod workflows;
mod worktree_state;

use std::process::ExitCode;

// The registry's types live in `registry` and the TABLE in `task_table`, split by area there,
// both because of `max-lines` - see those module headers. Re-exported here so every gate's
// `crate::Verdict` and every reader's `crate::tasks()` resolve through one path: this file stays
// the router.
use registry::{Kind, Task};
pub(crate) use registry::{Reads, Verdict};
pub(crate) use task_table::tasks;

/// Every task name. `check-guidance` reads this to reject a doc citing a task that is gone.
pub(crate) fn task_names() -> impl Iterator<Item = &'static str> {
    tasks().map(|t| t.name)
}

/// Every gate the `hygiene` sweep collects, with what it reads.
///
/// Derived from the same table and the same predicate `run_hygiene` filters on, because a second
/// list of the sweep's members is the drift this table was built to end.
pub(crate) fn hygiene_gates() -> impl Iterator<Item = (&'static str, Reads)> {
    tasks().filter_map(|t| match t.kind {
        Kind::Hygiene(reads) => Some((t.name, reads)),
        Kind::Standalone => None,
    })
}

/// Run every hygiene gate, in declaration order, stopping at the first failure.
///
/// Stopping rather than collecting: one failure to read beats the output of every gate behind it.
///
/// **The order is `crate::task_table`'s area order - SUBJECT order, not cost.** This said
/// `cheapest-first` until `github.com/telekom/sutura#610`, and that claim was held by nothing and
/// was also false: timed on the tree before the split, the FIRST gate was `check-boundaries` at
/// 21s, while the two cheapest measured - `text-hygiene` and `line-endings`, both under 1.5s -
/// stood 25th and 24th. Early return makes the order BEHAVIOURAL, since it decides which failure
/// a developer reads first, and splitting the table by area moved 36 of the 38 gates. What the
/// order buys now is that a failure arrives beside the other rules about the same subject.
/// Ordering by cost would need a per-row cost and a gate asserting the sequence: a doc comment
/// cannot hold it, which is why this one is a description rather than a promise.
fn run_hygiene(_args: &[String]) -> Verdict {
    let gates: Vec<&Task> = tasks().filter(|t| matches!(t.kind, Kind::Hygiene(_))).collect();
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
        Some(requested) => tasks().find(|t| t.name == requested).map_or_else(
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
    for task in tasks() {
        // A leading dot marks a member of the `hygiene` sweep, so the set is readable here
        // rather than only in the source.
        let mark = if matches!(task.kind, Kind::Hygiene(_)) { "." } else { " " };
        eprintln!("{mark} {:<18} {}", task.name, task.description);
    }
    eprintln!();
    eprintln!(". = run together by `cargo xtask hygiene`");
    eprintln!(
        "{} of {} hygiene gate(s) have a written own-rule falsifier; the rest await the per-gate \
         seed programme (`telekom/sutura#371`)",
        tasks()
            .filter(|t| matches!(t.kind, Kind::Hygiene(_)) && t.falsifier.in_scope.is_some())
            .count(),
        tasks().filter(|t| matches!(t.kind, Kind::Hygiene(_))).count()
    );
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
    use super::{task_names, tasks};

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
        for task in tasks() {
            assert!(!task.description.is_empty(), "{} has no --help line", task.name);
        }
    }

    #[test]
    fn the_hygiene_set_is_not_empty() {
        // An empty set would make `hygiene` a green no-op - the failure this whole change
        // exists to prevent, arrived at from the other direction.
        assert!(tasks().any(|t| matches!(t.kind, super::Kind::Hygiene(_))));
    }

    #[test]
    fn the_sweep_has_a_gate_on_each_side_of_the_prose_line() {
        // `Kind::Hygiene(Reads)` makes the classification total - a gate cannot join the sweep
        // without declaring a side - so what is left to check is that neither side is EMPTY.
        // An empty side is how the plan's argument passes vacuously: with nothing classified as
        // reading prose there is no table of deferred verdicts to be wrong, and with nothing
        // classified as reading code the skip it justifies covers nothing.
        let mut code = 0_usize;
        let mut prose = 0_usize;
        for (_, reads) in super::hygiene_gates() {
            match reads {
                super::Reads::Code => code += 1,
                super::Reads::Prose => prose += 1,
            }
        }
        assert!(
            code > 0,
            "no hygiene gate reads code - the skip in docs.yml justifies nothing"
        );
        assert!(
            prose > 0,
            "no hygiene gate reads prose - the deferred-verdict table is then empty"
        );
    }

    #[test]
    fn hygiene_does_not_contain_itself() {
        // Marked `Standalone`, so `run_hygiene` cannot collect and re-enter itself.
        let me = tasks().find(|t| t.name == "hygiene").expect("hygiene is registered");
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
            // Its argument is a store path nix interpolates at the call site, so an
            // argument-free invocation has nothing to read - and the sweep would call it that way.
            "check-devenv-linter",
        ];
        for name in needs_args {
            let task = tasks().find(|t| t.name == name).expect("task is registered");
            assert_eq!(task.kind, super::Kind::Standalone, "{name} must not be in the hygiene set");
        }
    }

    #[test]
    fn the_api_docs_gate_is_not_collected_into_hygiene() {
        // It takes no arguments, so the test above would not catch this one. The reason is
        // COST and REACH: it compiles the library crates and needs the nightly toolchain for
        // rustdoc's unstable JSON output, while the hygiene sweep runs on every commit and
        // inside the Nix sandbox, neither of which has a nightly.
        let task = tasks().find(|t| t.name == "check-api-docs").expect("task is registered");
        assert_eq!(task.kind, super::Kind::Standalone);
    }

    #[test]
    fn the_branch_cleanup_is_not_collected_into_hygiene() {
        // It takes no REQUIRED arguments, so the argument test above would not catch this one, and
        // it is the entry in this table that can DELETE things. A hygiene sweep runs on every
        // commit; a task that removes a branch may not be in it whatever its default mode is.
        let task = tasks().find(|t| t.name == "clean-branches").expect("task is registered");
        assert_eq!(task.kind, super::Kind::Standalone);
    }

    #[test]
    fn the_crap_gate_is_not_collected_into_hygiene() {
        // Same reasoning as the api-docs gate above, and the argument test would not catch this
        // one either: `crap` takes no arguments. It compiles the scoped crates under
        // `-C instrument-coverage` and shells out to two tools the cheap sweep must not require -
        // and the Nix sandbox and the commit hook both run that sweep.
        let task = tasks().find(|t| t.name == "crap").expect("task is registered");
        assert_eq!(task.kind, super::Kind::Standalone);
        // Its configuration half IS cheap and must stay in the sweep: that is what stops the
        // policy file from rotting on a tree nobody has run the expensive half against.
        let cheap = tasks().find(|t| t.name == "check-crap").expect("task is registered");
        assert!(matches!(cheap.kind, super::Kind::Hygiene(_)));
        // And the DELTA half is standalone for a third reason: it needs a baseline that arrives
        // over the network in CI, and the hygiene sweep runs in a sandbox with no network.
        let delta = tasks().find(|t| t.name == "crap-delta").expect("task is registered");
        assert_eq!(delta.kind, super::Kind::Standalone);
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
        // AND IT IS NOT 0, which is the whole of #307: a gate that measured nothing may not hand
        // a required step the same code a proof does. Asserted against `SUCCESS` as well as
        // against 3, because the failure mode being closed here is someone mapping it back.
        assert_eq!(
            format!("{:?}", Verdict::Inconclusive.exit_code()),
            format!("{:?}", ExitCode::from(3))
        );
        assert_ne!(
            format!("{:?}", Verdict::Inconclusive.exit_code()),
            format!("{:?}", ExitCode::SUCCESS)
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
