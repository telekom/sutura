use super::base::BaseOutcome;
use super::fixtures::{Mutation, inseparable_claim_case};
use super::{BaseState, retry_with_held_back};
use crate::Verdict;

/// `github.com/telekom/sutura#414`: `sources_under`, the step between the census and the
/// feature scan, carries a refusal through rather than an empty listing that would let
/// *nothing was enabled* pass unread. The scan itself reads the real repository and has no cell.
#[test]
fn sources_under_carries_a_census_refusal_through_and_keeps_only_that_directory() {
    let refused: super::features::Listing = Err(String::from("the census refused"));
    assert_eq!(
        super::sources_under(&refused, "crates/x"),
        Err(String::from("the census refused"))
    );
    let listed: super::features::Listing = Ok(vec![
        String::from("crates/x/src/lib.rs"),
        String::from("crates/xy/src/lib.rs"),
    ]);
    assert_eq!(
        super::sources_under(&listed, "crates/x"),
        Ok(vec![String::from("crates/x/src/lib.rs")])
    );
}

/// `github.com/telekom/sutura#837` direction 2, half one: a complete declaration on an
/// inseparable diff is EVALUATED, and a mutation that kills by the cell's own assertion is
/// accepted.
///
/// NOT DISCRIMINATING ALONE, stated rather than hidden: the old `Plan::NotSeparable`
/// fallback (`report_not_separable`) also answers `Verdict::Pass`, unconditionally, so this
/// assertion by itself cannot be shown red-on-base - confirmed by hand: neutralising the
/// dispatch guard below with `&& false` leaves this test green. The causal evidence that the
/// NEW dispatch, not the old fallback, is what answers here is the CONTRAST with the two
/// tests after it: the same fixture shape with an invalid declaration
/// (`Mutation::DoesNotKill`, `Mutation::Missing`) answers `Verdict::Fail` under the same
/// neutralising mutation - reversed to green - which an unconditional fallback could never
/// do. A single arm sensitive to the patch's validity is not reachable through one that is
/// not.
#[test]
fn a_declared_claim_cell_is_evaluated_from_an_inseparable_plan() {
    assert_eq!(
        inseparable_claim_case(true, Mutation::Kills, None),
        Verdict::Pass,
        "a complete declaration on an inseparable diff must be consulted, and its killing \
             mutation accepted"
    );
}

/// The arm is reached, not merely declared: a mutation that applies but does not kill is
/// refused rather than passing by the old unconsulted route (`Verdict::Pass`, unconditionally,
/// before this decision). CONFIRMED BY HAND: neutralising the dispatch guard with `&& false`
/// turns this back into `Verdict::Pass` - this is one of the two tests that discriminate the
/// new dispatch from the old fallback.
#[test]
fn an_inseparable_claim_cell_whose_mutation_does_not_kill_is_refused() {
    assert_eq!(
        inseparable_claim_case(true, Mutation::DoesNotKill, None),
        Verdict::Fail,
        "reaching the arm and finding the mutation does not kill must refuse - the old route \
             answered `Verdict::Pass` unconditionally here"
    );
}

/// `github.com/telekom/sutura#837` direction 2's own guard: a declaration with no committed
/// patch is `Cause::MissingPatch`, not treated as though nothing were declared. CONFIRMED BY
/// HAND: neutralising the dispatch guard with `&& false` turns this back into
/// `Verdict::Pass` too - the second of the two discriminating tests.
#[test]
fn an_inseparable_claim_cell_with_no_committed_patch_is_refused_as_missing() {
    assert_eq!(
        inseparable_claim_case(true, Mutation::Missing, None),
        Verdict::Fail,
        "a declared cell with no patch must refuse as missing - the old route answered \
             `Verdict::Pass` unconditionally here"
    );
}

#[test]
fn an_inseparable_claim_does_not_hide_an_ordinary_test() {
    assert_eq!(
        inseparable_claim_case(
            true,
            Mutation::Kills,
            Some("    #[test]\n    fn the_ordinary_one() { assert_eq!(super::g(), 1); }"),
        ),
        Verdict::Fail,
        "the claimed mutation cannot prove a neighbouring ordinary test in the same inseparable file"
    );
}

/// `github.com/telekom/sutura#970`: the patch is committed and no trailer declares it, so no
/// run would ever apply it - refused, where the base answered the NOT MECHANICALLY SEPARABLE pass.
#[test]
fn a_committed_mutation_no_trailer_declares_is_refused() {
    assert_eq!(inseparable_claim_case(false, Mutation::Kills, None), Verdict::Fail);
}

/// Half two, and UNCHANGED behaviour: with no `Claim-Cell:` trailer at all, an inseparable
/// plan still answers exactly the non-verdict it did before this decision. Not provable by
/// causality - the base tree already gives this same answer, by construction - so this pins
/// by inspection rather than by a mutation: `causality::run`'s `if let Some(..) = claim` guard
/// is `None` here and the new code path is never entered at all.
#[test]
fn an_inseparable_plan_with_no_declaration_stays_the_same_non_verdict() {
    assert_eq!(
        inseparable_claim_case(false, Mutation::Missing, None),
        Verdict::Pass,
        "no `Claim-Cell:` trailer at all must still be the loud NOT MECHANICALLY SEPARABLE pass"
    );
}

#[test]
fn only_a_tree_that_did_not_compile_is_asked_twice() {
    let one = String::from("crates/x/src/pinned.rs");
    let held = BaseState {
        restore: vec![&one],
        remove: Vec::new(),
        stub: &[],
    };
    let nothing_held = BaseState {
        restore: Vec::new(),
        remove: Vec::new(),
        stub: &[],
    };
    // The case the second attempt exists for.
    assert!(retry_with_held_back(&BaseOutcome::DidNotCompile, &held));
    // Nothing is held at HEAD, so a second attempt would reconstruct the same tree and fail
    // the same way. One build, not two.
    assert!(!retry_with_held_back(&BaseOutcome::DidNotCompile, &nothing_held));
    // A run that reached an assertion has ANSWERED. Retrying would revert the implementation
    // whose absence that assertion just measured, turning evidence into a different question.
    assert!(!retry_with_held_back(
        &BaseOutcome::RedByAssertion { failed: Vec::new() },
        &held
    ));
    assert!(!retry_with_held_back(&BaseOutcome::Green, &held));
}
