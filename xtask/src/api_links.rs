//! No page under `docs/api/` may carry a link destination mkdocs cannot follow.
//!
//! A rustdoc intra-doc link written inline - `` [`Foo`](crate::path::Foo) `` - carries a
//! destination only rustdoc can resolve, and the generator used to copy it into the published
//! page verbatim. **This gate asks mkdocs' own question about it rather than re-deriving a Rust
//! grammar**, because those two questions are not the same one and the first version of this gate
//! asked the second: it validated every segment of the destination as a Rust identifier, so five
//! shapes with a non-identifier tail - `](crate::plan::run())`, an `#anchor`, a `?query`, a
//! `/path`, a non-ASCII segment - published a dead href at exit 0 with this gate green.
//!
//! **The question mkdocs asks is `urlsplit`'s, and it has exactly two answers that matter.** A URL
//! scheme is an ASCII letter followed by `[A-Za-z0-9+.-]*` and then a `:`, so:
//!
//! * **A scheme mkdocs does not recognise.** `crate::path::Foo` parses as a URL whose scheme is
//!   `crate`, so mkdocs leaves the destination ALONE and publishes it verbatim - a **dead href**,
//!   silently, at exit 0. `github.com/telekom/sutura#321` counted 78 of them across seven pages.
//!   **This is this gate's class**, and it holds it whatever follows the scheme, which is what
//!   makes the five shapes above stop being special cases.
//! * **No scheme at all.** `sutura_domain::plan::LegPlan` has none, because the underscore is not
//!   a scheme character, so mkdocs resolves it against the pages on disk and
//!   `mkdocs build --strict` **ABORTS**: *"contains an unrecognized relative link
//!   `sutura_domain::plan::LegPlan`, it was left as is"*. That shipped on #352 with every local
//!   gate green, which is #356. **The site build is the holder of that class** - it is loud where
//!   the first class is silent - and `just docs` runs inside `just validate` for that reason.
//!
//! So the two are one defect, and the tolerated half is a rename away from the fatal half: every
//! crate here is `sutura-x`, i.e. `sutura_x` as a path. Confirmed by changing one character:
//! `suturadomain::plan::LegPlan` builds in 1.68s, and restoring the underscore aborts.
//!
//! **What holds it, and why this gate exists as well.** `docs/.tools/rustdoc_to_markdown.py`
//! classifies a destination the same way and drops it, keeping the text, so a page cannot be
//! GENERATED with one - rustdoc keeps the link for a reader of `cargo doc` and the page keeps the
//! code span. This gate is the rule over the OUTPUT: it fails however the destination got there,
//! which covers the hand-written `docs/api/index.md` that no byte-compare regenerates, a page
//! hand-edited in place, and the rewrite itself being deleted or narrowed.
//!
//! **The limits, next to the claim.**
//!
//! * **A scheme-less destination is the SITE BUILD's class, not this gate's.** This gate speaks up
//!   only for the `::` spelling, where it can name the line before mkdocs names the file. A
//!   single-segment `` [`Foo`](Foo) `` is a Rust path this gate calls a relative link, because it
//!   is indistinguishable from one; `--strict` aborts on it, measured.
//! * **A destination whose scheme IS real is not judged further.** An `https://` target that 404s
//!   is nobody's rule here.
//! * SCOPE is `docs/api/`. A dead-scheme destination on any other page under `docs/` is not held
//!   here - `check-docs` owns links on the hand-written pages, and there are none to hold today
//!   (measured 2026-09-06: zero outside `docs/api/`, and `http`/`https` are the only schemes any
//!   page in this tree writes).
//! * It says nothing about whether rustdoc RESOLVED the link. rustdoc was clean under both
//!   spellings on #352, so `rustdoc::broken_intra_doc_links` would not have caught that defect and
//!   this gate does not catch an unresolved link either - #360.
//! * It reads destinations, not targets: a nav entry, an anchor and a reference-style link
//!   definition are all somebody else's rule or nobody's.
//! * **ONE shape the generator leaves and this gate then fails**, so `just api` is not the fix for
//!   it: link text that itself contains a `]`, which the rewrite's text pattern stops at. Verified
//!   by calling `clean_docs` over it. A title and an angle-bracketed destination used to be in
//!   this list and are now rewritten, because the generator classifies rather than pattern-matches
//!   the destination. The message this gate prints names both fixes for that reason.

use std::collections::BTreeSet;
use std::path::Path;

use crate::api_docs::{GENERATED_MARKER, GENERATOR, PAGES_DIR};
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

/// The schemes mkdocs may leave a destination alone for, because they are real.
///
/// An ALLOWLIST, so it fails closed: a destination carrying any other scheme is a finding, and a
/// genuinely new scheme (`mailto:`, `tel:`) is one entry here plus a reader deciding it is real.
/// Measured 2026-09-06 rather than anticipated - `http` and `https` are the only schemes any page
/// in this tree writes, and the one `crate:` a grep finds is inside backticks in a skill file.
const REAL_SCHEMES: &[&str] = &["http", "https"];

/// How the generator spells the same list, so the two can be compared rather than trusted.
const SCHEMES_DECLARATION: &str = "REAL_SCHEMES = (";

/// The generator's copy of [`REAL_SCHEMES`], read out of its source.
///
/// **Two copies of one list, in two languages, and this is what makes them one mechanism.** The
/// generator DROPS a destination it judges unfollowable, so a scheme allowed here and not there
/// silently deletes a working link from a page, and a scheme allowed there and not here fails a
/// gate over a page whose fix - `just api` - cannot change it. Neither is visible without this.
///
/// Fails closed three ways: a generator it cannot read, a declaration it cannot find, and a
/// declaration holding nothing. A comparison that quietly has nothing to compare is the shape
/// this whole gate exists to avoid.
fn generator_schemes(root: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(root.join(GENERATOR)).map_err(|error| format!("could not read {GENERATOR}: {error}"))?;
    let mut found: Vec<String> = Vec::new();
    let mut declarations = 0_usize;
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix(SCHEMES_DECLARATION) else {
            continue;
        };
        declarations = declarations.saturating_add(1);
        let list = rest.split(')').next().unwrap_or_default();
        for part in list.split(',') {
            let name = part.trim().trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                found.push(String::from(name));
            }
        }
    }
    match declarations {
        0 => Err(format!(
            "{GENERATOR} declares no `{SCHEMES_DECLARATION}...)` on one line - the two scheme lists cannot be compared"
        )),
        1 if found.is_empty() => Err(format!("{GENERATOR}'s `{SCHEMES_DECLARATION}...)` names no scheme")),
        1 => Ok(found),
        _ => Err(format!(
            "{GENERATOR} declares `{SCHEMES_DECLARATION}...)` {declarations} times - which one is the list?"
        )),
    }
}

/// What mkdocs will do with a destination, which is the only question worth asking about one.
#[derive(Debug, PartialEq, Eq)]
enum Fate {
    /// A scheme this tree recognises. mkdocs leaves it alone, correctly.
    Url,
    /// A scheme mkdocs does not know. `urlsplit` finds one, so mkdocs leaves the destination
    /// ALONE and publishes it verbatim - a dead href, at exit 0. THIS GATE'S CLASS.
    DeadScheme,
    /// No scheme. mkdocs resolves it against the pages on disk and `--strict` ABORTS when it does
    /// not resolve, so the site build is the holder. This gate speaks up only for `::`.
    Relative,
}

/// The scheme `urlsplit` would find, lowercased - read off `CPython`'s rule rather than guessed.
///
/// `urlsplit` takes the text before the FIRST colon when that text is non-empty, begins with an
/// ASCII letter, and holds only `[A-Za-z0-9+.-]`. Every consequence this module is built on falls
/// out of that one rule: `crate::x` has the scheme `crate` and `sutura_domain::x` has none,
/// because `_` is not in the set.
fn scheme(dest: &str) -> Option<String> {
    let at = dest.find(':')?;
    let candidate = dest.get(..at)?;
    let mut chars = candidate.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let legal = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.');
    chars.all(legal).then(|| candidate.to_lowercase())
}

/// The destination as mkdocs reads it: without the angle brackets or the title a link may carry.
///
/// Both are ordinary `CommonMark` around the SAME destination, so stripping them is what keeps
/// `](<crate::x>)` and `](crate::x 'why')` from being two more special cases. A title is
/// whitespace-separated, which is why splitting on whitespace is enough.
fn destination(written: &str) -> &str {
    let first = written.split_whitespace().next().unwrap_or_default();
    first.strip_prefix('<').map_or(first, |s| s.strip_suffix('>').unwrap_or(s))
}

/// What mkdocs will do with this destination.
fn fate(written: &str) -> Fate {
    let dest = destination(written);
    match scheme(dest) {
        Some(found) if REAL_SCHEMES.contains(&found.as_str()) => Fate::Url,
        Some(_) => Fate::DeadScheme,
        None => Fate::Relative,
    }
}

/// Why this destination is a finding, or `None` when it is not one.
///
/// Two arms, and they are the two answers `urlsplit` gives. The second is deliberately NARROW: a
/// scheme-less destination is the site build's class, and `::` is the one shape this gate claims
/// there, because no page or asset in this tree is named with one - so a `::` where a page path
/// belongs is a Rust path, and naming its line beats mkdocs naming only the file. If a file is
/// ever named with `::`, this reports a legitimate link, and that is the direction to fail in.
fn finding(written: &str) -> Option<&'static str> {
    match fate(written) {
        Fate::DeadScheme => {
            Some("carries a scheme mkdocs does not recognise, so mkdocs publishes it verbatim - a dead href at exit 0")
        }
        Fate::Relative if destination(written).contains("::") => {
            Some("is a Rust path where a page path belongs, which aborts `mkdocs build --strict`")
        }
        // One arm and two reasons: a real scheme is a real URL, and a scheme-less destination
        // with no `::` is a page path, which is `check-docs`' concern and not this gate's.
        Fate::Url | Fate::Relative => None,
    }
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
    for found in &destinations {
        if let Some(why) = finding(found.text) {
            findings.push(Finding {
                page: String::from(page),
                detail: format!("line {}: `]({})` {why}", found.line, found.text),
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
    /// Pages read END TO END. **The floor**, and the only honest one available here.
    ///
    /// A DESTINATION total cannot be the floor and reviewing this gate proved it: 11 of the 13
    /// destinations in this directory are on the hand-written `index.md`, the 16 generated pages
    /// contribute 2 between them, and the whole point of the rewrite is to drive that to zero. So
    /// a `continue` after the marker count left 16 of 17 pages unscanned with the total moving by
    /// 2 and the verdict green - the `.take(1)` shape `../gates` records against `check-docs`,
    /// where a repo-wide link total was satisfied with 51 of 52 pages unread. A per-page
    /// destination floor is impossible (a clean generated page has none), so the quantity that
    /// works is the pages themselves: this must equal the number of pages found.
    scanned: usize,
    /// Pages carrying [`GENERATED_MARKER`]. A second floor: zero is a failure.
    generated: usize,
    /// Destinations examined. Printed, and deliberately NOT a floor - see `scanned`.
    destinations: usize,
}

fn sweep(root: &Path, pages: &BTreeSet<String>) -> Sweep {
    let mut out = Sweep {
        findings: Vec::new(),
        scanned: 0,
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
                out.scanned = out.scanned.saturating_add(1);
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
    let generator = match generator_schemes(&root) {
        Ok(schemes) => schemes,
        Err(reason) => {
            eprintln!("xtask check-api-links: FAILED - {reason}");
            return Verdict::Fail;
        }
    };
    let mine: Vec<String> = REAL_SCHEMES.iter().map(|s| String::from(*s)).collect();
    if generator != mine {
        eprintln!("xtask check-api-links: FAILED - the two scheme allowlists disagree");
        eprintln!("  this gate:   {mine:?}");
        eprintln!("  {GENERATOR}: {generator:?}");
        eprintln!("  A scheme on one side only either deletes a working link from a page or fails");
        eprintln!("  a gate whose own fix cannot change it. Add it to both, or to neither.");
        return Verdict::Fail;
    }
    let found = sweep(&root, &pages);

    // FINDINGS FIRST, and the order is a fix rather than a preference: with the floors ahead of
    // it, a directory of unreadable pages reported "no page carries the generated header" and
    // named none of them.
    if !found.findings.is_empty() {
        report(&found.findings);
        return Verdict::Fail;
    }
    if found.scanned != pages.len() {
        eprintln!(
            "xtask check-api-links: FAILED - read {} of {} page(s) and reported nothing about the rest",
            found.scanned,
            pages.len()
        );
        eprintln!("  A page neither scanned nor reported is a page this verdict does not cover.");
        return Verdict::Fail;
    }
    if found.generated == 0 {
        eprintln!("xtask check-api-links: FAILED - no page under {PAGES_DIR} carries the generated header");
        eprintln!("  The generated pages ARE the subject of this gate. `just api` writes them.");
        return Verdict::Fail;
    }
    println!(
        "xtask check-api-links: ok - {} of {} page(s) read, {} generated, {} link destination(s), every one a URL mkdocs can follow ({} scheme(s), agreed with the generator)",
        found.scanned,
        pages.len(),
        found.generated,
        found.destinations,
        mine.len()
    );
    Verdict::Pass
}

/// Every finding, then BOTH fixes - because which one applies depends on the page.
fn report(findings: &[Finding]) {
    eprintln!("xtask check-api-links: FAILED");
    for finding in findings {
        eprintln!("  {} {}", finding.page, finding.detail);
    }
    eprintln!();
    eprintln!("A Rust path is not a URL. Write the intra-doc link in the BRACKET form instead -");
    eprintln!("[`crate::path::Foo`] - which rustdoc resolves and the generator renders as a code span.");
    eprintln!();
    eprintln!("On a GENERATED page the fix is `just api`, which rewrites it from the doc comment. On");
    eprintln!("the hand-written docs/api/index.md, and where the rewrite cannot reach the link (link");
    eprintln!("text holding a `]`), the fix is the source line this names.");
}

#[cfg(test)]
mod tests {
    use super::{Fate, fate, scan};

    /// Every destination in this module's tests goes through the REAL pipeline, because the first
    /// version of this file asserted on a string the pipeline cannot produce.
    ///
    /// `markdown::destinations` stops a destination at the first `)`, so a predicate called with
    /// `crate::plan::run()` was a witness that held as a type and was empty as a fact - the call
    /// form reached mkdocs and this gate said nothing. Writing the link and reading the findings
    /// is what makes a test here mean something about a page.
    fn findings_over(body: &str) -> Vec<String> {
        let page = format!("A page.\n\n{body}\n");
        let found = scan("docs/api/x.md", &page).unwrap_or_else(|e| panic!("{}", e.detail));
        found.findings.into_iter().map(|f| f.detail).collect()
    }

    #[test]
    fn the_two_measured_spellings_are_both_findings() {
        // The one that aborts `mkdocs build --strict` (#356, shipped on #352): no scheme,
        // because of the underscore.
        assert_eq!(fate("sutura_domain::plan::LegPlan"), Fate::Relative);
        assert_eq!(findings_over("[`L`](sutura_domain::plan::LegPlan)").len(), 1);
        // The one mkdocs publishes as a dead href, because `crate` parses as a scheme (#321).
        assert_eq!(fate("crate::plan::QueryPlan"), Fate::DeadScheme);
        assert_eq!(findings_over("[`Q`](crate::plan::QueryPlan)").len(), 1);
    }

    #[test]
    fn every_tail_that_defeated_the_grammar_check_is_a_finding() {
        // THE REVIEWED DEFECT, and every row was measured publishing a dead href at exit 0 while
        // the gate that validated Rust segments said "no Rust path among them". Keying on the
        // SCHEME is what makes them one case: whatever follows `crate:` is not the question.
        for tail in ["", "#anchor", "?x=1", "/x"] {
            let body = format!("[`L`](crate::plan::LegPlan{tail})");
            assert_eq!(findings_over(&body).len(), 1, "tail {tail:?}");
        }
        // The call form, END TO END. The assertion this replaces called the predicate with
        // `crate::plan::QueryPlan::new()`, a string the destination walk could not produce
        // because it stopped at the first `)` - a witness that held as a type and was empty as
        // a fact. The walk balances parentheses now, so this shape is reachable.
        assert_eq!(findings_over("[`r`](crate::plan::run())").len(), 1);
        // A non-ASCII segment. The old ASCII narrowing claimed the site build caught this; it
        // does not - mkdocs published `href="crate::plan::Unicode"`-shaped destinations at exit 0.
        // `\u{dc}` is `U`-umlaut, escaped because this workspace denies a non-ASCII literal.
        assert_eq!(findings_over("[`E`](crate::plan::\u{dc}nicode)").len(), 1);
    }

    #[test]
    fn a_disambiguator_a_title_and_angle_brackets_do_not_hide_a_destination() {
        assert_eq!(findings_over("[`e`](fn@crate::warehouse::execute)").len(), 1);
        assert_eq!(findings_over("[`Q`](crate::plan::QueryPlan 'the plan')").len(), 1);
        assert_eq!(findings_over("[`Q`](<crate::plan::QueryPlan>)").len(), 1);
        assert_eq!(findings_over("[`r`](crate::macros::refuse!)").len(), 1);
    }

    #[test]
    fn a_destination_mkdocs_can_follow_is_not_a_finding() {
        for clean in [
            "sutura-domain.md",
            "../concepts.md#a-plan",
            "https://example.com/a::b",
            "HTTPS://example.com",
            "#anchor",
            "?x=1",
            "/absolute/page",
            // ONE segment is a relative link as far as this gate is concerned - the module's
            // limit, and `--strict` aborts on it.
            "LegPlan",
        ] {
            let body = format!("[`x`]({clean})");
            assert!(findings_over(&body).is_empty(), "{clean} was reported");
        }
    }

    #[test]
    fn a_scheme_that_is_real_but_unlisted_is_a_finding_rather_than_a_pass() {
        // FAIL CLOSED on the allowlist. `mailto:` is genuine and mkdocs leaves it alone
        // correctly, and this gate reports it until somebody adds it to REAL_SCHEMES - which is
        // the direction to be wrong in, since the alternative is any scheme passing.
        assert_eq!(fate("mailto:user@example.com"), Fate::DeadScheme);
        assert_eq!(findings_over("[mail](mailto:user@example.com)").len(), 1);
    }

    #[test]
    fn the_scheme_rule_is_urlsplits_and_not_an_approximation_of_it() {
        assert_eq!(super::scheme("crate::x"), Some(String::from("crate")));
        assert_eq!(super::scheme("Self::parse"), Some(String::from("self")));
        assert_eq!(super::scheme("h+t.t-p1://x"), Some(String::from("h+t.t-p1")));
        // `_` is not a scheme character, which is the whole of #356.
        assert_eq!(super::scheme("sutura_domain::x"), None);
        // A scheme may not start with a digit, and an empty one is not a scheme.
        assert_eq!(super::scheme("1x:y"), None);
        assert_eq!(super::scheme(":y"), None);
        assert_eq!(super::scheme("no-colon-here"), None);
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
