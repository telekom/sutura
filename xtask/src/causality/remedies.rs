//! What the gate prints when it has no verdict about the change to give, and the remedy each
//! answer asks for.
//!
//! `super::base` owns the verdict once BOTH runs have happened. These are the answers that come
//! before that - the plan could not be split, the scan could not name a test, a declaration put a
//! module of tests into the build, every added test is ignored, or the HEAD run itself was not
//! green - and they are here rather than in `super` because a printed remedy is prose, nothing
//! derives it, and the one thing that keeps it honest is reading them side by side.
//!
//! EACH ONE DECLARES ITS OWN DIRECTION at its own decision, because what a wrong answer costs
//! differs per answer: an unnameable test is a REFUSAL (running everything instead is the
//! unfiltered run whose verdict was a property of the suite), an inseparable file is a loud PASS
//! (the change is usually legitimate and a gate that reddens correct work gets disabled), and an
//! all-ignored diff is a loud PASS for a third reason again (nothing here can execute the tests,
//! so there is no measurement to refuse over).
//!
//! WHAT A PASS HERE HAS TO CARRY, and it did not. Two of these branches PASS while measuring
//! nothing, so `super::coverage`'s ratio is printed beside them: the reader is told `0 of 8` where
//! before the number was in the gate and in no sentence.
//!
//! A PRINTED CAUSE IS A CLAIM, and it is the claim these functions get wrong most easily -
//! `report_unnamed_tests` has been rewritten twice for exactly that, because the two causes it
//! first printed were both wrong for the shape that reached it most often. So each refusing answer
//! names ONE cause per file, taken from the value `super::scoped` classified, rather than
//! offering a reader a list to pick from.

use crate::Verdict;
use crate::causality::base::{names_no_tests, tail};
use crate::causality::coverage::Coverage;
use crate::causality::names::Ident;
use crate::causality::scoped::{Enabled, Silent};

/// Explain the inseparable case. Loud, and deliberately not a failure: the change may be
/// entirely legitimate, but the gate has not verified it and must not read as green.
pub(super) fn report_not_separable(files: &[String], coverage: &Coverage) -> Verdict {
    println!("xtask test-causality: NOT MECHANICALLY SEPARABLE");
    for f in files {
        println!("  {f} changes behaviour and adds tests in one file");
    }
    // The count, so the pass carries its own limit. This branch measures nothing, and saying
    // `0 of 8` beside the prose is the difference between a reader inferring that and being told.
    report_coverage(coverage);
    println!();
    println!("Rust keeps unit tests beside the code they test, so reverting the");
    println!("implementation would remove the test too. State the evidence in the");
    println!("handoff instead: the command you ran, the failure before the fix, and");
    println!("the pass after. This gate has NOT verified causality for this change.");
    Verdict::Pass
}

/// What the run measured, out of what the diff added, and which tests it left out.
///
/// PRINTED BY EVERY BRANCH THAT REACHES A VERDICT, which is a narrower claim than it reads and was
/// briefly a false one. Four arms return `Verdict::Pass` without a ratio and each says in prose
/// that it verified nothing: `Plan::NotRequired`, where the total is zero by construction;
/// `run`'s *tests changed but no implementation did*; [`report_only_ignored`]; and `prove`'s
/// *NO BASE BEHAVIOUR TO COMPARE AGAINST*. **Measured over thirteen recent branch diffs, two land
/// on one of those** - one on the no-revert arm with 3 added tests, one on the ignored arm with 7,
/// whose honest lines are `0 of 3` and `0 of 7`. So the accurate claim is: every branch that runs
/// the two runs prints it, and every branch that does not says instead that it measured nothing.
pub(super) fn report_coverage(coverage: &Coverage) {
    println!("  measured:  {}", coverage.ratio());
    for name in coverage.unmeasured() {
        println!("    not measured: {name}  (its file is kept at HEAD, so it has no base to be red against)");
    }
    for path in coverage.unestablished() {
        println!("    total not established: {path}  (this gate could not read what it added)");
    }
}

/// What a provable file that named no test is accounted for by.
///
/// Each line states the file this gate READ, not a shape it assumed: the sentence used to claim
/// *its own file names the tests* for every one of them, and nothing checked whether that file was
/// in the diff at all.
pub(super) fn report_silent(silent: &[Silent]) {
    for one in silent {
        match one.module {
            Some(ref module) => println!(
                "  named no test: {}  (declares a test module; {module} is in this diff and names them)",
                one.path
            ),
            None => println!(
                "  named no test: {}  (an inline test module around lines already compiled)",
                one.path
            ),
        }
    }
}

/// A separable plan with nothing to revert: the diff changed tests and no implementation.
///
/// PASSES, loudly. There is no old behaviour for a test to be red against, so the gate has no
/// question to ask - not a defect, and the shape a tests-only branch legitimately has.
///
/// It printed NO ratio until the count was measured across thirteen recent branch diffs and one of
/// them landed here with three added tests. `0 of 3` is the honest line: the filterset names them,
/// nothing ran, and *nothing to prove* is not the same sentence as *nothing was added*.
pub(super) fn report_nothing_to_revert(coverage: &Coverage) -> Verdict {
    println!("xtask test-causality: tests changed but no implementation did");
    println!("  measured:  {}", coverage.nothing_measured());
    println!("  Nothing to revert, so there is no old behaviour to be red against.");
    println!("  If this is a new test for existing behaviour, say so; it is not a");
    println!("  regression test and this gate cannot prove it is causal.");
    Verdict::Pass
}

/// Every changed implementation file is NEW on this branch, so there is no base to restore.
///
/// PASSES, loudly, for the same reason as the arm above and a different cause: reverting a file
/// that does not exist at base leaves a tree that does not compile, and a test that fails to
/// compile proves nothing about behaviour.
///
/// Reachable with tests added - two new gate modules plus a new test file is exactly that shape -
/// so it carries the zero-numerator ratio rather than returning before the number is printed,
/// which is what it used to do forty lines above the call that prints it.
pub(super) fn report_no_base_behaviour(base: &str, remove: &[&String], coverage: &Coverage) -> Verdict {
    println!("xtask test-causality: NO BASE BEHAVIOUR TO COMPARE AGAINST");
    for f in remove {
        println!("  {f} does not exist at {base}");
    }
    println!("  measured:  {}", coverage.nothing_measured());
    println!();
    println!("Every changed implementation file is new here, so there is no old behaviour");
    println!("for a test to be red against. Reverting them would leave a tree that does not");
    println!("compile, and a test that fails to compile proves nothing about behaviour.");
    println!("This gate has NOT verified causality for this change - state the evidence in");
    println!("the handoff if it is a bug fix.");
    Verdict::Pass
}

/// A plan whose every provable file declares a test module that named no test.
///
/// FAILS, and that direction is the whole point. Both runs are scoped to the tests the diff
/// added, so a scan that names none has two possible fallbacks: run nothing, or run everything.
/// Running everything is the unfiltered run whose verdict was a property of the suite - the defect
/// this scoping removes - and running nothing is a green gate over zero measurements. An empty
/// scan is therefore a refusal.
///
/// **WHAT REACHES HERE IS NARROWER THAN IT WAS, TWICE, and the printed cause narrowed with it.** A
/// `#[cfg(test)]` item that is not a module is test-only code and never enters the proof; an
/// attribute whose name could not be read has [`report_unreadable`]; a declaration whose module
/// file is not in this diff has [`report_enabled_tests`]. What is left is a diff whose every
/// provable file declares a test module that this gate ACCOUNTED FOR - the module's own file is in
/// the diff, or the module is inline - and no added line in any of them named a test. The reason
/// it is worth the words: the two causes this function first printed were both wrong for the shape
/// that reached it most often, which is how a reader was sent to fix an extractor that was working.
pub(super) fn report_unnamed_tests(test_files: &[String]) -> Verdict {
    eprintln!("xtask test-causality: FAILED - the added tests could not be NAMED");
    for f in test_files {
        eprintln!("  {f} declares a test module and names no test");
    }
    eprintln!();
    eprintln!("Both runs are scoped to the tests the diff added, so naming none of them would");
    eprintln!("leave the gate measuring the whole suite and reading any failure in it as evidence");
    eprintln!("about this change. What reaches here is narrow, so the cause is too: every one of");
    eprintln!("those files declares a test module this diff DOES account for - the module's own");
    eprintln!("file is in the diff, or the module is inline - and no added line anywhere named a");
    eprintln!("test. Usually that means test code moved or was re-wrapped rather than added, which");
    eprintln!("is not something either run can measure: state the evidence in the handoff. A");
    eprintln!("`#[cfg(test)]` item that is NOT a module never reaches here - it is test-only code");
    eprintln!("and is held - and both a name this could not read and a module this diff does not");
    eprintln!("contain are their own verdicts above.");
    Verdict::Fail
}

/// A provable file added a test ATTRIBUTE and the scan could not read a name from it.
///
/// FAILS, and it fails even when other files in the diff named tests fine - which is the whole
/// difference from [`report_unnamed_tests`]. `Scan::of` is aggregate: one nameable test anywhere
/// used to make the answer `Runnable` and carry this file along unmeasured and unmentioned, so the
/// verdict was over a subset and nothing said which. The two printed causes here are the real ones
/// for this shape, and neither is "your change".
pub(super) fn report_unreadable(files: &[String]) -> Verdict {
    eprintln!("xtask test-causality: FAILED - a test attribute this gate could not read a NAME from");
    for f in files {
        eprintln!("  {f} adds a test attribute and no name came out of it");
    }
    eprintln!();
    eprintln!("This file is part of the proof, so a run that skipped it would measure a subset -");
    eprintln!("and other files in this diff DID name tests, which is exactly how that used to go");
    eprintln!("unnoticed. Three causes: the function under the attribute is spelled in a way");
    eprintln!("`causality::scoped` cannot read, no `Cargo.toml` above the file declares a package,");
    eprintln!("or the file's post-image could not be read at all. Fix the extractor rather than");
    eprintln!("widening the run.");
    Verdict::Fail
}

/// A provable file declares a test module whose own file is not in this diff.
///
/// FAILS, and it is a DIFFERENT failure from [`report_unreadable`] because the remedy is
/// different: nothing is wrong with the extractor. A module that was never declared is now
/// compiled, so a whole module of pre-existing tests enters the build - which ENABLES tests rather
/// than adding them. No added line names any of them, and the declaring file is held at HEAD, so
/// they are in both trees and cannot be red on base either.
///
/// It used to be the passing arm. `Scoped::silent` printed *a test module arrived here; its own
/// file names the tests* over exactly this input, and nothing read whether that file existed in
/// the diff - so the run reported `1 of 1 added tests measured` over a module it never looked at.
/// `report_unnamed_tests` already carried the refusal for this cause and was only reachable when
/// no sibling named a test, so one input had two remedies and the one that fired was the pass.
pub(super) fn report_enabled_tests(enabled: &[Enabled]) -> Verdict {
    eprintln!("xtask test-causality: FAILED - a declaration puts tests this diff does not contain into the build");
    for one in enabled {
        eprintln!(
            "  {} declares a test module whose own file {} is not in this diff",
            one.path, one.module
        );
    }
    eprintln!();
    eprintln!("A module that was never compiled is now compiled, which ENABLES tests rather than");
    eprintln!("adding them: no added line names any of them, so neither run measures any of them -");
    eprintln!("and the declaring file is kept at HEAD, so they are in the base tree too and cannot");
    eprintln!("be red against it. Nothing here is an extractor fault. State the evidence in the");
    eprintln!("handoff: the task that runs those tests, and what their result was before and after.");
    Verdict::Fail
}

/// Every test the diff added is `#[ignore]`d, so no run in this venue reaches one.
///
/// PASSES, loudly, and the opposite direction to the refusal above for a reason: an ignored test
/// is not an extractor bug. Naming only ignored tests in a filterset matches nothing, which
/// nextest reports as `no tests to run` and this gate read as a failure - a false RED on
/// legitimate work, and a gate that reddens a correct change gets disabled. Running them instead
/// fails CLOSED in a tree nothing provisioned, the trap [`super::runner::nextest`] records for
/// tier-backed cells.
pub(super) fn report_only_ignored(names: &[Ident]) -> Verdict {
    println!("xtask test-causality: EVERY ADDED TEST IS `#[ignore]`d");
    for name in names {
        println!("  {} is ignored, so no run here reaches it", name.as_str());
    }
    println!();
    println!("Nothing this gate can execute measures the change, so it has NOT verified");
    println!(
        "causality - the honest ratio on this arm is `0 of {}`. State the evidence in the",
        names.len()
    );
    println!("handoff instead: the task that runs these, the failure before the fix, and the");
    println!("pass after.");
    Verdict::Pass
}

/// The HEAD run did not come back green, so nothing can be measured against it.
///
/// Two failures wearing one exit code, and they ask for different things. A filter that matched
/// NOTHING means the gate named a test nextest does not know - never a pass over zero tests, which
/// is what nextest's own `--no-tests` default makes impossible. Anything else means the tests this
/// diff added are simply red here.
pub(super) fn report_head_failure(output: &str, only: &str) -> Verdict {
    if names_no_tests(output) {
        eprintln!("xtask test-causality: FAILED - nextest matched none of the tests this diff added");
        eprintln!("  filter: {only}");
        eprintln!("  Nothing was measured, so this refuses rather than reporting on zero tests.");
        eprintln!("  Three causes: a test attribute `causality::scoped` does not recognise, a");
        eprintln!("  binary id or module path its file's PATH does not settle - a `[[test]]` whose");
        eprintln!("  name is not the file's stem - or a shared target directory still holding the");
        eprintln!("  base run's binaries: see `cargo_test`, and remove `target/causality-target`.");
    } else {
        eprintln!("xtask test-causality: FAILED - the tests this diff added are not green on HEAD");
    }
    eprintln!("{}", tail(output, 30));
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{
        Coverage, Enabled, Ident, Verdict, report_enabled_tests, report_head_failure, report_no_base_behaviour,
        report_not_separable, report_nothing_to_revert, report_only_ignored, report_unnamed_tests, report_unreadable,
    };

    /// A coverage value with nothing measured out of `total`.
    fn nothing_of(total: usize) -> Coverage {
        Coverage::Measured {
            measured: 0,
            unmeasured: (0..total).map(|n| format!("t{n}")).collect(),
        }
    }

    #[test]
    fn each_answer_keeps_the_direction_its_own_doc_argues_for() {
        // THE ONE MECHANICAL THING IN THIS MODULE, and it had no test at all - which matters more
        // here than elsewhere, because the module's whole argument is that the direction is
        // decided PER ANSWER and for a different reason each time. Nothing captures stdout in this
        // venue, so the sentences stay held by review; the direction does not have to be.
        //
        // Three PASS, and each for its own reason: an inseparable file is usually legitimate work,
        // a tests-only branch has no question to ask, and an all-ignored diff has nothing this
        // venue can execute. Flipping any of them to a failure reddens correct work, which is how
        // a gate gets disabled.
        let inseparable = vec![String::from("crates/x/src/a.rs")];
        assert_eq!(report_not_separable(&inseparable, &nothing_of(8)), Verdict::Pass);
        assert_eq!(report_nothing_to_revert(&nothing_of(3)), Verdict::Pass);
        let new_file = String::from("crates/x/src/new.rs");
        assert_eq!(
            report_no_base_behaviour("origin/main", &[&new_file], &nothing_of(2)),
            Verdict::Pass
        );
        let ignored = [Ident::parse("acceptance").expect("an identifier")];
        assert_eq!(report_only_ignored(&ignored), Verdict::Pass);

        // Four FAIL, and each because the alternative is a verdict over something unmeasured:
        // running the whole suite, measuring a subset in silence, passing over a module of tests
        // nothing looked at, or comparing against a HEAD run that was not green.
        assert_eq!(report_unnamed_tests(&inseparable), Verdict::Fail);
        assert_eq!(report_unreadable(&inseparable), Verdict::Fail);
        let enabled = [Enabled {
            path: String::from("crates/x/src/lib.rs"),
            module: String::from("crates/x/src/legacy.rs"),
        }];
        assert_eq!(report_enabled_tests(&enabled), Verdict::Fail);
        assert_eq!(report_head_failure("error: no tests to run", "test(/x/)"), Verdict::Fail);
        assert_eq!(report_head_failure("assertion failed", "test(/x/)"), Verdict::Fail);
    }

    #[test]
    fn an_arm_that_returns_before_either_run_prints_a_zero_numerator() {
        // The numerator is what the filterset NAMES, which equals what was measured only once
        // both runs have happened - so the two arms that return earlier may not print it. Seven
        // named and nothing run is `0 of 8`, never `7 of 8`.
        let named_seven = Coverage::Measured {
            measured: 7,
            unmeasured: vec![String::from("held")],
        };
        assert_eq!(named_seven.ratio(), "7 of 8 added tests measured");
        assert_eq!(named_seven.nothing_measured(), "0 of 8 added tests measured");
    }
}
