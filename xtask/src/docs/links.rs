//! The link half of the docs gate: a published page may not link into an excluded one.
//!
//! **This is a gate because `mkdocs build --strict` does not fail on it, measured.** With a link
//! from an ADR to an excluded page, `just docs` printed
//!
//! ```text
//! INFO - Doc file 'adr/0013-a-raw-sql-tool-off-by-default.md' contains a link to
//!        'implementation-plan.md' which is excluded from the built site.
//! ```
//!
//! and exited **0**. It cannot be escalated by configuration either: `mkdocs/structure/pages.py`
//! computes that level as `min(logging.INFO, validation.links.not_found)` and `structure/nav.py`
//! does the same for the nav case, so a `validation:` setting can only make it quieter. So a dead
//! link on the published site sits behind a green docs job, which is the same failure shape
//! `validation.omitted_files` exists for and the reason the other half of this gate counts pages
//! rather than trusting a build.
//!
//! **What a published page IS.** A page's body is not always the file under the docs directory:
//! `docs/contributing.md` and `docs/changelog.md` are `pymdownx.snippets` stubs whose published
//! body is `CONTRIBUTING.md` / `CHANGELOG.md` at the repo root, and a scan that stops at the stub
//! reads a sixteen-line include directive and calls it a page. Measured on the tree that shipped
//! it: `[the implementation plan](implementation-plan.md)` appended to `CONTRIBUTING.md` left
//! `check-docs` green with the link total unchanged. So [`sweep`] follows `--8<--` and resolves a
//! relative target inside an included file against the INCLUDING page, which is what mkdocs does
//! and what `docs/contributing.md`'s own comment warns about.
//!
//! **The limits, next to the claim.** This reads inline markdown links, `](target)`, in the prose
//! [`crate::markdown`] leaves after removing fenced code blocks, HTML comments and inline code
//! spans. Angle-bracket destinations, `](<a b.md>)`, percent-encoded paths and a case difference
//! against an exclusion are covered. Still NOT covered, and each is a real page shape:
//!
//! | Uncovered | What happens |
//! | --- | --- |
//! | A reference-style link, `[a][ref]` with `[ref]: plan.md` | not read |
//! | An HTML `<a href="plan.md">` | not read |
//! | An indented (four-column) code block showing a link | read as a link: a false POSITIVE |
//! | An inline code span wrapped across two lines | read as a link: a false POSITIVE |
//! | A destination whose path contains `)` or `>` | truncated at that character |
//! | The `--8<--` block form, and an unquoted include | REFUSED, so it is loud rather than unread |
//! | A page whose body an mkdocs plugin generates | not read |
//!
//! A link to a page that does not exist AT ALL is not this rule's business: mkdocs'
//! `unrecognized_links` covers that one as a warning, which `--strict` does escalate - measured on
//! a cross-crate rustdoc link, recorded in `.agents/skills/sutura/gates/SKILL.md`.

use std::collections::BTreeSet;
use std::path::Path;

use crate::markdown::{self, Unlexable};

use super::CONFIG;

/// The `pymdownx.snippets` marker.
const INCLUDE_MARKER: &str = "--8<--";

/// How many distinct files one page may pull in before this refuses to keep following.
///
/// A cycle is already stopped by the visited set; this stops a legitimately deep chain from
/// making one page's scan unbounded.
const MAX_INCLUDES: usize = 8;

/// Where the text being scanned lives, and which page publishes it.
///
/// The three are the same file for an ordinary page and three different things for a snippet
/// stub, which is the whole reason this is a struct: a finding has to name the file somebody
/// edits, while a relative target resolves against the page mkdocs publishes.
pub(super) struct Written<'a> {
    /// The docs-relative page a relative target resolves against.
    pub(super) page: &'a str,
    /// The repo-relative file the text was written in.
    pub(super) file: &'a str,
    /// The repo-relative path of the page itself.
    pub(super) published: &'a str,
}

/// Whether `mkdocs.yml` turns `--8<--` into an include at all.
#[derive(Clone, Copy)]
pub(super) enum Snippets {
    /// `pymdownx.snippets` is configured, so an include is followed - against the repo root,
    /// which is what `base_path: ["."]` means and what [`super::snippet_problems`] holds.
    Followed,
    /// The extension is not configured, so `--8<--` is literal text mkdocs publishes as-is. A
    /// page carrying one is then a finding of its own.
    NotConfigured,
}

/// What one `--8<--` line names.
enum Include {
    /// A file, repo-root-relative, with any `:section` suffix removed.
    File(String),
    /// A directive shape this does not follow. Carries the message, because the alternative is
    /// reading none of what it pulls in and saying nothing.
    Unfollowable(String),
}

/// One page's links and includes, judged against the exclusions.
pub(super) struct Scan {
    /// The findings.
    problems: Vec<String>,
    /// How many links to a page in this tree were read at all.
    read: usize,
    /// What this text pulls in with `--8<--`.
    includes: Vec<Include>,
}

/// Every inline link target in the prose, as written.
///
/// The walk is [`crate::markdown::destinations`], shared with `check-api-links` rather than
/// written twice: this gate resolves a target against the pages on disk and that one asks whether
/// it is a Rust path, and a second copy of "where does a destination start and end" is a second
/// answer to that question. The line number it carries is dropped here - this gate reports the
/// PAGE, since a dead link is a fact about the page rather than about one line of it.
fn targets(lines: &[String]) -> Vec<&str> {
    crate::markdown::destinations(lines).into_iter().map(|d| d.text).collect()
}

/// A percent-encoded destination, decoded.
///
/// mkdocs resolves `plan%20b.md` against the file `plan b.md`, so a link written that way reaches
/// an excluded page while an undecoded comparison does not see it. ASCII only: a `%XX` naming a
/// byte above 0x7F is left as written, because stitching UTF-8 back together here would be a
/// second parser for one page shape this tree does not have.
fn percent_decoded(path: &str) -> String {
    let mut out = String::new();
    let mut at = 0_usize;
    while at < path.len() {
        if path.as_bytes().get(at) == Some(&b'%')
            && let Some(hex) = path.get(at.saturating_add(1)..at.saturating_add(3))
            && let Ok(decoded) = u8::from_str_radix(hex, 16)
            && decoded.is_ascii()
        {
            out.push(char::from(decoded));
            at = at.saturating_add(3);
            continue;
        }
        let Some(next) = path.get(at..).and_then(|tail| tail.chars().next()) else {
            break;
        };
        out.push(next);
        at = at.saturating_add(next.len_utf8());
    }
    out
}

/// The docs-relative page a link points at, if it points at a page in this tree at all.
///
/// `from` is the page holding the link, docs-relative. A leading `/` is resolved against the docs
/// directory, which is what `validation.absolute_links: relative_to_docs` tells mkdocs to do, so
/// both spellings reach the same rule.
fn resolve(from: &str, target: &str) -> Option<String> {
    let target = target.trim();
    // `](<a b.md>)` is the angle-bracket destination form: the brackets delimit the path rather
    // than being part of it, and a title sits OUTSIDE them - so a bracketed destination must not
    // then be cut at its first space the way a bare one is.
    let (destination, bracketed) = match target.strip_prefix('<').and_then(|rest| rest.split_once('>')) {
        Some((inside, _)) => (inside.trim(), true),
        None => (target, false),
    };
    if destination.is_empty() || destination.contains("://") || destination.starts_with('#') || destination.starts_with("mailto:")
    {
        return None;
    }
    // An anchor or a link title is not part of the path.
    let path = destination.split(['#', '?']).next()?;
    let path = if bracketed { path } else { path.split_whitespace().next()? };
    let path = percent_decoded(path);
    if !path.to_ascii_lowercase().ends_with(".md") {
        return None;
    }
    let mut segments: Vec<&str> = Vec::new();
    if !path.starts_with('/') {
        let parent = from.rsplit_once('/').map_or("", |(head, _)| head);
        segments.extend(parent.split('/').filter(|s| !s.is_empty()));
    }
    for segment in path.trim_start_matches('/').split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

/// What one `--8<--` line names, or `None` when the line is not a directive.
fn include(line: &str) -> Option<Include> {
    let rest = line.trim().strip_prefix(INCLUDE_MARKER)?.trim();
    if rest.is_empty() {
        return Some(Include::Unfollowable(String::from(
            "a bare `--8<--` line opens the BLOCK form of a snippet include, which this gate does not follow - the published page would then carry files nothing here read. Write one `--8<-- \"path\"` line per include",
        )));
    }
    let quoted = rest.strip_prefix('"').and_then(|inner| inner.strip_suffix('"')).or_else(|| {
        let inner = rest.strip_prefix('\'')?;
        inner.strip_suffix('\'')
    });
    let Some(quoted) = quoted else {
        return Some(Include::Unfollowable(format!(
            "`{INCLUDE_MARKER} {rest}` is a snippet include this gate cannot resolve to a path, so whatever it publishes is unread. Quote the path"
        )));
    };
    // `--8<-- "file:section"` publishes one marked section. The FILE is what has to be read
    // either way, and reading all of it is the fail-closed direction.
    let path = quoted.split(':').next().unwrap_or(quoted).trim();
    Some(Include::File(String::from(path)))
}

/// One text's links and includes. Pure, so every arm is testable without a repo.
fn scan(written: &Written<'_>, text: &str, excluded: &BTreeSet<String>) -> Result<Scan, Unlexable> {
    let lines = markdown::prose(text)?;
    let mut out = Scan {
        problems: Vec::new(),
        read: 0,
        includes: Vec::new(),
    };
    for line in &lines {
        out.includes.extend(include(line));
    }
    // The file the reader has to edit, and - when a snippet include put the link in another file
    // - the page whose URL the dead link appears on.
    let via = if written.file == written.published {
        String::new()
    } else {
        format!(", published as `{}`,", written.published)
    };
    for target in targets(&lines) {
        let Some(page) = resolve(written.page, target) else {
            continue;
        };
        out.read = out.read.saturating_add(1);
        // Case-insensitively: half of this repo is developed on a filesystem that does not
        // distinguish `Plan.md` from `plan.md`, so a case difference is the same page there and a
        // finding either way.
        if excluded.iter().any(|kept_out| kept_out.eq_ignore_ascii_case(&page)) {
            out.problems.push(format!(
                "`{}`{via} links to `{target}`, which `exclude_docs` keeps out of the build - mkdocs logs that at INFO and exits 0, so the published page gets a dead link. Cite it as a backticked path instead",
                written.file
            ));
        }
    }
    Ok(out)
}

/// What sweeping every published page found.
pub(super) struct Sweep {
    /// The findings.
    pub(super) problems: Vec<String>,
    /// How many links to a page in this tree were read, over the whole tree.
    pub(super) read: usize,
    /// How many published pages were scanned END TO END - the page's own text, everything it
    /// includes, and no block this could not lex.
    ///
    /// **This is the number the sweep's floor needs, and a repo-wide link total is not.** The
    /// total was the floor that shipped: `.take(1)` on the page loop left it at 3 links from one
    /// page, exit 0, 51 of 52 pages unscanned - because the other pages supply the count and a
    /// per-tree quantifier defends nothing about WHICH page.
    pub(super) scanned: usize,
}

/// One published page and everything it pulls in. `true` when it was scanned end to end.
fn scan_page(
    root: &Path,
    written: &Written<'_>,
    text: String,
    excluded: &BTreeSet<String>,
    snippets: Snippets,
    out: &mut Sweep,
) -> bool {
    let mut pending = vec![(String::from(written.file), text)];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut whole = true;
    while let Some((file, body)) = pending.pop() {
        let here = Written {
            page: written.page,
            file: &file,
            published: written.published,
        };
        let scanned = match scan(&here, &body, excluded) {
            Ok(scanned) => scanned,
            Err(unlexable) => {
                out.problems.push(format!("`{file}`: {unlexable}"));
                whole = false;
                continue;
            }
        };
        out.problems.extend(scanned.problems);
        out.read = out.read.saturating_add(scanned.read);
        for named in scanned.includes {
            let named = match named {
                Include::File(named) => named,
                Include::Unfollowable(why) => {
                    out.problems.push(format!("`{file}`: {why}"));
                    whole = false;
                    continue;
                }
            };
            if matches!(snippets, Snippets::NotConfigured) {
                out.problems.push(format!(
                    "`{file}` includes `{named}` with `{INCLUDE_MARKER}`, and {CONFIG} does not configure `pymdownx.snippets` - mkdocs publishes the directive as literal text, so the page is not what it looks like"
                ));
                whole = false;
                continue;
            }
            if !seen.insert(named.clone()) {
                continue;
            }
            if seen.len() > MAX_INCLUDES {
                out.problems.push(format!(
                    "`{}` pulls in more than {MAX_INCLUDES} files with `{INCLUDE_MARKER}` - this stops following there rather than reporting a page it only partly read",
                    written.published
                ));
                whole = false;
                break;
            }
            if let Ok(body) = std::fs::read_to_string(root.join(&named)) {
                pending.push((named, body));
            } else {
                out.problems.push(format!(
                    "`{file}` includes `{named}` with `{INCLUDE_MARKER}`, and it is not readable from the repo root - the published page carries whatever that file links and this scan cannot see it"
                ));
                whole = false;
            }
        }
    }
    whole
}

/// Every published page's links, and how much of the tree was actually read.
///
/// Only the published pages: what an EXCLUDED page links to is nobody's business, since nothing
/// renders it.
pub(super) fn sweep(
    root: &Path,
    docs_dir: &str,
    published: &[&String],
    excluded: &BTreeSet<String>,
    snippets: Snippets,
) -> Sweep {
    let mut out = Sweep {
        problems: Vec::new(),
        read: 0,
        scanned: 0,
    };
    for page in published {
        let published_as = format!("{docs_dir}/{page}");
        let Ok(text) = std::fs::read_to_string(root.join(docs_dir).join(page.as_str())) else {
            // FAIL CLOSED. Dropping the page silently is what let one invalid UTF-8 byte in
            // `docs/qa.md` take a dead link out of the scan with the count falling 202 to 196
            // and the verdict staying green - eight lines above a comment claiming this half
            // fails closed. The sibling shape is `xtask/src/venues.rs`, where an unreadable page
            // IS the failure.
            out.problems.push(format!(
                "`{published_as}` is not readable, so its links were never scanned - a page this cannot open is a page nothing checked"
            ));
            continue;
        };
        let written = Written {
            page,
            file: &published_as,
            published: &published_as,
        };
        if scan_page(root, &written, text, excluded, snippets, &mut out) {
            out.scanned = out.scanned.saturating_add(1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Include, Scan, Snippets, Written, include, percent_decoded, resolve, scan, sweep, targets};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    fn prose(text: &str) -> Vec<String> {
        crate::markdown::prose(text).unwrap_or_else(|e| panic!("{e}"))
    }

    fn one(page: &str, text: &str, excluded: &BTreeSet<String>) -> Scan {
        let published = format!("docs/{page}");
        let written = Written {
            page,
            file: &published,
            published: &published,
        };
        scan(&written, text, excluded).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_target_is_read_once_per_link_and_not_out_of_a_fence() {
        assert_eq!(
            targets(&prose("see [one](a.md) and [two](b/c.md 'titled')")),
            vec!["a.md", "b/c.md 'titled'"]
        );
        // A page documenting a link is not making one.
        assert_eq!(targets(&prose("```\n[shown](a.md)\n```\n[real](b.md)\n")), vec!["b.md"]);
        assert!(targets(&prose("nothing here")).is_empty());
    }

    #[test]
    fn a_nested_fence_does_not_hide_the_links_below_it() {
        // THE REPRODUCED BUG, at this level: `docs/publishing.md` documents a mermaid fence
        // inside a four-backtick block. A parity toggle inverted for the rest of the page, so a
        // genuine link into an excluded page below it read as code and the total did not move.
        let page = "````text\n```mermaid\ngraph LR\n```\n````\n\nSee [the plan](implementation-plan.md).\n";
        let found = one("publishing.md", page, &set(&["implementation-plan.md"]));
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
        assert_eq!(found.read, 1);
    }

    #[test]
    fn an_unclosed_fence_refuses_instead_of_answering() {
        let published = String::from("docs/x.md");
        let written = Written {
            page: "x.md",
            file: &published,
            published: &published,
        };
        assert!(
            scan(&written, "```rust\nfn main() {}\n", &set(&["plan.md"])).is_err(),
            "an unclosed fence must not produce a link list"
        );
    }

    #[test]
    fn a_target_resolves_against_the_page_holding_it() {
        assert_eq!(resolve("adr/0013-x.md", "../plan.md").as_deref(), Some("plan.md"));
        assert_eq!(resolve("index.md", "adr/0001-x.md").as_deref(), Some("adr/0001-x.md"));
        assert_eq!(resolve("adr/0013-x.md", "0009-y.md").as_deref(), Some("adr/0009-y.md"));
        // An anchor is not part of the path, and neither is a link title.
        assert_eq!(resolve("index.md", "plan.md#row-21").as_deref(), Some("plan.md"));
        assert_eq!(resolve("index.md", "plan.md \"The plan\"").as_deref(), Some("plan.md"));
        // `absolute_links: relative_to_docs` is what mkdocs is told, so both spellings agree.
        assert_eq!(resolve("adr/0013-x.md", "/plan.md").as_deref(), Some("plan.md"));
    }

    #[test]
    fn an_angle_bracket_destination_and_a_percent_escape_reach_the_same_page() {
        // Both are mkdocs-legal spellings of a path with a space in it, and neither was read.
        assert_eq!(resolve("index.md", "<the plan.md>").as_deref(), Some("the plan.md"));
        assert_eq!(resolve("index.md", "the%20plan.md").as_deref(), Some("the plan.md"));
        assert_eq!(resolve("index.md", "<plan.md> \"titled\"").as_deref(), Some("plan.md"));
        assert_eq!(percent_decoded("a%2Fb.md"), "a/b.md");
        // A byte above ASCII is left alone rather than half-decoded.
        assert_eq!(percent_decoded("a%C3%A9.md"), "a%C3%A9.md");
        assert_eq!(percent_decoded("a%zz.md"), "a%zz.md");
    }

    #[test]
    fn a_case_difference_is_still_the_excluded_page() {
        let found = one(
            "index.md",
            "[the plan](Implementation-Plan.md)",
            &set(&["implementation-plan.md"]),
        );
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
    }

    #[test]
    fn what_is_deliberately_not_a_page_link() {
        assert_eq!(resolve("index.md", "https://example.com/a.md"), None);
        assert_eq!(resolve("index.md", "#section"), None);
        assert_eq!(resolve("index.md", "mailto:user@example.com"), None);
        // Not a markdown path: an asset, a directory URL, and nothing at all.
        assert_eq!(resolve("index.md", "assets/logo.svg"), None);
        assert_eq!(resolve("index.md", "../serving/"), None);
        assert_eq!(resolve("index.md", ""), None);
        // Climbing above the docs directory resolves to no page in this tree.
        assert_eq!(resolve("index.md", "../../elsewhere.md"), None);
    }

    #[test]
    fn a_link_into_an_excluded_page_is_a_problem_and_one_into_a_published_page_is_not() {
        let excluded = set(&["implementation-plan.md"]);
        let found = one(
            "adr/0013-x.md",
            "[the plan](../implementation-plan.md) carried a row",
            &excluded,
        );
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
        assert!(
            found.problems.first().is_some_and(|p| p.contains("dead link")),
            "{:?}",
            found.problems
        );
        assert_eq!(found.read, 1);

        let clean = one(
            "adr/0013-x.md",
            "[the other record](0009-y.md) and `docs/implementation-plan.md` in backticks",
            &excluded,
        );
        assert!(clean.problems.is_empty(), "{:?}", clean.problems);
        // The backticked path is prose and not a link, so one link was read and not two.
        assert_eq!(clean.read, 1);
    }

    #[test]
    fn a_snippet_include_is_read_and_a_shape_this_cannot_follow_is_refused() {
        assert!(matches!(include("--8<-- \"CONTRIBUTING.md\""), Some(Include::File(f)) if f == "CONTRIBUTING.md"));
        assert!(matches!(include("  --8<-- 'a/b.md:middle'"), Some(Include::File(f)) if f == "a/b.md"));
        assert!(include("nothing here").is_none());
        // Escaped, so it is text on the page rather than an include.
        assert!(include(";--8<-- \"x.md\"").is_none());
        // The block form and an unquoted path are REFUSED, not silently unread.
        assert!(matches!(include("--8<--"), Some(Include::Unfollowable(_))));
        assert!(matches!(include("--8<-- CONTRIBUTING.md"), Some(Include::Unfollowable(_))));
    }

    #[test]
    fn a_link_inside_an_included_file_is_the_including_pages_link() {
        // THE REPRODUCED BUG. `docs/contributing.md` is a stub; its published body is the
        // repo-root `CONTRIBUTING.md`, and a relative target inside that file resolves against
        // the STUB's URL. The scan read the stub's sixteen lines and the rule was silently false.
        let root = tempdir();
        write(
            &root,
            "docs/contributing.md",
            "---\ntitle: Contributing\n---\n\n--8<-- \"CONTRIBUTING.md\"\n",
        );
        write(&root, "CONTRIBUTING.md", "# How\n\nSee [the plan](implementation-plan.md).\n");
        let pages = [String::from("contributing.md")];
        let published: Vec<&String> = pages.iter().collect();
        let found = sweep(
            &root,
            "docs",
            &published,
            &set(&["implementation-plan.md"]),
            Snippets::Followed,
        );
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
        assert!(
            found.problems.first().is_some_and(|p| p.contains("CONTRIBUTING.md")),
            "the finding must name the file to edit: {:?}",
            found.problems
        );
        assert_eq!((found.read, found.scanned), (1, 1));
    }

    #[test]
    fn an_include_the_sweep_cannot_read_leaves_the_page_unscanned() {
        let root = tempdir();
        write(&root, "docs/contributing.md", "--8<-- \"GONE.md\"\n");
        let pages = [String::from("contributing.md")];
        let published: Vec<&String> = pages.iter().collect();
        let found = sweep(&root, "docs", &published, &set(&["plan.md"]), Snippets::Followed);
        assert_eq!(found.scanned, 0, "{:?}", found.problems);
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
        // And with the extension off, the directive is text mkdocs publishes verbatim.
        let unconfigured = sweep(&root, "docs", &published, &set(&["plan.md"]), Snippets::NotConfigured);
        assert!(
            unconfigured.problems.first().is_some_and(|p| p.contains("literal text")),
            "{:?}",
            unconfigured.problems
        );
    }

    #[test]
    fn a_page_the_sweep_cannot_open_is_a_finding_rather_than_a_smaller_total() {
        // Reproduced by the review that found it: one invalid UTF-8 byte took a dead link out of
        // the scan, the total fell 202 to 196, and the verdict stayed green.
        let root = tempdir();
        write(&root, "docs/index.md", "[a](b.md)\n");
        std::fs::write(root.join("docs/qa.md"), [b'[', b'a', b'x', 0xFF, b']']).unwrap();
        let pages = [String::from("index.md"), String::from("qa.md")];
        let published: Vec<&String> = pages.iter().collect();
        let found = sweep(&root, "docs", &published, &set(&["plan.md"]), Snippets::Followed);
        assert_eq!(found.scanned, 1, "the unreadable page must not count as scanned");
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
        assert!(
            found.problems.first().is_some_and(|p| p.contains("not readable")),
            "{:?}",
            found.problems
        );
    }

    /// A scratch directory for the sweep tests, removed by the OS rather than by a `Drop` this
    /// crate would have to own.
    fn tempdir() -> std::path::PathBuf {
        let unique = format!(
            "sutura-check-docs-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }
}
