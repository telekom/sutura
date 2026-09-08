//! The fuzz harness's own gate: is every target declared, seeded, and actually run?
//!
//! **Why this exists rather than a sentence somewhere.** Issue #146 measured it: a sweep over the
//! tree found no fuzz target at all, so *nothing would notice one arriving or disappearing*. A
//! `fuzz_targets/*.rs` with no `[[bin]]` entry compiles for nobody; a `[[bin]]` the workflow's
//! matrix does not name is a target CI never runs; a target with no tracked seed starts from
//! nothing every time and cannot replay a crash somebody already fixed. All three leave every other
//! check green, which is the definition of coverage that is not there.
//!
//! **Static, and cheap on purpose.** It compiles nothing and runs no fuzzer, so it belongs in the
//! hygiene sweep on every pull request - unlike fuzzing itself, which `.github/workflows/fuzz.yml`
//! argues has to stay off the merge path.
//!
//! **Fails closed.** No target directory, no manifest, no matrix block and an empty target list are
//! all failures rather than a pass over silence.
//!
//! **The limit, next to the claim.** Both readers are line-oriented over surface syntax - a TOML
//! table header and a YAML sequence - rather than a parse into a document model, because `xtask`
//! carries neither a TOML nor a YAML dependency and runs inside a nix sandbox. So a `[[bin]]`
//! spelled as an inline table, or a matrix in flow style, is invisible to this gate. It holds the
//! shapes this repository writes, and the unit tests below fix those shapes. It does not read what
//! a target's code DOES - that a target reaches the parser it claims is a review question, and
//! each target's header is where the claim and its limits are written.

use std::collections::BTreeSet;

use crate::Verdict;
use crate::repo;

/// The fuzz crate's manifest, relative to the repository root.
const MANIFEST: &str = "fuzz/Cargo.toml";

/// Where the target sources live.
const TARGETS_DIR: &str = "fuzz/fuzz_targets";

/// Where the TRACKED seeds live, one directory per target.
///
/// Not `fuzz/corpus/`, which is libFuzzer's writable working corpus and is gitignored - see
/// `fuzz/.gitignore` for why the two are separate directories rather than one.
const SEEDS_DIR: &str = "fuzz/seeds";

/// The workflow whose matrix decides which targets CI spends a budget on.
const WORKFLOW: &str = ".github/workflows/fuzz.yml";

/// The lock the fuzz crate resolves against.
///
/// Its own, because `fuzz/` is a workspace of its own - see the manifest's header. **`cargo deny`
/// reads the ROOT lock only**, so nothing advisory-checks this one; that is the cost of keeping
/// `libfuzzer-sys` and `#![no_main]` out of the shipped graph, and it is stated where a reader
/// looking for the guarantee will be.
const LOCK: &str = "fuzz/Cargo.lock";

/// The macro that makes a file a libFuzzer target.
///
/// It is also, separately, the exact string OSS Scorecard's Rust fuzzing detector looks for in a
/// `*.rs` file - so the shape that finds bugs and the shape that is detectable are the same shape,
/// and neither was chosen for the other.
const HARNESS_MACRO: &str = "libfuzzer_sys";

/// The panic strategy the harness has to compile under.
///
/// The whole argument for `fuzz/` is that a panic on untrusted input is process death on every
/// shipped profile. A harness that unwound would be measuring a build nobody runs.
const PANIC_ABORT: &str = "panic = \"abort\"";

/// A `[[bin]]` section being read: either key may not have arrived yet.
#[derive(Default)]
struct PendingBin {
    name: Option<String>,
    path: Option<String>,
}

impl PendingBin {
    /// The complete pair, or nothing - a section missing either key declares no binary.
    fn complete(self) -> Option<(String, String)> {
        Some((self.name?, self.path?))
    }
}

/// Every `[[bin]]` the manifest declares, as `(name, path)`.
fn declared_bins(manifest: &str) -> Vec<(String, String)> {
    let mut bins = Vec::new();
    let mut open: Option<PendingBin> = None;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if let Some(complete) = open.take().and_then(PendingBin::complete) {
                bins.push(complete);
            }
            open = (trimmed == "[[bin]]").then(PendingBin::default);
            continue;
        }
        if let Some(fields) = &mut open {
            if let Some(value) = quoted_value(trimmed, "name") {
                fields.name = Some(value);
            } else if let Some(value) = quoted_value(trimmed, "path") {
                fields.path = Some(value);
            }
        }
    }
    if let Some(complete) = open.and_then(PendingBin::complete) {
        bins.push(complete);
    }
    bins
}

/// The double-quoted value of `key` on one `key = "value"` line.
fn quoted_value(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
    let inner = rest.strip_prefix('"')?;
    let end = inner.find('"')?;
    Some(String::from(inner.split_at(end).0))
}

/// The target names the workflow's matrix expands over.
///
/// Fails closed: a workflow with no `target:` sequence under a `matrix:` returns an error rather
/// than an empty set, because "the matrix names none of them" and "this reader did not find the
/// matrix" must not be the same answer.
fn matrix_targets(workflow: &str) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    let mut inside = false;
    let mut indent = 0_usize;
    for line in workflow.lines() {
        let trimmed = line.trim();
        if inside {
            if let Some(item) = trimmed.strip_prefix("- ") {
                names.insert(String::from(item.trim()));
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Any other content at or left of the sequence's own key ends it.
            if line.len().saturating_sub(trimmed.len()) <= indent {
                inside = false;
            }
            continue;
        }
        if trimmed == "target:" {
            inside = true;
            indent = line.len().saturating_sub(trimmed.len());
        }
    }
    if names.is_empty() {
        return Err(format!("{WORKFLOW} declares no `target:` sequence - the matrix runs nothing"));
    }
    Ok(names)
}

/// Does the manifest compile the harness with the shipped panic strategy?
fn aborts_on_panic(manifest: &str) -> bool {
    manifest.lines().any(|line| line.trim() == PANIC_ABORT)
}

/// The verdict, given everything read off the tree.
fn report(
    sources: &BTreeSet<String>,
    bins: &[(String, String)],
    seeded: &BTreeSet<String>,
    harnessed: &BTreeSet<String>,
    matrix: &BTreeSet<String>,
    locked: bool,
    aborts: bool,
) -> Verdict {
    let mut failures = Vec::new();
    if sources.is_empty() {
        failures.push(format!("{TARGETS_DIR} holds no target - a green fuzz run over nothing"));
    }
    let declared: BTreeSet<String> = bins.iter().map(|(name, _)| name.clone()).collect();
    for name in sources.difference(&declared) {
        failures.push(format!(
            "{TARGETS_DIR}/{name}.rs has no `[[bin]]` entry in {MANIFEST} - it is never built"
        ));
    }
    for (name, path) in bins {
        if !sources.contains(name) {
            failures.push(format!("{MANIFEST} declares `{name}` at `{path}`, which is not a file"));
        } else if path != &format!("fuzz_targets/{name}.rs") {
            failures.push(format!(
                "{MANIFEST} points `{name}` at `{path}` rather than at its own source"
            ));
        }
    }
    for name in sources.difference(harnessed) {
        failures.push(format!(
            "{TARGETS_DIR}/{name}.rs does not name `{HARNESS_MACRO}` - it fuzzes nothing"
        ));
    }
    for name in sources.difference(seeded) {
        failures.push(format!(
            "{SEEDS_DIR}/{name}/ is missing or empty - the target starts from nothing"
        ));
    }
    for name in sources.difference(matrix) {
        failures.push(format!("{WORKFLOW} never runs `{name}` - CI spends no budget on it"));
    }
    for name in matrix.difference(sources) {
        failures.push(format!("{WORKFLOW} runs `{name}`, which is not a target"));
    }
    if !locked {
        failures.push(format!(
            "{LOCK} is absent - a fuzz run against an unlocked graph is not reproducible"
        ));
    }
    if !aborts {
        failures.push(format!(
            "{MANIFEST} does not declare `{PANIC_ABORT}` - the harness would not measure the shipped panic strategy"
        ));
    }
    if failures.is_empty() {
        println!("xtask check-fuzz: ok - {} target(s) declared, seeded and run", sources.len());
        return Verdict::Pass;
    }
    eprintln!("xtask check-fuzz: {} problem(s)", failures.len());
    for failure in &failures {
        eprintln!("  {failure}");
    }
    eprintln!();
    eprintln!("Add the target to fuzz/Cargo.toml, seed fuzz/seeds/<target>/, and name it in the");
    eprintln!("matrix of .github/workflows/fuzz.yml. `just fuzz-smoke` replays what is committed.");
    Verdict::Fail
}

/// Reads the tree and hands [`report`] the answer.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-fuzz: not inside a git repository");
        return Verdict::Fail;
    };
    let Ok(manifest) = std::fs::read_to_string(root.join(MANIFEST)) else {
        eprintln!("xtask check-fuzz: {MANIFEST} is unreadable - the fuzz crate is the thing this gate is about");
        return Verdict::Fail;
    };
    let Ok(workflow) = std::fs::read_to_string(root.join(WORKFLOW)) else {
        eprintln!("xtask check-fuzz: {WORKFLOW} is unreadable - nothing would run a target");
        return Verdict::Fail;
    };

    // The set of targets is the *.rs files under fuzz_targets - the same derivation run-fuzz.sh
    // uses, so the gate and the runner agree on what a target is by construction.
    let targets_dir = root.join(TARGETS_DIR);
    let sources: BTreeSet<String> = std::fs::read_dir(&targets_dir).map_or_else(
        |_| BTreeSet::new(),
        |entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter_map(|name| name.strip_suffix(".rs").map(String::from))
                .collect()
        },
    );

    let seeded: BTreeSet<String> = std::fs::read_dir(root.join(SEEDS_DIR)).map_or_else(
        |_| BTreeSet::new(),
        |entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| {
                    // A seed directory is only a seed directory if it holds at least one file.
                    std::fs::read_dir(root.join(SEEDS_DIR).join(name)).is_ok_and(|mut d| d.next().is_some())
                })
                .collect()
        },
    );

    let harnessed: BTreeSet<String> = sources
        .iter()
        .filter(|name| {
            std::fs::read_to_string(root.join(TARGETS_DIR).join(format!("{name}.rs")))
                .is_ok_and(|src| src.contains(HARNESS_MACRO))
        })
        .cloned()
        .collect();

    let bins = declared_bins(&manifest);
    let locked = root.join(LOCK).is_file();
    let aborts = aborts_on_panic(&manifest);
    match matrix_targets(&workflow) {
        Ok(matrix) => report(&sources, &bins, &seeded, &harnessed, &matrix, locked, aborts),
        Err(message) => {
            eprintln!("xtask check-fuzz: {message}");
            Verdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bins(spec: &[(&str, &str)]) -> Vec<(String, String)> {
        spec.iter().map(|(a, b)| (String::from(*a), String::from(*b))).collect()
    }
    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn a_complete_target_set_passes() {
        let verdict = report(
            &set(&["sql_expression", "question_body"]),
            &bins(&[
                ("sql_expression", "fuzz_targets/sql_expression.rs"),
                ("question_body", "fuzz_targets/question_body.rs"),
            ]),
            &set(&["sql_expression", "question_body"]),
            &set(&["sql_expression", "question_body"]),
            &set(&["sql_expression", "question_body"]),
            true,
            true,
        );
        assert!(verdict == Verdict::Pass, "a coherent set must pass");
    }

    #[test]
    fn a_target_without_a_bin_entry_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            true,
            true,
        );
        assert!(verdict == Verdict::Fail, "an unbuilt target must fail");
    }

    #[test]
    fn a_target_the_matrix_never_runs_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &set(&[]),
            true,
            true,
        );
        assert!(verdict == Verdict::Fail, "a target CI never runs must fail");
    }

    #[test]
    fn a_target_with_no_seed_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
            &set(&[]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            true,
            true,
        );
        assert!(verdict == Verdict::Fail, "an unseeded target must fail");
    }

    #[test]
    fn a_missing_lock_or_missing_panic_abort_fails() {
        for (locked, aborts) in [(false, true), (true, false)] {
            let verdict = report(
                &set(&["sql_expression"]),
                &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
                &set(&["sql_expression"]),
                &set(&["sql_expression"]),
                &set(&["sql_expression"]),
                locked,
                aborts,
            );
            assert!(verdict == Verdict::Fail, "an unlocked or unwinding harness must fail");
        }
    }

    #[test]
    fn a_single_bin_section_is_read() {
        let manifest =
            "[package]\nname = \"x\"\n\n[[bin]]\nname = \"sql_expression\"\npath = \"fuzz_targets/sql_expression.rs\"\n";
        assert_eq!(
            declared_bins(manifest),
            vec![(String::from("sql_expression"), String::from("fuzz_targets/sql_expression.rs"))]
        );
    }

    #[test]
    fn a_matrix_sequence_is_read_and_fails_closed_when_absent() {
        let workflow = "jobs:\n  fuzz:\n    strategy:\n      matrix:\n        target:\n          - sql_expression\n          - question_body\n";
        assert_eq!(matrix_targets(workflow).unwrap(), set(&["question_body", "sql_expression"]));
        matrix_targets("jobs:\n  fuzz:\n    strategy: {}\n").expect_err("a workflow with no matrix must fail closed");
    }

    #[test]
    fn a_target_is_read_off_its_suffix() {
        assert_eq!("sql_expression.rs".strip_suffix(".rs"), Some("sql_expression"));
    }
}
