//! The generator is the API reference's only owner: no page under `docs/api/` may be tracked.
//!
//! **An absence that nothing witnesses silently returns.** `.gitignore` keeps the directory out of a
//! working tree, and a tracked file ignores it: a merge that keeps a page, a branch that adds one
//! or `git add -f` puts a copy back, small enough to pass every size gate and stale from the next
//! doc comment. `check-attribution-owner` is the precedent for holding a deliberate absence with a
//! check rather than a sentence, and has the same `Reads::Code` reasoning.
//!
//! **Limit:** it reads the repository's file listing, which is the tracked files under git and every
//! file in the nix sandbox, so a page generated locally by `just api` is not a finding - that is the
//! ignored use. It judges nothing about what a page says.

use crate::Verdict;
use crate::repo;

use super::PAGES_DIR;

/// Every listed path under the pages directory, in listing order.
fn pages_in(paths: &[&str]) -> Vec<String> {
    let prefix = format!("{PAGES_DIR}/");
    paths
        .iter()
        .filter(|path| path.starts_with(&prefix))
        .map(|path| String::from(*path))
        .collect()
}

const fn every_candidate(_rel: &str) -> bool {
    true
}

/// `xtask check-api-pages-untracked`.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask check-api-pages-untracked: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let mut listed = Vec::new();
    let scope: repo::Scope = every_candidate;
    if let Err(why) = census.inspect(&["flake.nix"], scope, |rel, _bytes| listed.push(String::from(rel))) {
        eprintln!("xtask check-api-pages-untracked: FAILED - {}", why.describe());
        return Verdict::Fail;
    }
    let paths: Vec<&str> = listed.iter().map(String::as_str).collect();
    let found = pages_in(&paths);
    if found.is_empty() {
        println!(
            "xtask check-api-pages-untracked: ok - no tracked page under {PAGES_DIR}/ among {} file(s)",
            paths.len()
        );
        return Verdict::Pass;
    }
    eprintln!("xtask check-api-pages-untracked: FAILED - a generated page is tracked");
    for page in &found {
        eprintln!("  {page}");
    }
    eprintln!("\nThe API reference is generated at publish and committed nowhere. `git rm` the page; `just api`");
    eprintln!("writes it into a directory git ignores. A committed copy goes stale on the next doc comment.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::pages_in;

    #[test]
    fn only_a_path_under_the_pages_directory_is_a_tracked_page() {
        let found = pages_in(&[
            "docs/api/sutura-domain.md",
            "docs/api/nested/x.md",
            "docs/apiary.md",
            "docs/api.md",
            "crates/a/docs/api/y.md",
        ]);
        assert_eq!(found, ["docs/api/sutura-domain.md", "docs/api/nested/x.md"]);
    }
}
