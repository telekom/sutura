//! Exactly one Nix installer step may be able to run per event, in one job (#980).
//!
//! **The defect this holds.** `ci`'s first `cachix/install-nix-action` step carried no `if:` and
//! ran on every event; the second, PR-gated to add `sutura-prs`, then found Nix already installed
//! and exited in 18 ms with *"Aborting: Nix is already installed"* - measured on 76/76 PR `ci`
//! logs, 2026-09-16..23. So the PR's own additive cache config was written and never applied, and
//! `judge` (the sibling gate this file's own `problems` extends) stayed green over it the whole
//! time: it reads a writer's own gate, not whether a SIBLING installer already ran for the same
//! event. This module reads the EFFECT instead - can two installer steps in one job both admit
//! one event.
//!
//! **A closed vocabulary, not a general expression evaluator.** `admits` recognises exactly the
//! two shapes this repository writes - no gate at all, and `github.event_name == '<value>'` or
//! `!= '<value>'` with extra `&&` conjuncts - and refuses to guess on anything wider: a `||`
//! anywhere, or a gate this cannot parse, is read as admitting every event. That is the direction
//! that still catches a real overlap rather than waving one through, matching
//! `narrower_than_main_push`'s own refusal-favouring reading of an unrecognised shape.

use super::Step;

/// The one action this rule watches. A name list, like [`super::WRITERS`] - an installer nobody
/// has named here passes unseen, and that is the failure mode to widen when one arrives.
const INSTALLER: &str = "cachix/install-nix-action";

/// Every event a workflow step's `if:` can test in this repository.
const EVENTS: [&str; 3] = ["push", "pull_request", "merge_group"];

/// Refuses two [`INSTALLER`] steps in one job whose gates can both be true for one event.
pub(super) fn problems(files: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    for (label, text) in files {
        for (start, end) in jobs(text) {
            let lines: Vec<&str> = text.lines().collect();
            let block = lines.get(start..end).unwrap_or_default().join("\n");
            let installers: Vec<Step<'_>> = super::steps(&block)
                .into_iter()
                .filter(|step| step.uses().as_deref() == Some(INSTALLER))
                .collect();
            for event in EVENTS {
                let admitting: Vec<usize> = installers
                    .iter()
                    .filter(|step| admits(step.gate().as_deref(), event))
                    .map(|step| step.line.saturating_add(start))
                    .collect();
                if let [first, second, ..] = admitting.as_slice() {
                    out.push(format!(
                        "{label}:{first} and {label}:{second}  two {INSTALLER} steps in one job both admit `{event}` - the second finds Nix already installed and exits without applying its own config (\"Aborting: Nix is already installed\"), so gate one of them out of `{event}`"
                    ));
                }
            }
        }
    }
    out
}

/// Does a step gated by `gate` admit `event`?
///
/// `None` (no `if:` at all) admits everything. A `||` anywhere is refused rather than parsed - the
/// safe direction for a rule whose job is to catch an overlap, not to prove one absent. Otherwise
/// only the FIRST `&&`-conjunct is read, which is exactly what this repository writes today; a gate
/// that narrows further with a later conjunct still starts from "admits", never from "excludes".
fn admits(gate: Option<&str>, event: &str) -> bool {
    let Some(gate) = gate else { return true };
    if gate.contains("||") {
        return true;
    }
    let head = gate.split(" && ").next().unwrap_or(gate);
    if let Some(value) = head
        .strip_prefix("github.event_name == '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return value == event;
    }
    if let Some(value) = head
        .strip_prefix("github.event_name != '")
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return value != event;
    }
    true
}

/// Every job's own `start..end` line span in `text`, keyed the way `retired::job_span` draws it -
/// a two-space key ending in `:`, up to the next line no deeper than that.
///
/// A second, narrower walk rather than a shared one: that function takes a STEP LINE and returns
/// the one span containing it, which is the wrong shape for a caller that wants every span.
fn jobs(text: &str) -> Vec<(usize, usize)> {
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
    use super::admits;

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

    #[test]
    fn admits_reads_the_two_shapes_this_repository_writes() {
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
    }

    #[test]
    fn two_ungated_installers_in_one_job_are_refused_for_every_event() {
        let job = job(&format!("{}{}", installer(None), installer(None)));
        let found = super::problems(&[("ci.yml", &job)]);
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
        assert_eq!(super::problems(&[("ci.yml", &job)]), Vec::<String>::new());
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
        let found = super::problems(&[("ci.yml", &job)]);
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
        assert_eq!(super::problems(&[("ci.yml", &text)]), Vec::<String>::new());
    }

    #[test]
    fn the_committed_ci_workflow_has_no_two_event_overlapping_installer() {
        // THE LIVE ASSERTION - red on the tree #980 found, green once the first installer is
        // gated off `pull_request`.
        let Some(root) = crate::repo::root() else { return };
        let ordinary = crate::workflows::contexts::OrdinaryCi::read(&root);
        let files: Vec<(&str, &str)> = ordinary
            .closure()
            .inspected()
            .iter()
            .map(|file| (file.label(), file.text()))
            .collect();
        assert!(super::problems(&files).is_empty(), "{:#?}", super::problems(&files));
    }
}
