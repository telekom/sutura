//! A path in backticks that names a real thing must exist.
//!
//! `github.com/telekom/sutura#638`: the rule used to check a backticked path only under
//! `.agents/`, so a `docs/**` page could cite code that had moved and every gate stayed green -
//! the registry file was cited by its old name 15 times across ten files. Its own module for
//! `advice`'s reason in the parent header: this file has to have room for the tables, and this
//! scan shares nothing with them but the walk that reads text into memory.

use std::path::Path;

use crate::repo;

/// The directories a cited path may be checked against: this repository's own.
///
/// A path in SOMEONE ELSE'S repository is indistinguishable from a stale one by any local check -
/// upstream source files, a cloud role name and an org/repo slug are all path-shaped and all
/// correct - so the scope is first-party or nothing.
const FIRST_PARTY: &[&str] = &["crates/", "xtask/", "docs/", ".github/", "nix/", "demo/"];

/// The path a backticked token CITES, if it cites one at all.
///
/// Two scopes, because two different things get written in backticks. Under `.agents/` every
/// path-shaped token is a ROUTE a router sends an agent to, so all of them are checked and a dead
/// route is the failure that catches. Elsewhere a first-party path is a citation only when its
/// last segment carries an extension, i.e. when it names a FILE: a trailing name without one is as
/// often a record, a planned document or a two-word slug as a directory, and refusing those
/// reddens correct writing - which is worse than the rot, because a gate people disable catches
/// nothing.
///
/// **A `:line` or `:start-end` suffix is stripped first.** That is the most precise citation form
/// this repository writes and a bare `exists()` refuses every one of them.
fn cited_path(candidate: &str) -> Option<&str> {
    if candidate.contains(['*', ' ', '<']) {
        return None;
    }
    if candidate.starts_with(".agents/") {
        return Some(candidate);
    }
    if !FIRST_PARTY.iter().any(|prefix| candidate.starts_with(prefix)) {
        return None;
    }
    let path = without_line_suffix(candidate);
    path.rsplit('/').next().filter(|last| last.contains('.')).map(|_| path)
}

/// `generate.rs:302-307` cites `generate.rs`; the line numbers are not part of the path.
fn without_line_suffix(candidate: &str) -> &str {
    match candidate.rsplit_once(':') {
        Some((head, tail))
            if tail.chars().any(|c| c.is_ascii_digit()) && tail.chars().all(|c| c.is_ascii_digit() || c == '-') =>
        {
            head
        }
        _ => candidate,
    }
}

/// Every path a page's prose cites, with the line it sits on, as the gate reads them.
///
/// One definition, called by [`dead_paths`] and by the test that asserts this walk reads the tree
/// at all: a test carrying its own COPY of the extraction witnesses nothing about the gate's.
fn citations(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines().enumerate().flat_map(|(i, line)| {
        line.split('`')
            .skip(1)
            .step_by(2)
            .filter_map(move |piece| cited_path(piece.trim()).map(|path| (i.saturating_add(1), path)))
    })
}

/// A path in backticks that [`cited_path`] reads as a citation must exist.
///
/// **The escape hatch is the bare filename, and it is deliberate**: prose whose POINT is that a
/// path does not exist writes the file name without its directory prefix, which is shorthand
/// rather than a citation and out of scope by design. The refusal says so, because a rule whose
/// remedy is not beside it gets worked around instead of read.
///
/// Limit: the extraction is per LINE, so an inline-code span wrapped across two lines is missed.
/// That under-reports; it cannot redden a correct tree.
pub(in crate::guidance) fn dead_paths(root: &Path, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for rel in files {
        if !super::has_ext(rel, &["md"]) {
            continue;
        }
        let Some(text) = repo::read_subject(root, rel, &mut problems) else {
            continue;
        };
        for (line, path) in citations(&text) {
            if !root.join(path).exists() {
                problems.push(format!(
                    "{rel}:{line}: `{path}` does not exist\n      \
                     if the point IS that it does not exist, name the file without its directory prefix"
                ));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::{cited_path, dead_paths};

    /// What counts as a path CITATION, in both directions.
    ///
    /// The negative half is the point: this rule was scoped to `.agents/` precisely because a
    /// naive "every token that looks like a file" rule reddens a correct tree, and measured over
    /// this repository the families below are the ones that would have done it.
    #[test]
    fn a_first_party_file_is_cited_and_a_slug_is_not() {
        // Checked: a first-party file, with and without the line suffix that makes a citation
        // precise. The suffix is stripped rather than refused.
        assert_eq!(
            cited_path("crates/sutura-app/tests/adapters/adapters.rs"),
            Some("crates/sutura-app/tests/adapters/adapters.rs")
        );
        assert_eq!(cited_path("xtask/src/guidance.rs:383"), Some("xtask/src/guidance.rs"));
        assert_eq!(
            cited_path("crates/sutura-sql/src/generate.rs:302-307"),
            Some("crates/sutura-sql/src/generate.rs")
        );
        // Checked: every `.agents/` route, extension or not, because a router sends an agent there.
        assert_eq!(cited_path(".agents/skills/sutura"), Some(".agents/skills/sutura"));
        // NOT checked, and each one is a measured false-positive family:
        assert_eq!(cited_path("hosts.rs"), None, "a bare filename is prose shorthand");
        assert_eq!(cited_path("docs/inbound-identity"), None, "a planned record, never a file");
        assert_eq!(
            cited_path("docs/generated/"),
            None,
            "a directory, and this one is named to say it is absent"
        );
        assert_eq!(cited_path("sutura/invariants"), None, "a skill route is not a path");
        assert_eq!(
            cited_path("roles/bigquery.dataEditor"),
            None,
            "a cloud role name is path-shaped"
        );
        assert_eq!(cited_path("src/transport.rs"), None, "a path in someone else's repository");
        assert_eq!(cited_path("crates/<name>/Cargo.toml"), None, "a pattern, not a citation");
        assert_eq!(cited_path("docs/adr/0016"), None, "an abbreviated record reference");
    }

    /// The refusal itself, planted and then removed - a gate nobody has seen fail is not known to
    /// work, and the predicate above passing proves nothing about the walk calling it.
    #[test]
    fn a_dead_first_party_citation_is_refused() {
        let dir = std::env::temp_dir().join(format!("sutura-citations-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        std::fs::create_dir_all(dir.join("crates/sutura-app/tests/adapters")).expect("a fixture tree");
        std::fs::write(dir.join("crates/sutura-app/tests/adapters/adapters.rs"), "// the registry\n").expect("the fixture file");
        let rel = String::from("record.md");
        let files = vec![rel.clone()];

        // PLANTED: the shape this gate was written for - a page citing a file that moved.
        std::fs::write(dir.join(&rel), "The registry is `crates/sutura-app/tests/adapters/mod.rs`.\n").expect("the page");
        let problems = dead_paths(&dir, &files);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("record.md:1:"), "{problems:?}");
        assert!(
            problems[0].contains("without its directory prefix"),
            "the refusal must carry its remedy: {problems:?}"
        );

        // REMOVED: the same sentence, citing where the file actually lives.
        std::fs::write(
            dir.join(&rel),
            "The registry is `crates/sutura-app/tests/adapters/adapters.rs`.\n",
        )
        .expect("the page");
        assert!(dead_paths(&dir, &files).is_empty(), "a live citation must pass");

        // And the escape hatch works: the bare filename is not a citation even though no such
        // file exists anywhere in the fixture.
        std::fs::write(dir.join(&rel), "An earlier draft cited `federation.rs`, which is not here.\n").expect("the page");
        assert!(dead_paths(&dir, &files).is_empty(), "a bare filename must not be refused");
        std::fs::remove_dir_all(&dir).unwrap_or_default();
    }

    /// FAIL CLOSED over the real tree: an extraction that reads nothing passes everything.
    ///
    /// It calls the gate's own [`super::citations`] rather than repeating the walk, so breaking the
    /// extraction reddens this. Limit, stated because it is the #414 shape: this witnesses a BROKEN
    /// walk, not a NARROWED one - both numbers come from the same extraction, so a rule that
    /// stopped reading half the tree would still satisfy it.
    #[test]
    fn the_citation_walk_reads_both_scopes_of_the_real_tree() {
        let (root, all) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::Guidance))
            .expect("the file census");
        let (mut routes, mut files) = (0_usize, 0_usize);
        for rel in super::super::in_scope(&all) {
            if !super::super::has_ext(&rel, &["md"]) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
                continue;
            };
            for (_, path) in super::citations(&text) {
                if path.starts_with(".agents/") {
                    routes = routes.saturating_add(1);
                } else {
                    files = files.saturating_add(1);
                }
            }
        }
        assert!(
            routes > 0,
            "read no `.agents/` route out of any page - the extraction is broken, not the tree"
        );
        assert!(
            files > 0,
            "read no first-party file citation out of any page - the extraction is broken, not the tree"
        );
    }
}
