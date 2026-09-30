//! The release-output refusal over `cachix-push.yml`, which the ordinary-CI walk never opens.
//!
//! `github.com/telekom/sutura#1037` row m: that workflow runs on `push` to `main` only, so it is
//! outside the closure [`super::release_outputs`] walks, and a release output added to one of its
//! jobs stayed green. It is read DIRECTLY here, by path, not walked: nothing calls it.
//!
//! It exists to publish CI's store to the cache, so some of its literal builds are not release
//! outputs by intent. [`CACHE_ONLY`] is the owner-decided exemption list
//! (`github.com/telekom/sutura#1037`, issuecomment-5900986058), keyed by JOB as well as by name, so
//! a release output moved into an exempt job is still refused, and so is a listed name built from
//! any other job. That keying is [`CACHE_ONLY`]'s alone: `.#deps` (while it is `ciArtifacts`) and
//! every `checks.*` output pass in any job, as they do in ordinary CI.
//!
//! **The limit, next to the claim.** The one output the owner named NOT exempt - the cross-build
//! job's shipped binary - is built as `.#${bin}-${TARGET}-ci`, an interpolated name no rule here
//! reads, so on the committed tree nothing holds it. A literal spelling of it is refused, and
//! [`CACHE_ONLY`] may not name that job, both held by cells below. An exemption also trusts a NAME:
//! it does not check that `packages.deps-native-ci` still IS a dependency closure the way
//! `deps_is_the_dependency_closure` checks `packages.deps`.

use std::path::Path;

/// The cache-publish workflow, read by path.
const CACHE_PUBLISH: &str = ".github/workflows/cachix-push.yml";

/// `(job, output)` pairs `cachix-push.yml` may build literally: dependency and tool closures the
/// cache publishes and nothing ships. Owner decision on `github.com/telekom/sutura#1037` row m.
const CACHE_ONLY: &[(&str, &str)] = &[
    ("push", "xtask"),
    ("push", "jscpd"),
    ("downstream-deps", "deps-native-ci"),
    ("downstream-deps", "deps-native-release"),
    ("downstream-deps", "deps-musl-release"),
];

/// Every literal release output `cachix-push.yml` builds outside [`CACHE_ONLY`], labelled the way
/// [`super::release_outputs`] labels a walked file. An absent file contributes nothing.
pub(super) fn outputs(root: &Path, deps_exempt: bool) -> Vec<String> {
    std::fs::read_to_string(root.join(CACHE_PUBLISH)).map_or_else(|_| Vec::new(), |text| refused(&text, deps_exempt))
}

fn refused(text: &str, deps_exempt: bool) -> Vec<String> {
    let jobs = job_of_each_line(text);
    super::literal_release_builds(text, deps_exempt)
        .into_iter()
        .filter(|(line, output)| {
            let job = line.checked_sub(1).and_then(|index| jobs.get(index)).copied().flatten();
            !job.is_some_and(|job| CACHE_ONLY.contains(&(job, output.as_str())))
        })
        .map(|(line, output)| format!("cachix-push.yml:{line}  {output}"))
        .collect()
}

/// The job key each line sits under, `None` outside `jobs:`. A job key is two spaces under `jobs:`.
fn job_of_each_line(text: &str) -> Vec<Option<&str>> {
    let mut in_jobs = false;
    let mut job = None;
    text.lines()
        .map(|raw| {
            if !raw.starts_with(' ') && !raw.trim().is_empty() && !raw.starts_with('#') {
                in_jobs = raw.trim_end() == "jobs:";
                job = None;
            } else if in_jobs
                && let Some(key) = raw.strip_prefix("  ").and_then(|rest| rest.trim_end().strip_suffix(':'))
                && !key.is_empty()
                && !key.starts_with([' ', '#'])
            {
                job = Some(key);
            }
            job
        })
        .collect()
}

#[cfg(test)]
mod tests {
    const PUBLISH: &str = concat!(
        "on:\n",
        "  push:\n",
        "jobs:\n",
        "  push:\n",
        "    steps:\n",
        "      - run: |\n",
        "          nix build --print-build-logs \\\n",
        "            .#deps \\\n",
        "            .#xtask \\\n",
        "            .#jscpd \\\n",
        "            .#sutura-serve\n",
        "  cross-build:\n",
        "    steps:\n",
        "      - run: nix build --print-build-logs .#sutura-x86_64-unknown-linux-gnu-ci\n",
        "  downstream-deps:\n",
        "    steps:\n",
        "      - run: nix build .#deps-native-ci .#deps-native-release .#deps-musl-release .#sutura\n",
        "      - run: nix build .#xtask\n",
    );

    #[test]
    fn a_release_output_in_the_cache_publish_workflow_is_refused_and_the_exempt_closures_are_not() {
        assert_eq!(
            super::refused(PUBLISH, true),
            vec![
                String::from("cachix-push.yml:7  sutura-serve"),
                String::from("cachix-push.yml:14  sutura-x86_64-unknown-linux-gnu-ci"),
                String::from("cachix-push.yml:17  sutura"),
                String::from("cachix-push.yml:18  xtask"),
            ],
            "a release output continued onto a later line, the cross job's binary, a release output \
             beside exempt closures, and an exempt name from a job it is not exempt in are each refused"
        );
    }

    #[test]
    fn the_cross_build_job_is_never_cache_only() {
        assert!(
            super::CACHE_ONLY.iter().all(|&(job, _)| job != "cross-build"),
            "the owner decision on #1037 row m names the cross job's shipped binary NOT exempt"
        );
    }

    #[test]
    fn the_release_output_refusal_reads_the_cache_publish_workflow_it_never_walks() {
        let at = std::env::temp_dir().join(format!("sutura-cache-publish-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&at));
        std::fs::create_dir_all(at.join(".github/workflows")).expect("the workflows directory");
        std::fs::write(at.join(super::CACHE_PUBLISH), PUBLISH).expect("the publish workflow");
        let closure = super::super::Closure::from_roots(&at, Vec::new());
        let found = super::super::release_outputs(&at, &closure);
        std::fs::remove_dir_all(&at).expect("the scratch tree");
        assert!(
            found.contains(&String::from("cachix-push.yml:17  sutura")),
            "an empty walk still refuses what the cache-publish workflow builds: {found:?}"
        );
    }

    #[test]
    fn the_committed_cache_publish_workflow_builds_no_release_output() {
        let root = crate::repo::root().expect("the repo root");
        assert!(root.join(super::CACHE_PUBLISH).exists(), "the rule reads a file that is gone");
        let deps_exempt = super::super::deps_is_the_dependency_closure(&root);
        assert_eq!(super::outputs(&root, deps_exempt), Vec::<String>::new());
    }
}
