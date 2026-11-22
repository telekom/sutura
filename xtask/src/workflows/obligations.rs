//! The `ci.yml` steps whose obligation must not be skippable OR slow, and the line that holds each.
//!
//! **The finding this exists for.** `Secrets` calls itself the authoritative secret scan and
//! carried no condition at all, while the step above it was gated on the `rust` classification. A
//! failed step ends the job, so the scan was reachable only while every step above it was green:
//! a rustfmt violation skipped the credential scan entirely. The fix was a condition on the step,
//! and the argument for preferring it to a reorder was that **a line in the step is a mechanism
//! and a position is not** - the next reorder loses a position and keeps a line.
//!
//! That argument was false on arrival, because nothing required the line. The reorder it was
//! preferred to is at least visible in a diff; a condition nobody checks is one edit from gone,
//! and the PR that added it asserted a mechanism that did not exist. This module is that
//! mechanism. It is deliberately a rule inside `check-workflows` rather than a new hygiene gate:
//! the file is already parsed here, and a second gate over the same text is how two readers come
//! to disagree.
//!
//! # Why the required condition is not the same for every step
//!
//! Measured over this repository's own runs rather than chosen:
//!
//! * **A preceding step's RED.** `pr-cache` is gated on `!cancelled()` over `needs: [ci]`, and in
//!   twelve runs where the `ci` job concluded `failure` it ran in every one - never `skipped`. So
//!   `!cancelled()` survives a red, which is the case `Secrets` was broken for.
//! * **A CANCELLATION.** In five runs where the `ci` job itself concluded `cancelled`, the one
//!   step in it carrying `always()` ran in all five, and in two of those five every unconditional
//!   step was skipped. There is **no** step-level `!cancelled()` witness inside `ci` either way,
//!   because no step there carried a reachability condition before this rule.
//!
//! So they differ by whether a cancelled run still owes the obligation:
//!
//! * `Secrets` takes `always() && github.event_name != 'push'`. A committed credential is
//!   **already disclosed** by the time any gate runs, so there is nothing to defer to the next
//!   push, and `always()` is the only form with a step-level witness of surviving a
//!   cancellation. The `!= 'push'` conjunct is #985 D2: the merge queue already ran the same
//!   tree, so push `ci` is reduced to its 3 unique outputs and the secret scan runs only on
//!   pull request and merge group. The usual argument against `always()` does not reach here:
//!   `!cancelled()` is preferred for `pr-cache` because that job takes a **paid runner** and
//!   `always()` would start it after somebody pressed cancel. This is a two-second step inside
//!   a runner already held, so defeating a cancel costs two seconds.
//! * `Licensing` takes `!cancelled() && github.event_name != 'push'`. It is a `nix build`, so
//!   `always()` would extend a cancelled job by a real amount, and its value is
//!   **merge-blocking**: a licence violation cannot merge whatever this run says, so deferring
//!   it to the next push costs a round trip and not a guarantee. Cancel should mean cancel
//!   here. The `!= 'push'` conjunct is #985 D2, for the same reason as `Secrets`.
//! * `Chart` takes `!cancelled() && github.event_name != 'push'`, for `Licensing`'s own reason:
//!   a `nix build` over `charts/sutura` (#149 branch 1), merge-blocking, and cheap enough that
//!   `always()` buys nothing a cancelled run would rather skip. The `!= 'push'` conjunct is
//!   #985 D2.
//!
//! **The limit, next to the claim.** This rule holds the condition on the steps named in
//! [`REQUIRED`] and nothing wider. It does not require any OTHER step to stay reachable - adding
//! one is an edit here. Nor does it verify that `always()` behaves as measured; that is GitHub's
//! semantics, observed over the runs above and not reproduced by this repository.
//!
//! The release bump skips both core CI and the ordinary QA caller. The linked-driver exercise
//! inherits the QA caller's release guard. The tag runs full QA and checks the published binaries.
//!
//! # The second thing this holds: LATENCY, not just skippability (#512(b))
//!
//! A condition on a step proves it can still report after a red above it - it says nothing about
//! how long it waits to report. `Classify the change` measured a 238 s p90 over 20-21 s of it
//! being the `nix run .#xtask` closure, and every step named in [`REQUIRED`] used to sit BEHIND
//! it despite needing neither `xtask` nor `cargoArtifacts` - so a cheap gate that could report in
//! seconds instead waited on a closure it never touches. [`order_problems`] is the mechanism for
//! that half: every required step's position must be strictly before the first step in the file
//! whose body runs [`XTASK_INVOCATION`]. **This is a POSITION rule and the one place in this
//! module that is** - `check-workflows` reads the file text, not a schedule, so it cannot see
//! how long any step actually took; the 238 s figure above is measured once, from the jobs API,
//! and not reproduced here.
//!
//! # The pull-request TITLE obligation moved out with its step (#1257)
//!
//! The step left `ci.yml` (#1257); what decides the event it runs on is now `pr-title.yml`'s `on:`,
//! held by [`pr_title_check`] - it fires on `edited` for #1249 and on `merge_group` because the
//! context is required.

use std::path::Path;

/// A step whose `if:` is held by this rule, and the exact form it needs.
///
/// Every row is a REACHABILITY obligation: a condition that keeps the step reporting after a red
/// above it.
struct Obligation {
    /// The step's `name:` value, which is also how a reader finds it.
    step: &'static str,
    /// The `if:` expression, with `${{ }}` and surplus whitespace already removed.
    condition: &'static str,
}

const REQUIRED: &[Obligation] = &[
    Obligation {
        step: "Secrets",
        condition: "always() && github.event_name != 'push'",
    },
    Obligation {
        step: "Licensing",
        condition: "!cancelled() && github.event_name != 'push'",
    },
    Obligation {
        step: "Chart",
        condition: "!cancelled() && github.event_name != 'push'",
    },
];

/// How many obligations this rule holds, for the success line.
pub(super) const fn held() -> usize {
    REQUIRED.len()
}

/// The text that marks a step as paying for the `xtask` closure - see the header's second
/// section. Matched as a plain substring, never parsed, so it catches both a single-line
/// `run: nix run .#xtask -- …` step and a block-scalar one the same way.
const XTASK_INVOCATION: &str = "nix run .#xtask";

/// Every [`REQUIRED`] step found sitting behind the first step that runs [`XTASK_INVOCATION`].
///
/// A step already missing by name is not re-reported here - [`check`]'s own loop already refused
/// it, and comparing an absent step's position would either panic or say nothing new.
fn order_problems(steps: &[super::cache_scope::Step<'_>]) -> Vec<String> {
    let Some((xtask_at, xtask_step)) = steps
        .iter()
        .enumerate()
        .find(|(_, step)| step.contains_verbatim(XTASK_INVOCATION))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for want in REQUIRED {
        let Some((mine, step)) = steps
            .iter()
            .enumerate()
            .find(|(_, step)| step.input("name:") == Some(want.step))
        else {
            continue;
        };
        if mine > xtask_at {
            out.push(format!(
                "ci.yml:{} `{}` sits behind ci.yml:{} which runs `{XTASK_INVOCATION}` - a closure-free obligation must report before that closure is realised, or its `if:` only buys it a slow reachable report instead of a fast one",
                step.line, want.step, xtask_step.line
            ));
        }
    }
    out
}

/// Every step obligation this rule finds broken, empty when all of them hold.
///
/// Fails CLOSED on an unreadable file and on a step it cannot find: a named step that is absent
/// is indistinguishable, to a text reader, from a rule that has stopped matching anything, and
/// the reassuring pass is the failure mode a scan like this is most prone to. `pr-title.yml`'s
/// trigger obligations are appended, so `check-workflows` refuses them under the same call.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let path = root.join(".github/workflows/ci.yml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![format!("{} could not be read, so no step obligation is held", path.display())];
    };
    let mut out = check(&text);
    out.extend(pr_title_problems(root));
    out
}

/// The rule itself, over the text, so a test can put a tree in front of it.
fn check(text: &str) -> Vec<String> {
    let steps = super::cache_scope::steps(text);
    let mut out = Vec::new();
    for want in REQUIRED {
        let Some(step) = steps.iter().find(|step| step.input("name:") == Some(want.step)) else {
            out.push(format!(
                "ci.yml declares no step named `{}` - either it was renamed and its reachability is now held by nothing, or this rule has stopped matching",
                want.step
            ));
            continue;
        };
        match step.gate() {
            None => out.push(format!(
                "ci.yml:{} `{}` carries no `if:`. A failed step ends the job, so it runs only while every step above it is green - `if: ${{{{ {} }}}}` is what makes it reachable regardless",
                step.line, want.step, want.condition
            )),
            Some(gate) if gate != want.condition => out.push(format!(
                "ci.yml:{} `{}` is gated on `{gate}`, but its obligation needs `{}` - see the header of xtask/src/workflows/obligations.rs for which case each form survives",
                step.line, want.step, want.condition
            )),
            Some(_) => {}
        }
    }
    out.extend(order_problems(&steps));
    out
}

/// The activity types `pr-title.yml` must fire on. `edited` is #1249's: the `pull_request`
/// default types omit it, so a rename alone started no run.
const PR_TITLE_EVENT_TYPES: &[&str] = &["opened", "edited", "reopened", "synchronize"];

/// The trigger obligations of `pr-title.yml`. Fails CLOSED on an unreadable file, as [`problems`].
fn pr_title_problems(root: &Path) -> Vec<String> {
    let path = root.join(".github/workflows/pr-title.yml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![format!(
            "{} could not be read, so the pull-request title trigger obligation is held by nothing",
            path.display()
        )];
    };
    pr_title_check(&text)
}

/// The trigger rule over the text, so a test can put a workflow in front of it.
fn pr_title_check(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    match pull_request_types(text) {
        None => out.push(String::from(
            "pr-title.yml declares no `pull_request` trigger with a `types: [..]` list - the title check has to fire on the event that carries the subject, and on `edited` so a rename alone fires it (#1249)",
        )),
        Some(types) => out.extend(PR_TITLE_EVENT_TYPES.iter().filter(|want| !types.contains(want)).map(|want| {
            format!("pr-title.yml's `pull_request` trigger omits `{want}` - the title check must fire on it (#1249)")
        })),
    }
    if !super::contexts::block_keys(text, "on:")
        .iter()
        .any(|key| key == "merge_group")
    {
        out.push(String::from(
            "pr-title.yml declares no `merge_group` trigger - `pr-title` is a required context, so a queue entry waits on it until the queue's timeout drops the entry",
        ));
    }
    out
}

/// The flow-style `types: [..]` list directly under the top-level `on:` block's `pull_request:`.
fn pull_request_types(text: &str) -> Option<Vec<&str>> {
    let mut lines = text.lines().filter(|line| {
        let trimmed = line.trim_start();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    });
    lines.by_ref().find(|line| *line == "on:")?;
    let mut on = lines.take_while(|line| line.starts_with(' '));
    on.by_ref().find(|line| *line == "  pull_request:")?;
    on.take_while(|line| line.starts_with("    "))
        .find_map(|line| line.strip_prefix("    types:"))?
        .trim()
        .strip_prefix('[')?
        .strip_suffix(']')
        .map(|list| list.split(',').map(str::trim).collect())
}

/// The CI and reusable QA jobs skip the main release bump. Release calls full QA on its tag.
const RELEASE_SKIP_CLAUSE: &str = "!startsWith(github.event.head_commit.message || '', 'chore(release):')";
const RELEASE_SKIP: &[&str] = &["ci", "qa", "qa-plan"];

/// How many release-skip rows this rule holds, for the success line.
pub(super) const fn release_skip_held() -> usize {
    RELEASE_SKIP.len()
}

/// Both credentialed QA jobs retain their event eligibility gate.
const PINNED_JOB_IF: &[(&str, &str)] = &[
    (
        "bigquery-conformance",
        "contains(fromJSON(needs.qa-plan.outputs.suite-plan || '{}').selected, 'bigquery-conformance') && (github.event_name != 'pull_request' || (github.event.pull_request.head.repo.full_name == github.repository && github.event.pull_request.user.login != 'dependabot[bot]')) && (github.event_name != 'workflow_dispatch' || github.ref == 'refs/heads/main')",
    ),
    (
        "e2e-datahub-adbc",
        "contains(fromJSON(needs.qa-plan.outputs.suite-plan || '{}').selected, 'e2e-datahub-adbc') && (github.event_name != 'pull_request' || (github.event.pull_request.head.repo.full_name == github.repository && github.event.pull_request.user.login != 'dependabot[bot]')) && (github.event_name != 'workflow_dispatch' || github.ref == 'refs/heads/main')",
    ),
];

/// How many job conditions are pinned to exact text, for the success line.
pub(super) const fn pinned_job_if_held() -> usize {
    PINNED_JOB_IF.len()
}

/// The pinned-condition rule itself, over the text, so a test can put a tree in front of it.
fn pinned_job_if_check(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    PINNED_JOB_IF
        .iter()
        .filter_map(|&(job, pinned)| match job_if(&lines, job) {
            Some(gate) if gate == pinned => None,
            Some(gate) => Some(format!("ci.yml `{job}` is gated on `{gate}`, not the pinned `{pinned}`")),
            None => Some(format!(
                "ci.yml declares no `if:` on `{job}`, so its pinned condition `{pinned}` is gone"
            )),
        })
        .collect()
}

/// Every [`RELEASE_SKIP`] and [`PINNED_JOB_IF`] obligation this rule finds broken, empty when all
/// hold.
///
/// Fails CLOSED on an unreadable file and on a job with no `if:` - the same convention as
/// [`problems`]: a reassuring pass over an absent clause is the failure mode this scan is most
/// prone to, and a job that dropped its `if:` is indistinguishable from one that was renamed.
pub(super) fn job_if_problems(root: &Path) -> Vec<String> {
    let path = root.join(".github/workflows/ci.yml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![format!(
            "{} could not be read, so no job `if:` obligation is held",
            path.display()
        )];
    };
    let qa = std::fs::read_to_string(root.join(".github/workflows/qa.yml")).unwrap_or_default();
    let mut problems = [release_skip_check(&text), pinned_job_if_check(&text)].concat();
    if !linked_driver_runs_in_qa(root, &qa) {
        problems.push(String::from(
            "QA must execute the linked-driver check for selected BigQuery or PostgreSQL changes and require the matrix result",
        ));
    }
    problems
}

fn linked_driver_runs_in_qa(root: &Path, qa: &str) -> bool {
    let runner = std::fs::read_to_string(root.join("nix/ci-suite.sh")).unwrap_or_default();
    crate::changes::ci_suites::linked_driver_is_enrolled(root)
        && runner
            .lines()
            .any(|line| line == "bigquery-driver) bash nix/bigquery-driver-check.sh ;;")
        && super::step::job(qa, "integration")
            .is_some_and(|lines| lines.contains(&"      matrix: ${{ fromJSON(inputs.suite-plan).matrix }}"))
        && super::step::named_step(qa, "integration", "Run suite").is_some_and(|lines| {
            lines.contains(&"          SUITE: ${{ matrix.id }}")
                && lines.contains(&r#"        run: bash nix/ci-suite.sh "$SUITE""#)
        })
        && super::step::job(qa, "integration-verdict").is_some_and(|lines| {
            lines.contains(&"          MATRIX_COUNT: ${{ fromJSON(inputs.suite-plan || '{}').count }}")
                && lines.contains(&"          MATRIX_RESULT: ${{ needs.integration.result }}")
                && lines.contains(&r#"            *) require "integration matrix" "$MATRIX_RESULT" true ;;"#)
        })
}

/// The release-skip rule itself, over the text, so a test can put a tree in front of it.
fn release_skip_check(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for &job in RELEASE_SKIP {
        let Some(gate) = job_if(&lines, job) else {
            out.push(format!(
                "ci.yml declares no `if:` on `{job}` - the release-skip clause `{RELEASE_SKIP_CLAUSE}` is gone, so a release commit re-runs the job. Either the job was renamed and the clause moved with it, or this rule has stopped matching"
            ));
            continue;
        };
        if !carries_release_skip(&gate) {
            out.push(format!(
                "ci.yml `{job}` is gated on `{gate}`, which does not carry the release-skip clause `{RELEASE_SKIP_CLAUSE}` as a conjunct - a release commit re-runs the job. See the header of xtask/src/workflows/obligations.rs for what this pins and what it does not"
            ));
        }
    }
    out
}

/// Does `gate` carry [`RELEASE_SKIP_CLAUSE`] as a conjunct?
///
/// `CLAUSE` alone, or `CLAUSE && …` where the remainder holds no `||` - the same shape
/// `cache_scope::narrower_than_main_push` uses, because a `||` in the remainder can widen the
/// expression past the clause. Anything else is refused, including an expression that merely
/// CONTAINS the clause: `!(CLAUSE)` and `CLAUSE || true` both pass a `contains` and both fail here.
fn carries_release_skip(gate: &str) -> bool {
    let Some(rest) = gate.strip_prefix(RELEASE_SKIP_CLAUSE) else {
        return false;
    };
    rest.strip_prefix(" && ")
        .map_or(rest.is_empty(), |extra| !extra.is_empty() && !extra.contains("||"))
}

/// The value of a job's `if:` key, read at the job's own depth and normalized the way `Step::gate`
/// normalizes (`${{ }}` stripped, whitespace collapsed). `None` when the job declares no `if:`.
///
/// Reads at job-key depth only - the same approach `contexts::job_property` uses, because a
/// step-level `if:` sits two levels deeper and is not this rule's subject. A job key is two spaces
/// under `jobs:`, and its direct children sit at four, which is the only depth an `if:` on the job
/// itself can appear.
fn job_if(lines: &[&str], id: &str) -> Option<String> {
    let opener = format!("{id}:");
    let mut inside = false;
    for raw in lines {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let indent = raw.len().saturating_sub(trimmed.len());
        if !inside {
            inside = indent == JOB_INDENT && trimmed == opener;
            continue;
        }
        if indent <= JOB_INDENT {
            return None;
        }
        if indent == JOB_INDENT.saturating_add(2)
            && let Some(value) = trimmed.strip_prefix("if:")
        {
            let value = value.trim();
            let inner = value
                .strip_prefix("${{")
                .and_then(|rest| rest.strip_suffix("}}"))
                .unwrap_or(value);
            return Some(inner.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    None
}

/// A job key's column. Two spaces under `jobs:`, which is the only shape GitHub accepts.
const JOB_INDENT: usize = 2;

#[cfg(test)]
mod tests {
    use core::fmt::Write as _;

    use super::{
        PINNED_JOB_IF, PR_TITLE_EVENT_TYPES, RELEASE_SKIP, RELEASE_SKIP_CLAUSE, REQUIRED, check, pinned_job_if_check,
        pr_title_check, release_skip_check,
    };

    /// A `ci.yml` shaped like the real one, with each named step given `gate` as its condition.
    ///
    /// `None` writes the step with no `if:` at all, which is the state the finding was about.
    /// Every required step here is closure-free, so these trees exercise the condition half; the
    /// order half is exercised below with an explicit `xtask_step`.
    fn tree(gates: &[Option<&str>]) -> String {
        let mut text = String::from("jobs:\n  ci:\n    steps:\n");
        for (want, gate) in REQUIRED.iter().zip(gates) {
            text.push_str(&step(want.step, *gate));
        }
        text
    }

    /// One step, with `gate` as its condition - `None` writes no `if:` at all.
    fn step(name: &str, gate: Option<&str>) -> String {
        let mut text = format!("      - name: {name}\n");
        if let Some(gate) = gate {
            text.push_str("        if: ${{ ");
            text.push_str(gate);
            text.push_str(" }}\n");
        }
        text.push_str("        run: true\n");
        text
    }

    #[test]
    fn the_required_conditions_pass() {
        let gates: Vec<Option<&str>> = REQUIRED.iter().map(|want| Some(want.condition)).collect();
        assert_eq!(check(&tree(&gates)), Vec::<String>::new());
    }

    #[test]
    fn a_step_with_no_condition_is_refused() {
        let gates: Vec<Option<&str>> = REQUIRED.iter().map(|_| None).collect();
        let found = check(&tree(&gates));
        assert_eq!(
            found.len(),
            REQUIRED.len(),
            "every unconditioned step should be named: {found:?}"
        );
        assert!(found.iter().all(|problem| problem.contains("carries no `if:`")), "{found:?}");
    }

    /// The mutation the real finding was: a condition that exists but is the wrong form.
    #[test]
    fn the_wrong_form_is_refused() {
        let other = |want: &super::Obligation| {
            if want.condition == "always()" {
                "!cancelled()"
            } else {
                "always()"
            }
        };
        let gates: Vec<Option<&str>> = REQUIRED.iter().map(|want| Some(other(want))).collect();
        let found = check(&tree(&gates));
        assert_eq!(found.len(), REQUIRED.len(), "{found:?}");
        assert!(
            found.iter().all(|problem| problem.contains("its obligation needs")),
            "{found:?}"
        );
    }

    /// Fails closed: a renamed step must not read as a clean tree.
    #[test]
    fn a_missing_step_is_refused_rather_than_passing_over_nothing() {
        let found = check("jobs:\n  ci:\n    steps:\n      - name: Something Else\n        run: true\n");
        assert_eq!(found.len(), REQUIRED.len(), "{found:?}");
        assert!(
            found.iter().all(|problem| problem.contains("declares no step named")),
            "{found:?}"
        );
    }

    /// #149 branch 1: the chart's own gate is registered here alongside `Licensing`, on the
    /// same reasoning - a `nix build`, merge-blocking, closure-free. Naming it, rather than
    /// trusting the generic tests above to cover it: those iterate `REQUIRED` itself, so an
    /// entry that was never added would leave them passing over one fewer obligation and
    /// nothing would say so.
    #[test]
    fn the_chart_step_is_a_required_obligation() {
        assert!(
            REQUIRED
                .iter()
                .any(|o| o.step == "Chart" && o.condition == "!cancelled() && github.event_name != 'push'"),
            "the chart's step obligation is missing or holds the wrong condition"
        );
    }

    /// The specific finding, not just the count: a tree with every OTHER obligation met but no
    /// `Chart` step names `Chart`, not a generic count mismatch.
    #[test]
    fn a_tree_missing_the_chart_step_names_it_rather_than_a_count() {
        let found = check(concat!(
            "jobs:\n  ci:\n    steps:\n",
            "      - name: Secrets\n        if: ${{ always() && github.event_name != 'push' }}\n        run: true\n",
            "      - name: Licensing\n        if: ${{ !cancelled() && github.event_name != 'push' }}\n        run: true\n",
        ));
        assert!(
            found.iter().any(|problem| problem.contains("declares no step named `Chart`")),
            "{found:?}"
        );
    }

    /// THE RED-BEFORE-GREEN CELL FOR #1249, against the LIVE `pr-title.yml` rather than a fixture:
    /// the step must read the CURRENT title from the repository at run time, not a payload
    /// expression - `github.event.pull_request.title` is frozen when a run STARTS and never
    /// refreshes, so a rename alone could never fire a fresh verdict. RED ON BASE because base has
    /// no `pr-title.yml`, GREEN with the move.
    #[test]
    fn refuses_github_event_pull_request_title() {
        let root = crate::repo::root().expect("repository root");
        let path = root.join(".github/workflows/pr-title.yml");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let step = super::super::cache_scope::steps(&text)
            .into_iter()
            .find(|step| step.input("name:") == Some("PR title"))
            .expect("the live pr-title.yml has a `PR title` step");
        assert!(
            step.contains_verbatim("gh api \"repos/$GITHUB_REPOSITORY/pulls/$number\""),
            "pr-title.yml:{} `PR title` must resolve the pull-request number and fetch the CURRENT title from the repository over the API at run time (#1249)",
            step.line
        );
        assert!(
            !step.contains_verbatim("github.event") && !step.contains_verbatim("GITHUB_EVENT_PATH"),
            "pr-title.yml:{} `PR title` must read no event-payload expression - the payload is frozen at run start, so it would judge the OLD string after a rename (#1249)",
            step.line
        );
    }

    /// A step naming `nix run .#xtask` as its own `run:` body, block-scalar or single-line.
    fn xtask_step(name: &str, block_scalar: bool) -> String {
        if block_scalar {
            format!("      - name: {name}\n        run: |\n          nix run .#xtask -- classify\n")
        } else {
            format!("      - name: {name}\n        run: nix run .#xtask -- check-attribution\n")
        }
    }

    /// The order half, real finding: a required step placed BEHIND the xtask closure it never
    /// touches waits for it anyway, because a failed step ends the job either way.
    #[test]
    fn a_required_step_behind_an_xtask_invocation_is_refused() {
        let gates: Vec<Option<&str>> = REQUIRED.iter().map(|want| Some(want.condition)).collect();
        let mut text = xtask_step("Classify the change", true);
        text.push_str(&tree(&gates));
        let found = check(&text);
        assert_eq!(found.len(), REQUIRED.len(), "{found:?}");
        assert!(found.iter().all(|problem| problem.contains("sits behind")), "{found:?}");
    }

    /// Inverted: every required step ahead of the xtask closure is not this rule's problem.
    #[test]
    fn a_required_step_ahead_of_every_xtask_invocation_passes() {
        let gates: Vec<Option<&str>> = REQUIRED.iter().map(|want| Some(want.condition)).collect();
        let mut text = tree(&gates);
        text.push_str(&xtask_step("The attribution document generates completely", false));
        assert_eq!(check(&text), Vec::<String>::new());
    }

    // ---- pr-title.yml trigger (the event obligation #1257) ----

    /// A `pr-title.yml` with the standard pull-request trigger carrying `types`.
    fn pr_title_yml(types: &[&str]) -> String {
        let mut text = String::from("name: pr-title\non:\n  pull_request:\n    types: [");
        text.push_str(&types.join(", "));
        text.push_str(
            "]\n  merge_group:\npermissions:\n  contents: read\n  pull-requests: read\njobs:\n  title:\n    name: pr-title\n",
        );
        text
    }

    /// The real shape: all four activity types fire the title check.
    #[test]
    fn the_pr_title_trigger_carries_every_required_activity_type() {
        assert_eq!(pr_title_check(&pr_title_yml(PR_TITLE_EVENT_TYPES)), Vec::<String>::new());
    }

    /// `edited` is the point of #1249: dropping it leaves a rename alone never re-verified.
    #[test]
    fn omitting_edited_from_the_pr_title_trigger_is_refused() {
        let found = pr_title_check(&pr_title_yml(&["opened", "reopened", "synchronize"]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("`edited`"), "{found:?}");
    }

    /// Fails closed on no trigger at all - same convention as the step half. With no
    /// `pull_request` trigger and no `merge_group`, both refusals fire.
    #[test]
    fn a_pr_title_workflow_with_no_pull_request_trigger_is_refused() {
        let found = pr_title_check("name: pr-title\non:\n  push:\n    branches: [main]\n");
        assert!(
            found.iter().any(|p| p.contains("declares no `pull_request` trigger")),
            "{found:?}"
        );
    }

    /// `pr-title` is a required context, so a queue entry must get an answer: a workflow without
    /// `merge_group` would leave the entry pending until the queue's timeout drops it.
    #[test]
    fn a_pr_title_workflow_the_queue_cannot_reach_is_refused() {
        let text = pr_title_yml(PR_TITLE_EVENT_TYPES).replacen("  merge_group:\n", "", 1);
        let found = pr_title_check(&text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("`merge_group`"), "{found:?}");
    }

    /// Only `pull_request`'s own `types:` counts: a list under the next trigger, or under a job,
    /// leaves a bare `pull_request:` - which takes the default types and omits `edited` - refused.
    #[test]
    fn a_types_list_outside_the_pull_request_trigger_is_not_read() {
        let text = "name: pr-title\non:\n  pull_request:\n  merge_group:\n  workflow_dispatch:\n    types: [opened, edited, reopened, synchronize]\njobs:\n  title:\n    types: [opened, edited, reopened, synchronize]\n";
        let found = pr_title_check(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("declares no `pull_request` trigger"), "{found:?}");
    }

    // ---- release-skip (job-level `if:`) ----

    /// A `ci.yml` with the release-skip jobs carrying their real `if:` shape.
    fn release_skip_tree(qa_if: Option<&str>, ci_if: Option<&str>) -> String {
        let mut text = String::from("jobs:\n");
        text.push_str("  qa:\n");
        if let Some(gate) = qa_if {
            writeln!(text, "    if: ${{{{ {gate} }}}}").expect("writing into a String cannot fail");
        }
        text.push_str("    runs-on: ubuntu-latest\n    steps:\n      - run: true\n");
        text.push_str("  ci:\n");
        if let Some(gate) = ci_if {
            writeln!(text, "    if: ${{{{ {gate} }}}}").expect("writing into a String cannot fail");
        }
        text.push_str("    runs-on: ubuntu-latest\n    steps:\n      - run: true\n");
        for job in RELEASE_SKIP.iter().filter(|&&job| !["ci", "qa"].contains(&job)) {
            writeln!(text, "  {job}:").expect("writing into a String cannot fail");
            if ci_if.is_some() {
                writeln!(text, "    if: {RELEASE_SKIP_CLAUSE}").expect("writing into a String cannot fail");
            }
        }
        text
    }

    /// The real tree's shape: `qa` carries the clause as a conjunct beside the
    /// category test; `ci` carries it alone. Both pass.
    #[test]
    fn the_real_release_skip_shape_passes() {
        let text = release_skip_tree(
            Some(
                "!startsWith(github.event.head_commit.message || '', 'chore(release):') && needs.identity-classify.result == 'success'",
            ),
            Some("!startsWith(github.event.head_commit.message || '', 'chore(release):')"),
        );
        assert_eq!(release_skip_check(&text), Vec::<String>::new());
    }

    /// Dropping the QA caller's release guard would rerun the linked-driver suite on the bump.
    #[test]
    fn dropping_the_release_clause_from_bigquery_driver_check_is_refused() {
        let text = release_skip_tree(Some("needs.identity-classify.result == 'success'"), Some(RELEASE_SKIP_CLAUSE));
        let found = release_skip_check(&text);
        assert_eq!(found.len(), 1, "only qa is broken: {found:?}");
        assert!(found[0].contains("qa"), "the refusal names the job: {found:?}");
        assert!(
            found[0].contains("does not carry the release-skip clause"),
            "the refusal names the missing clause: {found:?}"
        );
    }

    /// A job with no `if:` at all is refused - fail closed, same convention as the step half.
    #[test]
    fn a_job_with_no_if_is_refused() {
        let text = release_skip_tree(None, None);
        let found = release_skip_check(&text);
        assert_eq!(found.len(), RELEASE_SKIP.len(), "every unconditioned job is named: {found:?}");
        assert!(
            found.iter().all(|p| p.contains("declares no `if:`")),
            "each refusal names the missing `if:`: {found:?}"
        );
    }

    /// A `||` in the remainder can widen past the clause, so it is refused - same shape as
    /// `narrower_than_main_push`.
    #[test]
    fn a_disjunction_in_the_remainder_is_refused() {
        let text = release_skip_tree(
            Some("!startsWith(github.event.head_commit.message || '', 'chore(release):') || true"),
            Some("!startsWith(github.event.head_commit.message || '', 'chore(release):')"),
        );
        let found = release_skip_check(&text);
        assert_eq!(found.len(), 1, "only the disjunctive job is broken: {found:?}");
        assert!(found[0].contains("qa"), "the refusal names the job: {found:?}");
    }

    /// A clause that merely CONTAINS the pin - negated - must not pass. `!(CLAUSE)` contains the
    /// text but inverts it; `contains` would pass it and this rule must not.
    #[test]
    fn a_negated_clause_does_not_pass() {
        let text = release_skip_tree(
            Some("!(!startsWith(github.event.head_commit.message || '', 'chore(release):'))"),
            Some("!startsWith(github.event.head_commit.message || '', 'chore(release):')"),
        );
        let found = release_skip_check(&text);
        assert_eq!(found.len(), 1, "the negated clause is refused: {found:?}");
    }

    /// A paraphrased clause fails closed: the exact text does not match, so the refusal fires.
    #[test]
    fn a_paraphrased_clause_fails_closed() {
        // Same semantics, different text: `head_commit.message` without the `|| ''` default.
        let text = release_skip_tree(
            Some(
                "!startsWith(github.event.head_commit.message, 'chore(release):') && needs.identity-classify.result == 'success'",
            ),
            Some("!startsWith(github.event.head_commit.message || '', 'chore(release):')"),
        );
        let found = release_skip_check(&text);
        assert_eq!(found.len(), 1, "the paraphrased clause is refused, not passed: {found:?}");
    }

    /// `ci`'s `if:` is the clause alone (textually identical to [`RELEASE_SKIP_CLAUSE`]), so it
    /// passes as a trivial conjunct.
    #[test]
    fn ci_clause_alone_passes() {
        let text = release_skip_tree(
            Some(
                "!startsWith(github.event.head_commit.message || '', 'chore(release):') && needs.identity-classify.result == 'success'",
            ),
            Some(RELEASE_SKIP_CLAUSE),
        );
        assert_eq!(release_skip_check(&text), Vec::<String>::new());
    }

    // ---- pinned job conditions (job-level `if:`) ----

    fn bigquery_conformance_tree(gate: Option<&str>) -> String {
        let mut text = format!(
            "jobs:\n  e2e-datahub-adbc:\n    if: {}\n  bigquery-conformance:\n",
            PINNED_JOB_IF[1].1
        );
        if let Some(gate) = gate {
            writeln!(text, "    if: {gate}").expect("writing into a String cannot fail");
        }
        text.push_str("    runs-on: ubuntu-latest\n    steps:\n      - run: true\n");
        text
    }

    /// The push arm and `needs.ci` the job carried before #1696. Red against base, which has no
    /// such rule and exactly this `if:`; green with the change.
    #[test]
    fn a_push_arm_on_bigquery_conformance_is_refused() {
        let base = "(github.event_name == 'push' || github.event_name == 'merge_group' || (github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository && github.event.pull_request.user.login != 'dependabot[bot]')) && needs.ci.outputs.data_source_bigquery == 'true'";
        let found = pinned_job_if_check(&bigquery_conformance_tree(Some(base)));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("bigquery-conformance"), "{found:?}");
        let found = pinned_job_if_check(&bigquery_conformance_tree(None));
        assert!(found.len() == 1 && found[0].contains("declares no `if:`"), "{found:?}");
        let (_, pinned) = PINNED_JOB_IF[0];
        assert_eq!(
            pinned_job_if_check(&bigquery_conformance_tree(Some(pinned))),
            Vec::<String>::new()
        );
    }

    // An ordinary or release caller cannot bypass the receipt and full mode pins.
    #[test]
    fn every_weakening_of_the_queue_validated_skip_is_refused() {
        let root = crate::repo::root().expect("repository root");
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read CI");
        let release = std::fs::read_to_string(root.join(".github/workflows/release.yml")).expect("read release");
        let problems = super::super::aggregate_inputs::problems(&root);
        assert!(problems.is_empty(), "the committed workflows: {problems:?}");
        for (file, before, after) in [
            (
                "ci",
                "id: pr-verdict\n        if: github.event_name == 'merge_group'",
                "id: pr-verdict",
            ),
            (
                "ci",
                "      tested-commit: ${{ needs.identity-classify.outputs.tested_commit }}",
                "      tested-commit: ${{ needs.identity-classify.outputs.green_jobs }}",
            ),
            (
                "ci",
                "      green-jobs: ${{ needs.identity-classify.outputs.green_jobs }}",
                "      green-jobs: ${{ needs.identity-classify.outputs.tested_commit }}",
            ),
            (
                "ci",
                "name: QA checkout (${{ github.sha }})",
                "name: QA checkout (${{ github.event.pull_request.head.sha }})",
            ),
            (
                "ci",
                "github.event_name == 'pull_request' && needs.causality.outputs.reusable == 'true'",
                "github.event_name == 'pull_request'",
            ),
            (
                "release",
                "      plan-only: true\n      fast-checked: true\n      run-all-checks: true",
                "      plan-only: true\n      fast-checked: true\n      run-all-checks: false",
            ),
            (
                "release",
                "      run-all-checks: true\n      suite-plan: ${{ needs.qa-plan.outputs.suite-plan }}",
                "      run-all-checks: false\n      suite-plan: ${{ needs.qa-plan.outputs.suite-plan }}",
            ),
            (
                "release",
                "needs: [build, gates, qa-plan, qa, bigquery-conformance, e2e-datahub-adbc]",
                "needs: [build, gates, qa-plan, qa]",
            ),
        ] {
            let source = if file == "ci" { &ci } else { &release };
            assert_eq!(
                source.matches(before).count(),
                1,
                "{file}: ambiguous or missing mutation {before}"
            );
            let (ci, release) = if file == "ci" {
                (ci.replacen(before, after, 1), release.clone())
            } else {
                (ci.clone(), release.replacen(before, after, 1))
            };
            let tree = crate::scratch_tree::Tree::of(
                "queue-validity",
                &[
                    (".github/workflows/ci.yml", ci.as_bytes()),
                    (".github/workflows/release.yml", release.as_bytes()),
                ],
            );
            let found = super::super::aggregate_inputs::problems(tree.root());
            assert!(
                found.iter().any(|p| p.contains("queue-validated rule")),
                "{file} {before}: {found:?}"
            );
        }
    }
}
