//! Every rule's verdict over one already-read tree, printed together rather than one per run.
//!
//! **ONE PASS OVER ALL OF THEM RATHER THAN THE FIRST.** [`super::run`] returned `Verdict::Fail`
//! out of the loop rule before it reached the drift report, so on a tree with an empty shipped set
//! in one file and a drifted literal in another the second was invisible: exit code right, half of
//! what the gate already knew unprinted, and a round trip to learn the rest. Measured in review
//! with `release.yml:82` nulled - the shape `github.com/telekom/sutura#329` was filed over - which
//! printed only `1 place(s) where an EMPTY shipped set goes unrefused` while
//! `declaration::read` had computed the mismatch two statements earlier. `max-lines` took exactly
//! this fix, and `.agents/skills/sutura/gates/SKILL.md` records it a few bullets above where this
//! gate writes.
//!
//! Its own file because the parent is against the unexemptable 1000-line cap, and it carries its
//! own test for the reason the test-only `crate::falsifier` states: a file that adds no `#[test]` is
//! revertible, so the causality base tree would delete it while a parent held for its tests still
//! declared the module.

use super::Reconciliation;
use super::declaration::Mismatch;
use crate::Verdict;

/// One rule that refused, carrying what its report needs.
///
/// **A variant per rule, matched EXHAUSTIVELY in [`Refused::print`]**: a rule added to this
/// collection cannot arrive already silent, because the match fails to compile (`error[E0004]`)
/// rather than falling into a `_` arm that inherits *print nothing*. That is the same reason
/// `declaration::Carried` is an enum rather than a predicate - `github.com/telekom/sutura#414`.
pub(super) enum Refused<'a> {
    /// The sibling loop rule refused. **No body, because `loops::verdict` prints its own report as
    /// it decides**; the variant exists so this composition can be driven and asserted on.
    EmptyLoop,
    /// Literal sets that disagree with `nix/shipped.nix`.
    Drift {
        expected: &'a [String],
        mismatches: &'a [Mismatch],
    },
    /// No single workflow holds the refusal this gate's remedies name.
    NoProbeRefusal,
    /// A page under `docs/` could not be read or lexed.
    UnreadablePage(&'a str),
    /// No page documents a feature build of a shipped package, so `probeFeatures` is reconciled
    /// against nothing.
    NothingProbed,
    /// Documented feature builds no probe covers.
    Unprobed { problems: &'a [String], host: Option<&'a str> },
}

impl Refused<'_> {
    /// Print this rule's report.
    fn print(&self) {
        match self {
            // `super::loops::verdict` printed it already - see the variant.
            Self::EmptyLoop => {}
            Self::Drift { expected, mismatches } => drift(expected, mismatches),
            Self::NoProbeRefusal => no_probe_refusal(),
            Self::UnreadablePage(why) => unreadable_page(why),
            Self::NothingProbed => nothing_probed(),
            Self::Unprobed { problems, host } => unprobed(problems, *host),
        }
    }
}

/// Which rules refuse over one already-read tree, in the order their reports read best.
///
/// **EVERY RULE IS CONSULTED**, which is the whole point: none of them returns, so a tree that
/// breaks two of them says so once. `loops` is passed as a verdict rather than called here because
/// it prints as it decides.
pub(super) fn refusals<'a>(
    loops: Verdict,
    expected: &'a [String],
    mismatches: &'a [Mismatch],
    host: Option<&'a str>,
    reconciled: Result<&'a Reconciliation, &'a str>,
) -> Vec<Refused<'a>> {
    let mut refused = Vec::new();
    if loops == Verdict::Fail {
        refused.push(Refused::EmptyLoop);
    }
    if !mismatches.is_empty() {
        refused.push(Refused::Drift { expected, mismatches });
    }
    if host.is_none() {
        refused.push(Refused::NoProbeRefusal);
    }
    match reconciled {
        Err(why) => refused.push(Refused::UnreadablePage(why)),
        Ok(reconciliation) => {
            if reconciliation.probed.is_empty() {
                refused.push(Refused::NothingProbed);
            }
            if !reconciliation.problems.is_empty() {
                refused.push(Refused::Unprobed {
                    problems: &reconciliation.problems,
                    host,
                });
            }
        }
    }
    refused
}

/// Print every rule that refused, and say whether any did.
pub(super) fn report(refused: &[Refused<'_>]) -> Verdict {
    for rule in refused {
        rule.print();
    }
    if refused.is_empty() { Verdict::Pass } else { Verdict::Fail }
}

/// Literal sets that disagree with `nix/shipped.nix`, and why the order is part of it.
fn drift(expected: &[String], mismatches: &[Mismatch]) {
    let source = super::SOURCE;
    eprintln!(
        "xtask check-shipped-binaries: FAILED - {} literal(s) disagree with {source}",
        mismatches.len()
    );
    eprintln!("  {source} ships: {}", expected.join(" "));
    for row in mismatches {
        let spells = row.spells.join(" ");
        let spells = if spells.is_empty() { "<nothing at all>" } else { &spells };
        eprintln!("  {} spells: {spells}", row.at);
    }
    eprintln!();
    eprintln!("  These are compared IN ORDER, because the order is read: `sutura` is what an");
    eprintln!("  unqualified download and an unqualified `docker pull` mean, and it is the first");
    eprintln!("  row of every table in the release notes.");
    eprintln!();
    eprintln!("  A binary added to {source} and not here is built by nothing and released as");
    eprintln!("  nothing - which is what #111 was. A name here that {source} does not ship is a");
    eprintln!("  `nix build` of an attribute that does not exist, minutes into a tagged run. And a");
    eprintln!("  declaration carrying NOTHING is a set of zero names here rather than a row that");
    eprintln!("  drops out of the comparison - `github.com/telekom/sutura#329`.");
}

/// No single workflow holds the refusal every remedy above points a reader at.
fn no_probe_refusal() {
    let source = super::SOURCE;
    eprintln!("xtask check-shipped-binaries: FAILED - no single workflow refuses an EMPTY probe manifest");
    eprintln!(
        "  Looked under `.github` for `{}` and `{}` in one file, and found",
        super::refusal::PROBE_REFUSAL[0],
        super::refusal::PROBE_REFUSAL[1]
    );
    eprintln!("  either none or several. Without that refusal a probe set emptied by a rename is a");
    eprintln!("  green link leg that measured nothing - the dead gate `{source}` records - and the");
    eprintln!("  remedy below has no file left to name.");
}

/// A page under `docs/` this gate could not read or could not lex.
fn unreadable_page(why: &str) {
    eprintln!("xtask check-shipped-binaries: FAILED - a page under docs/ could not be reconciled");
    eprintln!("  {why}");
    eprintln!("  A page this cannot read, one whose block never closes, and one instructing in an");
    eprintln!("  indented block are the same failure: the instructions on it go unreconciled while");
    eprintln!("  the other pages keep the count non-empty, so the page nobody reconciled is the one");
    eprintln!("  nobody hears about. The verdict is over the pages this names or it is nothing.");
}

/// Nothing was reconciled at all, which is a failure rather than a pass.
fn nothing_probed() {
    let source = super::SOURCE;
    eprintln!("xtask check-shipped-binaries: FAILED - no page under docs/ documents a `cargo build`");
    eprintln!("  with both a shipped package and `--features`, so {source}'s probeFeatures is");
    eprintln!("  compared to nothing. docs/adr/0017 claims the documented feature-on build is");
    eprintln!("  linked on every pull request; either a page states that build, or the claim goes.");
}

/// Documented feature builds that no probe covers.
fn unprobed(problems: &[String], host: Option<&str>) {
    let source = super::SOURCE;
    eprintln!(
        "xtask check-shipped-binaries: FAILED - {} documented feature build(s) are linked by nothing",
        problems.len()
    );
    for problem in problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    match host {
        Some(host) => eprintln!("  `{host}`'s probe step refuses an EMPTY manifest, which is not this:"),
        // The pointer refusal above fired too, so naming no file is the honest line here rather
        // than a remedy citing one that does not exist - the shape this gate's own `refusal`
        // module exists to stop.
        None => eprintln!("  No workflow holds the EMPTY-manifest refusal either, and that is not this:"),
    }
    eprintln!("  with one binary still declaring a probe the manifest is non-empty, every");
    eprintln!("  `cross / link` leg is green, and the build a reader is told to run is linked by");
    eprintln!("  nothing. The row belongs in `{source}`'s probeFeatures.");
}

#[cfg(test)]
mod tests {
    use super::{Refused, refusals};
    use crate::Verdict;

    #[test]
    fn every_broken_rule_is_collected_and_not_only_the_first() {
        // THE FINDING, driven through the composition: the loop rule used to return before the
        // drift report, so a tree with an empty shipped set in one file and a drifted literal in
        // another printed the first and hid the second. Both are here now, and this fails if any
        // rule goes back to short-circuiting the ones after it.
        let expected = [String::from("sutura")];
        let mismatches = [super::Mismatch {
            at: String::from(".github/workflows/release.yml:82"),
            spells: Vec::new(),
        }];
        let clean = super::Reconciliation {
            problems: Vec::new(),
            probed: vec![String::from("docs/p.md:2 sutura-cli [bigquery]")],
        };
        let refused = refusals(Verdict::Fail, &expected, &mismatches, Some("ci.yml"), Ok(&clean));
        assert!(
            matches!(refused.as_slice(), [Refused::EmptyLoop, Refused::Drift { .. }]),
            "both rules have to survive into the report, got {} rule(s)",
            refused.len()
        );
        assert_eq!(super::report(&refused), Verdict::Fail);
    }

    #[test]
    fn a_clean_tree_refuses_nothing() {
        // The other direction, so the collection cannot be green by being empty of everything:
        // with no loop violation, no drift and something reconciled, no rule may fire.
        let clean = super::Reconciliation {
            problems: Vec::new(),
            probed: vec![String::from("docs/p.md:2 sutura-cli [bigquery]")],
        };
        let refused = refusals(Verdict::Pass, &[], &[], Some("ci.yml"), Ok(&clean));
        assert!(refused.is_empty(), "{} rule(s) fired on a clean tree", refused.len());
        assert_eq!(super::report(&refused), Verdict::Pass);
    }

    #[test]
    fn an_unreadable_page_is_a_rule_of_its_own_and_not_a_missing_reconciliation() {
        // `documented::pages` returning `Err` used to return from `run` before the drift report
        // too. It is one rule among the others now, and it must not also report `NothingProbed` -
        // there is no reconciliation to be empty when the pages could not be read.
        let refused = refusals(Verdict::Pass, &[], &[], Some("ci.yml"), Err("docs/p.md: invalid utf-8"));
        assert!(
            matches!(refused.as_slice(), [Refused::UnreadablePage("docs/p.md: invalid utf-8")]),
            "{} rule(s)",
            refused.len()
        );
    }
}
