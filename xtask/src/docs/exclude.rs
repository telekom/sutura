//! `exclude_docs` matching, the way mkdocs matches it.
//!
//! **This gate compared an exclusion against the page list as a docs-root-anchored path, and
//! mkdocs does not.** `mkdocs/structure/files.py` builds a `pathspec.gitignore.GitIgnoreSpec`
//! from `exclude_docs`, where a pattern containing no `/` matches at ANY depth. So the set of
//! pages mkdocs dropped and the set this gate believed were dropped were different sets, and both
//! of the rules that read the set were bypassed on the difference: a nav entry to an excluded
//! page, and a published page linking one. Reproduced on the tree that shipped it: a page added
//! at `docs/notes/implementation-plan.md`, navigated to AND linked from a published page, left
//! `check-docs` green and `mkdocs build --strict` at exit 0 with two INFO lines.
//!
//! Only the subset the gate accepts is implemented, and the rest is REFUSED rather than
//! approximated: a glob, a directory pattern or a `!` negation is a finding, because a pattern
//! this cannot resolve is an exclusion nothing checks.
//!
//! **The collision is live rather than hypothetical.** `docs/index.md` and `docs/api/index.md`
//! share a basename, so an exclusion naming `index.md` drops both - and the old matcher resolved
//! it to one file and reported `1 excluded`. A pattern matching more than one page is therefore a
//! finding too, which is what makes the published claim that an exclusion "resolves to one file"
//! true rather than aspirational.

use std::collections::BTreeSet;

use super::CONFIG;

/// What makes a line a pattern rather than a path this gate can resolve.
const PATTERN_CHARS: &[char] = &['*', '?', '[', ']'];

/// The pages an exclusion list keeps off the site, and what is wrong with the list.
pub(super) struct Resolved {
    /// Every page mkdocs drops, docs-relative. The set every other rule reads.
    pub(super) pages: BTreeSet<String>,
    /// The findings.
    pub(super) problems: Vec<String>,
}

/// Every page `pattern` matches, under the gitignore semantics mkdocs applies.
///
/// Two cases and no more, because the shapes that need the rest are refused before this is
/// reached: a pattern containing a `/` is anchored at the docs directory, and one containing none
/// matches a BASENAME at any depth.
fn matching<'a>(pattern: &str, present: &'a BTreeSet<String>) -> Vec<&'a String> {
    if pattern.contains('/') {
        let anchored = pattern.trim_start_matches('/');
        return present.iter().filter(|page| page.as_str() == anchored).collect();
    }
    present
        .iter()
        .filter(|page| page.rsplit('/').next() == Some(pattern))
        .collect()
}

/// The exclusions, resolved against the pages on disk and judged against the nav.
///
/// Pure, so every arm is testable without a repo - the same reason the nav rule is.
pub(super) fn resolve(patterns: &[String], present: &BTreeSet<String>, nav: &BTreeSet<String>, docs_dir: &str) -> Resolved {
    let mut out = Resolved {
        pages: BTreeSet::new(),
        problems: Vec::new(),
    };
    for pattern in patterns {
        if pattern.starts_with('!') || pattern.ends_with('/') || pattern.contains(PATTERN_CHARS) {
            out.problems.push(format!(
                "{CONFIG} excludes `{pattern}`, which is a pattern rather than a page - this gate does not implement ignore-file syntax, so it cannot say which files that keeps off the site. Name each page literally"
            ));
            continue;
        }
        let matched = matching(pattern, present);
        if matched.is_empty() {
            out.problems.push(format!(
                "{CONFIG} excludes `{pattern}`, but there is no `{docs_dir}/{pattern}` - an exclusion over nothing reads as a page being kept off the site while none is, so delete it"
            ));
        }
        if matched.len() > 1 {
            let named: Vec<&str> = matched.iter().map(|page| page.as_str()).collect();
            out.problems.push(format!(
                "{CONFIG} excludes `{pattern}`, which names no directory - mkdocs matches such a pattern at EVERY depth, so it keeps {} pages off the site: {}. Write the path from the docs directory instead, so this list and the built site name the same pages",
                matched.len(),
                named.join(", ")
            ));
        }
        for page in matched {
            if nav.contains(page) {
                out.problems.push(format!(
                    "{CONFIG} both navigates to `{page}` and excludes it - mkdocs drops an excluded page from the build, so the nav entry is a link to nothing. Choose one"
                ));
            }
            out.pages.insert(page.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::resolve;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    fn patterns(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn a_pattern_with_no_slash_matches_at_any_depth_the_way_mkdocs_matches_it() {
        // THE REPRODUCED BUG. `docs/notes/implementation-plan.md` is dropped by mkdocs and was
        // not in this gate's excluded set, so a nav entry to it and a link into it were both
        // green while `site/notes/` did not exist.
        let present = set(&["index.md", "implementation-plan.md", "notes/implementation-plan.md"]);
        let found = resolve(&patterns(&["implementation-plan.md"]), &present, &set(&["index.md"]), "docs");
        assert_eq!(
            found.pages,
            set(&["implementation-plan.md", "notes/implementation-plan.md"]),
            "both depths are excluded"
        );
        // And it says so, because a pattern naming two pages cannot be the one page the
        // configuration's own comment claims it resolves to.
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
        assert!(
            found.problems.first().is_some_and(|p| p.contains("EVERY depth")),
            "{:?}",
            found.problems
        );
    }

    #[test]
    fn a_pattern_with_a_slash_is_anchored_at_the_docs_directory() {
        let present = set(&["index.md", "notes/why.md", "adr/notes/why.md"]);
        let found = resolve(&patterns(&["notes/why.md"]), &present, &set(&["index.md"]), "docs");
        assert_eq!(found.pages, set(&["notes/why.md"]), "{:?}", found.problems);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        // A leading `/` anchors the same way, which is what gitignore does with one.
        let leading = resolve(&patterns(&["/notes/why.md"]), &present, &set(&["index.md"]), "docs");
        assert_eq!(leading.pages, set(&["notes/why.md"]), "{:?}", leading.problems);
    }

    #[test]
    fn the_live_basename_collision_is_reported_rather_than_resolved_to_one_file() {
        // `docs/index.md` and `docs/api/index.md` share a basename in this tree today, so an
        // exclusion naming `index.md` would drop the API landing page too.
        let present = set(&["index.md", "api/index.md", "gates.md"]);
        let found = resolve(&patterns(&["index.md"]), &present, &set(&["gates.md"]), "docs");
        assert!(
            found.problems.iter().any(|p| p.contains("api/index.md")),
            "the second page it drops has to be named: {:?}",
            found.problems
        );
    }

    #[test]
    fn excluding_a_page_that_is_not_there_is_a_problem() {
        // The shape a stale exclusion takes: it reads as a page being kept off the site while
        // nothing is, and the nav check cannot see it because the page is gone.
        let found = resolve(
            &patterns(&["implementation-plan.md"]),
            &set(&["index.md"]),
            &set(&["index.md"]),
            "docs",
        );
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
        assert!(
            found
                .problems
                .first()
                .is_some_and(|p| p.contains("an exclusion over nothing")),
            "{:?}",
            found.problems
        );
        assert!(found.pages.is_empty());
    }

    #[test]
    fn excluding_a_page_the_nav_also_names_is_a_problem_at_every_depth_it_matches() {
        // mkdocs drops an excluded page from the build, so the nav entry links to nothing - and
        // the nav entry that matters is the one for the page the PATTERN reaches, not the one
        // spelled like the pattern.
        let found = resolve(
            &patterns(&["gates.md"]),
            &set(&["index.md", "ref/gates.md"]),
            &set(&["index.md", "ref/gates.md"]),
            "docs",
        );
        assert!(
            found.problems.iter().any(|p| p.contains("Choose one")),
            "{:?}",
            found.problems
        );
    }

    #[test]
    fn a_pattern_this_gate_cannot_resolve_to_a_file_is_refused() {
        // Refused rather than approximated: this gate does not implement ignore-file matching,
        // so an exclusion it cannot resolve is an exclusion nothing checks.
        for pattern in ["*.md", "drafts/", "!keep.md", "plan-[12].md"] {
            let found = resolve(&patterns(&[pattern]), &set(&["index.md"]), &set(&["index.md"]), "docs");
            assert_eq!(found.problems.len(), 1, "{pattern}: {:?}", found.problems);
            assert!(
                found.problems.first().is_some_and(|p| p.contains("Name each page literally")),
                "{pattern}: {:?}",
                found.problems
            );
        }
        // And a literal path that resolves is not.
        let clean = resolve(
            &patterns(&["notes/why-the-order.md"]),
            &set(&["index.md", "notes/why-the-order.md"]),
            &set(&["index.md"]),
            "docs",
        );
        assert!(clean.problems.is_empty(), "{:?}", clean.problems);
        assert_eq!(clean.pages, set(&["notes/why-the-order.md"]));
    }
}
