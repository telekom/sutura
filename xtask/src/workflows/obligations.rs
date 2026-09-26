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
//! * `Secrets` takes `always()`. A committed credential is **already disclosed** by the time any
//!   gate runs, so there is nothing to defer to the next push, and `always()` is the only form
//!   with a step-level witness of surviving a cancellation. The usual argument against it does not
//!   reach here: `!cancelled()` is preferred for `pr-cache` because that job takes a **paid
//!   runner** and `always()` would start it after somebody pressed cancel. This is a two-second
//!   step inside a runner already held, so defeating a cancel costs two seconds.
//! * `Licensing` takes `!cancelled()`. It is a `nix build`, so `always()` would extend a cancelled
//!   job by a real amount, and its value is **merge-blocking**: a licence violation cannot merge
//!   whatever this run says, so deferring it to the next push costs a round trip and not a
//!   guarantee. Cancel should mean cancel here.
//! * `Chart` takes `!cancelled()`, for `Licensing`'s own reason: a `nix build` over
//!   `charts/sutura` (#149 branch 1), merge-blocking, and cheap enough that `always()` buys
//!   nothing a cancelled run would rather skip.
//!
//! **The limit, next to the claim.** This rule holds the condition on the steps named in
//! [`REQUIRED`] and nothing wider. It does not require any OTHER step to stay reachable - adding
//! one is an edit here. Nor does it verify that `always()` behaves as measured; that is GitHub's
//! semantics, observed over the runs above and not reproduced by this repository.
//!
//! **The release-skip clause - what [`RELEASE_SKIP`] does and does not pin.** The `ci` job's own
//! `if:` skips the whole job - `Secrets` included - on a `chore(release):` commit, and
//! `bigquery-driver-check` repeats that test as a conjunct in its own `if:`. [`RELEASE_SKIP`] holds
//! that the clause APPEARS as a conjunct on each named job's `if:` - it does not pin the category
//! clause that sits beside it (`needs.identity-classify.outputs.data_source_bigquery == 'true'`),
//! because that axis is held by the category registry in `affected` and pinning it here would
//! couple the rule to a job rename. The clause is matched as exact normalized text: a paraphrased
//! clause FAILS CLOSED - it does not match and the refusal fires - rather than passing on a
//! looser `contains`. See [`carries_release_skip`].
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

use std::path::Path;

/// A step whose `if:` is held by this rule, and the exact form it needs.
///
/// **Two kinds of obligation now, and the table is the same shape for both.** Three rows are here
/// because a condition keeps the step REACHABLE after a red above it - the finding this module's
/// header is about. `PR title` is here because its condition decides the EVENT it runs on: the
/// title expression expands to the empty string on anything but a `pull_request`, so a dropped
/// `if:` would put the gate on a run with nothing to judge. Its own gate fails closed on that
/// rather than passing, which is why the two mechanisms are worth having together - this one names
/// the step in `check-workflows`, that one reddens the run.
struct Obligation {
    /// The step's `name:` value, which is also how a reader finds it.
    step: &'static str,
    /// The `if:` expression, with `${{ }}` and surplus whitespace already removed.
    condition: &'static str,
    /// Why the `if:` is held - see [`Kind`]. The None-arm's refusal text is written per kind, so
    /// an event-kind step whose `if:` goes missing is not told to make itself reachable.
    kind: Kind,
}

/// What the held condition is FOR. Two kinds, and they are not interchangeable:
///
/// - [`Kind::Reachability`] - the condition keeps the step REPORTING after a red above it. The
///   refusal for a missing `if:` names what reachability costs, and `always()` is the right fix.
/// - [`Kind::Event`] - the condition decides the EVENT the step runs on. `PR title`'s condition
///   is the example: the title expression expands to the empty string on anything but a
///   `pull_request`, and its own gate refuses an empty title rather than passing, so a dropped
///   `if:` does not expose an unchecked pass - it starts the run that carries the subject. The
///   refusal therefore says where the condition belongs, and NEVER reaches for `always()`, which
///   would put the gate on every event where there is nothing to judge.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Reachability,
    Event,
}

const REQUIRED: &[Obligation] = &[
    Obligation {
        step: "Secrets",
        condition: "always()",
        kind: Kind::Reachability,
    },
    Obligation {
        step: "Licensing",
        condition: "!cancelled()",
        kind: Kind::Reachability,
    },
    Obligation {
        step: "Chart",
        condition: "!cancelled()",
        kind: Kind::Reachability,
    },
    Obligation {
        // #935. The landed subject is composed from the pull-request title, so the gate holding the
        // commit vocabulary over it can only run where that title exists. No `always()` and no
        // `!cancelled()`: on every other event the expression supplying the title is empty, and the
        // step has nothing to judge.
        step: "PR title",
        condition: "github.event_name == 'pull_request'",
        kind: Kind::Event,
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
/// the reassuring pass is the failure mode a scan like this is most prone to.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let path = root.join(".github/workflows/ci.yml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![format!("{} could not be read, so no step obligation is held", path.display())];
    };
    check(&text)
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
            None => out.push(match want.kind {
                Kind::Reachability => format!(
                    "ci.yml:{} `{}` carries no `if:`. A failed step ends the job, so it runs only while every step above it is green - `if: ${{{{ {} }}}}` is what makes it reachable regardless",
                    step.line, want.step, want.condition
                ),
                Kind::Event => format!(
                    "ci.yml:{} `{}` carries no `if:`. Its condition decides the EVENT it runs on, not its reachability: `{}` is empty on every other event, and the gate it runs refuses an empty subject rather than passing, so the step would start and fail closed where there is nothing to judge - `if: ${{{{ {} }}}}` puts it on the event that carries the subject",
                    step.line, want.step, want.condition, want.condition
                ),
            }),
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
/// The release-commit skip clause that `ci` and `bigquery-driver-check` both carry, normalized the
/// way `Step::gate` normalizes (`${{ }}` stripped, whitespace collapsed to single spaces).
///
/// Pinned as exact text, not matched loosely: a paraphrased clause fails closed (the conjunct check
/// below does not match and the refusal fires) rather than passing on a `contains`. The category
/// clause beside it on `bigquery-driver-check` is deliberately NOT pinned here - see the header.
const RELEASE_SKIP_CLAUSE: &str = "!startsWith(github.event.head_commit.message || '', 'chore(release):')";

/// Jobs whose own `if:` must carry the release-skip clause as a conjunct, so a release commit does
/// not re-run the most expensive legs in the repository.
///
/// `ci` skips the whole job and everything under `needs: [ci]` skips with it. `bigquery-driver-check`
/// no longer needs `ci` and repeats the test in its own `if:` - so the clause must be pinned on
/// BOTH. `ci`'s `if:` is the clause alone (textually identical to [`RELEASE_SKIP_CLAUSE`]);
/// `bigquery-driver-check`'s is the clause as a conjunct beside the category test.
const RELEASE_SKIP: &[&str] = &["bigquery-driver-check", "ci"];

/// How many release-skip rows this rule holds, for the success line.
pub(super) const fn release_skip_held() -> usize {
    RELEASE_SKIP.len()
}

/// Every release-skip obligation this rule finds broken, empty when all hold.
///
/// Fails CLOSED on an unreadable file and on a job with no `if:` - the same convention as
/// [`problems`]: a reassuring pass over an absent clause is the failure mode this scan is most
/// prone to, and a job that dropped its `if:` is indistinguishable from one that was renamed.
pub(super) fn release_skip_problems(root: &Path) -> Vec<String> {
    let path = root.join(".github/workflows/ci.yml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![format!(
            "{} could not be read, so no release-skip obligation is held",
            path.display()
        )];
    };
    release_skip_check(&text)
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

    use super::{RELEASE_SKIP, RELEASE_SKIP_CLAUSE, REQUIRED, check, release_skip_check};

    /// A `ci.yml` shaped like the real one, with each named step given `gate` as its condition.
    ///
    /// `None` writes the step with no `if:` at all, which is the state the finding was about.
    /// `PR title` gets the body it really carries, because it is the one obligation that PAYS for
    /// the xtask closure - so these trees exercise the order half as well as the condition half.
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
        text.push_str(if name == "PR title" {
            "        run: nix run .#xtask -- check-pr-title \"$TITLE\"\n"
        } else {
            "        run: true\n"
        });
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

    /// The two kinds refuse DIFFERENTLY. An event-kind step whose `if:` goes missing must not be
    /// told to buy reachability - `always()` would put its gate on every event, where the subject
    /// it judges does not exist - so the reachability sentence must never appear in its refusal.
    #[test]
    fn the_event_kind_refusal_does_not_suggest_reachability() {
        let gates: Vec<Option<&str>> = REQUIRED.iter().map(|_| None).collect();
        let found = check(&tree(&gates));
        let title = found
            .iter()
            .find(|problem| problem.contains("`PR title`"))
            .expect("the unconditioned PR title step is reported");
        assert!(
            title.contains("decides the EVENT it runs on"),
            "the event-kind refusal must name what its condition is for: {title}"
        );
        assert!(
            !title.contains("reachable regardless"),
            "reachability is the other kind's reason, not this one's: {title}"
        );
        assert!(
            found
                .iter()
                .filter(|problem| !problem.contains("`PR title`"))
                .all(|problem| problem.contains("reachable regardless")),
            "the reachability kind keeps its own sentence: {found:?}"
        );
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
    /// #935: the step that reads the pull-request title, and the condition deciding the EVENT it
    /// runs on. Named rather than left to the loops above, for the reason the chart's test gives -
    /// an entry never added would leave them passing over one fewer obligation.
    #[test]
    fn the_pr_title_step_is_a_required_obligation() {
        assert!(
            REQUIRED
                .iter()
                .any(|o| o.step == "PR title" && o.condition == "github.event_name == 'pull_request'"),
            "the pull-request title's step obligation is missing or holds the wrong condition"
        );
    }

    /// The ORDER half over the shape #935 introduced: `PR title` realises the xtask closure, so
    /// every closure-free obligation has to stay ahead of it - and the rule may not report that
    /// step against itself, which is what placing it first measures.
    #[test]
    fn a_closure_free_obligation_behind_the_title_step_is_refused() {
        let mut text = String::from("jobs:\n  ci:\n    steps:\n");
        text.push_str(&step("PR title", Some("github.event_name == 'pull_request'")));
        for want in REQUIRED.iter().filter(|want| want.step != "PR title") {
            text.push_str(&step(want.step, Some(want.condition)));
        }

        let found = check(&text);

        assert_eq!(
            found.len(),
            REQUIRED.len().saturating_sub(1),
            "every closure-free obligation behind it, and only those: {found:?}"
        );
        assert!(
            found.iter().all(|problem| problem.contains("sits behind ci.yml:")),
            "{found:?}"
        );
        assert!(
            !found.iter().any(|problem| problem.contains("`PR title` sits behind")),
            "the first xtask step is not behind itself: {found:?}"
        );
    }

    #[test]
    fn the_chart_step_is_a_required_obligation() {
        assert!(
            REQUIRED.iter().any(|o| o.step == "Chart" && o.condition == "!cancelled()"),
            "the chart's step obligation is missing or holds the wrong condition"
        );
    }

    /// The specific finding, not just the count: a tree with every OTHER obligation met but no
    /// `Chart` step names `Chart`, not a generic count mismatch.
    #[test]
    fn a_tree_missing_the_chart_step_names_it_rather_than_a_count() {
        let found = check(concat!(
            "jobs:\n  ci:\n    steps:\n",
            "      - name: Secrets\n        if: ${{ always() }}\n        run: true\n",
            "      - name: Licensing\n        if: ${{ !cancelled() }}\n        run: true\n",
        ));
        assert!(
            found.iter().any(|problem| problem.contains("declares no step named `Chart`")),
            "{found:?}"
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
    // ---- release-skip (job-level `if:`) ----

    /// A `ci.yml` with both release-skip jobs carrying their real `if:` shape.
    fn release_skip_tree(bigquery_if: Option<&str>, ci_if: Option<&str>) -> String {
        let mut text = String::from("jobs:\n");
        text.push_str("  bigquery-driver-check:\n");
        if let Some(gate) = bigquery_if {
            writeln!(text, "    if: ${{{{ {gate} }}}}").expect("writing into a String cannot fail");
        }
        text.push_str("    runs-on: ubuntu-latest\n    steps:\n      - run: true\n");
        text.push_str("  ci:\n");
        if let Some(gate) = ci_if {
            writeln!(text, "    if: ${{{{ {gate} }}}}").expect("writing into a String cannot fail");
        }
        text.push_str("    runs-on: ubuntu-latest\n    steps:\n      - run: true\n");
        text
    }

    /// The real tree's shape: `bigquery-driver-check` carries the clause as a conjunct beside the
    /// category test; `ci` carries it alone. Both pass.
    #[test]
    fn the_real_release_skip_shape_passes() {
        let text = release_skip_tree(
            Some(
                "!startsWith(github.event.head_commit.message || '', 'chore(release):') && needs.identity-classify.outputs.data_source_bigquery == 'true'",
            ),
            Some("!startsWith(github.event.head_commit.message || '', 'chore(release):')"),
        );
        assert_eq!(release_skip_check(&text), Vec::<String>::new());
    }

    /// Mutation (a): drop the release clause from `bigquery-driver-check`, leaving only the category
    /// clause. Red against base (no refusal exists), green with the change - the rule names the line.
    #[test]
    fn dropping_the_release_clause_from_bigquery_driver_check_is_refused() {
        let text = release_skip_tree(
            Some("needs.identity-classify.outputs.data_source_bigquery == 'true'"),
            Some("!startsWith(github.event.head_commit.message || '', 'chore(release):')"),
        );
        let found = release_skip_check(&text);
        assert_eq!(found.len(), 1, "only bigquery-driver-check is broken: {found:?}");
        assert!(
            found[0].contains("bigquery-driver-check"),
            "the refusal names the job: {found:?}"
        );
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
        assert!(
            found[0].contains("bigquery-driver-check"),
            "the refusal names the job: {found:?}"
        );
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
                "!startsWith(github.event.head_commit.message, 'chore(release):') && needs.identity-classify.outputs.data_source_bigquery == 'true'",
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
                "!startsWith(github.event.head_commit.message || '', 'chore(release):') && needs.identity-classify.outputs.data_source_bigquery == 'true'",
            ),
            Some(RELEASE_SKIP_CLAUSE),
        );
        assert_eq!(release_skip_check(&text), Vec::<String>::new());
    }
}
