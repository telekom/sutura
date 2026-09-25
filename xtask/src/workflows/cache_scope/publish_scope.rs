//! Which shared cache a `cachix/cachix-action` step may target, given the events its job can
//! ever run on (issue #1000, out of review 5303436511).
//!
//! **The defect this holds.** `retired.rs` and its `stores` submodule pin WHICH substituters a
//! step may TRUST and which job may hold a WRITE credential at all - they read `name:`, `with:`
//! and `environment:` on the INSTALLER side. Nothing reads the `name:` a `cachix/cachix-action`
//! PUBLISH step actually writes TO against the events the job carrying it can run on. Measured in
//! review: rebinding `pr-cache`'s own `name: sutura-prs` to `name: sutura` - a same-repository
//! pull request's write landing in the shared cache every other job trusts unconditionally -
//! reported `check-workflows: ok`. This module reads that EFFECT: a shared cache's name in a job
//! reachable from `pull_request`/`merge_group`, or `sutura-prs` in a job reachable from `push`.
//!
//! **Reuses [`super::installers`]'s job-admission machinery rather than a second one**: `jobs`,
//! `job_if` and `admits` already answer "can this job run under event E" for the installer rule,
//! and a cache writer's job is exactly the same question over a different step. What this module
//! adds on top is [`workflow_triggers`] - `admits` alone over-reports for a job with NO job-level
//! `if:` at all, which is `cachix-push.yml`'s own shape: its three jobs admit every event by
//! `admits`'s "no gate, admits everything" rule, even though the WORKFLOW itself triggers on
//! `push` alone. A writer rule that ignored the file's own `on:` would refuse the very jobs this
//! cache posture depends on.
//!
//! **The same closed-vocabulary limits as `installers`**: a job-level `if:` this cannot parse (a
//! `||`, a multi-line `if: |`) is read as admitting every event, the refusal-favouring direction.

use std::path::Path;

use super::installers::{EVENTS, admits, every_workflow_file, job_if, jobs};

/// The publish action this rule watches - the write half `retired::PUBLISH` also names.
const PUBLISH: &str = "cachix/cachix-action";

/// The three caches only a main push may write - `cachix-push.yml`'s own `push`/`cross-build`/
/// `connectors` jobs, never a pull request's or a queued entry's.
const SHARED: [&str; 3] = ["sutura", "sutura-cross-build", "sutura-connectors"];

/// The one cache a pull request's OWN additive write may target - never a push's.
const PR_ONLY: &str = "sutura-prs";

/// Every workflow file, walked for [`judge`].
pub(super) fn problems(root: &Path) -> Vec<String> {
    match every_workflow_file(root) {
        Ok(files) => {
            let borrowed: Vec<(&str, &str)> = files.iter().map(|(label, text)| (label.as_str(), text.as_str())).collect();
            judge(&borrowed)
        }
        Err(why) => vec![format!(
            "could not read every workflow file under .github/workflows - the writer-scope rule would pass over a partial tree: {why}"
        )],
    }
}

/// The rule over labelled text, so a fixture can hold it.
fn judge(files: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    for (label, text) in files {
        let lines: Vec<&str> = text.lines().collect();
        let triggers = workflow_triggers(text);
        for (start, end) in jobs(text) {
            let Some(name) = job_name(&lines, start) else { continue };
            let block = lines.get(start..end).unwrap_or_default().join("\n");
            let job_gate = job_if(&lines, start, end);
            let reachable = |event: &str| triggers.contains(&event) && admits(job_gate.as_deref(), event);
            for step in super::steps(&block) {
                if step.uses().as_deref() != Some(PUBLISH) {
                    continue;
                }
                let Some(cache) = step.input("name:").map(str::trim) else {
                    continue;
                };
                let at = step.line.saturating_add(start);
                if SHARED.contains(&cache) && (reachable("pull_request") || reachable("merge_group")) {
                    out.push(format!(
                        "{label}:{at}  job `{name}` writes the shared cache `{cache}` with {PUBLISH}, and can run on pull_request or merge_group - only a push to main may write a shared cache"
                    ));
                }
                if cache == PR_ONLY && reachable("push") {
                    out.push(format!(
                        "{label}:{at}  job `{name}` writes `{PR_ONLY}` with {PUBLISH}, and can run on push - only a pull request's own additive write may target that cache"
                    ));
                }
            }
        }
    }
    out
}

/// The job's own name, read off its header line (one line above `start`, [`jobs`]'s convention).
fn job_name(lines: &[&str], start: usize) -> Option<String> {
    let header = lines.get(start.checked_sub(1)?)?.trim();
    header.strip_suffix(':').map(String::from)
}

/// Which of [`EVENTS`] this WORKFLOW FILE's own top-level `on:` block can ever fire.
///
/// A narrow, local reader rather than `contexts::block_keys` reused across a sibling module
/// boundary: that function is private to a file this one has no other reason to depend on, and
/// the shape it reads (mapping keys directly under one top-level block) is a dozen lines here.
/// Without this, a job with no `if:` of its own - `cachix-push.yml`'s three jobs, none of which
/// need one because their WORKFLOW already triggers on `push` alone - reads as admitting every
/// event through `admits`'s own "no gate, admits everything" rule, and the writer-scope check
/// would refuse the very jobs this posture depends on.
fn workflow_triggers(text: &str) -> Vec<&'static str> {
    let mut inside = false;
    let mut found = Vec::new();
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let indent = raw.len().saturating_sub(trimmed.len());
        if indent == 0 {
            inside = trimmed == "on:";
            continue;
        }
        if !inside || indent != 2 {
            continue;
        }
        let Some(key) = trimmed.strip_suffix(':') else { continue };
        if let Some(event) = EVENTS.iter().find(|e| **e == key) {
            found.push(*event);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::judge;

    /// One job: `name`, an optional `if:`, and one `cachix/cachix-action` step naming `cache`.
    fn job(name: &str, gate: Option<&str>, cache: &str) -> String {
        let if_line = gate.map_or_else(String::new, |g| format!("    if: {g}\n"));
        format!(
            "  {name}:\n{if_line}    steps:\n      - uses: {}@bbbb # v17\n        with:\n          name: {cache}\n",
            super::PUBLISH
        )
    }

    /// One workflow: the `on:` triggers named, then the jobs.
    fn workflow(triggers: &[&str], jobs: &str) -> String {
        let on_block = triggers.iter().fold(String::new(), |mut acc, t| {
            acc.push_str("  ");
            acc.push_str(t);
            acc.push_str(":\n");
            acc
        });
        format!("on:\n{on_block}jobs:\n{jobs}")
    }

    #[test]
    fn a_shared_cache_in_a_job_reachable_from_pull_request_is_refused() {
        // The exact mutation review 5303436511 measured passing: `pr-cache` renamed to write
        // `sutura` instead of `sutura-prs`.
        let text = workflow(
            &["push", "pull_request", "merge_group"],
            &job("pr-cache", Some("github.event_name == 'pull_request'"), "sutura"),
        );
        let found = judge(&[("ci.yml", &text)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].contains("pr-cache"), "{found:#?}");
        assert!(found[0].contains("sutura"), "{found:#?}");
    }

    #[test]
    fn a_shared_cache_in_a_job_reachable_from_merge_group_is_refused() {
        let text = workflow(&["push", "merge_group"], &job("queued", None, "sutura-connectors"));
        let found = judge(&[("ci.yml", &text)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found[0].contains("merge_group") || found[0].contains("job `queued`"),
            "{found:#?}"
        );
    }

    #[test]
    fn a_pr_only_cache_in_a_job_reachable_from_push_is_refused() {
        let text = workflow(&["push"], &job("push", None, "sutura-prs"));
        let found = judge(&[("ci.yml", &text)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].contains("sutura-prs"), "{found:#?}");
    }

    #[test]
    fn the_committed_pr_cache_job_is_accepted() {
        let text = workflow(
            &["push", "pull_request", "merge_group"],
            &job("pr-cache", Some("github.event_name == 'pull_request'"), "sutura-prs"),
        );
        assert_eq!(judge(&[("ci.yml", &text)]), Vec::<String>::new());
    }

    #[test]
    fn a_shared_cache_in_a_push_only_workflow_with_no_job_level_if_is_accepted() {
        // cachix-push.yml's own shape: no job-level `if:` at all, because the WORKFLOW already
        // triggers on `push` alone - `workflow_triggers` is what keeps this job from reading as
        // "admits everything" through `admits`'s own bare-`None` rule.
        let text = workflow(&["push"], &job("push", None, "sutura"));
        assert_eq!(judge(&[("ci.yml", &text)]), Vec::<String>::new());
    }

    #[test]
    fn the_committed_workflows_write_every_shared_cache_only_from_a_reachable_push() {
        let Some(root) = crate::repo::root() else { return };
        let found = super::problems(&root);
        assert!(found.is_empty(), "{found:#?}");
    }
}
