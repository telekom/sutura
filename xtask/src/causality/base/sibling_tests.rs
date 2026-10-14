//! `github.com/telekom/sutura#1239`: a red base run is judged per ADDED test, so a green sibling
//! is named and a moved, parametrised-red or never-launched one is not.

use super::{BaseOutcome, Moved, Reverted, classify_base, report_base, reported_per_test};
use crate::Verdict;
use crate::causality::fixtures::{named, scoped};

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

#[test]
fn a_base_run_that_never_reached_its_tier_is_not_a_proof() {
    // The base run keeps the head run's requirement, so a cell whose tier is absent there panics
    // in the lookup: red, and about nothing the diff changed.
    let text = format!(
        "        FAIL [   0.021s] (1/1) pa tests::t\n\
         thread 'tests::t' panicked at dev/src/provisioned.rs:413:41:\n\
         {}: no `postgres` endpoint\n\
         error: test run failed\n",
        sutura_dev::requirement::REQUIRED
    );
    let under_test = scoped("pa", "pa/src/lib.rs", &["t"]);
    let outcome = classify_base(&text, false, &under_test, &Moved::Nothing, &Reverted::Behaviour);
    assert_eq!(
        report_base(&outcome, &text, false, &named(1), &Moved::Nothing, &Reverted::Behaviour),
        Verdict::Inconclusive,
        "{outcome:?}"
    );
}

#[test]
fn a_tier_panic_nested_in_a_message_is_not_the_tests_own_site() {
    // Measured: a cell whose message printed a tier-absent fixture read as a tier panic, and that
    // made the whole base run INCONCLUSIVE. Its own panic is the assertion, so it is red by one.
    let text = format!(
        "        FAIL [   0.070s] (1/1) pa tests::outer\n\
         \x20   thread 'tests::outer' (7) panicked at pa/src/lib.rs:9:5:\n\
         \x20   the base run said:\n\
         \x20           FAIL [   0.021s] (1/1) pa tests::t\n\
         \x20   thread 'tests::t' panicked at dev/src/provisioned.rs:413:41:\n\
         \x20   {}: no `postgres` endpoint\n\
         error: test run failed\n",
        sutura_dev::requirement::REQUIRED
    );
    let under_test = scoped("pa", "pa/src/lib.rs", &["outer"]);
    assert_eq!(
        classify_base(&text, false, &under_test, &Moved::Nothing, &Reverted::Behaviour),
        BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::outer")]
        }
    );
}

#[test]
fn a_green_sibling_is_judged_before_the_tier_read() {
    let text = format!(
        "        FAIL [   0.021s] (1/2) pa tests::t\n\
         \x20   thread 'tests::t' (9) panicked at dev/src/provisioned.rs:413:41:\n\
         \x20   {}: no `postgres` endpoint\n\
         \n\
         \x20       PASS [   0.010s] (2/2) pa tests::pin\n\
         error: test run failed\n",
        sutura_dev::requirement::REQUIRED
    );
    let under_test = scoped("pa", "pa/src/lib.rs", &["t", "pin"]);
    let outcome = classify_base(&text, false, &under_test, &Moved::Nothing, &Reverted::Behaviour);
    assert_eq!(
        report_base(&outcome, &text, false, &named(2), &Moved::Nothing, &Reverted::Behaviour),
        Verdict::Fail,
        "{outcome:?}"
    );
}

#[test]
fn a_tier_site_without_the_requirement_under_it_is_an_ordinary_red() {
    // The requirement counts only in the test's own message, which ends at the first blank line.
    let text = format!(
        "        FAIL [   0.021s] (1/2) pa tests::t\n\
         \x20   thread 'tests::t' (9) panicked at dev/src/provisioned.rs:200:5:\n\
         \x20   a malformed endpoint\n\
         \n\
         \x20       FAIL [   0.021s] (2/2) pa tests::elsewhere\n\
         \x20   thread 'tests::elsewhere' (10) panicked at dev/src/provisioned.rs:413:41:\n\
         \x20   {}: no `postgres` endpoint\n\
         error: test run failed\n",
        sutura_dev::requirement::REQUIRED
    );
    let under_test = scoped("pa", "pa/src/lib.rs", &["t"]);
    assert_eq!(
        classify_base(&text, false, &under_test, &Moved::Nothing, &Reverted::Behaviour),
        BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::t")]
        }
    );
}
