//! What a venues ROW has to be, before any verdict rule may read one.
//!
//! The third seam `venues.rs` has been split on, and the same one both times: `page` says what a
//! row IS and decides nothing, `sources` says what is true of the tree, and this says what a row
//! must HOLD for the matrix cells above it to mean anything. Split out when the file reached the
//! unexemptable 1000-line gate a third time - nothing under `xtask/` may be listed in
//! `devco/max-lines-ignore`, so the file splits or the gate fails.
//!
//! **The split moved no assertion**, which is what `.agents/skills/sutura/gates` prescribes: the
//! causality gate never reverts a file that adds a `#[test]`, so every `#[test]` stayed in
//! `venues.rs` and nothing is orphaned. That file carries implementation and tests together, so
//! the plan is `NotSeparable`, nothing is reverted and no base build is attempted - the verdict is
//! `NOT MECHANICALLY SEPARABLE` and a mutation stands in for the proof.

use super::PAGE;
use super::page::{NOT_BUILT, RUN_SITES, Venue};
use super::sources::cited_invocations;

/// Everything wrong with a venues ROW, as against the matrix cells that read one.
///
/// Two rules over the two cells every verdict rule resolves, and they are one argument: a row
/// states its own state in prose, and prose that nothing can classify is a state nothing can judge.
///
/// **`Reached by` was the cheapest bypass on the page, and it predates `wired`.** Every such rule
/// resolves that cell through [`cited_invocations`], which wants a backticked `just <task>` or
/// `nix run .#<app>` - so dropping that prefix made the cell resolve to nothing and the venue's
/// verdict permanent at exit 0, whatever CI ran. **`Where it runs` is the same shape one cell
/// over**, found by review of the change that made it load-bearing: [`Venue::runs_nowhere`] read
/// the SUBSTRING `nowhere`, so a synonym turned off the only remaining guard on `yes` for leg 2.
/// [`RUN_SITES`] carries the measurement and the limit a vocabulary does not reach.
pub(super) fn row_problems(listed: &[Venue]) -> Vec<String> {
    let mut problems: Vec<String> = listed
        .iter()
        .filter(|venue| venue.site().is_none())
        .map(|venue| {
            format!(
                "{PAGE}: `{}` says `{}` about where it runs, which is not one of {RUN_SITES:?}. \
                 That cell decides whether this venue may be cited at all, so it states a place \
                 from a closed list or it is a sentence nothing reads - its claims cells' own rule",
                venue.name, venue.runs
            )
        })
        .collect();
    problems.extend(
        listed
            .iter()
            .filter(|venue| venue.is_built() && cited_invocations(&venue.reached).is_empty())
            .map(|venue| {
                format!(
                    "{PAGE}: `{}` is reached by `{}`, which names no `just <task>` and no \
                     `nix run .#<app>` in backticks - every rule that decides this venue's state \
                     resolves that cell, so a `Reached by` nothing can resolve is a venue whose \
                     state cannot be judged. Either name the invocation the way this page names \
                     one, or the cell is `{NOT_BUILT}`",
                    venue.name, venue.reached
                )
            }),
    );
    problems
}
