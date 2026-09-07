//! What a line of code NAMES under `examples/`, and what the published listing says about it.
//!
//! Split from the rule for the reason [`super::scope`] is: this half is a string question - which
//! substring on a line is a repo-relative reach, and which of them git actually publishes - and
//! every function in it is pure over a listing. The rule that decides whether such a line is
//! EVIDENCE is [`super`]'s, and it needs whether the test around the line runs, which is a
//! different question entirely.

use std::collections::BTreeSet;

/// The directory whose children are the deployment variants, with the separator that makes it a
/// path prefix. One constant: a scan for the segment and a message naming the directory must not
/// drift apart.
pub(super) const EXAMPLES: &str = "examples/";

/// The variant a reach is evidence for, or `None` if the path it names is not one git publishes.
///
/// The scan reads a LITERAL and the literal is repo-relative only under an assumption about the
/// root it is joined to - `github.com/telekom/sutura#308`'s second limit. Requiring the path to
/// resolve against the published listing does not close that (measured: the issue's own
/// `tmp.join("examples/multi-player")` names a directory this repository publishes, so it still
/// counts); what it removes is the shape a synthetic corpus actually takes, a FABRICATED path at
/// the same anchor - and, in the same move, a stale one, which used to hold a variant green while
/// naming a file nothing could open.
///
/// A directory counts through the files under it, because git publishes no directory: one
/// authority for this and for [`variants`], which is what stops the two halves disagreeing.
pub(super) fn resolved(reach: &str, published: &BTreeSet<String>) -> Option<String> {
    if !published.contains(reach) {
        return None;
    }
    reach
        .strip_prefix(EXAMPLES)
        .map(|rest| rest.split('/').next().unwrap_or_default())
        .filter(|name| !name.is_empty())
        .map(String::from)
}

/// Every path under `examples/` this line of code reaches for.
///
/// `find` in a loop rather than once, because a second path on the same line used to be invisible.
/// The whole path rather than the variant name alone, so [`resolved`] can ask the listing about
/// what the line actually names: `examples/x` and `examples/x/corpus-with-two-metrics.json` are
/// the same variant and are not the same claim.
pub(super) fn reaches_for(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = 0_usize;
    while let Some(offset) = line.get(at..).and_then(|rest| rest.find(EXAMPLES)) {
        let start = at.saturating_add(offset);
        at = start.saturating_add(EXAMPLES.len());
        if !anchored(line.get(..start).unwrap_or_default()) {
            continue;
        }
        let rest: String = line
            .get(at..)
            .unwrap_or_default()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(*c, '-' | '_' | '.' | '/'))
            .collect();
        let named = rest.trim_end_matches('/');
        if !named.is_empty() {
            found.push(format!("{EXAMPLES}{named}"));
        }
    }
    found
}

/// Is a path boundary in front of the match, of one of the two shapes a repo-relative reach takes?
///
/// The start of a string literal, or a `../` walking up out of `CARGO_MANIFEST_DIR`. Measured over
/// this workspace: those two cover every reach in it, and nothing else does. `#` is deliberately
/// not a comment marker anywhere here - it opens one in a shell and an ATTRIBUTE in Rust, and
/// `#[path = "../../examples/x/mod.rs"]` is a real reach.
pub(super) fn anchored(before: &str) -> bool {
    before.is_empty() || before.ends_with('"') || before.ends_with("../")
}

/// The deployment variants, read off the same listing the evidence is read from.
///
/// One authority for both halves of the gate. `read_dir` was the other candidate and disagreed
/// with the scan in two directions, both reproduced: `mkdir examples/x` was a local FAILURE that
/// the git-derived nix sandbox and CI could not see, because git tracks no empty directory - a red
/// no venue reproduces; and a gitignored or symlinked child was a variant that can never be
/// published, while `hygiene` is a pre-commit hook, so it blocked every commit over a path git
/// will never publish. A directory git would not publish is not one a reader can find, which is
/// the failure this gate exists for.
pub(super) fn variants(files: &[String]) -> BTreeSet<String> {
    files
        .iter()
        .filter_map(|rel| rel.strip_prefix(EXAMPLES))
        .filter_map(|rest| rest.split_once('/'))
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, _)| String::from(name))
        .collect()
}

/// Every path under `examples/` git publishes, each directory included through the files in it.
///
/// The same listing [`variants`] reads, for the same reason: a reach resolved against the
/// filesystem and a variant read off git would disagree exactly where that function's doc says
/// they did. A directory is in no listing, so each ancestor of a published file is added here -
/// which is what lets `join("../../examples/single-player")` resolve while
/// `join("../../examples/single-player/gone.yaml")` does not.
pub(super) fn publishes(files: &[String]) -> BTreeSet<String> {
    let mut published = BTreeSet::new();
    for rel in files.iter().filter(|rel| rel.starts_with(EXAMPLES)) {
        let mut at = 0_usize;
        while let Some(offset) = rel.get(at..).and_then(|rest| rest.find('/')) {
            at = at.saturating_add(offset).saturating_add(1);
            published.insert(String::from(rel.get(..at.saturating_sub(1)).unwrap_or_default()));
        }
        published.insert(rel.clone());
    }
    published
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{reaches_for, variants};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn several_variants_on_one_line_are_all_read() {
        // `find` used to be read once per line, so a second path on the same line was invisible.
        assert_eq!(
            reaches_for("let both = (\"examples/one/data\", \"examples/two\");"),
            vec![String::from("examples/one/data"), String::from("examples/two")],
            "the whole path, so the listing can be asked about what the line names"
        );
        assert_eq!(
            reaches_for("let p = \"../../examples/x/\";"),
            vec![String::from("examples/x")],
            "a trailing separator names the directory, not a child of it"
        );
    }

    #[test]
    fn a_variant_is_a_directory_git_would_publish() {
        // The same listing the evidence comes from, so the two halves cannot disagree. An empty
        // directory is in NO listing, which is exactly why the filesystem was the wrong authority.
        assert_eq!(
            variants(&[
                String::from("examples/README.md"),
                String::from("examples/a-variant/README.md"),
                String::from("examples/b-variant/data/rows.csv"),
                String::from("crates/x/examples/not-a-variant/main.rs"),
            ]),
            set(&["a-variant", "b-variant"]),
            "a file directly under examples/ is not a variant, and a per-crate examples/ is not ours"
        );
        assert!(
            variants(&[String::from("examples/a-variant")]).is_empty(),
            "a file, not a directory"
        );
    }
}
