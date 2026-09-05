//! Which workflow refuses an EMPTY probe manifest, answered by reading rather than by recall.
//!
//! **Its own file for the reason `guidance/claims/remedies.rs` is: the parent is against the
//! unexemptable 1000-line cap.** Unlike that split this one takes its fixtures with it, because
//! they are NEW tests rather than moved ones - a file that adds a `#[test]` is one the causality
//! gate keeps, so nothing here can be orphaned by reverting the parent. The one assertion left
//! behind is the one over the real tree, which needs the parent's file walk.

use std::collections::BTreeMap;

/// The two literals that together identify the workflow refusing an EMPTY probe manifest.
///
/// `feature-probes-` is the fixed attribute name `nix/shipped.nix` builds the manifest at, and the
/// `-s` test is the refusal. BOTH, in one file: the attribute alone is a step that reads however
/// many rows the manifest has and loops over them, which is the shape that measures nothing.
pub(super) const PROBE_REFUSAL: [&str; 2] = ["feature-probes-", "-s \"$manifest\""];

/// Which workflow or action holds every one of `needles`, as a repo-relative path.
///
/// **DERIVED, because the literal rotted and nothing saw it.** Two of the parent's remedies named
/// `ci.yml` as the file refusing an empty probe manifest; the step moved into a reusable
/// workflow and the sentences kept pointing at a file that no longer held it, green through
/// `just hygiene` the whole time. `check-guidance` cannot catch that - a bare filename with no slash is deliberately unread
/// there, because which of eight workflows a sentence meant would be a guess and a gate may not
/// guess - so the file is printed rather than written down, and no claim is left for a move to
/// falsify. ONE FILE OR NONE: two refusals are two copies to keep in step, and a list orients
/// nobody.
pub(super) fn hosting(files: &BTreeMap<String, String>, needles: &[&str]) -> Option<String> {
    let mut found = files
        .iter()
        .filter(|(_, text)| needles.iter().all(|needle| text.contains(needle)))
        .map(|(name, _)| name.clone());
    let one = found.next()?;
    found.next().is_none().then_some(one)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    #[test]
    fn the_workflow_refusing_an_empty_probe_manifest_is_found_and_not_guessed() {
        let body = String::from("nix build \".#feature-probes-${TARGET}\"\nif [ ! -s \"$manifest\" ]; then\n");
        let mut files = BTreeMap::from([
            (String::from(".github/workflows/link.yml"), body.clone()),
            (
                String::from(".github/workflows/ci.yml"),
                String::from("uses: ./.github/workflows/link.yml\n"),
            ),
        ]);
        assert_eq!(
            super::hosting(&files, &super::PROBE_REFUSAL).as_deref(),
            Some(".github/workflows/link.yml")
        );
        // A SECOND file refusing it is a verdict rather than a pointer: two copies to keep in step,
        // and a remedy that hands a reader a list orients nobody.
        files.insert(String::from(".github/workflows/other.yml"), body);
        assert_eq!(super::hosting(&files, &super::PROBE_REFUSAL), None);
    }

    #[test]
    fn reading_the_probe_manifest_without_refusing_an_empty_one_is_not_the_refusal() {
        // BOTH needles, because the attribute alone is a step that loops over however many rows the
        // manifest has - zero included, which is the shape that measures nothing and reads green.
        let files = BTreeMap::from([(
            String::from(".github/workflows/link.yml"),
            String::from("manifest=\"$(nix build \".#feature-probes-${TARGET}\")\"\nwhile read -r probe; do\n"),
        )]);
        assert_eq!(super::hosting(&files, &super::PROBE_REFUSAL), None);
    }
}
