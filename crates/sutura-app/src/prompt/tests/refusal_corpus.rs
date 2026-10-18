//! One instance of every refusal variant, so the prompt's guidance can be checked against it.
//!
//! A submodule of [`super`] rather than a fixture inside it, and the reason is the mechanical one
//! [`super::column_zero`] gives: `cargo xtask max-lines` fails at a thousand lines under `crates/`
//! and cannot be exempted, and that file was one line under it.
//!
//! The list itself is `sutura_domain::query::every_refusal`, beside the enum, so this file names no
//! variant: a change that adds one need not edit it, and `just causality` can rebuild the base tree.

use sutura_domain::query::RefusalReason;

/// One instance of every refusal variant, from the list beside the enum.
///
/// The second net rather than the first: what forces an author to write a guide is `guide_for` in
/// `prompt/refusal.rs` failing to compile. What this list adds is that once they have, the set of
/// guides the prompt renders and the set `guide_for` can return are asserted to be the same.
pub(super) fn every_refusal() -> Vec<RefusalReason> {
    sutura_domain::query::every_refusal()
}

/// G4 - the witness pairing, driven by every variant in this corpus.
///
/// In the corpus file because these are the tests that READ it, and `prompt/tests.rs` is one line
/// off the thousand-line ceiling. The pairing the set test in `prompt/tests.rs` deliberately does
/// NOT make: it compares two SETS of reason strings, so swapping any two `Guide.reason` consts
/// stays green. Asserting the pairing per variant - a guide's `reason` must be the variant's OWN
/// machine code - reddens exactly that mutation, which is the witness G4 exists to hold.
use super::super::refusal::{GUIDES, guide_for};

/// A guide's `reason` is the domain's canonical `code()` for the variant it was matched from.
#[test]
fn guide_key_carries_the_variant_it_was_matched_from() {
    for reason in every_refusal() {
        assert_eq!(
            guide_for(&reason).reason,
            reason.code(),
            "guide_for returned another variant's key"
        );
    }
}

/// The list beside the enum and the guides are the same size, and every key is its variant's code.
///
/// The exhaustive match is in `sutura_domain::query::every_refusal`, where a new variant fails to
/// compile until it is named. What this cell holds at run time is the count: a variant with a guide
/// and no entry in that list is a count that differs. A count misses a duplicate row that takes the
/// place of the missing one; `every_refusal_a_caller_can_be_given_has_guidance_in_the_prompt` compares
/// the two sets and holds that case.
#[test]
fn guide_key_carries_every_variant() {
    let all = every_refusal();
    assert_eq!(
        all.len(),
        GUIDES.len(),
        "a guide has no entry in the list, or an entry has no guide"
    );
    for reason in all {
        assert_eq!(guide_for(&reason).reason, reason.code(), "the key is not this variant's code");
    }
}

/// A cross-source relationship with a truncated key is refused under its own code, and the corpus
/// holds it: the guide cells above then check its guide.
#[test]
fn the_corpus_holds_the_truncated_crossing_key_refusal() {
    assert!(
        every_refusal()
            .iter()
            .any(|reason| reason.code() == "federation_link_truncated"),
        "no refusal in the corpus carries the truncated-key code"
    );
}
