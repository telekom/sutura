//! `github.com/telekom/sutura#1239`: a red base run is judged per ADDED test, so a green sibling
//! is named and a moved, parametrised-red or never-launched one is not.

use super::{BaseOutcome, Moved, Reverted, classify_base, reported_per_test};
use crate::causality::fixtures::scoped;

#[test]
fn a_green_sibling_is_not_hidden_by_a_red_added_test() {
    // THE DEFECT THIS CLOSES. Two added tests in one diff, one red and one green on base:
    // the old whole-run classifier answered from the failure alone, so the green sibling - a
    // second added test that PASSES with the change reverted, and so proves nothing of its
    // own - rode along unmentioned on the red one's strength. The run reports per-test names,
    // and the per-test rule is the only one that surfaces the green sibling.
    let text = concat!(
        "    Starting 2 tests across 1 binary\n",
        "        FAIL [   0.010s] (1/2) pa tests::the_red_one\n",
        "        PASS [   0.010s] (2/2) pa tests::the_green_one\n",
        "error: test run failed\n",
    );
    let under_test = scoped("pa", "pa/src/lib.rs", &["the_red_one", "the_green_one"]);
    let outcome = classify_base(text, false, &under_test, &Moved::Nothing, &Reverted::Behaviour);
    match outcome {
        BaseOutcome::RedWithGreenSibling { ref red, ref green } => {
            assert_eq!(red, &[String::from("pa tests::the_red_one")], "{red:?}");
            assert_eq!(green, &[String::from("the_green_one in pa/src/lib.rs")]);
        }
        ref other => panic!("expected RedWithGreenSibling, got {other:?}"),
    }
    assert!(reported_per_test(&outcome), "both tests ran, so the run reported per test");
}

#[test]
fn only_a_new_test_that_passed_whole_is_a_green_sibling() {
    // A MOVED test passes on base by definition, so it is not a sibling this diff added.
    let two = scoped("pa", "pa/src/lib.rs", &["genuinely_new", "a_moved_assertion"]);
    let moved = concat!(
        "        FAIL [   0.010s] (1/2) pa tests::genuinely_new\n",
        "        PASS [   0.010s] (2/2) pa tests::a_moved_assertion\n",
        "error: test run failed\n",
    );
    let partly = Moved::Partly(vec![String::from("a_moved_assertion")]);
    assert_eq!(
        classify_base(moved, false, &two, &partly, &Reverted::Behaviour),
        BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::genuinely_new")]
        }
    );
    // A parametrised test with one red case is red as a whole, and `XFAIL` is not a pass.
    let cases = scoped("pa", "pa/src/lib.rs", &["renders", "never_launched"]);
    let mixed = concat!(
        "        PASS [   0.010s] (1/3) pa tests::renders::case_1\n",
        "        FAIL [   0.010s] (2/3) pa tests::renders::case_2\n",
        "       XFAIL [   0.010s] (3/3) pa tests::never_launched\n",
        "error: test run failed\n",
    );
    assert_eq!(
        classify_base(mixed, false, &cases, &Moved::Nothing, &Reverted::Behaviour),
        BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::renders::case_2")]
        }
    );
}
