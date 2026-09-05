//! Every directory under `examples/` is run by a test, or it does not exist.
//!
//! The plan's own rule for deployment variants is *"every deployment variant gets a WORKING
//! end-to-end example, and 'working' means a test runs it. Not a README describing what would
//! happen"* - and `github.com/telekom/sutura#130` measured what it looks like when nothing holds
//! it: `examples/multi-player` was a README and nothing else, describing a shape no binary here
//! could open. Both examples satisfy the rule today. Nothing made them.
//!
//! **A README is the failure this catches**, because a directory of prose under `examples/` reads
//! as something you can run. So the rule is: for each `examples/<name>`, some line of TEST CODE
//! must name `examples/<name>`.
//!
//! # What a line of test code is, and why neither half is written here
//!
//! Both halves are questions this repository has already answered once, so this gate asks rather
//! than answers them. A gate that scans a language has to LEX it, and the first version of this
//! one hand-rolled both halves and got both wrong in the same direction - permissively.
//!
//! * **What is code** is [`code_lines`](crate::serde_parse::scan::code_lines): every line with its
//!   comments and its multi-line string interiors blanked out. A doc comment naming a path is a
//!   claim; a line of code naming it is a read - and both examples here are named in prose several
//!   times over, so a rule that accepted a comment would have passed the README-only directory it
//!   exists to catch. Rust has THREE comment forms and `!line.starts_with("//")` saw one: measured
//!   on this tree, one sentence about a README-only variant written as `code; // ...` or inside
//!   `/* ... */` turned the verdict green. That lexer also blanks a string that SPANS LINES, which
//!   matters for a second reason - the whole evidence for the flagship example was once one line of
//!   a gate's `\`-continued remedy prose.
//! * **Which lines are test code** is [`regions::scope`](crate::causality::regions::scope): a
//!   dedicated `tests/` target, a `#![cfg(test)]` file, an out-of-line module a parent declares
//!   under `#[cfg(test)]`, or the brace-balanced range of each `#[cfg(test)]` item. Per LINE, which
//!   is the half a whole-file predicate got wrong twice over: `#[test]` appearing ANYWHERE in a
//!   file made the file's production half passing evidence, and five files in this tree qualified
//!   on the substring alone - the sharpest being a module doc that reads *"a file that adds no
//!   `#[test]`"*, classified as test code because the sentence contains the string.
//!
//! # The limits, next to the claim
//!
//! * **A mention is not an execution.** `let _ = "examples/foo";` in a test would satisfy this. The
//!   gate proves a test REACHES for the directory, not that it asserts anything about what came
//!   back - that is the reviewer's job, and `crates/sutura-cli/tests/example.rs` is the bar.
//! * **The literal has to sit on ONE line, anchored at a path boundary.** What passes is
//!   `examples/<name>` either starting a string literal or reached by `../` - the two shapes a
//!   repo-relative reach takes here. So `join("../../examples/multi-player/question.json")` is a
//!   read and `join("examples").join("multi-player")` is not, and neither is `format!` over a
//!   variant name, because `{` is outside the path charset and the name comes out empty. The
//!   failure message says which shape it looked for, so a false red is diagnosable rather than
//!   being read as the remedy it prints.
//! * **The anchor is a path boundary, not the repo root.** It rejects `my_examples/<name>`, a
//!   per-crate `crates/<name>/examples/` and a `format!("{tmp}/examples/<name>")`, all of which
//!   satisfied the unanchored scan. It cannot tell the repo's directory from a same-shaped path a
//!   test builds under a temp root: `tmp.join("examples/multi-player")` starts a literal and counts.
//! * **`#[ignore]` is invisible.** The scan is over lines, and nothing here resolves a line to the
//!   test function containing it, so `#[ignore]` on the only cell that reaches a variant keeps the
//!   verdict green. Held by review, not by this gate.
//! * **A variant is a directory git would publish**, read off the same listing as the evidence -
//!   see [`variants`] for the two ways the filesystem disagreed with it.
//! * **It says nothing about what is IN the directory.** An example whose data was deleted fails
//!   the test that reads it, in `nextest`, which is the venue for that.
//!
//! Four fail-closed arms, because this gate exists to stop a directory going unwatched and every
//! one of them would otherwise read as a clean tree: no variant, no line of test code, an
//! exclusion that removed nothing, and a file the scan could not read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::Verdict;
use crate::causality::regions;
use crate::repo;
use crate::serde_parse::scan::code_lines;

/// The directory whose children are the deployment variants, with the separator that makes it a
/// path prefix. One constant: a scan for the segment and a message naming the directory must not
/// drift apart.
const EXAMPLES: &str = "examples/";

/// The crate this gate lives in, whose files are excluded from its own scan.
///
/// Not tidiness - it is the difference between a gate and a mirror, and the mirror was live one
/// file over. This module's own fixtures hold paths under `examples/`, and `changes.rs` holds
/// `examples/single-player/...` as a classification fixture; both are test code by every rule this
/// gate uses. Measured: repointing all 23 `crates/**/*.rs` mentions of `examples/single-player`
/// left the verdict green on those two fixtures alone. No gate's test runs a deployment example,
/// so the crate is the honest scope rather than one file.
///
/// DERIVED rather than written down, because a path constant is held by recall and fails OPEN when
/// it stops matching: `mv xtask/src/examples.rs xtask/src/examples/mod.rs` still compiles, and it
/// silently re-admitted this file's fixtures with the file count as the only tell. The manifest
/// directory's own last segment cannot disagree with where this file lives, and [`run`] fails
/// closed if it excludes nothing at all.
fn gate_crate() -> Option<&'static str> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
}

/// Is this path Rust? Case-insensitive, for the reason `docs::is_markdown` gives: half of this
/// repo is developed on a filesystem that does not distinguish `.RS` from `.rs`.
fn is_rust(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Is this path inside `dir`, as a whole leading path segment?
fn is_under(rel: &str, dir: &str) -> bool {
    rel.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// Which files reach a variant, and the first line in each that names it.
type Reaches = BTreeMap<String, BTreeMap<String, usize>>;

/// What the scan found.
struct Evidence {
    /// Variant name to the files whose test code reaches for it.
    reaches: Reaches,
    /// Files carrying at least one line of test code.
    test_files: usize,
}

/// Every `examples/<name>` a line of test code reaches for, and where from.
///
/// `files` maps a repo-relative path to that file's RAW text, and the blanking happens HERE rather
/// than in the caller - so a unit test over this function covers the blanking too, which is the
/// difference between testing the rule and testing the plumbing above it. One map for the whole
/// scan rather than a file at a time, because `regions::scope` follows a `#[cfg(test)] mod x;` into
/// the PARENT file: an out-of-line test module is test code and nothing inside the file says so.
fn evidence(files: &BTreeMap<String, String>) -> Evidence {
    let blanked: BTreeMap<String, String> = files
        .iter()
        .map(|(rel, text)| (rel.clone(), code_lines(text).join("\n")))
        .collect();
    // The owned `String` is the `PostImage` port's signature rather than a borrow-checker dodge,
    // and only the parent-module lookup asks for one.
    let read = |path: &str| blanked.get(path).cloned();
    let mut reaches: Reaches = BTreeMap::new();
    let mut test_files = 0_usize;
    for (rel, text) in &blanked {
        let tests = regions::scope(rel, &read);
        let mut carries_test_code = false;
        for (index, line) in text.lines().enumerate() {
            let number = index.saturating_add(1);
            if !tests.covers(number) {
                continue;
            }
            carries_test_code = true;
            for name in reaches_for(line) {
                reaches.entry(name).or_default().entry(rel.clone()).or_insert(number);
            }
        }
        if carries_test_code {
            test_files = test_files.saturating_add(1);
        }
    }
    Evidence { reaches, test_files }
}

/// Every `examples/<name>` this line of code reaches for.
///
/// `find` in a loop rather than once, because a second path on the same line used to be invisible.
fn reaches_for(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = 0_usize;
    while let Some(offset) = line.get(at..).and_then(|rest| rest.find(EXAMPLES)) {
        let start = at.saturating_add(offset);
        at = start.saturating_add(EXAMPLES.len());
        if !anchored(line.get(..start).unwrap_or_default()) {
            continue;
        }
        let name: String = line
            .get(at..)
            .unwrap_or_default()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
            .collect();
        if !name.is_empty() {
            found.push(name);
        }
    }
    found
}

/// Is a path boundary in front of the match, of one of the two shapes a repo-relative reach takes?
///
/// The start of a string literal, or a `../` walking up out of `CARGO_MANIFEST_DIR`. Measured over
/// this workspace: those two cover every reach in it, and nothing else does. `#` is deliberately
/// not a comment marker anywhere here - it opens one in a shell and an ATTRIBUTE in Rust, and
/// `#[path = "../../examples/x/mod.rs"]` is a real reach.
fn anchored(before: &str) -> bool {
    before.is_empty() || before.ends_with('"') || before.ends_with("../")
}

/// The deployment variants, read off the same listing the evidence is read from.
///
/// One authority for both halves of the gate. `read_dir` was the other candidate and disagreed
/// with the scan in two directions, both reproduced: `mkdir examples/x` was a local FAILURE that
/// the git-derived nix sandbox and CI could not see, because git tracks no empty directory - a red
/// no venue reproduces; and a gitignored or symlinked child was a variant that can never be
/// published, while `hygiene` is a pre-commit hook, so it blocked every commit over a path git
/// will never publish. A directory git would not publish is not one a reader can find, which is
/// the failure this gate exists for.
fn variants(files: &[String]) -> BTreeSet<String> {
    files
        .iter()
        .filter_map(|rel| rel.strip_prefix(EXAMPLES))
        .filter_map(|rest| rest.split_once('/'))
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, _)| String::from(name))
        .collect()
}

/// What the read left behind, beside the evidence itself. Named for the scan rather than
/// `Read`, which is a trait everybody already knows.
struct Scan {
    /// The gate's own crate directory, left out of the scan.
    crate_dir: &'static str,
    /// How many files that removed. Zero is a broken exclusion, not a clean tree.
    excluded: usize,
    /// Files the scan could not read, each with the error.
    unreadable: Vec<String>,
}

/// The verdict, as a list of problems. Pure, so every arm is testable without a repo - which is
/// the point: an arm reached only through the filesystem is an arm nothing holds.
fn problems(variants: &BTreeSet<String>, evidence: &Evidence, scan: &Scan) -> Vec<String> {
    let mut problems = Vec::new();
    let crate_dir = scan.crate_dir;
    if scan.excluded == 0 {
        // FAIL CLOSED. The self-exclusion is what keeps this a gate rather than a mirror, and it
        // fails OPEN when it stops matching - so it proves it removed something.
        problems.push(format!(
            "excluded no file under `{crate_dir}/` - this gate's own fixtures name paths under `{EXAMPLES}` and would satisfy it, so an exclusion that matches nothing is a broken scan"
        ));
    }
    for entry in &scan.unreadable {
        // NOT swallowed. `test_files` is a count over the files that WERE read, so it cannot see
        // this: one unreadable test file made the gate blame the tree for the reach it had missed.
        problems.push(format!(
            "could not read {entry} - a variant this scan calls unreached may be reached from the part it could not read"
        ));
    }
    if variants.is_empty() {
        problems.push(format!(
            "no directory under `{EXAMPLES}` - this gate watches deployment variants and found none, so the scan is broken rather than the tree clean"
        ));
        return problems;
    }
    if evidence.test_files == 0 {
        // FAIL CLOSED, and instead of the arm below rather than beside it: with no line of test
        // code read, every variant is unreached and that message would blame the tree for a scan
        // that never ran.
        problems.push(String::from(
            "read no line of test code out of the workspace - the scan is broken, not the examples",
        ));
        return problems;
    }
    for variant in variants.iter().filter(|variant| !evidence.reaches.contains_key(*variant)) {
        problems.push(format!(
            "`{EXAMPLES}{variant}/` is named by no test - a directory of prose under `{EXAMPLES}` reads as something a reader can run, so give it a test that reaches for it or delete it until the deployment it describes exists. Looked for: the literal `{EXAMPLES}{variant}` on one line of test code, starting a string literal or reached by `../`"
        ));
    }
    problems
}

/// One variant's evidence, for the verdict.
///
/// The verdict used to state the SCAN's size - `each reached by one of N test file(s)` - which is a
/// count over a set that is not the claim being made. Measured: `single-player` was reached from 16
/// files and `multi-player` from exactly one, and the line stated neither, so thinning the evidence
/// for a variant down to two fixtures inside this crate printed a verdict byte-identical to the
/// healthy one. The file is named when there is only one, because one is the state worth reading.
fn report(variant: &str, from: &BTreeMap<String, usize>) -> String {
    match from.iter().next() {
        Some((rel, line)) if from.len() == 1 => format!("{variant}: reached from 1 file - {rel}:{line}"),
        _ => format!("{variant}: reached from {} file(s)", from.len()),
    }
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-examples: could not determine the repo root");
        return Verdict::Fail;
    };
    let Some(crate_dir) = gate_crate() else {
        eprintln!("xtask check-examples: could not derive this gate's own crate directory");
        return Verdict::Fail;
    };

    let mut sources: BTreeMap<String, String> = BTreeMap::new();
    let mut scan = Scan {
        crate_dir,
        excluded: 0,
        unreadable: Vec::new(),
    };
    for rel in files.iter().filter(|rel| is_rust(rel)) {
        if is_under(rel, crate_dir) {
            scan.excluded = scan.excluded.saturating_add(1);
            continue;
        }
        match std::fs::read_to_string(root.join(rel)) {
            Ok(text) => {
                sources.insert(rel.clone(), text);
            }
            Err(error) => scan.unreadable.push(format!("`{rel}`: {error}")),
        }
    }

    let found = variants(&files);
    let evidence = evidence(&sources);
    let problems = problems(&found, &evidence, &scan);

    if problems.is_empty() {
        println!(
            "xtask check-examples: ok - {} variant(s) under {EXAMPLES}, from {} file(s) of test code ({} under `{crate_dir}/` excluded)",
            found.len(),
            evidence.test_files,
            scan.excluded
        );
        let nowhere = BTreeMap::new();
        for variant in &found {
            println!("  {}", report(variant, evidence.reaches.get(variant).unwrap_or(&nowhere)));
        }
        return Verdict::Pass;
    }

    eprintln!("xtask check-examples: FAILED");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("An example is a promise that something runs, and a test is the only thing that makes it.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{Evidence, Scan, anchored, evidence, is_under, problems, reaches_for, report, variants};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    /// The scan's input: repo-relative paths to RAW file text, exactly what `run` hands it.
    fn files(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(rel, text)| (String::from(*rel), String::from(*text)))
            .collect()
    }

    fn reached(entries: &[(&str, &str)]) -> BTreeSet<String> {
        evidence(&files(entries)).reaches.keys().cloned().collect()
    }

    /// A scan that removed something and dropped nothing, so the other arms are what is under test.
    fn whole() -> Scan {
        Scan {
            crate_dir: "xtask",
            excluded: 73,
            unreadable: Vec::new(),
        }
    }

    #[test]
    fn a_variant_reached_from_test_code_is_clean() {
        let found = evidence(&files(&[(
            "crates/x/tests/example.rs",
            "fn root() { Path::new(\"../../examples/a-variant\") }",
        )]));
        assert_eq!(found.reaches.keys().cloned().collect::<BTreeSet<_>>(), set(&["a-variant"]));
        assert_eq!(found.test_files, 1);
        assert!(problems(&set(&["a-variant"]), &found, &whole()).is_empty());
    }

    #[test]
    fn a_variant_only_a_comment_mentions_is_a_problem() {
        // The README-only directory this gate exists for was already DISCUSSED in the test that
        // could not run it, so a rule accepting a comment would have passed it.
        let found = evidence(&files(&[(
            "crates/x/tests/example.rs",
            "//! `Path::new(\"../../examples/a-variant\")` is the deployment shape.\n#[test]\nfn t() {}\n",
        )]));
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        let said = problems(&set(&["a-variant"]), &found, &whole());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first().is_some_and(|first| first.contains("named by no test")),
            "{said:?}"
        );
        // The remedy names the shape that was looked for, so a false red is diagnosable.
        assert!(said.first().is_some_and(|first| first.contains("Looked for:")), "{said:?}");
    }

    #[test]
    fn a_comment_is_not_code_in_any_of_rusts_three_forms() {
        // Every fixture below wraps a path the ANCHOR would accept, so the blanking is the only
        // thing that can make it not a reach. THAT IS THE POINT: a fixture whose path is
        // unanchored passes this test with the lexer taken out, which is the vacuous shape the
        // review that produced this test was about. Measured - removing `code_lines` reddens it.
        for source in [
            "// let p = \"examples/x\";\n",
            "    /// `Path::new(\"../../examples/x\")` is the corpus.\n",
            "    //! see `Path::new(\"../../examples/x\")`\n",
            // The two forms the predicate this replaced could not see at all.
            "let n = 1; // was let p = \"examples/x\";\n",
            "/* let p = \"examples/x\"; */\n",
            "/*\n * let p = \"examples/x\";\n */\n",
            "/* /* let p = \"examples/x\"; */ */\n",
            // A string that SPANS LINES is prose about a path, not a read of one - which is what
            // the flagship example's whole evidence once was.
            "const R: &str = \"restore ../../examples/x \\\n    and rerun\";\n",
        ] {
            assert!(reached(&[("crates/x/tests/t.rs", source)]).is_empty(), "{source}");
        }
        // Code that names a path still counts, in both of the shapes this tree writes.
        assert_eq!(reached(&[("crates/x/tests/t.rs", "let p = \"examples/x\";\n")]), set(&["x"]));
        assert_eq!(
            reached(&[("crates/x/tests/t.rs", "#[path = \"../../examples/x/mod.rs\"]\n")]),
            set(&["x"])
        );
    }

    #[test]
    fn a_reach_has_to_be_anchored_at_a_path_boundary() {
        assert!(anchored(""), "the start of a line");
        assert!(anchored("let p = \""), "the start of a string literal");
        assert!(anchored("Path::new(\"../../"), "a walk up out of the manifest directory");
        assert!(!anchored("let p = \"my_"), "my_examples/ is a different directory");
        assert!(
            !anchored("format!(\"{tmp}/"),
            "a synthetic tree under a temp root is not this one"
        );
        assert!(
            !anchored("\"crates/x/"),
            "cargo's per-crate examples/ convention is not this one"
        );
        // End to end, because the anchor is only worth what the scan does with it.
        assert!(reached(&[("crates/x/tests/t.rs", "let p = \"/tmp/s/my_examples/x/q.json\";\n")]).is_empty());
        assert_eq!(
            reached(&[("crates/x/tests/t.rs", "let p = \"../../examples/x/q.json\";\n")]),
            set(&["x"])
        );
    }

    #[test]
    fn only_the_test_lines_of_a_file_are_evidence() {
        // THE defect this closes: `#[test]` anywhere in a file made the WHOLE file evidence, so a
        // dead `const` in the production half of any `src/*.rs` carrying a unit-test module
        // satisfied the gate.
        let source = "const SHAPE: &str = \"examples/x\";\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n";
        assert!(
            reached(&[("crates/x/src/lib.rs", source)]).is_empty(),
            "production code in a file with a test module is not test code"
        );
        let inside = "#[cfg(test)]\nmod tests {\n    const SHAPE: &str = \"examples/x\";\n    #[test]\n    fn t() {}\n}\n";
        assert_eq!(
            reached(&[("crates/x/src/lib.rs", inside)]),
            set(&["x"]),
            "the same line inside the test module is"
        );
    }

    #[test]
    fn a_file_whose_only_test_marker_is_prose_is_not_test_code() {
        // Measured on this tree: five files were test code by the `#[test]` substring and by
        // nothing else, the sharpest being a module doc reading "a file that adds no `#[test]`".
        let prose =
            "//! A file that adds no `#[test]` is one the causality gate may revert.\nconst SHAPE: &str = \"examples/x\";\n";
        let found = evidence(&files(&[("crates/x/src/notes.rs", prose)]));
        assert!(found.reaches.is_empty(), "{:?}", found.reaches);
        assert_eq!(found.test_files, 0, "a sentence about a test is not a test");
    }

    #[test]
    fn several_variants_on_one_line_are_all_read() {
        // `find` used to be read once per line, so a second path on the same line was invisible.
        assert_eq!(
            reaches_for("let both = (\"examples/one/data\", \"examples/two\");"),
            vec![String::from("one"), String::from("two")]
        );
    }

    #[test]
    fn a_variant_is_a_directory_git_would_publish() {
        // The same listing the evidence comes from, so the two halves cannot disagree. An empty
        // directory is in NO listing, which is exactly why the filesystem was the wrong authority.
        assert_eq!(
            variants(&[
                String::from("examples/README.md"),
                String::from("examples/a-variant/README.md"),
                String::from("examples/b-variant/data/rows.csv"),
                String::from("crates/x/examples/not-a-variant/main.rs"),
            ]),
            set(&["a-variant", "b-variant"]),
            "a file directly under examples/ is not a variant, and a per-crate examples/ is not ours"
        );
        assert!(
            variants(&[String::from("examples/a-variant")]).is_empty(),
            "a file, not a directory"
        );
    }

    #[test]
    fn the_gate_excludes_its_own_crate_and_says_so_when_it_excluded_nothing() {
        assert!(is_under("xtask/src/examples.rs", "xtask"));
        assert!(
            is_under("xtask/src/examples/mod.rs", "xtask"),
            "a move inside the crate stays excluded"
        );
        assert!(
            is_under("xtask/src/changes.rs", "xtask"),
            "the classification fixture next door too"
        );
        assert!(!is_under("xtaskish/src/lib.rs", "xtask"), "a whole segment, not a prefix");
        assert!(!is_under("crates/x/tests/t.rs", "xtask"));
    }

    #[test]
    fn the_verdict_states_each_variants_evidence_and_names_a_lone_file() {
        let mut from = BTreeMap::new();
        from.insert(String::from("crates/x/tests/t.rs"), 69_usize);
        assert_eq!(
            report("multi-player", &from),
            "multi-player: reached from 1 file - crates/x/tests/t.rs:69"
        );
        from.insert(String::from("crates/y/tests/u.rs"), 7_usize);
        assert_eq!(report("multi-player", &from), "multi-player: reached from 2 file(s)");
    }

    #[test]
    fn an_examples_directory_with_no_variants_is_a_broken_scan() {
        let empty = Evidence {
            reaches: BTreeMap::new(),
            test_files: 1,
        };
        let said = problems(&BTreeSet::new(), &empty, &whole());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first().is_some_and(|first| first.contains("the scan is broken")),
            "{said:?}"
        );
    }

    #[test]
    fn no_line_of_test_code_blames_the_scan_rather_than_the_tree() {
        let nothing = Evidence {
            reaches: BTreeMap::new(),
            test_files: 0,
        };
        let said = problems(&set(&["a-variant"]), &nothing, &whole());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first().is_some_and(|first| first.contains("read no line of test code")),
            "{said:?}"
        );
        assert!(
            said.iter().all(|problem| !problem.contains("named by no test")),
            "the unreached message would blame the tree for a scan that never ran: {said:?}"
        );
    }

    #[test]
    fn an_exclusion_that_removed_nothing_is_a_broken_scan() {
        // THE direction that made this worth a mechanism: a self-exclusion that stops matching
        // fails OPEN, into exactly the mirror it exists to prevent, and the only tell was a file
        // count moving by one.
        let reached = evidence(&files(&[(
            "crates/x/tests/example.rs",
            "fn root() { Path::new(\"../../examples/a-variant\") }",
        )]));
        let nothing_removed = Scan {
            crate_dir: "xtask",
            excluded: 0,
            unreadable: Vec::new(),
        };
        let said = problems(&set(&["a-variant"]), &reached, &nothing_removed);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("excluded no file under `xtask/`")),
            "{said:?}"
        );
    }

    #[test]
    fn a_file_the_scan_could_not_read_is_a_problem_rather_than_a_silent_drop() {
        // `.ok()` swallowed this, and the `test_files` floor cannot catch it: the count is over
        // the files that WERE read, so a partial read never fires the arm that exists for it.
        let reached = evidence(&files(&[(
            "crates/x/tests/example.rs",
            "fn root() { Path::new(\"../../examples/a-variant\") }",
        )]));
        let partial = Scan {
            crate_dir: "xtask",
            excluded: 73,
            unreadable: vec![String::from("`crates/y/tests/t.rs`: stream did not contain valid UTF-8")],
        };
        let said = problems(&set(&["a-variant"]), &reached, &partial);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said.first()
                .is_some_and(|first| first.contains("could not read `crates/y/tests/t.rs`")),
            "{said:?}"
        );
    }
}
