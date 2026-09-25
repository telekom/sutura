//! Exactly one Nix installer step may be able to run per event, in one job (#980).
//!
//! **The defect this holds.** `ci`'s first `cachix/install-nix-action` step carried no `if:` and
//! ran on every event; the second, PR-gated to add `sutura-prs`, then found Nix already installed
//! and exited in 18 ms with *"Aborting: Nix is already installed"* - measured on 76/76 PR `ci`
//! logs, 2026-09-16..23. So the PR's own additive cache config was written and never applied, and
//! `judge` (the sibling gate this file's own `problems` extends) stayed green over it the whole
//! time: it reads a writer's own gate, not whether a SIBLING installer already ran for the same
//! event. This module reads the EFFECT instead - can two installer steps in one job both admit
//! one event, or (review finding 4) can a job that runs `nix` admit an event with NONE.
//!
//! **A closed vocabulary, not a general expression evaluator.** [`admits`] recognises exactly the
//! two shapes this repository writes - no gate at all, and `github.event_name == '<value>'` or
//! `!= '<value>'` with extra `&&` conjuncts - and refuses to guess on anything wider: a `||`
//! anywhere, or a gate this cannot parse, is read as admitting every event. That is the direction
//! that still catches a real overlap rather than waving one through, matching
//! `narrower_than_main_push`'s own refusal-favouring reading of an unrecognised shape. Event
//! names compare `eq_ignore_ascii_case` - GitHub's own comparison is case-insensitive - so a gate
//! spelled `'Push'` is not a way around either half of this rule.
//!
//! **Scoped to every workflow file, not the ordinary-CI closure `judge`/`retired` read.** Review
//! finding 4(b): a duplicate installer in `cachix-push.yml`'s `push` job - a workflow with no
//! gating trigger, so it is outside `OrdinaryCi` - passed unseen. This rule's property has
//! nothing to do with which workflow gates a merge, so [`problems`] walks
//! `.github/workflows/*.yml` (and `.yaml`) directly.

use std::path::Path;

use super::Step;

/// The one action this rule watches. A name list, like [`super::WRITERS`] - an installer nobody
/// has named here passes unseen, and that is the failure mode to widen when one arrives.
const INSTALLER: &str = "cachix/install-nix-action";

/// Every event a workflow step's `if:` can test in this repository.
///
/// `pub(super)` - [`super::publish_scope`] (#1000) walks the same three events over the same
/// job-admission question, over a different step.
pub(super) const EVENTS: [&str; 3] = ["push", "pull_request", "merge_group"];

/// One workflow file's label and text, named for `clippy::type_complexity`'s sake - the same
/// reason `nix_block::Block` exists.
type WorkflowFile = (String, String);

/// Every `.yml`/`.yaml` file directly under `.github/workflows`, read and labelled.
///
/// `Err` on anything [`crate::repo::census::Census::inspect`] itself refuses (an unreachable
/// subtree, an empty discovery) - fails closed rather than silently checking a partial tree, the
/// same direction [`super::super::contexts::OrdinaryCi::read`] takes for its own narrower set.
///
/// `pub(super)` - [`super::publish_scope`] needs the identical file set.
pub(super) fn every_workflow_file(root: &Path) -> Result<Vec<WorkflowFile>, String> {
    let census = crate::repo::collect_files(root, &root.join(".github/workflows"), &["yml", "yaml"]);
    let mut found = Vec::new();
    census
        .inspect(
            &[],
            |_rel: &str| true,
            |rel, bytes| {
                if let Ok(text) = std::str::from_utf8(bytes) {
                    found.push((String::from(rel), String::from(text)));
                }
            },
        )
        .map(|_| ())
        .map_err(|why| format!("{why:?}"))?;
    Ok(found)
}

/// Every workflow file, walked for [`judge`]'s two rules.
pub(super) fn problems(root: &Path) -> Vec<String> {
    match every_workflow_file(root) {
        Ok(files) => {
            let borrowed: Vec<(&str, &str)> = files.iter().map(|(label, text)| (label.as_str(), text.as_str())).collect();
            judge(&borrowed)
        }
        Err(why) => vec![format!(
            "could not read every workflow file under .github/workflows - the two rules below would pass over a partial tree: {why}"
        )],
    }
}

/// The same two rules over labelled text, so a fixture can hold them.
///
/// 1. Refuses two [`INSTALLER`] steps in one job whose gates can both be true for one event.
/// 2. Refuses a job that runs `nix` and admits an event with NO installer admitting it - review
///    finding 4(a): deleting `ci`'s PR-only installer left `pull_request` with zero, and nothing
///    here said so.
fn judge(files: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    for (label, text) in files {
        let lines: Vec<&str> = text.lines().collect();
        for (start, end) in jobs(text) {
            let block = lines.get(start..end).unwrap_or_default().join("\n");
            let installers: Vec<Step<'_>> = super::steps(&block)
                .into_iter()
                .filter(|step| step.uses().as_deref() == Some(INSTALLER))
                .collect();
            let job_gate = job_if(&lines, start, end);
            // COMMENTS EXCLUDED, through `key_of` - a raw `block.contains(...)` over the whole
            // span (comments included) is what first tripped this rule on `crap-comment`: the
            // job right above `identity-classify` in the file, whose own trailing comment block
            // (attributed to `crap-comment`'s span, not `identity-classify`'s - the same
            // "comments own no steps, whichever job's span they land in" shape `retired::job_span`
            // documents) mentions `nix run .#xtask -- classify` in PROSE. `crap-comment` itself
            // runs no nix at all.
            let runs_nix = lines
                .get(start..end)
                .unwrap_or_default()
                .iter()
                .any(|line| super::key_of(line).is_some_and(|key| key.contains("nix run") || key.contains("nix build")));
            for event in EVENTS {
                if !admits(job_gate.as_deref(), event) {
                    continue;
                }
                let admitting: Vec<usize> = installers
                    .iter()
                    .filter(|step| admits(step.gate().as_deref(), event))
                    .map(|step| step.line.saturating_add(start))
                    .collect();
                match admitting.as_slice() {
                    [] => {
                        if runs_nix {
                            out.push(format!(
                                "{label}:{}  this job runs nix and admits `{event}`, but no {INSTALLER} step admits it - nix has never been installed on that path",
                                start.saturating_add(1)
                            ));
                        }
                    }
                    [_] => {}
                    [first, second, ..] => {
                        out.push(format!(
                            "{label}:{first} and {label}:{second}  two {INSTALLER} steps in one job both admit `{event}` - the second finds Nix already installed and exits without applying its own config (\"Aborting: Nix is already installed\"), so gate one of them out of `{event}`"
                        ));
                    }
                }
            }
        }
    }
    out
}

/// Does a step (or job) gated by `gate` admit `event`?
///
/// `None` (no `if:` at all) admits everything. A `||` anywhere is refused rather than parsed - the
/// safe direction for a rule whose job is to catch an overlap, not to prove one absent. Otherwise
/// only the FIRST `&&`-conjunct is read, which is exactly what this repository writes today; a gate
/// that narrows further with a later conjunct still starts from "admits", never from "excludes".
/// Compared `eq_ignore_ascii_case`: GitHub Actions compares strings case-insensitively, so a gate
/// spelled `'Push'` still admits `push`.
///
/// `pub(super)` - [`super::publish_scope`] asks the same question of a job's `if:`.
pub(super) fn admits(gate: Option<&str>, event: &str) -> bool {
    let Some(gate) = gate else { return true };
    if gate.contains("||") {
        return true;
    }
    let head = gate.split(" && ").next().unwrap_or(gate);
    if let Some(value) = head
        .strip_prefix("github.event_name == '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return value.eq_ignore_ascii_case(event);
    }
    if let Some(value) = head
        .strip_prefix("github.event_name != '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return !value.eq_ignore_ascii_case(event);
    }
    true
}

/// The `if:` this job carries at its OWN key depth (a sibling of `steps:`/`runs-on:`), collapsed
/// like [`Step::gate`] - `None` where the job carries no such key, which [`admits`] reads as
/// "runs on everything", the direction that keeps checking rather than waving a job through.
///
/// `start`/`end` are the job's own body span from [`jobs`] - without `end` this would scan past
/// the job entirely and could match a LATER job's `if:` at the same depth as its own.
/// `start` is also the first line of the body (one past the job's `<name>:` header), so the
/// header's own depth - and therefore the body's key depth - is read off the line before it.
///
/// `pub(super)` - [`super::publish_scope`] needs the same job-level gate.
pub(super) fn job_if(lines: &[&str], start: usize, end: usize) -> Option<String> {
    let header = lines.get(start.checked_sub(1)?)?;
    let scope = header.len().saturating_sub(header.trim_start().len()).saturating_add(2);
    for line in lines.get(start..end).into_iter().flatten() {
        let depth = line.len().saturating_sub(line.trim_start().len());
        if !line.trim().is_empty() && depth < scope {
            break;
        }
        if depth != scope {
            continue;
        }
        let Some(key) = super::key_of(line) else { continue };
        let Some(value) = key.strip_prefix("if:") else { continue };
        let value = value.trim();
        let inner = value
            .strip_prefix("${{")
            .and_then(|rest| rest.strip_suffix("}}"))
            .unwrap_or(value);
        return Some(inner.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    None
}

/// Every job's own `start..end` line span in `text`, keyed the way `retired::job_span` draws it -
/// a two-space key ending in `:`, up to the next line no deeper than that.
///
/// A second, narrower walk rather than a shared one: that function takes a STEP LINE and returns
/// the one span containing it, which is the wrong shape for a caller that wants every span.
///
/// `pub(super)` - [`super::publish_scope`] walks the same job spans.
pub(super) fn jobs(text: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut spans = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if line.len().saturating_sub(trimmed.len()) != 2 || !trimmed.ends_with(':') || trimmed.starts_with('#') {
            continue;
        }
        let end = lines
            .iter()
            .enumerate()
            .skip(index.saturating_add(1))
            .find(|(_, next)| {
                let nt = next.trim_start();
                !nt.is_empty() && next.len().saturating_sub(nt.len()) <= 2 && nt.ends_with(':') && !nt.starts_with('#')
            })
            .map_or(lines.len(), |(at, _)| at);
        spans.push((index.saturating_add(1), end));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::{admits, judge};

    fn job(steps: &str) -> String {
        format!("  example:\n    steps:\n{steps}  next-job:\n    steps:\n")
    }

    fn installer(gate: Option<&str>) -> String {
        let if_line = gate.map_or_else(String::new, |g| format!("        if: ${{{{ {g} }}}}\n"));
        format!(
            "      - uses: {}@bbbb # v31\n{if_line}        with:\n          extra_nix_config: |\n            fallback = true\n",
            super::INSTALLER
        )
    }

    /// A step that runs nix, so [`judge`]'s zero-installer rule has something to react to. No
    /// existing fixture in this file ran nix at all, which is why that rule needed its own tests
    /// rather than inheriting coverage from the overlap ones.
    fn runs_nix() -> &'static str {
        "      - name: Build\n        run: nix build .#checks.x86_64-linux.hygiene -L\n"
    }

    #[test]
    fn admits_reads_the_two_shapes_this_repository_writes_case_insensitively() {
        assert!(admits(None, "pull_request"), "no gate admits everything");
        assert!(admits(Some("github.event_name == 'pull_request'"), "pull_request"));
        assert!(!admits(Some("github.event_name == 'pull_request'"), "push"));
        assert!(admits(Some("github.event_name != 'pull_request'"), "push"));
        assert!(!admits(Some("github.event_name != 'pull_request'"), "pull_request"));
        // Extra `&&` conjuncts do not widen what the head already excluded.
        assert!(!admits(
            Some("github.event_name != 'pull_request' && !cancelled()"),
            "pull_request"
        ));
        // An unrecognised shape, or one carrying `||`, is read as admitting - the refusal direction.
        assert!(admits(Some("github.ref == 'refs/heads/main'"), "push"));
        assert!(admits(
            Some("github.event_name == 'pull_request' || github.event_name == 'push'"),
            "merge_group"
        ));
        // GitHub compares event names case-insensitively - review finding 4: a gate spelled
        // `'Push'` still admits `push`, on both the `==` and `!=` arms.
        assert!(admits(Some("github.event_name == 'Push'"), "push"));
        assert!(!admits(Some("github.event_name != 'Push'"), "push"));
    }

    #[test]
    fn two_ungated_installers_in_one_job_are_refused_for_every_event() {
        let job = job(&format!("{}{}", installer(None), installer(None)));
        let found = judge(&[("ci.yml", &job)]);
        assert_eq!(found.len(), super::EVENTS.len(), "{found:#?}");
        for event in super::EVENTS {
            assert!(found.iter().any(|p| p.contains(&format!("`{event}`"))), "{found:#?}");
        }
    }

    #[test]
    fn one_gated_off_the_other_s_event_is_accepted() {
        // The shape #980 fixes ci.yml INTO: the unconditional step gated OFF `pull_request`, the
        // PR-only step unchanged. Neither admits the other's sole event.
        let job = job(&format!(
            "{}{}",
            installer(Some("github.event_name != 'pull_request'")),
            installer(Some("github.event_name == 'pull_request'"))
        ));
        assert_eq!(judge(&[("ci.yml", &job)]), Vec::<String>::new());
    }

    #[test]
    fn the_defect_shape_is_refused_for_pull_request_only() {
        // The shape #980 fixes ci.yml OUT OF: one unconditional installer, one PR-gated - both
        // admit `pull_request`, and only that event.
        let job = job(&format!(
            "{}{}",
            installer(None),
            installer(Some("github.event_name == 'pull_request'"))
        ));
        let found = judge(&[("ci.yml", &job)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].contains("pull_request"), "{found:#?}");
    }

    #[test]
    fn installers_in_different_jobs_never_overlap() {
        let text = format!(
            "  example:\n    steps:\n{}  next-job:\n    steps:\n{}",
            installer(None),
            installer(None)
        );
        assert_eq!(judge(&[("ci.yml", &text)]), Vec::<String>::new());
    }

    #[test]
    fn a_job_that_runs_nix_with_zero_admitting_installers_is_refused() {
        // Review finding 4(a), the exact probe: deleting the PR-only installer step leaves the
        // unconditional one gated OFF pull_request, so pull_request keeps zero. Reproduced
        // directly - this fixture never had the second installer at all - because a diff cannot
        // "delete" a fixture, only start from the after-shape.
        let job = format!(
            "  example:\n    steps:\n{}{}  next-job:\n    steps:\n",
            installer(Some("github.event_name != 'pull_request'")),
            runs_nix(),
        );
        let found = judge(&[("ci.yml", &job)]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].contains("pull_request"), "{found:#?}");
        assert!(found[0].contains("no"), "{found:#?}");
    }

    #[test]
    fn a_job_that_runs_nix_with_an_installer_for_every_admitted_event_is_accepted() {
        let job = format!("  example:\n    steps:\n{}{}", installer(None), runs_nix());
        assert_eq!(judge(&[("ci.yml", &job)]), Vec::<String>::new());
    }

    #[test]
    fn a_job_gated_off_an_event_is_not_refused_for_running_nix_there() {
        // pr-cache's own shape: the JOB's `if:` restricts it to `pull_request` alone, so it is
        // not refused for admitting no installer on `push`/`merge_group` - it never runs there.
        let job = format!(
            "  pr-cache:\n    if: github.event_name == 'pull_request' && !cancelled()\n    steps:\n{}{}",
            installer(Some("github.event_name == 'pull_request'")),
            runs_nix(),
        );
        assert_eq!(judge(&[("ci.yml", &job)]), Vec::<String>::new());
    }

    #[test]
    fn a_job_that_does_not_run_nix_needs_no_installer_at_all() {
        let job = "  crap-comment:\n    steps:\n      - name: Post\n        run: gh api ...\n";
        assert_eq!(judge(&[("ci.yml", job)]), Vec::<String>::new());
    }

    #[test]
    fn a_comment_mentioning_nix_does_not_make_a_job_look_like_it_runs_one() {
        // THE DEFECT THIS RULE SHIPPED WITH, found by provocation on the real tree:
        // `crap-comment`'s own trailing comment block - attributed to ITS span rather than the
        // next job's, the same "comments own no steps, whichever job's span they land in" shape
        // `retired::job_span` documents - mentions `nix run .#xtask` in prose. A raw substring
        // scan over the whole span read that as the job running nix; only `key_of`-filtered
        // lines do not.
        let job = concat!(
            "  crap-comment:\n",
            "    steps:\n",
            "      - name: Post\n",
            "        run: gh api ...\n",
            "  # This job pays the SAME `nix run .#xtask -- classify` cost as elsewhere.\n",
            "  next-job:\n",
            "    steps:\n",
            "      - name: Real\n",
            "        run: gh api ...\n",
        );
        assert_eq!(judge(&[("ci.yml", job)]), Vec::<String>::new());
    }

    #[test]
    fn the_committed_workflows_have_no_two_event_overlapping_or_zero_event_installer() {
        // THE LIVE ASSERTION - red on the tree #980 found, green once the first installer is
        // gated off `pull_request`; ALSO scoped to every workflow file now (review finding 4(b)),
        // not only the ones ordinary CI reaches - `cachix-push.yml`'s three jobs included.
        let Some(root) = crate::repo::root() else { return };
        let found = super::problems(&root);
        assert!(found.is_empty(), "{found:#?}");
    }
}
