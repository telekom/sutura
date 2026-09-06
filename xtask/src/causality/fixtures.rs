//! The harness three of this gate's test modules were each carrying a copy of.
//!
//! A post-image reader, a line builder, and the two shapes an assertion about a RUN needs - a
//! filterset's coverage and the keys a file's tests land under. It lives in its own file for the
//! reason the gate itself teaches: a file with no `#[test]` is one the gate may revert, so a
//! harness is exactly what may move out of a test-bearing file, and assertions are exactly what
//! may not. `named` and `scoped` moved here from `super::base` when that file reached the
//! unexemptable 1000-line cap, which is that rule applied to this gate's own source.

use crate::causality::coverage::Coverage;
use crate::causality::diff::{ChangedFile, RemovedLine};
use crate::causality::place::AddedTest;
use crate::causality::regions::AddedLine;
use crate::causality::scoped::Scan;

/// A post-image reader over a fixed set of files, standing in for the working tree.
pub(crate) fn tree(files: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|&(path, text)| (String::from(path), String::from(text)))
        .collect();
    move |wanted: &str| owned.iter().find(|(path, _)| path == wanted).map(|(_, text)| text.clone())
}

/// Added lines numbered consecutively from `first`.
pub(crate) fn added_from(first: usize, texts: &[&str]) -> Vec<AddedLine> {
    texts
        .iter()
        .enumerate()
        .map(|(offset, text)| AddedLine::new(first + offset, *text))
        .collect()
}

/// A manifest declaring one package, as the post-image reader hands it back.
///
/// Three test modules were spelling this three ways, and only the shape `changes::package_name`
/// parses matters - so a fourth spelling drifting is the thing this removes.
pub(crate) fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion.workspace = true\n")
}

/// A changed file whose added lines run consecutively from `first`.
pub(crate) fn changed(path: &str, first: usize, texts: &[&str]) -> ChangedFile {
    ChangedFile {
        path: String::from(path),
        added: added_from(first, texts),
        removed: Vec::new(),
    }
}

/// A changed file that also REMOVED lines, added from `at` and removed from `was`.
///
/// TWO starting lines because the two sides are numbered in different images: an added line has a
/// post-image number and a removed one has a pre-image number. A fixture that shared one would be
/// asserting against a shape `super::diff` cannot produce.
pub(crate) fn changed_removing(path: &str, at: usize, added: &[&str], was: usize, removed: &[&str]) -> ChangedFile {
    ChangedFile {
        path: String::from(path),
        added: added_from(at, added),
        removed: removed
            .iter()
            .enumerate()
            .map(|(offset, text)| RemovedLine {
                before: was + offset,
                text: String::from(*text),
            })
            .collect(),
    }
}

/// A filterset that NAMED `count` tests, out of `count` - the shape an inconclusive run has.
pub(crate) fn named(count: usize) -> Coverage {
    Coverage::Measured {
        measured: count,
        unmeasured: Vec::new(),
        not_runnable: Vec::new(),
    }
}

/// The tests under test, as if one file in `package` had added each of `names`.
///
/// Through `Scan::of` rather than a hand-built key, so what an assertion measures is the key the
/// gate actually builds from a diff - a fixture that constructed one directly would pass while the
/// extractor produced something else.
pub(crate) fn scoped(package: &str, file: &str, names: &[&str]) -> Vec<AddedTest> {
    let lines: Vec<String> = names
        .iter()
        .flat_map(|name| [String::from("#[test]"), format!("fn {name}() {{}}")])
        .collect();
    let text = format!("{}\n", lines.join("\n"));
    let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
    let dir = file
        .split_once("/src/")
        .or_else(|| file.split_once("/tests/"))
        .expect("a package directory")
        .0;
    let read = tree(&[(file, &text), (&format!("{dir}/Cargo.toml"), &manifest(package))]);
    match Scan::of(&[changed(file, 1, &borrowed)], &[String::from(file)], &read) {
        Scan::Runnable(found) => found.tests().to_vec(),
        other => panic!("expected runnable tests, got {other:?}"),
    }
}

/// The audit record from #276, as the branch that first exposed the wide run declared it.
pub(crate) fn audit_record() -> Vec<AddedTest> {
    scoped(
        "sutura-cli",
        "crates/sutura-cli/src/audit.rs",
        &["a_refused_question_is_recorded_and_names_the_refusal"],
    )
}

/// The base run that reported the false green, as nextest printed it. Two tier-backed cells failed
/// after 86 of 1810 tests and the branch's own test never ran.
pub(crate) const UNRELATED_RED: &str = concat!(
    "    Starting 1810 tests across 47 binaries\n",
    "        FAIL [   0.313s] (86/1810) sutura-app::differential tests::postgres::sums_by_month\n",
    "        FAIL [   0.204s] (87/1810) sutura-app::differential tests::postgres::one_row_per_month\n",
    "  Cancelling due to test failure: \n",
    "     Summary [   4.118s] 87 tests run: 85 passed, 2 failed, 1723 skipped\n",
    "error: test run failed\n",
);
