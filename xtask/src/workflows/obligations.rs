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
//! semantics, observed over the runs above and not reproduced by this repository. Nor does it see
//! the `ci` job's own `if:`, which skips the whole job - `Secrets` included - on a
//! `chore(release):` commit; that exemption is accepted deliberately at the job level and this
//! rule has no view of it either way.
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

/// A step that must stay reachable, and the exact condition that keeps it so.
struct Obligation {
    /// The step's `name:` value, which is also how a reader finds it.
    step: &'static str,
    /// The `if:` expression, with `${{ }}` and surplus whitespace already removed.
    condition: &'static str,
}

/// The steps whose obligation survives a red above them, and the form each needs.
///
/// A table rather than two hand-written arms so that adding a third is one line, and so the
/// verdict can print how many it held - a rule over an empty table would pass by finding nothing.
const REQUIRED: &[Obligation] = &[
    Obligation {
        step: "Secrets",
        condition: "always()",
    },
    Obligation {
        step: "Licensing",
        condition: "!cancelled()",
    },
    Obligation {
        step: "Chart",
        condition: "!cancelled()",
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

#[cfg(test)]
mod tests {
    use super::{REQUIRED, check};

    /// A `ci.yml` shaped like the real one, with each named step given `gate` as its condition.
    ///
    /// `None` writes the step with no `if:` at all, which is the state the finding was about.
    fn tree(gates: &[Option<&str>]) -> String {
        let mut text = String::from("jobs:\n  ci:\n    steps:\n");
        for (want, gate) in REQUIRED.iter().zip(gates) {
            text.push_str("      - name: ");
            text.push_str(want.step);
            text.push('\n');
            if let Some(gate) = gate {
                text.push_str("        if: ${{ ");
                text.push_str(gate);
                text.push_str(" }}\n");
            }
            text.push_str("        run: true\n");
        }
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
}
