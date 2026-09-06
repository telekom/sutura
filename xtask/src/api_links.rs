//! No page under `docs/api/` may link to a Rust path.
//!
//! A rustdoc intra-doc link written inline - `` [`Foo`](crate::path::Foo) `` - carries a
//! destination only rustdoc can resolve, and the generator used to copy it into the published
//! page verbatim. What mkdocs does with it was MEASURED rather than assumed, and it does two
//! different things for one defect:
//!
//! * `urlsplit` decides a destination's scheme, and a URL scheme is `[a-zA-Z][a-zA-Z0-9+.-]*`.
//!   `crate::path::Foo` therefore parses as a URL whose scheme is `crate`, and mkdocs publishes
//!   it untouched: a **dead href** on a reader-facing page, and `github.com/telekom/sutura#321`
//!   counted 78 of them across seven pages.
//! * `sutura_domain::plan::LegPlan` is NOT a legal scheme, because of the underscore. It falls
//!   through to relative-path handling and `mkdocs build --strict` **aborts**:
//!   *"contains an unrecognized relative link `sutura_domain::plan::LegPlan`, it was left as
//!   is"*. That shipped on #352 with every local gate green, which is #356.
//!
//! **So the two are one defect and the tolerated half is a rename away from the fatal half.**
//! Every crate here is `sutura-x`, i.e. `sutura_x` as a path, so re-spelling one of those 78
//! `crate::` links as the crate it names turns a dead href into a red site build. Confirmed by
//! changing one character: `suturadomain::plan::LegPlan` builds in 1.68s, and restoring the
//! underscore aborts.
//!
//! **What holds it, and why this gate exists as well.** `docs/.tools/rustdoc_to_markdown.py`
//! drops the destination and keeps the text, so a page cannot be GENERATED with one - rustdoc
//! keeps the link for a reader of `cargo doc` and the page keeps the code span. This gate is the
//! rule over the OUTPUT: it fails on a Rust-path destination however it got there, which covers
//! the hand-written `docs/api/index.md` that no byte-compare regenerates, a page hand-edited in
//! place, and the rewrite itself being deleted or narrowed.
//!
//! **The limits, next to the claim.**
//!
//! * SCOPE is `docs/api/`. A Rust-path destination on any other page under `docs/` is not held
//!   here - `check-docs` owns links on the hand-written pages, and there are none to hold today
//!   (measured 2026-09-06: zero outside `docs/api/`).
//! * A SINGLE-segment destination - ``[`Foo`](Foo)`` - is not a Rust path to this gate, because it
//!   is indistinguishable from a relative link to a page. mkdocs aborts on it as an unrecognized
//!   relative link, and `just docs` inside `just validate` is the venue that says so.
//! * It says nothing about whether rustdoc RESOLVED the link. rustdoc was clean under both forms
//!   on #352, so `rustdoc::broken_intra_doc_links` would not have caught that defect and this
//!   gate does not catch an unresolved link either. `#321`'s first class is still open.
//! * It reads destinations, not targets: a nav entry, an anchor, an external URL that 404s and a
//!   reference-style link definition are all somebody else's rule or nobody's.
//! * The generator's rewrite and this gate do not agree on every shape, and the DIRECTION is the
//!   safe one. The rewrite's link-text pattern stops at a `]`, so a destination behind link text
//!   that itself contains one is left in place - and then this gate fails while `just api` cannot
//!   fix it. That is the right way round: the message names the bracket form, which is an edit to
//!   the doc comment. No such link exists in the tree today.

use std::collections::BTreeSet;
use std::path::Path;

use crate::api_docs::{GENERATED_MARKER, PAGES_DIR};
use crate::{Verdict, markdown, repo};

/// One finding: the page, the line, and the destination as written.
#[derive(Debug)]
struct Finding {
    /// Repo-relative, so the message names a file somebody can open.
    page: String,
    /// What went wrong, already worded.
    detail: String,
}

/// One page, read end to end.
///
/// The destination count is part of the verdict rather than a debugging aid: it is what says the
/// scan read the page rather than merely opened it.
#[derive(Debug)]
struct PageScan {
    /// Every destination examined, whether or not it was a problem.
    destinations: usize,
    /// The Rust paths among them.
    findings: Vec<Finding>,
}

/// A rustdoc disambiguator prefix, which is part of the destination and not part of the path.
///
/// ``[`f`](fn@crate::f)`` is how rustdoc is told which `f` is meant when a module and a function
/// share a name. Stripping it is what keeps the prefixed spelling from reading as a URL scheme
/// of its own and slipping the rule.
fn without_disambiguator(dest: &str) -> &str {
    match dest.split_once('@') {
        Some((prefix, rest)) if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_lowercase()) => rest,
        _ => dest,
    }
}

/// One path segment: a Rust identifier, ASCII only.
///
/// ASCII only is a narrowing and it is deliberate: a non-ASCII identifier is legal Rust and would
/// read as prose here, so it is a MISS rather than a false positive, and the site build behind
/// this gate is what would then catch it.
fn is_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Is this destination a Rust path rather than something a browser can fetch?
///
/// TWO or more segments, because one segment is a relative link to a page - see the module's
/// limits. A trailing `()` or `!` is rustdoc's way of naming a function or a macro and is part of
/// the destination, so it is stripped before the segments are read; a title after the destination
/// (`](crate::Foo 'why')`) is dropped for the same reason, since the path is still the href.
fn is_rust_path(dest: &str) -> bool {
    let first = dest.split_whitespace().next().unwrap_or_default();
    let bare = first.strip_prefix('<').map_or(first, |s| s.strip_suffix('>').unwrap_or(s));
    let path = without_disambiguator(bare);
    let path = path.strip_suffix("()").or_else(|| path.strip_suffix('!')).unwrap_or(path);
    let mut segments = path.split("::");
    let Some(first_segment) = segments.next() else {
        return false;
    };
    let mut count = 1_usize;
    if !is_segment(first_segment) {
        return false;
    }
    for segment in segments {
        if !is_segment(segment) {
            return false;
        }
        count = count.saturating_add(1);
    }
    count > 1
}

/// What one page's destinations say, or why the page could not be judged.
///
/// FAIL CLOSED on both refusals. An unreadable page and a page whose fences do not close are the
/// two ways a scan reads nothing and reports it as nothing wrong - the shape
/// `xtask/src/docs/links.rs` records from one invalid UTF-8 byte taking a dead link out of a
/// green verdict.
fn scan(page: &str, text: &str) -> Result<PageScan, Finding> {
    let lines = markdown::prose(text).map_err(|error| Finding {
        page: String::from(page),
        detail: format!("could not be lexed, so nothing on it was read: {error}"),
    })?;
    let mut findings = Vec::new();
    let destinations = markdown::destinations(&lines);
    for destination in &destinations {
        if is_rust_path(destination.text) {
            findings.push(Finding {
                page: String::from(page),
                detail: format!(
                    "line {}: `]({})` is a Rust path, which resolves to nothing on the site",
                    destination.line, destination.text
                ),
            });
        }
    }
    Ok(PageScan {
        destinations: destinations.len(),
        findings,
    })
}

/// Every `*.md` in the pages directory, sorted.
///
/// `Err` when the directory cannot be listed at all: the subject of this gate is the generated
/// pages, and a scan that cannot find where they live must not answer.
fn pages(root: &Path) -> Result<BTreeSet<String>, String> {
    let dir = root.join(PAGES_DIR);
    let entries = std::fs::read_dir(&dir).map_err(|error| format!("could not read {PAGES_DIR}: {error}"))?;
    let mut found = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read an entry under {PAGES_DIR}: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // Case-SENSITIVE, through `extension` rather than a suffix test: mkdocs matches `*.md`
        // and a `.MD` file is not a page it renders, so treating one as a page would put this
        // gate's scope out of step with the site's.
        if Path::new(&name).extension().is_some_and(|ext| ext == "md") {
            found.insert(name);
        }
    }
    if found.is_empty() {
        return Err(format!("{PAGES_DIR} holds no `.md` page - there is nothing here to check"));
    }
    Ok(found)
}

/// What the sweep found, and how much of the tree it actually read.
struct Sweep {
    /// Every finding, and every page that could not be judged.
    findings: Vec<Finding>,
    /// Pages carrying [`GENERATED_MARKER`]. The floor: zero is a failure.
    generated: usize,
    /// Destinations examined, for the same reason the page count is printed.
    destinations: usize,
}

fn sweep(root: &Path, pages: &BTreeSet<String>) -> Sweep {
    let mut out = Sweep {
        findings: Vec::new(),
        generated: 0,
        destinations: 0,
    };
    for page in pages {
        let rel = format!("{PAGES_DIR}/{page}");
        let Ok(text) = std::fs::read_to_string(root.join(PAGES_DIR).join(page)) else {
            out.findings.push(Finding {
                page: rel,
                detail: String::from("is not readable, so its links were never scanned"),
            });
            continue;
        };
        if text.contains(GENERATED_MARKER) {
            out.generated = out.generated.saturating_add(1);
        }
        match scan(&rel, &text) {
            Ok(read) => {
                out.destinations = out.destinations.saturating_add(read.destinations);
                out.findings.extend(read.findings);
            }
            Err(refusal) => out.findings.push(refusal),
        }
    }
    out
}

/// `xtask check-api-links` - no published API page links to a Rust path.
pub(crate) fn run(args: &[String]) -> Verdict {
    if !args.is_empty() {
        eprintln!("xtask check-api-links: takes no arguments");
        return Verdict::Usage;
    }
    let Some(root) = repo::root() else {
        eprintln!("xtask check-api-links: could not determine the repo root");
        return Verdict::Fail;
    };

    let pages = match pages(&root) {
        Ok(pages) => pages,
        Err(reason) => {
            eprintln!("xtask check-api-links: FAILED - {reason}");
            return Verdict::Fail;
        }
    };
    let found = sweep(&root, &pages);

    if found.generated == 0 {
        eprintln!("xtask check-api-links: FAILED - no page under {PAGES_DIR} carries the generated header");
        eprintln!("  The generated pages ARE the subject of this gate. `just api` writes them.");
        return Verdict::Fail;
    }
    if found.findings.is_empty() {
        println!(
            "xtask check-api-links: ok - {} page(s), {} generated, {} link destination(s), no Rust path among them",
            pages.len(),
            found.generated,
            found.destinations
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-api-links: FAILED");
    for finding in &found.findings {
        eprintln!("  {} {}", finding.page, finding.detail);
    }
    eprintln!();
    eprintln!("A Rust path is not a URL. mkdocs publishes `crate::...` as a dead href and ABORTS on");
    eprintln!("`sutura_x::...`, so both spellings are the same defect. Write the intra-doc link in the");
    eprintln!("bracket form instead - [`crate::path::Foo`] - which rustdoc resolves and the generator");
    eprintln!("renders as a code span, then run `just api` to rewrite the pages.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{is_rust_path, scan};

    #[test]
    fn the_two_measured_spellings_are_both_rust_paths() {
        // The one that aborts `mkdocs build --strict` (#356, shipped on #352).
        assert!(is_rust_path("sutura_domain::plan::LegPlan"));
        // The one mkdocs publishes as a dead href because `crate` is a legal URL scheme (#321).
        assert!(is_rust_path("crate::plan::QueryPlan"));
    }

    #[test]
    fn a_disambiguator_a_call_and_a_title_do_not_hide_a_path() {
        assert!(is_rust_path("fn@crate::warehouse::execute"));
        assert!(is_rust_path("method@crate::surface::Surface::answer"));
        assert!(is_rust_path("crate::plan::QueryPlan::new()"));
        assert!(is_rust_path("crate::macros::refuse!"));
        assert!(is_rust_path("crate::plan::QueryPlan 'the plan'"));
        assert!(is_rust_path("<crate::plan::QueryPlan>"));
    }

    #[test]
    fn a_destination_a_browser_can_fetch_is_not_a_rust_path() {
        assert!(!is_rust_path("sutura-domain.md"));
        assert!(!is_rust_path("../concepts.md#a-plan"));
        assert!(!is_rust_path("https://example.com/a::b"));
        assert!(!is_rust_path("mailto:user@example.com"));
        assert!(!is_rust_path("#anchor"));
        assert!(!is_rust_path(""));
        // ONE segment is a relative link as far as this gate is concerned - the module's limit.
        assert!(!is_rust_path("LegPlan"));
        // A trailing `::` is not a segment, so this is not a path either.
        assert!(!is_rust_path("crate::"));
    }

    #[test]
    fn a_page_that_documents_a_link_is_not_making_one() {
        let page = "```rust\n/// [`Foo`](crate::path::Foo)\n```\n\nSee [`Foo`](crate::path::Foo).\n";
        let found = scan("docs/api/x.md", page).unwrap_or_else(|e| panic!("{}", e.detail));
        assert_eq!(found.destinations, 1, "the fenced one is not a link");
        assert_eq!(found.findings.len(), 1, "{:?}", found.findings);
        let detail = found.findings.first().map_or("", |f| f.detail.as_str());
        assert!(detail.contains("line 5"), "{detail}");
    }

    #[test]
    fn a_clean_page_is_clean_and_its_destinations_are_still_counted() {
        let page = "See [the domain](sutura-domain.md) and [`crate::path::Foo`].\n";
        let found = scan("docs/api/index.md", page).unwrap_or_else(|e| panic!("{}", e.detail));
        assert_eq!(found.destinations, 1);
        assert!(found.findings.is_empty(), "{:?}", found.findings);
    }

    #[test]
    fn a_page_whose_fence_never_closes_is_a_refusal_and_not_a_clean_page() {
        // FAIL CLOSED. A page this cannot lex is a page nothing checked, and the alternative is
        // a green verdict over an unread page - the defect `xtask/src/docs/links.rs` records.
        let refusal = scan("docs/api/x.md", "```rust\nfn f() {}\n").expect_err("an unclosed fence must refuse");
        assert!(refusal.detail.contains("could not be lexed"), "{}", refusal.detail);
    }
}
