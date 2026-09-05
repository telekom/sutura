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
//! and exited **0**. `--strict` escalates WARNINGs, and mkdocs logs this one at INFO - so a dead
//! link on the published site sits behind a green docs job, which is the same failure shape
//! `validation.omitted_files` exists for and the reason the other half of this gate counts pages
//! rather than trusting a build.
//!
//! **The limits, next to the claim.** This reads inline markdown links, `](target)`, outside
//! fenced code blocks. A reference-style link, an HTML `<a href>`, a link to a directory URL and
//! a target that is not a `.md` path are not read. A link to a page that does not exist AT ALL is
//! not this rule's business either: mkdocs' `unrecognized_links` covers that one as a warning,
//! which `--strict` does escalate - measured on a cross-crate rustdoc link, recorded in
//! `.agents/skills/sutura/gates/SKILL.md`.

use std::collections::BTreeSet;

/// Every inline link target in `text`, as written, outside fenced code blocks.
///
/// A fence is skipped because a page that DOCUMENTS a link would otherwise be a finding - the
/// gate would then need an exemption list, and an exemption list is a hole anything can be added
/// to.
fn targets(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("](") {
            let after = rest.get(at.saturating_add(2)..).unwrap_or("");
            let end = after.find(')').unwrap_or(after.len());
            if let Some(target) = after.get(..end) {
                found.push(target.trim());
            }
            rest = after.get(end..).unwrap_or("");
        }
    }
    found
}

/// The docs-relative page a link points at, if it points at a page in this tree at all.
///
/// `from` is the page holding the link, docs-relative. A leading `/` is resolved against the docs
/// directory, which is what `validation.absolute_links: relative_to_docs` tells mkdocs to do, so
/// both spellings reach the same rule.
fn resolve(from: &str, target: &str) -> Option<String> {
    if target.is_empty() || target.contains("://") || target.starts_with('#') || target.starts_with("mailto:") {
        return None;
    }
    // An anchor or a link title is not part of the path.
    let path = target.split(['#', '?']).next()?.split_whitespace().next()?;
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

/// One page's links, judged against the exclusions: the problems, and how many links to a page
/// in this tree were read at all.
///
/// The second number is what makes the sweep's fail-closed check possible. A scanner that reads
/// nothing reports nothing, which is indistinguishable from a clean tree from the verdict line -
/// the shape `check-guidance` guards with its printed-citation count.
pub(super) fn problems(from: &str, text: &str, excluded: &BTreeSet<String>, docs_dir: &str) -> (Vec<String>, usize) {
    let mut problems = Vec::new();
    let mut read = 0_usize;
    for target in targets(text) {
        let Some(page) = resolve(from, target) else {
            continue;
        };
        read = read.saturating_add(1);
        if excluded.contains(&page) {
            problems.push(format!(
                "`{docs_dir}/{from}` links to `{target}`, which `exclude_docs` keeps out of the build - mkdocs logs that at INFO and exits 0, so the published page gets a dead link. Cite it as a backticked path instead"
            ));
        }
    }
    (problems, read)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{problems, resolve, targets};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn a_target_is_read_once_per_link_and_not_out_of_a_fence() {
        assert_eq!(
            targets("see [one](a.md) and [two](b/c.md 'titled')"),
            vec!["a.md", "b/c.md 'titled'"]
        );
        // A page documenting a link is not making one.
        assert_eq!(targets("```\n[shown](a.md)\n```\n[real](b.md)\n"), vec!["b.md"]);
        assert!(targets("nothing here").is_empty());
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
        let (found, read) = problems(
            "adr/0013-x.md",
            "[the plan](../implementation-plan.md) carried a row",
            &excluded,
            "docs",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("dead link"), "{found:?}");
        assert_eq!(read, 1);

        let (clean, read) = problems(
            "adr/0013-x.md",
            "[the other record](0009-y.md) and `docs/implementation-plan.md` in backticks",
            &excluded,
            "docs",
        );
        assert!(clean.is_empty(), "{clean:?}");
        // The backticked path is prose and not a link, so one link was read and not two.
        assert_eq!(read, 1);
    }
}
