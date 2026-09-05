//! What the gate prints when it has no verdict about the change to give, and the remedy each
//! answer asks for.
//!
//! `super::base` owns the verdict once BOTH runs have happened. These are the four answers that
//! come before that - the plan could not be split, the scan could not name a test, every added
//! test is ignored, or the HEAD run itself was not green - and they are here rather than in
//! `super` because a printed remedy is prose, nothing derives it, and the one thing that keeps it
//! honest is reading the four side by side.
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

use crate::Verdict;
use crate::causality::base::{names_no_tests, tail};
use crate::causality::coverage::Coverage;
use crate::causality::names::Ident;

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
/// Printed by every branch that reaches a verdict, because the omission is silent otherwise: the
/// gate knew both numbers and printed neither, so `ok - red on base, green on head` read as a
/// verdict over the change while measuring a subset of it.
pub(super) fn report_coverage(coverage: &Coverage) {
    println!("  measured:  {}", coverage.ratio());
    for name in coverage.unmeasured() {
        println!("    not measured: {name}  (its file is kept at HEAD, so it has no base to be red against)");
    }
}

/// A plan with test files whose tests this gate could not NAME.
///
/// FAILS, and that direction is the whole point. Both runs are scoped to the tests the diff
/// added, so a scan that names none has two possible fallbacks: run nothing, or run everything.
/// Running everything is the unfiltered run whose verdict was a property of the suite - the defect
/// this scoping removes - and running nothing is a green gate over zero measurements. An empty
/// scan is therefore a refusal.
///
/// **WHAT REACHES HERE IS NARROWER THAN IT WAS, and the printed cause had to narrow with it.** A
/// `#[cfg(test)]` item that is not a module is test-only code and never enters the proof; an
/// attribute whose name could not be read has [`report_unreadable`]. What is left is a diff whose
/// every provable file declares a test MODULE and nothing else - so the remedy names that, and no
/// longer offers two causes this branch can no longer have. The reason it is worth the words: the
/// two causes it USED to print were both wrong for the shape that reached it most often, which is
/// how a reader was sent to fix an extractor that was working.
pub(super) fn report_unnamed_tests(test_files: &[String]) -> Verdict {
    eprintln!("xtask test-causality: FAILED - the added tests could not be NAMED");
    for f in test_files {
        eprintln!("  {f} declares a test module and names no test");
    }
    eprintln!();
    eprintln!("Both runs are scoped to the tests the diff added, so naming none of them would");
    eprintln!("leave the gate measuring the whole suite and reading any failure in it as evidence");
    eprintln!("about this change. What reaches here is narrow, so the cause is too: every one of");
    eprintln!("those files declares a test MODULE and nothing else, and the module's own file is");
    eprintln!("not in this diff - so its tests are not added lines and this cannot read a name.");
    eprintln!("Usually that means a module that was never declared is now compiled, which enables");
    eprintln!("tests rather than adding them: state the evidence in the handoff. A `#[cfg(test)]`");
    eprintln!("item that is NOT a module never reaches here - it is test-only code and is held -");
    eprintln!("and an attribute whose NAME could not be read is its own verdict above.");
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
    eprintln!("unnoticed. Two causes: the function under the attribute is spelled in a way");
    eprintln!("`causality::scoped` cannot read, or no `Cargo.toml` above the file declares a");
    eprintln!("package. Fix the extractor rather than widening the run.");
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
    println!("causality. State the evidence in the handoff instead: the task that runs these,");
    println!("the failure before the fix, and the pass after.");
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
