//! The docs gate: keep `docs/src/SUMMARY.md` and the pages on disk in agreement.
//!
//! `mdbook` renders only what the summary links to, and by default it CREATES a file named
//! in the summary that does not exist - so both failure modes are silent:
//!
//! * a chapter linking a page that was renamed or deleted - the reader gets an empty page
//!   where a rule was supposed to be;
//! * a page under `docs/src/` that no chapter links - it is published nowhere while looking
//!   published to whoever wrote it. This is the one that matters, and the one a build cannot
//!   catch: an unreachable page still builds fine.
//!
//! So this checks BOTH directions, plus that `docs/book.toml` exists, since a book with no
//! configuration is not a book `mdbook` can build.

use std::collections::BTreeSet;
use std::path::Path;

use crate::Verdict;
use crate::repo;

/// The book root: `book.toml` plus `src/`.
const BOOK_CONFIG: &str = "docs/book.toml";
/// Where the pages live. Paths in the report are relative to this.
const SRC_DIR: &str = "docs/src";
/// The navigation. It is a chapter list, not a chapter, so it is never counted as a page.
const SUMMARY: &str = "docs/src/SUMMARY.md";

/// Every link target in a summary, with `./` stripped and any `#anchor` cut.
///
/// Hand-scanned rather than parsed: a summary is a list of `[Title](target)` links in a file
/// this repo controls, and a Markdown dependency to read it would be a poor trade. External
/// links are dropped - they are not this gate's business - and so is the draft form
/// `[Title]()`, which deliberately names no file.
fn linked(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(at) = rest.find("](") {
            let after = rest.get(at + 2..).unwrap_or("");
            let Some(end) = after.find(')') else {
                break;
            };
            let target = after.get(..end).unwrap_or("");
            rest = after.get(end + 1..).unwrap_or("");
            let target = target.split('#').next().unwrap_or(target).trim();
            let target = target.strip_prefix("./").unwrap_or(target);
            if target.is_empty() || target.starts_with("http") {
                continue;
            }
            out.insert(String::from(target));
        }
    }
    out
}

/// Every page under `docs/src/`, relative to it, with `/` separators. `SUMMARY.md` is
/// excluded: it is the navigation, so requiring it to link itself would be circular.
fn pages(root: &Path) -> BTreeSet<String> {
    let mut found = Vec::new();
    repo::collect_files(root, &root.join(SRC_DIR), &["md"], &mut found);
    found
        .iter()
        .filter_map(|rel| {
            let tail = rel.strip_prefix(SRC_DIR)?;
            tail.strip_prefix('/')
        })
        .filter(|rel| *rel != "SUMMARY.md")
        .map(String::from)
        .collect()
}

/// Is this link target a markdown page? Case-insensitive, because half of this repo is
/// developed on a filesystem that does not distinguish `.MD` from `.md`.
fn is_markdown(target: &str) -> bool {
    Path::new(target)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// Both directions, as a list of problems. Pure, so the rule is testable without a repo.
fn problems(linked: &BTreeSet<String>, present: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    for target in linked {
        if !is_markdown(target) {
            problems.push(format!(
                "{SUMMARY} links `{target}`, which is not a markdown page - a chapter is a `.md` file"
            ));
        } else if !present.contains(target) {
            problems.push(format!("{SUMMARY} links `{target}`, but `{SRC_DIR}/{target}` does not exist"));
        }
    }
    for orphan in present.difference(linked) {
        problems.push(format!(
            "`{SRC_DIR}/{orphan}` is in no chapter - an unreachable page is published nowhere, so link it from {SUMMARY} or delete it"
        ));
    }
    problems
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-docs: could not determine the repo root");
        return Verdict::Fail;
    };

    let present = pages(&root);
    let summary_path = root.join(SUMMARY);
    if !summary_path.is_file() {
        // Not an error: a repo need not have a book. Pages with no summary are.
        if present.is_empty() {
            println!("xtask check-docs: ok - no book");
            return Verdict::Pass;
        }
        eprintln!("xtask check-docs: FAILED - pages exist under {SRC_DIR} but {SUMMARY} does not");
        return Verdict::Fail;
    }

    let Ok(text) = std::fs::read_to_string(&summary_path) else {
        eprintln!("xtask check-docs: could not read {SUMMARY}");
        return Verdict::Fail;
    };

    let chapters = linked(&text);
    let mut found = problems(&chapters, &present);
    if !root.join(BOOK_CONFIG).is_file() {
        found.push(format!("{BOOK_CONFIG} is missing - there is nothing for mdbook to build"));
    }

    if found.is_empty() {
        println!(
            "xtask check-docs: ok - {} chapter(s), {} page(s), every page reachable",
            chapters.len(),
            present.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-docs: FAILED");
    for problem in &found {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("The book is what a reader sees, so a page it does not reach is a rule nobody reads.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{linked, problems};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn reads_chapter_targets() {
        let summary = "# Summary\n\n- [Introduction](index.md)\n- [Gates](gates.md)\n";
        assert_eq!(linked(summary), set(&["index.md", "gates.md"]));
    }

    #[test]
    fn normalises_prefixes_and_anchors() {
        let summary = "- [A](./a.md)\n- [B](b.md#a-heading)\n- [C](nested/c.md)\n";
        assert_eq!(linked(summary), set(&["a.md", "b.md", "nested/c.md"]));
    }

    #[test]
    fn ignores_external_links_and_drafts() {
        let summary = "- [Upstream](https://example.com/x.md)\n- [Draft]()\n- [Real](real.md)\n";
        assert_eq!(linked(summary), set(&["real.md"]));
    }

    #[test]
    fn a_matching_pair_is_clean() {
        assert!(problems(&set(&["index.md", "gates.md"]), &set(&["index.md", "gates.md"])).is_empty());
    }

    #[test]
    fn a_chapter_with_no_file_is_a_problem() {
        let found = problems(&set(&["index.md", "gone.md"]), &set(&["index.md"]));
        assert_eq!(found.len(), 1);
        assert!(found.iter().any(|p| p.contains("does not exist")), "{found:?}");
    }

    #[test]
    fn a_page_in_no_chapter_is_a_problem() {
        // The direction a build cannot catch: an orphan page renders, and is read by nobody.
        let found = problems(&set(&["index.md"]), &set(&["index.md", "orphan.md"]));
        assert_eq!(found.len(), 1);
        assert!(found.iter().any(|p| p.contains("in no chapter")), "{found:?}");
    }

    #[test]
    fn both_directions_are_reported_together() {
        let found = problems(&set(&["index.md", "gone.md"]), &set(&["index.md", "orphan.md"]));
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn a_chapter_that_is_not_markdown_is_a_problem() {
        let found = problems(&set(&["index.html"]), &BTreeSet::new());
        assert_eq!(found.len(), 1);
        assert!(found.iter().any(|p| p.contains("not a markdown page")), "{found:?}");
    }

    // NOTE: there is deliberately no test here that reads the real `SUMMARY.md` and the real
    // `docs/src/` tree. That property is enforced by `cargo xtask check-docs`, which runs in
    // the hooks and in the `hygiene` flake check - and `hygiene` is the check given the whole
    // repository as its source. A unit test cannot do it: the `nextest` check gets crane's
    // Cargo-only source filter, so `docs/` is not there, and the same mistake already failed
    // CI once for `.agents/`. Widening the test derivation to the whole repo would make every
    // documentation edit invalidate the test build. Hence the fixtures above: the rule is a
    // pure function of two sets, which is the part worth testing.
}
