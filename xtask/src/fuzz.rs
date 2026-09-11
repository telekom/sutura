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
//! **And it refuses the fuzzer inside the RELEASE workflow**, which is the same argument one venue
//! over: fuzzing runs on the release tag now, and a job that ran it inside `release.yml` would be
//! one `needs:` away from letting a fresh random finding block every release. See
//! [`RELEASE_WORKFLOW`] for why that is worse than the cost it would save, and
//! [`FUZZ_INVOCATIONS`] for what the refusal cannot see.
//!
//! **Fails closed.** No target directory, no manifest, no matrix block and an empty target list are
//! all failures rather than a pass over silence.
//!
//! **A dictionary libFuzzer refuses is a run that executes nothing.** `-dict=` is validated at
//! startup and an unparseable line is fatal: libFuzzer prints `ParseDictionaryFile: error in line
//! N` and exits 1 - but only AFTER the release build the job already paid for, so the leg looks
//! like a fuzz run, costs like one, and mutates zero inputs. Nothing else in the tree reads these
//! files, so the only witness was a log line. Measured against a libFuzzer binary rather than read
//! off its source: a name is optional, the value must be double-quoted, `\\`, `\"` and `\xAB` are
//! the only escapes, an EMPTY value is refused, and a trailing `# comment` after the closing quote
//! is refused too - a `#` only starts a comment at the head of a line.
//!
//! **The limit, next to the claim.** All three readers are line-oriented over surface syntax - a
//! TOML table header, a YAML sequence and a dictionary entry - rather than a parse into a document
//! model, because `xtask` carries neither a TOML nor a YAML dependency and runs inside a nix
//! sandbox. So a `[[bin]]` spelled as an inline table, or a matrix in flow style, is invisible to
//! this gate. It holds the shapes this repository writes, and the unit tests below fix those
//! shapes. The dictionary reader checks the line shape and the escapes, not the token libFuzzer
//! builds from them, and it says nothing about whether a dictionary is USEFUL to its target. It
//! does not read what a target's code DOES - that a target reaches the parser it claims is a
//! review question, and each target's header is where the claim and its limits are written.

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

/// Where the OPTIONAL libFuzzer dictionaries live, one `<target>.dict` per target that has one.
///
/// Optional on purpose - `nix/run-fuzz.sh` drops `-dict=` when the file is absent, so a target
/// without one runs without one. Present and unparseable is the case this gate exists for.
const DICTIONARIES_DIR: &str = "fuzz/dictionaries";

/// The workflow whose matrix decides which targets CI spends a budget on.
const WORKFLOW: &str = ".github/workflows/fuzz.yml";

/// The workflow that publishes a release, held against the fuzzer never running INSIDE it.
///
/// **Why this is a refusal and not a sentence in a header.** Fuzzing is unbounded by nature: a
/// target that finds nothing today finds something on a path nobody has generated yet, and
/// `sql_expression` has produced a real defect on runs before now. A fuzz job inside this file is
/// one `needs:` away from deciding whether a tag may ship, and at that point the first fresh
/// finding blocks every release until somebody fixes it - which is a far worse outcome than the
/// cost of running the fuzzer where it cannot. So [`WORKFLOW`] runs on the same `v*` tag as its own
/// SEPARATE run: no job in one workflow can depend on another workflow's run, so the structure
/// carries the property rather than a reviewer remembering it.
const RELEASE_WORKFLOW: &str = ".github/workflows/release.yml";

/// The three ways the fuzzer is invoked in this repository: the runner script, the flake app, and
/// `cargo-fuzz` itself.
///
/// **A NAME REFUSAL AND NOTHING MORE**, and the limits matter more than the list. It reads one
/// file's text, so a fuzz job in a workflow `release.yml` CALLS is invisible to it. It says nothing
/// about [`WORKFLOW`]'s own triggers: a `pull_request` or `merge_group` leg added there is held
/// elsewhere, by `check-workflows`, which requires every job that reports on a gating event to be
/// classified in `devco/required-contexts`. And a re-added `schedule` or push-to-`main` leg is held
/// by nothing here on purpose - that is a COST decision, reversible in one line, not a way for
/// fuzzing to block a release.
const FUZZ_INVOCATIONS: [&str; 3] = ["run-fuzz.sh", "nix run .#fuzz", "cargo fuzz"];

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

/// The 1-indexed lines of the release workflow that invoke the fuzzer, with the form each used.
///
/// Comment lines are skipped: this file's own argument for the split names the runner script, and a
/// gate that refused the sentence explaining it would make the explanation unwritable.
fn release_invocations(workflow: &str) -> Vec<(usize, &'static str)> {
    workflow
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with('#'))
        .flat_map(|(index, line)| {
            FUZZ_INVOCATIONS
                .iter()
                .filter(move |needle| line.contains(**needle))
                .map(move |needle| (index.saturating_add(1), *needle))
        })
        .collect()
}

/// Does the manifest compile the harness with the shipped panic strategy?
fn aborts_on_panic(manifest: &str) -> bool {
    manifest.lines().any(|line| line.trim() == PANIC_ABORT)
}

/// Would libFuzzer accept this one dictionary line?
///
/// Everything before the first `"` is the optional name and is ignored, exactly as libFuzzer
/// ignores it; the last non-space character has to be the closing `"`; the value has to be
/// non-empty and may escape only `\\`, `\"` and `\xAB` - a lowercase `x` and either hex case.
/// Blank and comment lines never reach here.
fn dictionary_entry_is_parseable(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(open) = trimmed.find('"') else { return false };
    let Some(value) = trimmed.get(open + 1..).and_then(|rest| rest.strip_suffix('"')) else {
        return false;
    };
    if value.is_empty() {
        return false;
    }
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            continue;
        }
        match chars.next() {
            Some('\\' | '"') => (),
            // Lowercase `x` ONLY: `\X41` is refused, measured against a libFuzzer binary and
            // against this file's first reading of libFuzzer's source, which had it as either case.
            Some('x') => match (chars.next(), chars.next()) {
                (Some(high), Some(low)) if high.is_ascii_hexdigit() && low.is_ascii_hexdigit() => (),
                _ => return false,
            },
            _ => return false,
        }
    }
    true
}

/// The 1-INDEXED lines of one dictionary that libFuzzer would refuse.
///
/// One-indexed because the runner's own error is, and a gate that renumbered the defect would cost
/// the reader the one thing the log already told them.
fn unparseable_entries(dictionary: &str) -> Vec<usize> {
    dictionary
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let head = line.trim_start();
            !head.is_empty() && !head.starts_with('#')
        })
        .filter(|(_, line)| !dictionary_entry_is_parseable(line))
        .map(|(index, _)| index + 1)
        .collect()
}

/// The verdict, given everything read off the tree.
fn report(
    sources: &BTreeSet<String>,
    bins: &[(String, String)],
    seeded: &BTreeSet<String>,
    harnessed: &BTreeSet<String>,
    matrix: &BTreeSet<String>,
    refused: &[(String, usize)],
    in_release: &[(usize, &str)],
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
    for (dictionary, line) in refused {
        failures.push(format!(
            "{DICTIONARIES_DIR}/{dictionary}: error in line {line} - not a `name=\"value\"` entry, so libFuzzer refuses the dictionary after the build and the target executes nothing"
        ));
    }
    for (line, form) in in_release {
        failures.push(format!(
            "{RELEASE_WORKFLOW}:{line} invokes the fuzzer (`{form}`) - a fuzz job there is one `needs:` away from deciding whether a tag ships, and the first finding on a fresh random path would then block every release. {WORKFLOW} runs it on the same `v*` tag as a separate run, which no release job can depend on"
        ));
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
    eprintln!("A fuzz invocation in the release workflow belongs in .github/workflows/fuzz.yml,");
    eprintln!("which runs on the same tag without a release job depending on it. Otherwise: add");
    eprintln!("the target to fuzz/Cargo.toml, seed fuzz/seeds/<target>/, and name it in the");
    eprintln!("matrix of .github/workflows/fuzz.yml. An entry in fuzz/dictionaries/<target>.dict is");
    eprintln!("`name=\"value\"`, escaping only \\\\, \\\" and \\xAB. `just fuzz-smoke` replays what is");
    eprintln!("committed.");
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
    // Fails closed for the reason every other read here does: a release workflow this gate cannot
    // open is one whose fuzz job it cannot refuse.
    let Ok(release) = std::fs::read_to_string(root.join(RELEASE_WORKFLOW)) else {
        eprintln!(
            "xtask check-fuzz: {RELEASE_WORKFLOW} is unreadable - a fuzz job inside it would gate every release, and this is what refuses one"
        );
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

    // Only `<target>.dict` files, the same derivation run-fuzz.sh uses to build its `-dict=`. An
    // absent directory is not a failure: the dictionaries are an optimisation, not a requirement.
    let dictionaries_dir = root.join(DICTIONARIES_DIR);
    let mut names: Vec<String> = std::fs::read_dir(&dictionaries_dir).map_or_else(
        |_| Vec::new(),
        |entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "dict"))
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        },
    );
    names.sort();
    let mut refused = Vec::new();
    for name in names {
        let Ok(dictionary) = std::fs::read_to_string(dictionaries_dir.join(&name)) else {
            eprintln!(
                "xtask check-fuzz: {DICTIONARIES_DIR}/{name} is unreadable - a dictionary nothing can check is one libFuzzer may refuse"
            );
            return Verdict::Fail;
        };
        refused.extend(unparseable_entries(&dictionary).into_iter().map(|line| (name.clone(), line)));
    }

    let bins = declared_bins(&manifest);
    let locked = root.join(LOCK).is_file();
    let aborts = aborts_on_panic(&manifest);
    let in_release = release_invocations(&release);
    match matrix_targets(&workflow) {
        Ok(matrix) => report(
            &sources,
            &bins,
            &seeded,
            &harnessed,
            &matrix,
            &refused,
            &in_release,
            locked,
            aborts,
        ),
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
            &[],
            &[],
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
            &[],
            &[],
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
            &[],
            &[],
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
            &[],
            &[],
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
                &[],
                &[],
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

    #[test]
    fn a_dictionary_libfuzzer_would_refuse_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &[(String::from("sql_expression.dict"), 17)],
            &[],
            true,
            true,
        );
        assert!(verdict == Verdict::Fail, "a dictionary libFuzzer refuses must fail");
    }

    /// The oracle is a real libFuzzer binary, not this file's reading of libFuzzer's source: every
    /// row was run through `-dict=` with a one-line dictionary and the verdict recorded.
    #[test]
    fn the_shapes_libfuzzer_accepts_and_the_ones_it_refuses() {
        for accepted in [
            r#""abc""#,
            r#"name="abc""#,
            r#"name = "abc""#,
            r#"anything before the quote "abc""#,
            r#"name="ab"cd""#,
            r#"name="\\""#,
            r#"name="\"""#,
            r#"name="\x41""#,
            r#"date="\x22\\d{4}-\\d{2}-\\d{2}\x22""#,
        ] {
            assert!(dictionary_entry_is_parseable(accepted), "libFuzzer accepts {accepted}");
        }
        for refused in [
            "foo",                      // a bare unquoted token
            r#"""#,                     // one quote
            r#""a"#,                    // unterminated
            r#""""#,                    // an empty value
            r#"name="""#,               // an empty value with a name
            r#"name="abc" # trailing"#, // a `#` is only a comment at the head of a line
            r#"name="\q""#,             // not an escape
            r#"name="\""#,              // a trailing backslash
            r#"name="\x2""#,            // a short hex escape
            r#"name="\xZZ""#,           // not hex
            r#"name="\X41""#,           // the hex marker is lowercase-only
            r#"open={"\x22"#,           // the two committed lines that cost every scheduled run
            r#"close=\x22"}"#,
        ] {
            assert!(!dictionary_entry_is_parseable(refused), "libFuzzer refuses {refused}");
        }
    }

    #[test]
    fn refused_lines_are_reported_one_indexed_past_comments_and_blanks() {
        let dictionary = "# a header comment\n\nmetric=\"\\x22metric\\x22\"\nopen={\"\\x22\nclose=\\x22\"}\n";
        assert_eq!(
            unparseable_entries(dictionary),
            vec![4, 5],
            "the line numbers must be the ones libFuzzer's own error prints"
        );
        assert!(
            unparseable_entries("# only a comment\n\n\tsum=\"SUM(\"\n").is_empty(),
            "comments, blank lines and a valid entry are not defects"
        );
    }

    #[test]
    fn a_fuzz_invocation_in_the_release_workflow_fails() {
        let verdict = report(
            &set(&["sql_expression"]),
            &bins(&[("sql_expression", "fuzz_targets/sql_expression.rs")]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &set(&["sql_expression"]),
            &[],
            &[(41, "run-fuzz.sh")],
            true,
            true,
        );
        assert!(verdict == Verdict::Fail, "a fuzz job inside the release workflow must fail");
    }

    /// Every form a release job could reach the fuzzer by, and the one shape that must stay
    /// writable: the comment explaining why the split exists names the runner script itself.
    #[test]
    fn each_invocation_form_is_found_and_a_comment_about_one_is_not() {
        assert_eq!(
            release_invocations("jobs:\n  fuzz:\n    steps:\n      - run: bash nix/run-fuzz.sh run 900 x\n"),
            vec![(4, "run-fuzz.sh")]
        );
        assert_eq!(
            release_invocations("      - run: nix run .#fuzz -- run\n"),
            vec![(1, "nix run .#fuzz")]
        );
        assert_eq!(
            release_invocations("      - run: cargo fuzz run x\n"),
            vec![(1, "cargo fuzz")]
        );
        assert!(
            release_invocations("# fuzzing runs on the same tag via nix/run-fuzz.sh, in its own workflow\n").is_empty(),
            "a comment naming the runner is the explanation, not an invocation"
        );
    }

    #[test]
    fn the_release_workflow_in_this_tree_invokes_no_fuzzer() {
        let Some(root) = repo::root() else { return };
        let Ok(release) = std::fs::read_to_string(root.join(RELEASE_WORKFLOW)) else {
            panic!("{RELEASE_WORKFLOW} has to be readable - the rule fails closed on it");
        };
        assert!(
            release_invocations(&release).is_empty(),
            "the release workflow must not run the fuzzer - see RELEASE_WORKFLOW for why"
        );
    }
}
