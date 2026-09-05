//! Every directory under `examples/` is run by a test, or it does not exist.
//!
//! The plan's own rule for deployment variants is *"every deployment variant gets a WORKING
//! end-to-end example, and 'working' means a test runs it. Not a README describing what would
//! happen"* - and `github.com/telekom/sutura#130` measured what it looks like when nothing holds
//! it: `examples/multi-player` was a README and nothing else, describing a shape no binary here
//! could open. Both examples satisfy the rule today. Nothing made them.
//!
//! **A README is the failure this catches**, because a directory of prose under `examples/` reads
//! as something you can run. So the rule is: for each `examples/<name>`, some TEST file must name
//! `examples/<name>` on a line that is not a comment.
//!
//! # Why the mention has to be in code
//!
//! A doc comment naming a path is a claim; a line of code naming it is a read. Both examples here
//! are named in prose several times over - `crates/sutura-catalog-datahub/tests/multi_player.rs`
//! opens with four paragraphs about `examples/multi-player` - so a rule that accepted a comment
//! would have passed the README-only directory it exists to catch, since the README was already
//! discussed in the test that could not run it. Comment lines are therefore dropped before the
//! search, which makes the passing evidence a `join(..)`, an `include_str!` or an argument vector.
//!
//! # The limits, next to the claim
//!
//! * **A mention is not an execution.** `let _ = "examples/foo";` in a test would satisfy this. The
//!   gate proves a test REACHES for the directory, not that it asserts anything about what came
//!   back - that is the reviewer's job, and `crates/sutura-cli/tests/example.rs` is the bar.
//! * **The test set is decided by path and by attribute**, not by what the harness runs: a `.rs`
//!   file under a `tests/` directory, or one containing `#[test]`. A fixture builder that holds the
//!   path while its assertions live elsewhere still counts, which is deliberate -
//!   `crates/sutura-serve/tests/served/harness.rs` is exactly that file.
//! * **It says nothing about what is IN the directory.** An example whose data was deleted fails
//!   the test that reads it, in `nextest`, which is the venue for that.
//!
//! Fails closed in both directions: no `examples/` subdirectory and no test file are each a broken
//! scan rather than a clean tree, because this gate exists to stop a directory going unwatched.

use std::collections::BTreeSet;
use std::path::Path;

use crate::Verdict;
use crate::repo;

/// The directory whose children are the deployment variants.
const EXAMPLES: &str = "examples";

/// This file, which is excluded from its own scan.
///
/// Not tidiness - it is the difference between a gate and a mirror. The unit tests below hold
/// fixture paths under `examples/`, and they are test code by every rule this gate uses, so
/// scanning them would let the gate satisfy itself: the real test could stop naming a variant and
/// this file's fixtures would keep the verdict green. `check-guidance` excludes `.rs` from its own
/// phrase scan for exactly this reason, and states it the same way.
const SELF: &str = "xtask/src/examples.rs";

/// Is this path Rust? Case-insensitive, for the reason `docs::is_markdown` gives: half of this
/// repo is developed on a filesystem that does not distinguish `.RS` from `.rs`.
fn is_rust(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// Is this a line of code rather than a comment? See the module doc for why it matters.
///
/// Only `//`, which covers `///` and `//!` too. **Not `#`**, which opens a comment in a shell and
/// an ATTRIBUTE in Rust - and this reads Rust, so treating `#[path = "../../examples/x/mod.rs"]` as
/// a comment would have made a real reach invisible and reddened a variant that is reached. A
/// mention inside a `/* */` block counts as code, which is the permissive direction: this gate's
/// failure mode should be a variant nobody reaches, not a false accusation.
fn is_code(line: &str) -> bool {
    !line.trim_start().starts_with("//")
}

/// Is this file test code, by the rule the module doc states?
fn is_test_file(rel: &str, text: &str) -> bool {
    is_rust(rel) && (rel.contains("/tests/") || rel.ends_with("/tests.rs") || text.contains("#[test]"))
}

/// Every `examples/<name>` a test file reaches for in code.
fn reached(files: &[(String, String)]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (rel, text) in files {
        if !is_test_file(rel, text) {
            continue;
        }
        for line in text.lines().filter(|line| is_code(line)) {
            let mut rest = line;
            while let Some(at) = rest.find("examples/") {
                let tail = rest.get(at.saturating_add(EXAMPLES.len()).saturating_add(1)..).unwrap_or("");
                let name: String = tail
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
                    .collect();
                if !name.is_empty() {
                    found.insert(name);
                }
                rest = tail;
            }
        }
    }
    found
}

/// The verdict, as a list of problems. Pure, so every arm is testable without a repo.
fn problems(variants: &BTreeSet<String>, reached: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    if variants.is_empty() {
        problems.push(format!(
            "no directory under `{EXAMPLES}/` - this gate watches deployment variants and found none, so the scan is broken rather than the tree clean"
        ));
        return problems;
    }
    for variant in variants.difference(reached) {
        problems.push(format!(
            "`{EXAMPLES}/{variant}/` is named by no test - a directory of prose under `{EXAMPLES}/` reads as something a reader can run, so give it a test that reaches for it or delete it until the deployment it describes exists"
        ));
    }
    problems
}

/// The immediate subdirectories of `examples/`.
fn variants(root: &Path) -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(root.join(EXAMPLES)) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-examples: could not determine the repo root");
        return Verdict::Fail;
    };

    let sources: Vec<(String, String)> = files
        .iter()
        .filter(|rel| is_rust(rel) && rel.as_str() != SELF)
        .filter_map(|rel| std::fs::read_to_string(root.join(rel)).ok().map(|text| (rel.clone(), text)))
        .collect();
    let tests = sources.iter().filter(|(rel, text)| is_test_file(rel, text)).count();
    let found = variants(&root);
    let mut problems = problems(&found, &reached(&sources));
    if tests == 0 {
        // FAIL CLOSED. With no test file read, every variant is unreached and the messages above
        // would blame the tree for a scan that never ran.
        problems.push(String::from(
            "read no test file out of the workspace - the scan is broken, not the examples",
        ));
    }

    if problems.is_empty() {
        println!(
            "xtask check-examples: ok - {} variant(s) under {EXAMPLES}/, each reached by one of {tests} test file(s)",
            found.len()
        );
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
    use std::collections::BTreeSet;

    use super::{is_code, is_test_file, problems, reached};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    fn files(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(rel, text)| (String::from(*rel), String::from(*text)))
            .collect()
    }

    #[test]
    fn a_variant_reached_from_test_code_is_clean() {
        let read = reached(&files(&[(
            "crates/x/tests/example.rs",
            "fn root() { Path::new(\"../../examples/a-variant\") }",
        )]));
        assert_eq!(read, set(&["a-variant"]));
        assert!(problems(&set(&["a-variant"]), &read).is_empty());
    }

    #[test]
    fn a_variant_only_a_comment_mentions_is_a_problem() {
        // The README-only directory this gate exists for was already DISCUSSED in the test that
        // could not run it, so a rule accepting a comment would have passed it.
        let read = reached(&files(&[(
            "crates/x/tests/example.rs",
            "//! `examples/a-variant/README.md` is the deployment shape.\n#[test]\nfn t() {}\n",
        )]));
        assert!(read.is_empty(), "{read:?}");
        let found = problems(&set(&["a-variant"]), &read);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("named by no test"), "{found:?}");
    }

    #[test]
    fn what_counts_as_test_code() {
        assert!(is_test_file("crates/x/tests/example.rs", ""));
        assert!(is_test_file("crates/x/tests/served/harness.rs", ""));
        assert!(is_test_file("crates/x/src/lib/tests.rs", ""));
        // A unit test module inside ordinary source counts by its attribute.
        assert!(is_test_file(
            "crates/x/src/lib.rs",
            "#[cfg(test)]\nmod tests { #[test] fn t() {} }"
        ));
        // Ordinary source does not, and neither does a page that happens to name a path.
        assert!(!is_test_file("crates/x/src/lib.rs", "fn main() {}"));
        assert!(!is_test_file("examples/a-variant/README.md", "#[test]"));
    }

    #[test]
    fn a_comment_is_not_code_and_an_attribute_is() {
        assert!(!is_code("// examples/x"));
        assert!(!is_code("    //! examples/x"));
        assert!(!is_code("    /// examples/x"));
        assert!(is_code("    let p = \"examples/x\";"));
        // `#` opens a comment in a shell and an attribute in Rust. This reads Rust.
        assert!(is_code("#[path = \"../../examples/x/mod.rs\"]"));
    }

    #[test]
    fn several_variants_on_one_line_are_all_read() {
        // `find` used to be read once per line, so a second path on the same line was invisible.
        let read = reached(&files(&[(
            "crates/x/tests/t.rs",
            "let both = (\"examples/one/data\", \"examples/two\");",
        )]));
        assert_eq!(read, set(&["one", "two"]));
    }

    #[test]
    fn an_examples_directory_with_no_variants_is_a_broken_scan() {
        let found = problems(&BTreeSet::new(), &set(&["a-variant"]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("the scan is broken"), "{found:?}");
    }
}
