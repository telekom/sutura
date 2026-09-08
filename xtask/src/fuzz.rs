//! The fuzz harness's own gate: is every target declared, seeded, and actually run?
//!
//! **Why this exists rather than a sentence in a README.** `github.com/telekom/sutura#146` measured
//! it: `git grep -niE 'fuzz|proptest' -- xtask` returned nothing, so *nothing in the sweep would
//! notice a fuzz target arriving or disappearing*. A `fuzz_targets/*.rs` file with no `[[bin]]`
//! entry compiles for nobody; a `[[bin]]` the workflow's matrix does not name is a target CI never
//! runs; a target with no tracked seed starts from nothing every time and cannot replay a crash
//! somebody already fixed. All three leave every other check green, which is the definition of
//! coverage that is not there.
//!
//! **Static, and cheap on purpose.** It compiles nothing and runs no fuzzer, so it belongs in the
//! hygiene sweep on every pull request - unlike fuzzing itself, which `.github/workflows/fuzz.yml`
//! argues has to stay off the merge path.
//!
//! **Fails closed.** No target directory, no manifest, no matrix block and an empty target list are
//! all failures rather than a pass over silence. A gate that reports success by finding nothing to
//! check is how this hole opened the first time.
//!
//! **The limit, next to the claim.** Both readers are line-oriented over surface syntax - a TOML
//! table header and a YAML sequence - rather than a parse into a document model, because `xtask`
//! carries neither a TOML nor a YAML dependency and runs inside a nix sandbox. So a `[[bin]]`
//! spelled as an inline table, or a matrix written in flow style, is invisible to this gate and
//! would read as absent: it holds the shapes this repository writes, and the unit tests below fix
//! those shapes. It does not read what a target's code DOES - that a target actually reaches the
//! parser it claims to is a review question, and the module header of each target is where the
//! claim and its limits are written. **Nor is `fuzz/` formatted by any gate:** `cargo xtask fmt`
//! derives its scope from the root workspace's members and `fuzz/` is a workspace of its own, so
//! `checks.fmt` never sees it. The files are rustfmt-clean by hand rather than by mechanism, which
//! is a rule with no mechanism and is recorded as one.

use std::collections::BTreeSet;
use std::path::Path;

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
/// A file without it builds a binary that fuzzes nothing. It is also, separately, the exact string
/// OSS Scorecard's Rust fuzzing detector looks for in a `*.rs` file - so the shape that finds bugs
/// and the shape that is detectable are the same shape, and neither was chosen for the other.
const HARNESS_MACRO: &str = "libfuzzer_sys";

/// The panic strategy the harness has to compile under.
///
/// The whole argument for `fuzz/` is that a panic on untrusted input is process death on every
/// shipped profile. A harness that unwound would be measuring a build nobody runs, so the manifest
/// declaring otherwise is a failure rather than a preference.
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
///
/// Line-oriented: a `[[bin]]` header opens a section, any other `[` at column zero closes it, and
/// `name`/`path` are read inside one. See the module header for what that cannot see.
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
        let Some(ref mut fields) = open else { continue };
        if let Some(value) = quoted_value(trimmed, "name") {
            fields.name = Some(value);
        } else if let Some(value) = quoted_value(trimmed, "path") {
            fields.path = Some(value);
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
///
/// Separated from the reading so the whole decision is testable without a repository.
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
    let matrix = match matrix_targets(&workflow) {
        Ok(names) => names,
        Err(refusal) => {
            eprintln!("xtask check-fuzz: {refusal}");
            return Verdict::Fail;
        }
    };

    let mut sources = BTreeSet::new();
    let mut harnessed = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(root.join(TARGETS_DIR)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(std::ffi::OsStr::to_str).map(String::from) else {
                continue;
            };
            if std::fs::read_to_string(&path).is_ok_and(|text| text.contains(HARNESS_MACRO)) {
                harnessed.insert(name.clone());
            }
            sources.insert(name);
        }
    }

    let seeded: BTreeSet<String> = sources
        .iter()
        .filter(|name| has_seed(&root.join(SEEDS_DIR).join(name)))
        .cloned()
        .collect();

    report(
        &sources,
        &declared_bins(&manifest),
        &seeded,
        &harnessed,
        &matrix,
        root.join(LOCK).is_file(),
        aborts_on_panic(&manifest),
    )
}

/// Does this seed directory hold at least one file?
fn has_seed(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| entries.flatten().any(|entry| entry.path().is_file()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{aborts_on_panic, declared_bins, matrix_targets, quoted_value, report};
    use crate::Verdict;

    const MANIFEST: &str = r#"
[workspace]

[package]
name = "sutura-fuzz"

[profile.release]
panic = "abort"

[[bin]]
name = "keyset"
path = "fuzz_targets/keyset.rs"
test = false

[[bin]]
name = "bearer_token"
path = "fuzz_targets/bearer_token.rs"
"#;

    const WORKFLOW_YAML: &str = "
jobs:
  fuzz:
    strategy:
      matrix:
        # A comment inside the sequence.
        target:
          - bearer_token
          - keyset
    steps:
      - run: true
";

    fn names(of: &[&str]) -> BTreeSet<String> {
        of.iter().map(|one| String::from(*one)).collect()
    }

    fn bins(of: &[&str]) -> Vec<(String, String)> {
        of.iter()
            .map(|one| (String::from(*one), format!("fuzz_targets/{one}.rs")))
            .collect()
    }

    #[test]
    fn a_bin_section_yields_its_name_and_path() {
        assert_eq!(
            declared_bins(MANIFEST),
            vec![
                (String::from("keyset"), String::from("fuzz_targets/keyset.rs")),
                (String::from("bearer_token"), String::from("fuzz_targets/bearer_token.rs")),
            ]
        );
    }

    #[test]
    fn a_key_outside_a_bin_section_is_not_a_bin() {
        assert!(declared_bins("[package]\nname = \"sutura-fuzz\"\n").is_empty());
    }

    #[test]
    fn a_quoted_value_stops_at_its_closing_quote() {
        assert_eq!(quoted_value("name = \"keyset\"", "name").as_deref(), Some("keyset"));
        assert_eq!(quoted_value("test = false", "name"), None);
    }

    #[test]
    fn the_matrix_yields_the_targets_it_expands_over() {
        assert_eq!(matrix_targets(WORKFLOW_YAML), Ok(names(&["bearer_token", "keyset"])));
    }

    #[test]
    fn a_workflow_with_no_matrix_is_a_refusal_and_not_an_empty_set() {
        assert_eq!(
            matrix_targets("jobs:\n  fuzz:\n    steps:\n      - run: true\n"),
            Err(format!(
                "{} declares no `target:` sequence - the matrix runs nothing",
                super::WORKFLOW
            ))
        );
    }

    #[test]
    fn the_panic_strategy_is_read_off_the_manifest() {
        assert!(aborts_on_panic(MANIFEST));
        assert!(!aborts_on_panic("[profile.release]\npanic = \"unwind\"\n"));
    }

    #[test]
    fn a_complete_harness_passes() {
        let all = names(&["keyset", "bearer_token"]);
        assert_eq!(
            report(&all, &bins(&["keyset", "bearer_token"]), &all, &all, &all, true, true),
            Verdict::Pass
        );
    }

    #[test]
    fn no_target_at_all_fails_rather_than_passing_over_nothing() {
        // EVERY input empty, and that is the whole point of the cell: with a non-empty matrix here
        // the `matrix.difference(sources)` rule fires instead and the verdict is `Fail` whatever
        // the emptiness rule does. Measured - the first version of this test passed with the
        // emptiness refusal deleted, which is a test that looks like coverage and is not.
        let none = BTreeSet::new();
        assert_eq!(report(&none, &[], &none, &none, &none, true, true), Verdict::Fail);
    }

    #[test]
    fn each_missing_piece_fails_on_its_own() {
        let all = names(&["keyset"]);
        let none = BTreeSet::new();
        // Undeclared in the manifest.
        assert_eq!(report(&all, &[], &all, &all, &all, true, true), Verdict::Fail);
        // Declared at the wrong path.
        let wrong = vec![(String::from("keyset"), String::from("fuzz_targets/other.rs"))];
        assert_eq!(report(&all, &wrong, &all, &all, &all, true, true), Verdict::Fail);
        // No seed corpus.
        assert_eq!(report(&all, &bins(&["keyset"]), &none, &all, &all, true, true), Verdict::Fail);
        // No harness macro, so the binary fuzzes nothing.
        assert_eq!(report(&all, &bins(&["keyset"]), &all, &none, &all, true, true), Verdict::Fail);
        // The workflow does not run it.
        assert_eq!(report(&all, &bins(&["keyset"]), &all, &all, &none, true, true), Verdict::Fail);
        // The workflow runs something that is not a target.
        assert_eq!(
            report(&all, &bins(&["keyset"]), &all, &all, &names(&["keyset", "ghost"]), true, true),
            Verdict::Fail
        );
        // No lock.
        assert_eq!(report(&all, &bins(&["keyset"]), &all, &all, &all, false, true), Verdict::Fail);
        // The harness would unwind where the deployment aborts.
        assert_eq!(report(&all, &bins(&["keyset"]), &all, &all, &all, true, false), Verdict::Fail);
    }

    #[test]
    fn the_real_tree_satisfies_its_own_gate() {
        assert_eq!(super::run(&[]), Verdict::Pass);
    }
}
