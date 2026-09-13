//! The `ci.yml` steps whose obligation must not be skippable, and the line that holds each.
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
//!
//! **The limit, next to the claim.** This rule holds the condition on the steps named in
//! [`REQUIRED`] and nothing wider. It cannot see position, so it does not know whether a step
//! moved, and it does not require any OTHER step to stay reachable - adding one is an edit here.
//! Nor does it verify that `always()` behaves as measured; that is GitHub's semantics, observed
//! over the runs above and not reproduced by this repository.

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
];

/// How many obligations this rule holds, for the success line.
pub(super) const fn held() -> usize {
    REQUIRED.len()
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
}
