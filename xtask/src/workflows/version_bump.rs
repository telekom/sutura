//! Does `.github/workflows/version-bump.yml`'s `bump` job still keep the four properties the
//! v0.6.0 release incident needed, before it commits and tags - and read the right previous tag?
//!
//! `telekom/sutura#1150` review, finding 1a: nothing held these lines before this gate. A
//! reviewer deleted the dprint step, the fuzz lock update, `fuzz/Cargo.lock` from the commit's
//! `git add`, and the pre-commit hygiene run - all four at once - and every one of 44
//! `cargo xtask hygiene` members still exited 0, because nothing read this workflow's own shape
//! at all. Four rules, each over the SAME job's own lines, in COMMIT ORDER:
//!
//!   1. the step that writes `CHANGELOG.md` from git-cliff is followed, before the commit, by a
//!      `dprint` line - the v0.6.0 release shipped an unformatted changelog because nothing was;
//!   2. some step before the commit updates `fuzz/Cargo.lock` (a `cargo ... update
//!      --manifest-path fuzz/Cargo.toml` line) - the same release shipped a lock a version bump
//!      left stale, `fuzz.rs`'s own header;
//!   3. the commit step's own `git add` line names `fuzz/Cargo.lock`, so the update in (2) is
//!      not computed and then silently left uncommitted;
//!   4. some step before the commit runs the SAME structural sweep `ci.yml` runs over every pull
//!      request (`xtask -- hygiene` or `cargo xtask hygiene`) - so a bump that would redden main
//!      fails in the bump instead (preventing releases that break at tag-time).
//!
//! And two about WHICH TAG the bump reads as the previous release, since
//! `release-performance.yml` tags `v<version>-performance` on a released commit and `v*` matches
//! it:
//!
//!   5. every `git describe` in the job carries `--exclude 'v*-performance'` - measured on a
//!      scratch repository, an annotated `v0.6.1-performance` wins an unmatched describe over the
//!      release's own `v0.6.1`;
//!   6. `cliff.toml` carries the anchored `tag_pattern` - without it, on the same repository,
//!      `git-cliff --bumped-version` answered `v0.6.1-performance.1`.
//!
//! Reads the job's raw lines through [`super::step::job`] rather than a YAML parser, for the
//! reason every other `xtask` workflow gate does: no dependency, and the shapes this one
//! workflow writes are simple enough for a line scan to hold. **What this does not reach:** a
//! step reordered so the commit runs FIRST and one of the four runs uselessly after it renders
//! as the SAME failure this gate reports for "never runs at all" - the message names the
//! missing property, not the shape of what is missing. A line that is wholly a comment is
//! dropped before any rule reads the job, because it runs nothing - `947bb315d` counted one, so
//! commenting out both `dprint` commands, the fuzz update or the hygiene run stayed green. A
//! marker inside a string, or after code on the same line, still counts.

use std::path::Path;

use crate::Verdict;
use crate::repo;

use super::step;

const WORKFLOW: &str = ".github/workflows/version-bump.yml";
const JOB: &str = "bump";

/// The release commit's own marker. Longer than `git commit` so it pins THE release commit, not
/// whichever `git commit` a later step might add.
const COMMIT_MARKER: &str = "git commit -m \"chore(release):";
const CHANGELOG_WRITE_MARKER: &str = "-o CHANGELOG.md";
const DPRINT_MARKER: &str = "dprint";
const FUZZ_LOCK_UPDATE_MARKER: &str = "--manifest-path fuzz/Cargo.toml";
const FUZZ_LOCK_PATH: &str = "fuzz/Cargo.lock";
const GIT_ADD_MARKER: &str = "git add ";
const HYGIENE_MARKERS: [&str; 2] = ["xtask -- hygiene", "xtask hygiene"];
const DESCRIBE_MARKER: &str = "git describe";
const PERFORMANCE_EXCLUDE: &str = "--exclude 'v*-performance'";
const CLIFF: &str = "cliff.toml";
/// The exact line, so a widened pattern is a diff this rule refuses rather than a reading of it.
const TAG_PATTERN: &str = r#"tag_pattern = "^v[0-9]+\\.[0-9]+\\.[0-9]+$""#;

/// Rules 1-5, over one job's lines. Empty is a pass.
pub(crate) fn check(workflow: &str) -> Vec<String> {
    let Some(job) = step::job(workflow, JOB) else {
        return vec![format!(
            "{WORKFLOW} declares no `{JOB}:` job - none of the four rules below could be checked"
        )];
    };
    let job: Vec<&str> = job.into_iter().filter(|line| !line.trim_start().starts_with('#')).collect();
    let Some(commit_at) = job.iter().position(|line| line.contains(COMMIT_MARKER)) else {
        return vec![format!(
            "{WORKFLOW}'s `{JOB}` job never commits (`{COMMIT_MARKER}` was not found) - none of the four rules below could be checked against it"
        )];
    };
    // `.get` rather than direct indexing: `commit_at`/`write_at` are always valid positions in
    // the same slice they were found in, but `-D clippy::indexing_slicing` denies the bracket
    // form outright regardless, so the fallback is dead code rather than a real bound.
    let before_commit = job.get(..commit_at).unwrap_or(&[]);
    let mut problems = Vec::new();

    match before_commit.iter().position(|line| line.contains(CHANGELOG_WRITE_MARKER)) {
        Some(write_at) => {
            if !before_commit
                .get(write_at..)
                .unwrap_or(&[])
                .iter()
                .any(|line| line.contains(DPRINT_MARKER))
            {
                problems.push(format!(
                    "{WORKFLOW}: the step that writes CHANGELOG.md (`{CHANGELOG_WRITE_MARKER}`) is not followed by a `{DPRINT_MARKER}` line before the commit - a release can tag an unformatted changelog again"
                ));
            }
        }
        None => problems.push(format!(
            "{WORKFLOW}: no step before the commit writes CHANGELOG.md via `{CHANGELOG_WRITE_MARKER}`"
        )),
    }

    if !before_commit
        .iter()
        .any(|line| line.contains(FUZZ_LOCK_UPDATE_MARKER) && line.contains("update"))
    {
        problems.push(format!(
            "{WORKFLOW}: no step before the commit updates fuzz/Cargo.lock (a `... update {FUZZ_LOCK_UPDATE_MARKER}` line) - a version bump can leave it stale again"
        ));
    }

    // The WHOLE job, not scoped to `commit_at..`: the `git add` line and the `git commit` line
    // it stages for are two adjacent lines inside the SAME step's multi-line `run: |` block, and
    // the add line comes first - `commit_at` already points past it.
    match job.iter().find(|line| line.contains(GIT_ADD_MARKER)) {
        Some(add_line) if add_line.contains(FUZZ_LOCK_PATH) => {}
        Some(_) => problems.push(format!(
            "{WORKFLOW}: the commit step's `{GIT_ADD_MARKER}` line does not name `{FUZZ_LOCK_PATH}` - an updated lock would be computed and left uncommitted"
        )),
        None => problems.push(format!(
            "{WORKFLOW}: the commit step has no `{GIT_ADD_MARKER}` line at all - nothing is staged, `{FUZZ_LOCK_PATH}` included"
        )),
    }

    if !before_commit
        .iter()
        .any(|line| HYGIENE_MARKERS.iter().any(|marker| line.contains(marker)))
    {
        problems.push(format!(
            "{WORKFLOW}: no step before the commit runs the structural sweep ({HYGIENE_MARKERS:?}) - a bump that would redden main is only found there, afterwards"
        ));
    }

    let describes: Vec<&&str> = job.iter().filter(|line| line.contains(DESCRIBE_MARKER)).collect();
    if describes.is_empty() || describes.iter().any(|line| !line.contains(PERFORMANCE_EXCLUDE)) {
        problems.push(format!(
            "{WORKFLOW}: a `{DESCRIBE_MARKER}` in the job lacks `{PERFORMANCE_EXCLUDE}`, or there is none - the bump can read an optimised build's `v<version>-performance` tag as the previous release"
        ));
    }

    problems
}

/// Rule 6, over `cliff.toml`'s text.
pub(crate) fn cliff_problems(cliff: &str) -> Vec<String> {
    if cliff.lines().any(|line| line.trim() == TAG_PATTERN) {
        return Vec::new();
    }
    vec![format!(
        "{CLIFF} lacks `{TAG_PATTERN}` - git-cliff then reads `v<version>-performance` as a release and bumps it"
    )]
}

/// Reads [`WORKFLOW`] off the tree and hands [`check`] the text.
pub(crate) fn run(_args: &[String]) -> Verdict {
    run_over(repo::root().as_deref())
}

fn run_over(root: Option<&Path>) -> Verdict {
    let Some(root) = root else {
        eprintln!("xtask check-version-bump: could not locate the repo root");
        return Verdict::Fail;
    };
    let Ok(workflow) = std::fs::read_to_string(root.join(WORKFLOW)) else {
        eprintln!("xtask check-version-bump: {WORKFLOW} is unreadable - none of the four rules below could be checked");
        return Verdict::Fail;
    };
    let Ok(cliff) = std::fs::read_to_string(root.join(CLIFF)) else {
        eprintln!("xtask check-version-bump: {CLIFF} is unreadable - rule 6 could not be checked");
        return Verdict::Fail;
    };
    let mut problems = check(&workflow);
    problems.extend(cliff_problems(&cliff));
    if problems.is_empty() {
        println!("xtask check-version-bump: ok - {WORKFLOW}'s `{JOB}` job keeps all six properties");
        return Verdict::Pass;
    }
    eprintln!("xtask check-version-bump: {} problem(s)", problems.len());
    for problem in &problems {
        eprintln!("  {problem}");
    }
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "jobs:\n  bump:\n";

    fn job(steps: &str) -> String {
        format!("{HEADER}{steps}")
    }

    /// Every rule held, in the shape this workflow actually writes.
    const COMPLETE: &str = "    steps:
      - name: Write the release changelog
        run: |
          nix run .#git-cliff -- --tag \"$VERSION\" -o CHANGELOG.md
          nix run .#dprint -- fmt CHANGELOG.md
      - name: Compute the next version
        run: |
          prev_raw=\"$(git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude 'v*-performance' 2>/dev/null || true)\"
      - name: Set the workspace version
        run: |
          nix run .#cargo -- update --workspace
          nix run .#cargo -- update --workspace --manifest-path fuzz/Cargo.toml
      - name: Refuse a release this bump would leave stale
        run: |
          nix run .#xtask -- hygiene
      - name: Commit and tag
        run: |
          git add CHANGELOG.md Cargo.toml Cargo.lock fuzz/Cargo.lock
          git commit -m \"chore(release): $VERSION\"
";

    #[test]
    fn all_four_properties_pass() {
        assert_eq!(check(&job(COMPLETE)), Vec::<String>::new());
    }

    /// THE PRODUCTION ENTRY POINT over the real tree, so a mutation of either real file reddens a
    /// test and not only the `check-version-bump` gate.
    #[test]
    fn the_real_workflow_and_changelog_config_keep_all_six() {
        let root = crate::repo::root().expect("repo root");
        let workflow = std::fs::read_to_string(root.join(WORKFLOW)).expect("the workflow");
        let cliff = std::fs::read_to_string(root.join(CLIFF)).expect("cliff.toml");
        let mut problems = check(&workflow);
        problems.extend(cliff_problems(&cliff));
        assert_eq!(problems, Vec::<String>::new());
    }

    /// Rule 5: the release's own tag must win `git describe`, whatever the optimised build tags.
    #[test]
    fn a_describe_that_can_read_the_performance_tag_fails() {
        for steps in [
            COMPLETE.replace(" --exclude 'v*-performance'", ""),
            COMPLETE.replace("prev_raw=", "# prev_raw="),
        ] {
            let problems = check(&job(&steps));
            assert_eq!(problems.len(), 1, "only rule 5 should fire: {problems:?}");
            assert!(problems[0].contains("v<version>-performance"), "{problems:?}");
        }
    }

    /// Rule 6, on the real `cliff.toml` and on one that lost or widened the pattern.
    #[test]
    fn a_changelog_config_that_reads_the_performance_tag_fails() {
        let root = crate::repo::root().expect("repo root");
        let cliff = std::fs::read_to_string(root.join(CLIFF)).expect("cliff.toml");
        assert_eq!(cliff_problems(&cliff), Vec::<String>::new());
        assert_eq!(cliff_problems(&cliff.replace(TAG_PATTERN, "")).len(), 1);
        assert_eq!(cliff_problems(&cliff.replace("[0-9]+$", "[0-9]+")).len(), 1);
    }

    #[test]
    fn a_changelog_write_with_no_following_dprint_line_fails() {
        let steps = COMPLETE.replace("          nix run .#dprint -- fmt CHANGELOG.md\n", "");
        let problems = check(&job(&steps));
        assert_eq!(problems.len(), 1, "only rule 1 should fire: {problems:?}");
        assert!(problems[0].contains("dprint"));
    }

    #[test]
    fn no_fuzz_lock_update_before_the_commit_fails() {
        let steps = COMPLETE.replace(
            "          nix run .#cargo -- update --workspace --manifest-path fuzz/Cargo.toml\n",
            "",
        );
        let problems = check(&job(&steps));
        assert_eq!(problems.len(), 1, "only rule 2 should fire: {problems:?}");
        assert!(problems[0].contains("fuzz/Cargo.lock"));
    }

    #[test]
    fn a_commit_that_does_not_add_the_fuzz_lock_fails() {
        let steps = COMPLETE.replace(
            "git add CHANGELOG.md Cargo.toml Cargo.lock fuzz/Cargo.lock",
            "git add CHANGELOG.md Cargo.toml Cargo.lock",
        );
        let problems = check(&job(&steps));
        assert_eq!(problems.len(), 1, "only rule 3 should fire: {problems:?}");
        assert!(problems[0].contains("git add"));
    }

    #[test]
    fn no_hygiene_run_before_the_commit_fails() {
        let steps = COMPLETE.replace(
            "      - name: Refuse a release this bump would leave stale
        run: |
          nix run .#xtask -- hygiene
",
            "",
        );
        let problems = check(&job(&steps));
        assert_eq!(problems.len(), 1, "only rule 4 should fire: {problems:?}");
        assert!(problems[0].contains("structural sweep"));
    }

    /// Each command commented out rather than deleted: a comment runs nothing, so each rule fires.
    fn commented_out(line: &str) -> Vec<String> {
        let steps = COMPLETE.replace(line, &line.replacen("nix", "# nix", 1));
        assert_ne!(steps, COMPLETE, "fixture line not found: {line}");
        check(&job(&steps))
    }

    #[test]
    fn a_commented_out_dprint_line_fails() {
        let problems = commented_out("          nix run .#dprint -- fmt CHANGELOG.md\n");
        assert_eq!(problems.len(), 1, "only rule 1 should fire: {problems:?}");
        assert!(problems[0].contains("dprint"));
    }

    #[test]
    fn a_commented_out_fuzz_lock_update_fails() {
        let problems = commented_out("          nix run .#cargo -- update --workspace --manifest-path fuzz/Cargo.toml\n");
        assert_eq!(problems.len(), 1, "only rule 2 should fire: {problems:?}");
        assert!(problems[0].contains("fuzz/Cargo.lock"));
    }

    #[test]
    fn a_commented_out_hygiene_run_fails() {
        let problems = commented_out("          nix run .#xtask -- hygiene\n");
        assert_eq!(problems.len(), 1, "only rule 4 should fire: {problems:?}");
        assert!(problems[0].contains("structural sweep"));
    }

    /// All four gone at once - the exact review mutation (finding 1a) - fails all four.
    #[test]
    fn deleting_all_four_lines_at_once_fails_all_four_rules() {
        let steps = "    steps:
      - name: Write the release changelog
        run: |
          nix run .#git-cliff -- --tag \"$VERSION\" -o CHANGELOG.md
          git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude 'v*-performance'
      - name: Set the workspace version
        run: |
          nix run .#cargo -- update --workspace
      - name: Commit and tag
        run: |
          git add CHANGELOG.md Cargo.toml Cargo.lock
          git commit -m \"chore(release): $VERSION\"
";
        assert_eq!(check(&job(steps)).len(), 4);
    }

    #[test]
    fn no_bump_job_fails_closed() {
        assert_eq!(check("jobs:\n  other:\n    steps: []\n").len(), 1);
    }

    #[test]
    fn no_commit_step_fails_closed() {
        let steps = "    steps:
      - name: Write the release changelog
        run: nix run .#git-cliff -- -o CHANGELOG.md
";
        assert_eq!(check(&job(steps)).len(), 1);
    }
}
