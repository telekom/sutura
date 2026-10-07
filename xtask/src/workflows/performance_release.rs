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
//! | that `gh release create` line carries `--prerelease` and `--latest=false` | without both, the optimised build becomes the repository's Latest release, which `docs/getting-started.md` downloads from |
//! | no `gh release upload`, anywhere in the file | an upload onto a published release is `HTTP 422` under immutable releases; one `gh release create` carrying every asset is the only path |
//! | every workflow's `tags:` trigger that takes `v*` also carries `!v*-performance` | the optimised build's tag matches `v*`, and `release.yml` started from one moves the `latest` image to it, `docs.yml` publishes a directory for it |
//!
//!
//! And one cell that EXECUTES rather than reads: `push-images`' own shell, run at both profiles
//! against a `docker` shell function that records its arguments, must tag `latest` at `release`
//! and never at `performance`. The stub answers every push with a digest and every list with both
//! platforms, so what that cell proves is the tags the script asks for, not that a registry takes
//! them. A second runs the release step's own shell against a `gh` shell function and holds the
//! image tags its notes name to the lists that `push-images` run creates: the release is
//! `v<version>-performance`, the images keep no `v`. Three more run real shell: `push-images`
//! and `docs.yml`'s ref guard refuse the optimised tag as a ref, and the release step refuses a tag
//! of its name already at another commit.
//!
//! # What it does NOT hold
//!
//! * **Whether any of it runs.** `publish` runs only on a dispatch with `publish=true` from a tag,
//!   which nothing in this tree can start, and a step's `if:` is not judged - a condition that never
//!   holds satisfies every rule here.
//! * **What the actions do.** `attest-and-sign` signs what it is handed, so an artefact that never
//!   reaches `dist/` is absent rather than unsigned; `publish`'s arrival check refuses that, and
//!   nothing here reads that check.
//! * **Tag creation.** A tag of the release's name created between the release step's check and
//!   its `gh release create` is not caught.
//! * **YAML.** A `tags:` trigger is read as one flow-form line; any other form is refused rather
//!   than parsed. An indentation reader, with [`super::step::shell`]'s limits: a step is the lines from
//!   one `      - ` to the next, comment lines dropped.

use std::path::Path;

use super::step::{job, step_key};

/// The optimised build, the one file these rules read.
const WORKFLOW: &str = ".github/workflows/release-performance.yml";

/// Every rule above, broken, over one repository root.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let mut found = match std::fs::read_to_string(root.join(WORKFLOW)) {
        Ok(text) => judge(&text),
        Err(error) => vec![format!("{WORKFLOW} could not be read: {error}")],
    };
    match super::sources::ci_sources(root) {
        Some(sources) => {
            for source in sources.iter().filter(|source| !source.label.contains('/')) {
                found.extend(triggers(&source.label, &source.text));
            }
        }
        None => found.push(String::from(
            "the CI sources could not be read, so no `tags:` trigger was checked",
        )),
    }
    found
}

/// Each `tags:` line of one workflow that an optimised build's `v<version>-performance` tag can start.
fn triggers(label: &str, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("tags:"))
        .filter(|(_, line)| {
            let value = line.trim_start().trim_start_matches("tags:").trim_start();
            !value.starts_with('[') || (value.contains("\"v*\"") && !value.contains("\"!v*-performance\""))
        })
        .map(|(index, _)| {
            format!(
                "{label}:{}: a `tags:` trigger that is not one flow-form line, or takes `v*` without \
                 `\"!v*-performance\"` - an optimised build's tag would start this workflow",
                index.saturating_add(1)
            )
        })
        .collect()
}

fn judge(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let build = steps_of(text, "build");
    if !build
        .iter()
        .any(|step| calls(step, "build-artefacts") && says(step, "profile: performance"))
    {
        found.push(format!(
            "{WORKFLOW}: job `build` does not run `build-artefacts` with `profile: \
             performance`, so no target gets an optimised image, tarball or SBOM"
        ));
    }
    let publish = steps_of(text, "publish");
    if !publish
        .iter()
        .any(|step| calls(step, "push-images") && says(step, "profile: performance"))
    {
        found.push(format!(
            "{WORKFLOW}: job `publish` does not run `push-images` with `profile: \
             performance`, so the four leaves and both `-performance` manifest lists are not pushed"
        ));
    }
    let signed = publish.iter().position(|step| calls(step, "attest-and-sign"));
    let is_release = |line: &str| line.contains("gh release create") && line.contains(" dist/* ");
    let created = publish.iter().position(|step| step.iter().any(|line| is_release(line)));
    if let Some(line) = publish.iter().flatten().find(|line| is_release(line)) {
        for flag in ["--prerelease", "--latest=false"] {
            if !line.split_whitespace().any(|word| word == flag) {
                found.push(format!(
                    "{WORKFLOW}: the `gh release create` that attaches `dist/*` lacks `{flag}` - the \
                     optimised build must never become the repository's Latest release"
                ));
            }
        }
    }
    if !matches!((signed, created), (Some(signed), Some(created)) if signed < created) {
        found.push(format!(
            "{WORKFLOW}: job `publish` must run `attest-and-sign` BEFORE the step \
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

/// Does `step` run the local composite action `action`, in the self-repository `$/` spelling
/// every workflow here uses since `github.com/telekom/sutura#1161`?
fn calls(step: &[&str], action: &str) -> bool {
    let want = format!("uses: $/.github/actions/{action}");
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
      - uses: $/.github/actions/build-artefacts
        with:
          profile: performance
  publish:
    steps:
      - uses: $/.github/actions/push-images
        with:
          profile: performance
      - uses: $/.github/actions/attest-and-sign
      - name: Publish
        run: |
          gh release create \"$release\" dist/* --prerelease --latest=false
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
            "- uses: $/.github/actions/build-artefacts\n        with:\n          profile: performance\n",
            "- uses: $/.github/actions/build-artefacts\n",
            "job `build`",
        );
    }

    #[test]
    fn a_publish_without_the_performance_images_is_refused() {
        refused(
            "- uses: $/.github/actions/push-images\n        with:\n          profile: performance\n",
            "- uses: $/.github/actions/push-images\n        with:\n          profile: release\n",
            "push-images",
        );
    }

    /// A binary published without a signature: signing after the release exists, or not at all.
    #[test]
    fn a_release_created_before_signing_is_refused() {
        refused(
            "      - uses: $/.github/actions/attest-and-sign\n      - name: Publish\n        run: |\n          gh release create \"$release\" dist/* --prerelease --latest=false\n",
            "      - name: Publish\n        run: |\n          gh release create \"$release\" dist/* --prerelease --latest=false\n      - uses: $/.github/actions/attest-and-sign\n",
            "BEFORE",
        );
        refused(
            "      - uses: $/.github/actions/attest-and-sign\n",
            "      # - uses: $/.github/actions/attest-and-sign\n",
            "unsigned",
        );
        refused("          gh release create", "          # gh release create", "unsigned");
    }

    /// A hand-picked glob can leave a signed file behind or attach one the loop never saw.
    #[test]
    fn a_release_attaching_less_than_the_signed_directory_is_refused() {
        refused(" dist/* ", " dist/*.tar.gz ", "attaches `dist/*`");
    }

    #[test]
    fn an_upload_onto_an_existing_release_is_refused() {
        refused(
            "--latest=false\n",
            "--latest=false\n          gh release upload \"$GITHUB_REF_NAME\" dist/*\n",
            "`gh release upload`",
        );
        assert!(
            judged(&SOUND.replacen("--latest=false\n", "--latest=false\n          # gh release upload x\n", 1)).is_empty(),
            "a comment is not an upload"
        );
    }

    /// Either flag dropped makes `<version>-performance` the release an unqualified download takes.
    #[test]
    fn a_release_that_could_become_latest_is_refused() {
        refused(" --prerelease ", " ", "lacks `--prerelease`");
        refused(" --latest=false", "", "lacks `--latest=false`");
    }

    /// A repo-relative path and its contents, as `crate::scratch_tree::Tree::of` takes them.
    type Fixture<'a> = (&'a str, &'a [u8]);

    /// A scratch tree no other run shares: `cargo test` runs these cells as threads of one process,
    /// and [`crate::scratch_tree::Tree::of`] tells trees apart only by tag and pid.
    fn scratch(tag: &str, files: &[Fixture<'_>]) -> crate::scratch_tree::Tree {
        static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::scratch_tree::Tree::of(&format!("{tag}-{run}"), files)
    }

    /// What one run of `push-images`' shell asked `docker` to do, and how it ended.
    struct Pushed {
        succeeded: bool,
        status: std::process::ExitStatus,
        calls: String,
        /// stdout then stderr: a `::error::` annotation is an `echo`, so it lands on stdout.
        printed: String,
    }

    /// `push-images`' real `run:` body at `profile`, against a `docker` shell function. With
    /// `digestless`, every push succeeds and prints no digest.
    ///
    /// The stub's `login` DRAINS its stdin, as `--password-stdin` does. Without that the token's
    /// `printf` races a reader that has already exited, and a SIGPIPE under `pipefail` ends the
    /// step with nothing on stderr - the likeliest cause of the one silent failure of the release
    /// half seen in validate's nix leg, which 3600 local runs under contention did not reproduce.
    fn pushed(profile: &str, digestless: bool) -> Pushed {
        pushed_from(profile, digestless, "v0.6.1")
    }

    /// [`pushed`], dispatched from `git_ref`.
    fn pushed_from(profile: &str, digestless: bool, git_ref: &str) -> Pushed {
        const TRIPLES: [&str; 4] = [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
        ];
        const DOCKER: &str = r#"docker() {
  printf '%s\n' "$*" >> "$DOCKER_LOG"
  case "$1" in
    login) cat > /dev/null ;;
    push) [ -n "${DIGESTLESS:-}" ] || printf 'digest: sha256:%064d size: 1\n' 0 ;;
    buildx) if [ "$3" = inspect ]; then if [ "$5" = --format ]; then echo "linux/amd64 linux/arm64 "; else echo raw; fi; fi ;;
  esac
}
"#;
        let root = crate::repo::root().expect("repo root");
        let action = std::fs::read_to_string(root.join(".github/actions/push-images/action.yml")).expect("the action");
        let lines: Vec<&str> = action.lines().collect();
        let body = crate::workflows::step::shell(&lines)
            .into_iter()
            .skip(1)
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        let suffix = if profile == "performance" { "-performance" } else { "" };
        let images: Vec<String> = TRIPLES
            .iter()
            .map(|triple| format!("images/sutura-oci-{triple}{suffix}.tar.gz"))
            .collect();
        let fixtures: Vec<_> = images.iter().map(|path| (path.as_str(), &b""[..])).collect();
        let tree = scratch("push-images", &fixtures);
        let scratch = tree.root();
        let output = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", &format!("{DOCKER}{body}")])
            .current_dir(scratch)
            .env_remove("BASH_ENV")
            .env("DOCKER_LOG", scratch.join("docker.log"))
            .env("DIGESTLESS", if digestless { "1" } else { "" })
            .env("TARGETS", TRIPLES.join(" "))
            .env("BINARIES", "sutura")
            .env("PROFILE", profile)
            .env("IMAGE", "ghcr.io/example/sutura")
            .env("GHCR_USERNAME", "user")
            .env("GHCR_TOKEN", "token")
            .env("GITHUB_REF_NAME", git_ref)
            .env("GITHUB_STEP_SUMMARY", scratch.join("summary.md"))
            .output()
            .expect("the action's shell executes");
        Pushed {
            succeeded: output.status.success(),
            status: output.status,
            calls: std::fs::read_to_string(scratch.join("docker.log")).unwrap_or_default(),
            printed: format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }

    /// `latest` is what an unqualified pull resolves, so the optimised images must never take it -
    /// and the release profile must, or the stub would pass by never seeing the tag at all.
    #[test]
    fn the_optimised_images_never_move_latest() {
        let release = pushed("release", false);
        assert!(
            release.succeeded,
            "{}: {}\n{}",
            release.status, release.printed, release.calls
        );
        assert!(release.calls.contains("ghcr.io/example/sutura:latest"), "{}", release.calls);
        let optimised = pushed("performance", false);
        assert!(
            optimised.succeeded,
            "{}: {}\n{}",
            optimised.status, optimised.printed, optimised.calls
        );
        let lists: Vec<&str> = optimised
            .calls
            .lines()
            .filter(|call| call.starts_with("buildx imagetools create"))
            .collect();
        assert_eq!(lists.len(), 2, "{}", optimised.calls);
        assert!(
            lists[0].starts_with("buildx imagetools create -t ghcr.io/example/sutura:0.6.1-performance ghcr.io/"),
            "{lists:?}"
        );
        assert!(
            lists[1].starts_with("buildx imagetools create -t ghcr.io/example/sutura:0.6.1-performance-musl ghcr.io/"),
            "{lists:?}"
        );
        assert!(
            !optimised.calls.contains("ghcr.io/example/sutura:latest"),
            "{}",
            optimised.calls
        );
    }

    /// A composite `shell: bash` runs with `pipefail`, which once ended the step at the digest
    /// capture, before the refusal that names the leaf.
    #[test]
    fn a_push_that_prints_no_digest_is_refused_by_name() {
        let run = pushed("performance", true);
        assert!(!run.succeeded, "{}", run.calls);
        assert!(
            run.printed
                .contains("no digest in the push output for x86_64-unknown-linux-gnu-performance"),
            "{}",
            run.printed
        );
    }

    /// One step's real `run:` body: the step named `name` in job `job` of `file`.
    fn step_body(file: &str, job: &str, name: &str) -> String {
        let root = crate::repo::root().expect("repo root");
        let text = std::fs::read_to_string(root.join(file)).expect("the workflow");
        let steps = super::steps_of(&text, job);
        let want = format!("- name: {name}");
        let step = steps
            .iter()
            .find(|step| step.iter().any(|line| line.trim() == want))
            .expect("the step");
        crate::workflows::step::shell(step)
            .into_iter()
            .skip(1)
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The built commit every release run below is dispatched at.
    const BUILT: &str = "c0ffee";

    /// What one run of the release step did: how it ended, its notes, the `gh` calls it made.
    struct Released {
        succeeded: bool,
        /// stdout then stderr: a `::error::` annotation is an `echo`, so it lands on stdout.
        printed: String,
        notes: String,
        gh: String,
    }

    /// The release step's real `run:` body against `gh` and `git` shell functions. `git ls-remote`
    /// answers `tag_refs`, the remote's lines for the release's tag.
    fn released(tag_refs: &str) -> Released {
        const STUBS: &str = r#"gh() {
  printf '%s\n' "$*" >> "$GH_LOG"
  case "$*" in
    "release view "*--json\ isDraft*) echo false ;;
    "release view "*--json\ assets*) find dist -maxdepth 1 -type f | wc -l ;;
    "release view "*) return 1 ;;
  esac
}
git() { [ "$1" = ls-remote ] && printf '%b' "$TAG_REFS"; }
"#;
        let body = step_body(super::WORKFLOW, "publish", "Publish the performance release");
        let tree = scratch(
            "performance-release",
            &[
                ("dist/sutura-provenance.intoto.jsonl", &b"{}\n"[..]),
                ("dist/image-digests.txt", &b"# kind name reference@digest\n"[..]),
            ],
        );
        let scratch = tree.root();
        let output = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", &format!("{STUBS}{body}")])
            .current_dir(scratch)
            .env_remove("BASH_ENV")
            .env("GH_LOG", scratch.join("gh.log"))
            .env("TAG_REFS", tag_refs)
            .env("GITHUB_REF_NAME", "v0.6.1")
            .env("GITHUB_SERVER_URL", "https://github.com")
            .env("GITHUB_REPOSITORY", "example/sutura")
            .env("IMAGE", "ghcr.io/example/sutura")
            .env("SHA", BUILT)
            .output()
            .expect("the step's shell executes");
        Released {
            succeeded: output.status.success(),
            printed: format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            notes: std::fs::read_to_string(scratch.join("notes.md")).unwrap_or_default(),
            gh: std::fs::read_to_string(scratch.join("gh.log")).unwrap_or_default(),
        }
    }

    /// `--target` is ignored for a tag that exists, so one at another commit is refused before
    /// `gh release create` attaches anything - and one already at the built commit, either shape, is not.
    #[test]
    fn a_tag_already_at_another_commit_is_refused() {
        let elsewhere = released("0badc0de\\trefs/tags/v0.6.1-performance\\n");
        assert!(!elsewhere.succeeded, "{}", elsewhere.gh);
        assert!(
            elsewhere
                .printed
                .contains("already exists at 0badc0de, not at the built commit c0ffee"),
            "{}",
            elsewhere.printed
        );
        assert!(
            !elsewhere.gh.contains("dist/sutura-provenance.intoto.jsonl"),
            "{}",
            elsewhere.gh
        );
        for here in [
            format!("{BUILT}\\trefs/tags/v0.6.1-performance\\n"),
            format!("7a9\\trefs/tags/v0.6.1-performance\\n{BUILT}\\trefs/tags/v0.6.1-performance^{{}}\\n"),
        ] {
            let run = released(&here);
            assert!(run.succeeded, "{here}: {}", run.printed);
            assert!(run.gh.contains("dist/sutura-provenance.intoto.jsonl"), "{}", run.gh);
        }
    }

    /// The accept arm `v[0-9]*.[0-9]*.[0-9]*` also matches `v0.6.1-performance`, which would push
    /// `-performance-performance` images and create an immutable release of that name.
    #[test]
    fn a_publish_from_the_performance_tag_is_refused() {
        let run = pushed_from("performance", false, "v0.6.1-performance");
        assert!(!run.succeeded, "{}", run.calls);
        assert!(run.printed.contains("an optimised release's own tag"), "{}", run.printed);
        assert!(!run.calls.contains("imagetools create"), "{}", run.calls);
    }

    /// `docs.yml`'s ref guard, the real step: main takes `latest`, a release tag gets its own
    /// directory and no alias, and the optimised build's tag is refused.
    #[test]
    fn the_docs_never_publish_the_performance_tag() {
        let body = step_body(".github/workflows/docs.yml", "publish", "What is being published");
        let run = |kind: &str, name: &str| {
            let tree = scratch("docs-target", &[]);
            let scratch = tree.root();
            let output = std::process::Command::new("bash")
                .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", &body])
                .current_dir(scratch)
                .env_remove("BASH_ENV")
                .env("REF_TYPE", kind)
                .env("REF_NAME", name)
                .env("GITHUB_OUTPUT", scratch.join("output"))
                .output()
                .expect("the step's shell executes");
            (
                output.status.success(),
                std::fs::read_to_string(scratch.join("output")).unwrap_or_default(),
            )
        };
        assert_eq!(run("branch", "main"), (true, String::from("version=main\nalias=latest\n")));
        assert_eq!(run("tag", "v0.6.1"), (true, String::from("version=0.6.1\nalias=\n")));
        assert_eq!(run("tag", "v0.6.1-performance"), (false, String::new()));
    }

    #[test]
    fn a_tag_trigger_the_performance_tag_can_start_is_refused() {
        let at = |line: &str| super::triggers("w.yml", &format!("on:\n  push:\n{line}\n"));
        assert_eq!(at(r#"    tags: ["v*", "!v*-performance"]"#), Vec::<String>::new());
        assert_eq!(at(r#"    tags: ["v*"]"#).len(), 1);
        assert_eq!(at("    tags:\n      - v*").len(), 1);
    }

    /// The notes' image rows name the lists `push-images` creates - the release is
    /// `v<version>-performance`, the image tags keep no `v`.
    #[test]
    fn the_notes_name_the_image_tags_that_were_pushed() {
        let optimised = pushed("performance", false);
        assert!(optimised.succeeded, "{}", optimised.printed);
        let pushed: Vec<&str> = optimised
            .calls
            .lines()
            .filter_map(|call| call.strip_prefix("buildx imagetools create -t "))
            .filter_map(|rest| rest.split_whitespace().next())
            .collect();
        let release = released("");
        assert!(release.succeeded, "{}", release.printed);
        let named: Vec<&str> = release
            .notes
            .lines()
            .filter(|line| line.starts_with("| glibc |") || line.starts_with("| static musl |"))
            .filter_map(|line| line.split('`').nth(1))
            .collect();
        assert_eq!(named, pushed, "{}", release.notes);
    }
}
