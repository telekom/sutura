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
//! every `checks.*` output pass in any job, as they do in ordinary CI. The list is held in both
//! directions: a name the file builds outside it is refused, and so is a pair in it that the file
//! no longer builds.
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
    ("downstream-deps", "adbc-driver-bigquery-x86_64-unknown-linux-gnu"),
    ("downstream-deps", "adbc-driver-bigquery-x86_64-unknown-linux-musl"),
    ("downstream-deps", "adbc-driver-postgresql-x86_64-unknown-linux-musl"),
    ("downstream-deps", "adbc-driver-duckdb-x86_64-unknown-linux-musl"),
];

/// Every literal release output `cachix-push.yml` builds outside [`CACHE_ONLY`], then every
/// [`CACHE_ONLY`] pair the file does not build, labelled the way [`super::release_outputs`] labels
/// a walked file. An absent file contributes nothing.
pub(super) fn outputs(root: &Path, deps_exempt: bool) -> Vec<String> {
    std::fs::read_to_string(root.join(CACHE_PUBLISH)).map_or_else(
        |_| Vec::new(),
        |text| {
            refused(&text, deps_exempt)
                .into_iter()
                .chain(unbuilt(&text, deps_exempt))
                .collect()
        },
    )
}

/// The job `line` (1-based) sits in, `None` outside `jobs:`.
fn job_at<'a>(jobs: &[Option<&'a str>], line: usize) -> Option<&'a str> {
    line.checked_sub(1).and_then(|index| jobs.get(index)).copied().flatten()
}

/// Every literal release output `cachix-push.yml` builds outside [`CACHE_ONLY`].
fn refused(text: &str, deps_exempt: bool) -> Vec<String> {
    let jobs = job_of_each_line(text);
    super::literal_release_builds(text, deps_exempt)
        .into_iter()
        .filter(|(line, output)| !job_at(&jobs, *line).is_some_and(|job| CACHE_ONLY.contains(&(job, output.as_str()))))
        .map(|(line, output)| format!("cachix-push.yml:{line}  {output}"))
        .collect()
}

/// The [`CACHE_ONLY`] pairs the workflow does not build: an exemption for a build that is gone is a
/// name nothing holds.
fn unbuilt(text: &str, deps_exempt: bool) -> Vec<String> {
    let jobs = job_of_each_line(text);
    let built = super::literal_release_builds(text, deps_exempt);
    CACHE_ONLY
        .iter()
        .filter(|&&(job, name)| {
            !built
                .iter()
                .any(|(line, output)| output == name && job_at(&jobs, *line) == Some(job))
        })
        .map(|(job, name)| format!("cachix-push.yml  {name} is exempt in `{job}` and that job does not build it"))
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

    const EXEMPT: &str = concat!(
        "on:\n",
        "  push:\n",
        "jobs:\n",
        "  push:\n",
        "    steps:\n",
        "      - run: |\n",
        "          nix build --print-build-logs \\\n",
        "            .#xtask \\\n",
        "            .#jscpd\n",
        "  downstream-deps:\n",
        "    steps:\n",
        "      - run: |\n",
        "          nix build --print-build-logs \\\n",
        "            .#deps-native-ci \\\n",
        "            .#deps-native-release \\\n",
        "            .#deps-musl-release \\\n",
        "            .#adbc-driver-bigquery-x86_64-unknown-linux-gnu \\\n",
        "            .#adbc-driver-bigquery-x86_64-unknown-linux-musl \\\n",
        "            .#adbc-driver-duckdb-x86_64-unknown-linux-musl \\\n",
        "            .#adbc-driver-postgresql-x86_64-unknown-linux-musl\n",
    );

    const WRONG_JOB: &str = concat!(
        "on:\n",
        "  push:\n",
        "jobs:\n",
        "  push:\n",
        "    steps:\n",
        "      - run: nix build .#jscpd\n",
        "  downstream-deps:\n",
        "    steps:\n",
        "      - run: |\n",
        "          nix build --print-build-logs \\\n",
        "            .#xtask \\\n",
        "            .#deps-native-ci \\\n",
        "            .#deps-native-release \\\n",
        "            .#deps-musl-release \\\n",
        "            .#adbc-driver-bigquery-x86_64-unknown-linux-gnu \\\n",
        "            .#adbc-driver-bigquery-x86_64-unknown-linux-musl \\\n",
        "            .#adbc-driver-postgresql-x86_64-unknown-linux-musl \\\n",
        "            .#adbc-driver-duckdb-x86_64-unknown-linux-musl\n",
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
    fn a_cache_only_pair_the_workflow_still_builds_is_not_reported() {
        assert_eq!(super::unbuilt(EXEMPT, true), Vec::<String>::new());
    }

    #[test]
    fn a_cache_only_name_the_workflow_no_longer_builds_is_refused() {
        // The reviewed mutation: the DuckDB line leaves the `nix build`, `CACHE_ONLY` keeps its name.
        let without = EXEMPT.replace("            .#adbc-driver-duckdb-x86_64-unknown-linux-musl \\\n", "");
        assert_ne!(without, EXEMPT);
        assert_eq!(
            super::unbuilt(&without, true),
            vec![String::from(
                "cachix-push.yml  adbc-driver-duckdb-x86_64-unknown-linux-musl is exempt in \
                 `downstream-deps` and that job does not build it"
            )]
        );
    }

    #[test]
    fn a_cache_only_name_built_in_another_job_does_not_count() {
        // `xtask` is exempt in `push`; built only from `downstream-deps` it does not satisfy `push`.
        assert_eq!(
            super::unbuilt(WRONG_JOB, true),
            vec![String::from(
                "cachix-push.yml  xtask is exempt in `push` and that job does not build it"
            )]
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

    #[test]
    fn the_committed_cache_publish_workflow_builds_every_cache_only_name() {
        let root = crate::repo::root().expect("the repo root");
        let text = std::fs::read_to_string(root.join(super::CACHE_PUBLISH)).expect("the cache-publish workflow");
        let deps_exempt = super::super::deps_is_the_dependency_closure(&root);
        assert_eq!(super::unbuilt(&text, deps_exempt), Vec::<String>::new());
    }
}
