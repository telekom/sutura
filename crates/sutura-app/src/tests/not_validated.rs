//! One cell split out of `super` (`tests.rs`) for that file's own `max-lines` cap - not for
//! thematic tidiness, so `use super::*` reaches every fixture this test read before the move.

use super::*;

#[test]
fn a_bundle_whose_anchor_could_not_run_does_not_come_back_validated() {
    // The other half of the invariant, and the half a report could not carry: the ONLY way to a
    // `Validated` bundle runs the anchors, so a data system that answers nothing yields no
    // bundle at all. Before this operation existed, the same situation was a report a caller was
    // free to ignore - and `Validated::new` was happy to be handed a different one.
    let error = verify_and_validate(bundle(), &Warehouses::of(FixedWarehouse::new(source(), shared())))
        .expect_err("a data system that fails every statement cannot validate a bundle");
    let NotValidated::AnchorNotExecuted { ref metric, .. } = error else {
        panic!("a failed anchor check is a not-executed verdict, not {error:?}");
    };
    assert_eq!(metric, &self::metric());
}
