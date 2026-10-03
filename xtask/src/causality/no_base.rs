//! The exemption that lets an added test with no base result pass, and the refusal that holds it.
//!
//! An added test that produced no base result - "not run at base" in [`super::base::report`] - used
//! to be a line PRINTED beside whichever verdict the rest of the run earned, so a feature-gated
//! test that could never run at the reconstructed base sat beside a green pass and read as evidence
//! it was not. It is a REFUSAL now: [`refusals`] returns a sentence for every added test the base
//! run did not account for, unless that test is exempted by name on the list at
//! `devco/causality-no-base-exemptions`.
//!
//! The list is one test-fn name per line with a `# reason` required on each, so an entry is a CLAIM
//! a reviewer can read the point of, not a checkbox. Three shapes fail CLOSED rather than silently:
//! an entry with no reason is malformed, an entry naming no added test is stale, and an added test
//! with no base result and no entry is unmeasured. The last two refuse even when the rest of the
//! run is green, because a file full of dead exempt-thoughts rots exactly the way
//! `devco/max-lines-ignore` describes for its parasite allowance.

use crate::causality::place::AddedTest;

/// The refusal sentences an exemption file earns against a run, empty when it is consistent.
///
/// `no_base` is the scope's tests the base run produced no result for, or `None` when they cannot
/// be identified safely (no per-test evidence, or a summary per-test output cannot reconcile) - a
/// refusal can only ever cite names it can see, so the `None` case contributes nothing to the
/// unmeasured arm while the malformed and stale arms still refuse.
pub(crate) fn refusals(encoded: &str, scoped: &[AddedTest], no_base: Option<&[&AddedTest]>) -> Vec<String> {
    let mut out = Vec::new();
    for line in encoded.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, reason)) = line.split_once('#') else {
            out.push(format!("  exemption `{line}` has no reason - write `{line} # <reason>`"));
            continue;
        };
        let (name, reason) = (name.trim(), reason.trim());
        if name.is_empty() || reason.is_empty() {
            out.push(format!("  exemption `{line}` has no reason - write `{line} # <reason>`"));
            continue;
        }
        if !scoped.iter().any(|test| test.name() == name) {
            out.push(format!(
                "  exemption `{name}` names no test this diff added - remove the stale line"
            ));
        }
    }
    if let Some(unreported) = no_base {
        for test in unreported {
            if !exempts(encoded, test.name()) {
                out.push(format!(
                    "  {} in {} produced no base result; either make it run at base or exempt it in \
                     devco/causality-no-base-exemptions as `{} # <why>`",
                    test.name(),
                    test.file(),
                    test.name()
                ));
            }
        }
    }
    out
}

/// Is `name` on the list, with a reason? A reason-less or comment-only line never exempts.
fn exempts(encoded: &str, name: &str) -> bool {
    encoded.lines().any(|line| match line.trim().split_once('#') {
        Some((bare, reason)) => !reason.trim().is_empty() && bare.trim() == name,
        None => false,
    })
}

#[cfg(test)]
mod tests {
    use super::refusals;
    use crate::Verdict;
    use crate::causality::base::{BaseOutcome, report_base_scoped};
    use crate::causality::fixtures::{named, scoped};
    use crate::causality::provenance::Moved;
    use crate::causality::reverted::Reverted;

    /// A scope whose one added test `ran` on base, protected by a `RedByAssertion` outcome that
    /// normally PASSES - so a refusal from an exemption file stands out from the run's own verdict.
    fn passed_otherwise(encoded: &str) -> Verdict {
        let scope = scoped("pa", "pa/src/lib.rs", &["ran"]);
        let outcome = BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::ran")],
        };
        let output = "     Summary [   0.1s] 1 test run: 1 passed, 0 skipped\n";
        report_base_scoped(
            &outcome,
            output,
            false,
            &named(1),
            &Moved::Nothing,
            &Reverted::Behaviour,
            &scope,
            encoded,
        )
    }

    #[test]
    fn an_added_test_with_no_base_result_and_no_exemption_is_refused_by_name() {
        // The defect this module exists for: `skipped_a` was added, the base run never produced a
        // result for it, and nothing exempts it - so even though the run is otherwise
        // red-on-base/green-on-head, the gate FAILS and names exactly which test to act on.
        let scope = scoped("pa", "pa/src/lib.rs", &["ran", "skipped_a"]);
        let outcome = BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::ran")],
        };
        let output = concat!(
            "        PASS [   0.021s] (1/1) pa tests::ran\n",
            "     Summary [   0.4s] 1 test run: 1 passed, 1 skipped\n",
        );
        let verdict = report_base_scoped(
            &outcome,
            output,
            false,
            &named(2),
            &Moved::Nothing,
            &Reverted::Behaviour,
            &scope,
            "",
        );
        assert_eq!(verdict, Verdict::Fail);
        let refused = refusals("", &scope, Some(&[&scope[1]]));
        assert!(
            refused
                .iter()
                .any(|line| line.contains("skipped_a in pa/src/lib.rs") && line.contains("no base result")),
            "names the test: {refused:?}"
        );
        assert!(
            refused.iter().any(|line| line.contains("causality-no-base-exemptions")),
            "says how to exempt: {refused:?}"
        );
    }

    #[test]
    fn an_exempted_test_with_a_reason_passes() {
        // Same run as above, but the no-base test is on the list with a reason - the point of the
        // exemption. The gate returns the pass it would have before this module existed.
        let scope = scoped("pa", "pa/src/lib.rs", &["ran", "skipped_a"]);
        let outcome = BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::ran")],
        };
        let output = concat!(
            "        PASS [   0.021s] (1/1) pa tests::ran\n",
            "     Summary [   0.4s] 1 test run: 1 passed, 1 skipped\n",
        );
        let encoded = "# feature-gated behind rdbms in the reconstructed base\nskipped_a # only built with the rdbms feature\n";
        let verdict = report_base_scoped(
            &outcome,
            output,
            false,
            &named(2),
            &Moved::Nothing,
            &Reverted::Behaviour,
            &scope,
            encoded,
        );
        assert_eq!(verdict, Verdict::Pass);
        assert!(
            refusals(encoded, &scope, Some(&[&scope[1]])).is_empty(),
            "consistent list refuses nothing"
        );
    }

    #[test]
    fn an_exemption_line_without_a_reason_is_refused() {
        // A name with no `# reason` is a claim with nothing to review, so it refuses even though
        // nothing here is unmeasured and the run itself would pass. It is refused by name.
        let refused = refusals("ran\n", &scoped("pa", "pa/src/lib.rs", &["ran"]), None);
        assert!(
            refused
                .iter()
                .any(|line| line.contains("`ran`") && line.contains("no reason")),
            "{refused:?}"
        );
        assert_eq!(passed_otherwise("ran\n"), Verdict::Fail);
    }

    #[test]
    fn a_stale_exemption_is_refused() {
        // An entry naming a test the diff does not add exempts nothing and is itself refused, so
        // the file cannot fill with dead lines that look like coverage.
        let scope = scoped("pa", "pa/src/lib.rs", &["ran"]);
        let refused = refusals("gone_forever # the test was deleted\n", &scope, None);
        assert!(
            refused
                .iter()
                .any(|line| line.contains("`gone_forever`") && line.contains("stale")),
            "{refused:?}"
        );
        assert_eq!(passed_otherwise("gone_forever # the test was deleted\n"), Verdict::Fail);
    }
}
