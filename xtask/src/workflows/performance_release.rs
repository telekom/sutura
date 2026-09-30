//! Does the optimised build publish what the release publishes, signed, onto a release of its own?
//!
//! `release-performance.yml` once built four binaries and discarded them, and pushed one image for
//! one architecture. What it publishes now comes from the composite actions `release.yml` calls, so
//! this holds that it still CALLS them, and in the order that makes the claim true:
//!
//! | Rule | Why |
//! | --- | --- |
//! | `build` runs `build-artefacts` with `profile: performance` | the tarballs, images and SBOMs of every matrix cell; `cross_link` holds the cells to the full four |
//! | `publish` runs `push-images` with `profile: performance` | four leaves and both manifest lists, and never `latest` |
//! | `publish` runs `attest-and-sign` before the step whose `gh release create` attaches `dist/*` | every attached file has a bundle and provenance before the release exists |
//! | no `gh release upload`, anywhere in the file | an upload onto a published release is `HTTP 422` under immutable releases; one `gh release create` carrying every asset is the only path |
//!
//! # What it does NOT hold
//!
//! * **Whether any of it runs.** `publish` runs only on a dispatch with `publish=true` from a tag,
//!   which nothing in this tree can start, and a step's `if:` is not judged - a condition that never
//!   holds satisfies every rule here.
//! * **What the actions do.** `attest-and-sign` signs what it is handed, so an artefact that never
//!   reaches `dist/` is absent rather than unsigned; `publish`'s arrival check refuses that, and
//!   nothing here reads that check.
//! * **YAML.** An indentation reader, with [`super::step::shell`]'s limits: a step is the lines from
//!   one `      - ` to the next, comment lines dropped.

use std::path::Path;

use super::step::{job, step_key};

/// The optimised build, the one file these rules read.
const WORKFLOW: &str = ".github/workflows/release-performance.yml";

/// Every rule above, broken, over one repository root.
pub(super) fn problems(root: &Path) -> Vec<String> {
    match std::fs::read_to_string(root.join(WORKFLOW)) {
        Ok(text) => judge(&text),
        Err(error) => vec![format!("{WORKFLOW} could not be read: {error}")],
    }
}

fn judge(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let build = steps_of(text, "build");
    if !build
        .iter()
        .any(|step| calls(step, "build-artefacts") && says(step, "profile: performance"))
    {
        found.push(format!(
            "{WORKFLOW}: job `build` does not run `./.github/actions/build-artefacts` with `profile: \
             performance`, so no target gets an optimised image, tarball or SBOM"
        ));
    }
    let publish = steps_of(text, "publish");
    if !publish
        .iter()
        .any(|step| calls(step, "push-images") && says(step, "profile: performance"))
    {
        found.push(format!(
            "{WORKFLOW}: job `publish` does not run `./.github/actions/push-images` with `profile: \
             performance`, so the four leaves and both `-performance` manifest lists are not pushed"
        ));
    }
    let signed = publish.iter().position(|step| calls(step, "attest-and-sign"));
    let created = publish.iter().position(|step| {
        step.iter()
            .any(|line| line.contains("gh release create") && line.contains(" dist/* "))
    });
    if !matches!((signed, created), (Some(signed), Some(created)) if signed < created) {
        found.push(format!(
            "{WORKFLOW}: job `publish` must run `./.github/actions/attest-and-sign` BEFORE the step \
             whose `gh release create` attaches `dist/*` - found the signing step at {signed:?} and \
             the release at {created:?} - or an asset reaches the release unsigned"
        ));
    }
    for (index, line) in text.lines().enumerate() {
        if !line.trim_start().starts_with('#') && line.contains("gh release upload") {
            found.push(format!(
                "{WORKFLOW}:{}: `gh release upload` - an existing release is immutable once \
                 published; attach every asset in the one `gh release create`",
                index.saturating_add(1)
            ));
        }
    }
    found
}

/// The steps of job `name`, each from its `      - ` line up to the next, comment lines dropped.
fn steps_of<'a>(text: &'a str, name: &str) -> Vec<Vec<&'a str>> {
    let mut steps: Vec<Vec<&str>> = Vec::new();
    for line in job(text, name).unwrap_or_default() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with("      - ") {
            steps.push(vec![line]);
        } else if let Some(step) = steps.last_mut() {
            step.push(line);
        }
    }
    steps
}

/// Does `step` run the local composite action `action`?
fn calls(step: &[&str], action: &str) -> bool {
    let want = format!("uses: ./.github/actions/{action}");
    step.iter().any(|line| step_key(line).trim_end() == want)
}

/// Does `step` carry `line`, whatever its indentation?
fn says(step: &[&str], line: &str) -> bool {
    step.iter().any(|candidate| candidate.trim() == line)
}

#[cfg(test)]
mod tests {
    /// Every rule satisfied, in the shape the real file has.
    const SOUND: &str = "\
jobs:
  build:
    strategy:
      matrix:
        target:
          - x86_64-unknown-linux-gnu
    steps:
      - uses: ./.github/actions/build-artefacts
        with:
          profile: performance
  publish:
    steps:
      - uses: ./.github/actions/push-images
        with:
          profile: performance
      - uses: ./.github/actions/attest-and-sign
      - name: Publish
        run: |
          gh release create \"$release\" dist/* --prerelease
";

    fn judged(text: &str) -> Vec<String> {
        super::judge(text)
    }

    /// One mutation of [`SOUND`], which must be refused exactly once, for `because`.
    fn refused(from: &str, to: &str, because: &str) {
        assert!(SOUND.contains(from), "the fixture no longer holds `{from}`");
        let found = judged(&SOUND.replacen(from, to, 1));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains(because), "{found:?}");
    }

    /// THE PRODUCTION ENTRY POINT, against the real tree: each refusal below breaks one input to
    /// this same rule set.
    #[test]
    fn the_optimised_release_publishes_every_signed_artefact() {
        let root = crate::repo::root().expect("repo root");
        let found = super::problems(&root);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn the_synthetic_workflow_is_clean() {
        assert!(judged(SOUND).is_empty(), "{:?}", judged(SOUND));
    }

    /// The release profile's images under the optimised build's name, or none at all.
    #[test]
    fn a_build_without_the_performance_profile_is_refused() {
        refused(
            "- uses: ./.github/actions/build-artefacts\n        with:\n          profile: performance\n",
            "- uses: ./.github/actions/build-artefacts\n",
            "job `build`",
        );
    }

    #[test]
    fn a_publish_without_the_performance_images_is_refused() {
        refused(
            "- uses: ./.github/actions/push-images\n        with:\n          profile: performance\n",
            "- uses: ./.github/actions/push-images\n        with:\n          profile: release\n",
            "push-images",
        );
    }

    /// A binary published without a signature: signing after the release exists, or not at all.
    #[test]
    fn a_release_created_before_signing_is_refused() {
        refused(
            "      - uses: ./.github/actions/attest-and-sign\n      - name: Publish\n        run: |\n          gh release create \"$release\" dist/* --prerelease\n",
            "      - name: Publish\n        run: |\n          gh release create \"$release\" dist/* --prerelease\n      - uses: ./.github/actions/attest-and-sign\n",
            "BEFORE",
        );
        refused(
            "      - uses: ./.github/actions/attest-and-sign\n",
            "      # - uses: ./.github/actions/attest-and-sign\n",
            "unsigned",
        );
    }

    /// A hand-picked glob can leave a signed file behind or attach one the loop never saw.
    #[test]
    fn a_release_attaching_less_than_the_signed_directory_is_refused() {
        refused(" dist/* ", " dist/*.tar.gz ", "attaches `dist/*`");
    }

    #[test]
    fn an_upload_onto_an_existing_release_is_refused() {
        refused(
            "--prerelease\n",
            "--prerelease\n          gh release upload \"$GITHUB_REF_NAME\" dist/*\n",
            "`gh release upload`",
        );
        assert!(
            judged(&SOUND.replacen("--prerelease\n", "--prerelease\n          # gh release upload x\n", 1)).is_empty(),
            "a comment is not an upload"
        );
    }
}
