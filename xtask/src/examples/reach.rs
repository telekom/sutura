//! Which lines of test code reach which deployment variant, and one variant's evidence for the
//! verdict.
//!
//! Split out of `examples.rs` (`github.com/telekom/sutura#414`) once that file's own migration onto
//! [`super::corpus::Corpus::read`] pushed it over the 1000-line cap - a concept-named module rather
//! than a `part2`, because "which lines reach which example" is already a term of art in this gate
//! (`corpus::Reaches` does not exist; this one does) and is a self-contained scan with no
//! dependency on the walk or the manifest, unlike everything left in `examples.rs`.

use std::collections::{BTreeMap, BTreeSet};

use super::corpus::{reaches_for, resolved};
use crate::causality::{attributes, regions};
use crate::serde_parse::scan::code_lines;

/// Which files reach a variant, and the first line in each that names it.
pub(super) type Reaches = BTreeMap<String, BTreeMap<String, usize>>;

/// What the scan found.
#[derive(Debug, Default)]
pub(super) struct Evidence {
    /// Variant name to the files whose test code reaches for it.
    pub(super) reaches: Reaches,
    /// Files carrying at least one line of test code a run in this venue reaches.
    pub(super) test_files: usize,
    /// Tests declared across those files that a run reaches.
    ///
    /// The SECOND number, and it is here because one number cannot witness this scan: the count
    /// above is over files and this one is over the attributes inside them, so the vocabulary
    /// silently ceasing to match leaves `test_files` at its healthy line and collapses this to
    /// zero. [`super::problems`] refuses on that pair rather than printing it.
    pub(super) declared: usize,
    /// Files whose every declared test is `#[ignore]`d, from which nothing was taken as evidence.
    pub(super) all_ignored: usize,
    /// Files carrying a test-declaring attribute whose item could not be resolved, with the count.
    ///
    /// A REFUSAL, not a note: a test this cannot resolve is one this cannot say runs, so every
    /// line in that file is evidence of unknown reach. Measured over this tree before it was made
    /// one - see [`super::run`]'s verdict, which states the number it resolved.
    pub(super) unresolvable: Vec<String>,
    /// Cells whose run is decided by an attribute the scan cannot evaluate, so their lines are
    /// not evidence, as `<attribute> over the test at line <n> in <file>`.
    ///
    /// STATED rather than refused, and that is a correction to
    /// `github.com/telekom/sutura#400`'s own remedy 3 rather than a softening of it: routing these
    /// to the `unresolvable` refusal turns this gate RED on the healthy tree, because eight cells
    /// in `crates/sutura-cli/src/sources/bigquery.rs` and `crates/sutura-cli/src/serve/tests/bigquery.rs` are
    /// legitimately written under `#[cfg(feature = "bigquery")]` or its negation. Fail-closed for
    /// the CLAIM is what this gate needs: the cell is not a run this venue reaches, so a variant
    /// whose only reach sits in one fails on the variant, with this note beside it.
    pub(super) undecidable: BTreeSet<String>,
    /// Reaches dropped for naming a path git does not publish, as `<path> (<file>:<line>)`.
    ///
    /// Carried rather than dropped silently: a stale or fabricated path stops being evidence
    /// here, and a variant that then has none fails on the message about the variant - which
    /// reads as *nothing reaches it* when the truth is *what reaches it names nothing*. Stated
    /// rather than refused, because a path a test builds and does not read is legitimate and this
    /// gate is not the venue for what is IN the directory.
    pub(super) unpublished: BTreeSet<String>,
}

/// Every `examples/<name>` a line of test code reaches for, and where from.
///
/// `files` maps a repo-relative path to that file's RAW text, and the blanking happens HERE rather
/// than in the caller - so a unit test over this function covers the blanking too, which is the
/// difference between testing the rule and testing the plumbing above it. One map for the whole
/// scan rather than a file at a time, because `regions::scope` follows a `#[cfg(test)] mod x;` into
/// the PARENT file: an out-of-line test module is test code and nothing inside the file says so.
///
/// `published` is the listing a reach has to resolve against, and it comes from the same place
/// [`super::corpus::variants`] does - see [`resolved`] for what that removes and what it does not.
///
/// `Err` when the loop below did not visit every file it was handed. The corpus's own length is a
/// different derivation from this loop's counter - the length comes off
/// [`super::corpus::Corpus`]'s private field, which only its walk fills - so narrowing either one
/// breaks the equality rather than moving both, which is what `github.com/telekom/sutura#414`
/// measured five gates doing. A truncated walk here gave 12 of 205 files, 69 of 1333 declarations
/// and exit 0.
pub(super) fn evidence(files: &BTreeMap<String, String>, published: &BTreeSet<String>) -> Result<Evidence, String> {
    let blanked: BTreeMap<String, String> = files
        .iter()
        .map(|(rel, text)| (rel.clone(), code_lines(text).join("\n")))
        .collect();
    // The owned `String` is the `PostImage` port's signature rather than a borrow-checker dodge,
    // and only the parent-module lookup asks for one.
    let read = |path: &str| blanked.get(path).cloned();
    let mut found = Evidence::default();
    let mut looked = 0_usize;
    for (rel, text) in &blanked {
        looked = looked.saturating_add(1);
        let tests = regions::scope(rel, &read);
        // BOTH images: the file as written, whose comment lines keep an attribute block together,
        // and the blanked one, in which a commented-out `#[test]` is not a declaration.
        let cells = attributes::cells(files.get(rel).map_or("", String::as_str), text);
        for entry in cells.unresolved() {
            found.unresolvable.push(format!("`{rel}`: {entry}"));
        }
        for entry in cells.undecidable() {
            found.undecidable.insert(format!("{entry} in `{rel}`"));
        }
        if cells.nothing_runs() {
            // THE FILE-LEVEL FLOOR. Nothing in it runs, so a helper in it is unreachable from
            // this venue through this file - which is the half a per-line rule cannot reach, and
            // the half this repository's own flagship evidence needs: see `Cells::nothing_runs`.
            found.all_ignored = found.all_ignored.saturating_add(1);
            continue;
        }
        found.declared = found.declared.saturating_add(cells.runs());
        let mut carries_test_code = false;
        for (index, line) in text.lines().enumerate() {
            let number = index.saturating_add(1);
            if !tests.covers(number) {
                continue;
            }
            carries_test_code = true;
            // THE LINE-LEVEL RULE, beside the floor rather than instead of it: a file with four
            // running tests and the reach inside its one `#[ignore]`d cell passes the floor.
            if cells.unreached(number) {
                continue;
            }
            for reach in reaches_for(line) {
                let Some(variant) = resolved(&reach, published) else {
                    found.unpublished.insert(format!("`{reach}` ({rel}:{number})"));
                    continue;
                };
                found.reaches.entry(variant).or_default().entry(rel.clone()).or_insert(number);
            }
        }
        if carries_test_code {
            found.test_files = found.test_files.saturating_add(1);
        }
    }
    if looked != files.len() {
        return Err(format!(
            "looked at {looked} of the {} file(s) in the corpus - the rest were never scanned, so a variant this calls unreached may be reached from the part the loop skipped",
            files.len()
        ));
    }
    Ok(found)
}

/// One variant's evidence, for the verdict.
///
/// The verdict used to state the SCAN's size - `each reached by one of N test file(s)` - which is a
/// count over a set that is not the claim being made. Measured: one variant was reached from a
/// dozen-odd files and the other from exactly one, and the line stated neither, so thinning the
/// evidence for a variant down to two fixtures inside this crate printed a verdict byte-identical
/// to the healthy one. No figure is written here, because the two the verdict prints move with the
/// tree and a copy of them rots first. The file is named when there is only one, because one is
/// the state worth reading.
pub(super) fn report(variant: &str, from: &BTreeMap<String, usize>) -> String {
    match from.iter().next() {
        Some((rel, line)) if from.len() == 1 => format!("{variant}: reached from 1 file - {rel}:{line}"),
        _ => format!("{variant}: reached from {} file(s)", from.len()),
    }
}
