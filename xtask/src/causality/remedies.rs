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
//! nothing, so `super::coverage`'s zero numerator is printed beside them: the reader is told
//! `0 of 8` where before the number was in the gate and in no sentence. [`report_not_separable`]
//! and [`no_base_behaviour`] each present [`Attributed::Nothing`] for it, which is the whole
//! accounting - an arm here ran neither run, so there is no other value it could present.
//!
//! AND THE LINE A READER MEETS FIRST IS NOT ONE OF THESE VERDICTS. [`report_scope`] is printed by
//! `super::prove` BEFORE the head run, and it used to print the ratio: on an inconclusive run the
//! output carried `measured:  1 of 1 added tests measured` twenty lines above a verdict saying
//! `0 of 1`, the same sentence with two numerators, reviewed and reproduced on the head that
//! introduced `super::base::earned` to stop precisely that. It says [`Coverage::named`] now -
//! `scope: N of M added tests named` - and the reason it CANNOT go back is in `super::coverage`:
//! the measured wording takes an [`Attributed`], and nothing before a run can produce one.
//!
//! AND ONE OF THOSE PASSES HAD ITS WORDING PINNED BY REVIEW ONLY, which is the shape this module
//! keeps producing: [`no_base_behaviour`] is pure now because the test named for that choice
//! asserted two methods on a value nothing here produced, and the mutation it is named for was
//! green. A printed sentence is prose; a derived one can be read.
//!
//! A PRINTED CAUSE IS A CLAIM, and it is the claim these functions get wrong most easily -
//! `report_unnamed_tests` has been rewritten twice for exactly that, because the two causes it
//! first printed were both wrong for the shape that reached it most often. So each refusing answer
//! names ONE cause per file, taken from the value `super::scoped` classified, rather than
//! offering a reader a list to pick from.

use crate::Verdict;
use crate::causality::base::{names_no_tests, tail};
use crate::causality::coverage::{Attributed, Coverage};
use crate::causality::features::{Because, Enabled, Unread};
use crate::causality::names::Ident;
use crate::causality::provenance::Moved;
use crate::causality::scoped::Silent;

/// Explain the inseparable case. Loud, and deliberately not a failure: the change may be
/// entirely legitimate, but the gate has not verified it and must not read as green.
pub(super) fn report_not_separable(files: &[String], coverage: &Coverage, build_inputs: &[String]) -> Verdict {
    println!("xtask test-causality: NOT MECHANICALLY SEPARABLE");
    for f in files {
        println!("  {f} changes behaviour and adds tests in one file");
    }
    // The count, so the pass carries its own limit. This branch measures nothing, and saying
    // `0 of 8` beside the prose is the difference between a reader inferring that and being told.
    //
    // [`Attributed::Nothing`] is stated here rather than arriving from the caller. It USED to be
    // honest by accident: `plan`'s inseparable arm hands over a `Coverage::of(&[], ..)`, whose
    // numerator is zero by construction, so the ratio and the zero numerator were the same string
    // and a caller passing a populated `Coverage` would have printed a claim about runs that never
    // happened.
    println!("  measured:  {}", coverage.measured(Attributed::Nothing));
    for line in unmeasured_lines(coverage).into_iter().chain(unreverted_lines(build_inputs)) {
        println!("{line}");
    }
    println!();
    println!("Rust keeps unit tests beside the code they test, so reverting the");
    println!("implementation would remove the test too. State the evidence in the");
    println!("handoff instead: the command you ran, the failure before the fix, and");
    println!("the pass after. This gate has NOT verified causality for this change.");
    Verdict::Pass
}

/// What the two runs are ABOUT to be scoped to, printed before either has happened.
///
/// **NOT A MEASUREMENT, AND IT SAID IT WAS.** This printed `measured:  N of M added tests measured`
/// from `super::prove`, ahead of the head run, so an inconclusive verdict's honest `0 of M` arrived
/// twenty lines below the same sentence with a different numerator - in one output, in the same
/// paste a handoff carries. The failure is the one `super::base::earned` was built for, one caller
/// further out: `earned` cannot be consulted here because no run has produced an outcome yet.
///
/// So the line states the SCOPE - `scope: N of M added tests named` - and names what the filterset
/// left out. It is the same count; it is not the same claim, and `super::coverage` is where the two
/// are kept apart by the compiler rather than by this wording.
///
/// WHICH ARMS PRINT A MEASUREMENT INSTEAD, because `every verdict carries the ratio` was briefly
/// false: **FIVE arms return `Verdict::Pass` having run neither run** - `Plan::NotRequired`; `run`'s
/// *tests changed but no implementation did*; [`report_only_ignored`]; `prove`'s
/// *NO BASE BEHAVIOUR TO COMPARE AGAINST*; and [`report_not_separable`]. **Measured over thirteen
/// recent branch diffs, two land on one of them** - one on the no-revert arm with 3 added tests, one
/// on the ignored arm with 7, whose honest lines are `0 of 3` and `0 of 7`. Four of the five print a
/// zero numerator, from [`Attributed::Nothing`]; `Plan::NotRequired` prints no number at all,
/// because the denominator is zero by construction and `0 of 0` reads as *the diff added no tests*
/// rather than as *nothing was measured*. The count was `four`/`they all do` in three places until
/// `github.com/telekom/sutura#319` corrected it.
///
/// So the citable claim is: **exactly one line per run carries `added tests measured`, and a branch
/// that did not run the two runs either says it measured nothing or had nothing to count.**
pub(super) fn report_scope(coverage: &Coverage) {
    for line in scope_lines(coverage) {
        println!("{line}");
    }
}

/// Every line [`report_scope`] prints, in order.
///
/// PURE, for the reason [`no_base_behaviour`] is: nothing in this venue captures stdout, and the
/// defect this function exists to remove is a WORDING - so the assertion has to be able to read the
/// wording. `an_arm_that_returns_before_either_run_prints_a_zero_numerator` is the same shape and
/// was added for the same reason.
fn scope_lines(coverage: &Coverage) -> Vec<String> {
    let mut lines = vec![format!("  scope:     {}", coverage.named())];
    lines.extend(unmeasured_lines(coverage));
    lines
}

/// The changed files the reconstruction may not revert, named wherever an arm says what it did not
/// measure.
///
/// **ONE FUNCTION AND FOUR CALLERS, because three of them had none.** `separable.build_inputs` was
/// printed at exactly one place - inside `super::prove`'s block - and the two arms that return
/// BEFORE that block are precisely where a manifest is the whole implementation change:
/// `Cargo.toml` plus a test that needs the dependency it adds gives an empty `revert`, so
/// [`report_nothing_to_revert`] answered *tests changed but no implementation did* at exit 0 and
/// never named the manifest. Naming it is all this does; what a manifest diff PUT INTO THE BUILD is
/// `super::features`' question and its own refusal. The sentence that arm carried instead pointed at *"the
/// proof's own block"*, which that arm never prints - the cross-arm citation `super::base`'s own
/// header records as not allowed.
pub(super) fn report_unreverted(build_inputs: &[String]) {
    for line in unreverted_lines(build_inputs) {
        println!("{line}");
    }
}

/// Every line [`report_unreverted`] prints, in order.
fn unreverted_lines(build_inputs: &[String]) -> Vec<String> {
    build_inputs
        .iter()
        .map(|path| format!("  not reverted: {path}  (a build input: reverting it changes what cargo RESOLVES)"))
        .collect()
}

/// What the filterset left out, and what stopped the total being established.
///
/// Shared by the pre-run scope line and the inseparable verdict, because the LIST is the same
/// question in both and only the numerator's claim differs. One place, so a reader gets the names
/// whichever line they are reading.
fn unmeasured_lines(coverage: &Coverage) -> Vec<String> {
    let mut lines = Vec::new();
    for name in coverage.unmeasured() {
        lines.push(format!(
            "    not measured: {name}  (its file is kept at HEAD, so it has no base to be red against)"
        ));
    }
    for path in coverage.unestablished() {
        lines.push(format!(
            "    total not established: {path}  (this gate could not read what it added)"
        ));
    }
    // THE THIRD LIST, and the reason it is here rather than on one arm: an `#[ignore]`d test is in
    // NEITHER of the two numbers, so `0 of 0 added tests measured` was everything an all-ignored
    // diff was told on every arm but one - and that reads as *this diff added no tests*.
    for name in coverage.not_runnable() {
        lines.push(format!(
            "    not runnable here: {name}  (`#[ignore]`d, so no run in this venue reaches it)"
        ));
    }
    lines
}

/// Which of the tests about to run the base tree ALREADY had.
///
/// Printed before either run, beside the scope, because it decides what a green base run MEANS and
/// a reader who sees the verdict without it has no way to tell the two readings apart. Nothing at
/// all when every test in scope is new here, which is the ordinary case: a line saying *none of
/// these moved* on every run would be noise, and this gate has enough output.
pub(super) fn report_moved(moved: &Moved) {
    for line in moved_lines(moved) {
        println!("{line}");
    }
}

/// Every line [`report_moved`] prints, in order.
///
/// PURE for the reason the two functions above are: the wording is the whole of what a test can
/// read here, and *which* wording is the fact that matters - `already at base` under the ALL arm is
/// about to become an INCONCLUSIVE verdict, and under the SOME arm it is a failure naming the tests
/// it is not about.
fn moved_lines(moved: &Moved) -> Vec<String> {
    let mut lines = Vec::new();
    for name in moved.names() {
        lines.push(format!(
            "  already at base: {name}  (a `.rs` file this diff also touched had it - a MOVE, not an addition)"
        ));
    }
    if matches!(*moved, Moved::Wholly(_)) {
        lines.push(String::from(
            "  Every test in scope is one of those, so a green base run proves nothing here.",
        ));
    }
    lines
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
pub(super) fn report_nothing_to_revert(coverage: &Coverage, build_inputs: &[String]) -> Verdict {
    for line in nothing_to_revert(coverage, build_inputs) {
        println!("{line}");
    }
    Verdict::Pass
}

/// Every line the arm above prints, in order.
///
/// PURE, and this arm is the one that needed it most: it is where a manifest-only implementation
/// change lands, and the naming this PR claimed for that case was printed by a different arm
/// entirely. A test can read the wording now - `a_manifest_only_change_is_named_on_the_arm_it_lands_on`.
fn nothing_to_revert(coverage: &Coverage, build_inputs: &[String]) -> Vec<String> {
    let mut lines = vec![
        String::from("xtask test-causality: tests changed but no implementation did"),
        format!("  measured:  {}", coverage.measured(Attributed::Nothing)),
    ];
    // The limits beside the number, for the reason `unmeasured_lines` gives: this arm printed the
    // ratio and none of the lists, so an all-`#[ignore]`d tests-only diff read as *no tests*.
    lines.extend(unmeasured_lines(coverage));
    // AND THE FILES THIS GATE MAY NOT REVERT, in place rather than by citation. A manifest is the
    // whole implementation change on this arm more often than anywhere else - a new dependency plus
    // the test that uses it - and *nothing to revert* over an unnamed `Cargo.toml` reads as *your
    // change added no implementation*.
    lines.extend(unreverted_lines(build_inputs));
    lines.push(String::from(
        "  Nothing to revert, so there is no old behaviour to be red against.",
    ));
    lines.push(String::from(
        "  If this is a new test for existing behaviour, say so; it is not a",
    ));
    lines.push(String::from("  regression test and this gate cannot prove it is causal."));
    if !build_inputs.is_empty() {
        lines.push(String::from(
            "  The build input(s) above are why there was nothing: this gate holds them",
        ));
        lines.push(String::from(
            "  at HEAD, because reverting one changes what cargo RESOLVES rather than what",
        ));
        lines.push(String::from("  the tests measure. State the evidence in the handoff."));
    }
    lines
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
pub(super) fn report_no_base_behaviour(base: &str, remove: &[&String], coverage: &Coverage, build_inputs: &[String]) -> Verdict {
    for line in no_base_behaviour(base, remove, coverage, build_inputs) {
        println!("{line}");
    }
    Verdict::Pass
}

/// Every line the arm above prints, in order.
///
/// PURE, AND THAT IS THE POINT. This arm is the ONE place where the choice between the two
/// [`Coverage::measured`] wordings is observable - the two other zero-numerator arms are handed a
/// `Coverage::of(&[], ..)`, whose `measured` is 0 by construction, so both [`Attributed`] values
/// return the same string there and a mutation of either is a no-op. That choice IS the defect #299 was about, and it was pinned by review only: the test
/// named for it asserted both methods on a SYNTHETIC `Coverage` and called no arm, so a reviewer's
/// mutation of the `println!` here to `ratio()` - `7 of 8`, where the filterset merely NAMED seven
/// and nothing ran - reddened nothing at all. Recorded as
/// `github.com/telekom/sutura#319`; nothing in this venue captures stdout, and a function that
/// RETURNS the lines needs no capture.
///
/// **The limit, stated because this is a fix to an overstated control:** what a test can read here
/// is the wording the arm derives. That it was PRINTED is held by the loop above and by review, the
/// same way every other printed sentence in this module is.
fn no_base_behaviour(base: &str, remove: &[&String], coverage: &Coverage, build_inputs: &[String]) -> Vec<String> {
    let mut lines = vec![String::from("xtask test-causality: NO BASE BEHAVIOUR TO COMPARE AGAINST")];
    for f in remove {
        lines.push(format!("  {f} does not exist at {base}"));
    }
    lines.push(format!("  measured:  {}", coverage.measured(Attributed::Nothing)));
    lines.extend(unmeasured_lines(coverage));
    lines.extend(unreverted_lines(build_inputs));
    lines.push(String::new());
    lines.push(String::from(
        "Every changed implementation file is new here, so there is no old behaviour",
    ));
    lines.push(String::from(
        "for a test to be red against. Reverting them would leave a tree that does not",
    ));
    lines.push(String::from(
        "compile, and a test that fails to compile proves nothing about behaviour.",
    ));
    lines.push(String::from(
        "This gate has NOT verified causality for this change - state the evidence in",
    ));
    lines.push(String::from("the handoff if it is a bug fix."));
    lines
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
        // ONE CAUSE PER FINDING, taken from the value rather than from this function's guess -
        // the same rule `report_unnamed_tests` has been rewritten twice for. The two causes ask
        // for the same evidence and name different things, and the match is exhaustive so a third
        // is `error[E0004]` here rather than a finding printed under somebody else's sentence.
        match one.because {
            Because::Declared => eprintln!(
                "  {} declares a test module whose own file {} is not in this diff",
                one.path, one.module
            ),
            Because::Feature { ref manifest, ref name } => eprintln!(
                "  {manifest} declares the feature `{name}`, which the base does not, and {} gates {} on it",
                one.path, one.module
            ),
        }
    }
    eprintln!();
    eprintln!("A module that was never compiled is now compiled, which ENABLES tests rather than");
    eprintln!("adding them: no added line names any of them, so neither run measures any of them -");
    eprintln!("and the declaring file is kept at HEAD, so they are in the base tree too and cannot");
    eprintln!("be red against it. A manifest is not reverted either, for the reason");
    eprintln!("`causality::provenance::Reach::BuildInput` states, so a newly declared feature is on");
    eprintln!("in both trees. Nothing here is an extractor fault. State the evidence in the handoff:");
    eprintln!("the task that runs those tests, and what their result was before and after.");
    Verdict::Fail
}

/// A changed manifest whose own inputs the feature scan could not read.
///
/// FAILS, and it is the arm that stops *nothing was enabled* from being said over a table or a
/// source listing that never came back - the same fail-closed direction `report_unreadable` holds
/// for a post-image, one level up. Each cause prints what it could not read, because the remedy
/// differs: a missing base blob is a fetch or a shallow clone, an empty source listing is a path
/// this gate resolved wrongly, and a listed source that would not read is a working tree that
/// changed under the scan.
pub(super) fn report_unread_manifests(unread: &[Unread]) -> Verdict {
    eprintln!("xtask test-causality: FAILED - a changed manifest this gate could not read");
    for one in unread {
        match *one {
            Unread::Manifest(ref path) => eprintln!("  {path} changed and its content could not be read"),
            Unread::AtBase(ref path) => {
                eprintln!("  {path} exists at the base commit and its content there could not be read");
            }
            Unread::Sources { ref manifest, ref dir } => {
                eprintln!("  {manifest} declares a new feature and no source file was listed under {dir}");
            }
            Unread::Listed(ref path) => {
                eprintln!("  {path} is a listed source of a package whose manifest changed, and could not be read");
            }
        }
    }
    eprintln!();
    eprintln!("A manifest diff can put a whole module of pre-existing tests into the build by");
    eprintln!("declaring the feature a `#[cfg(feature = ..)] mod ..;` is gated on, and answering");
    eprintln!("that it did not needs the feature table on BOTH sides plus the sources that could");
    eprintln!("carry such a declaration. One of those did not come back, so this gate has no answer");
    eprintln!("rather than a reassuring one. Check that the base commit is present locally - a");
    eprintln!("shallow clone is the usual cause - and rerun `just causality`.");
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
pub(super) fn report_only_ignored(names: &[Ident], coverage: &Coverage) -> Verdict {
    println!("xtask test-causality: EVERY ADDED TEST IS `#[ignore]`d");
    for name in names {
        println!("  {} is ignored, so no run here reaches it", name.as_str());
    }
    // THE RATIO IS NOT THIS ARM'S TO FORMAT, and it used to be: `0 of {names.len()}` was spelled
    // here in prose while every other arm printed `Coverage`'s own numbers - two formatters for one
    // fact, and the reason #344 left this one alone is that #314 owns what those numbers may leave
    // unsaid. One place formats it now, and the ignored names are in it.
    println!("  measured:  {}", coverage.measured(Attributed::Nothing));
    for line in unmeasured_lines(coverage) {
        println!("{line}");
    }
    println!();
    println!("Nothing this gate can execute measures the change, so it has NOT verified");
    println!("causality. State the evidence in the handoff instead: the task that runs");
    println!("these, the failure before the fix, and the pass after.");
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
        // THREE CAUSES, AND DELETING THE THIRD WAS WRONG TWICE OVER. It was deleted on the strength
        // of `causality::isolation`'s removal - which at the time removed nothing, because it named
        // no profile - so the arm stopped offering the one remedy that resolved the live defect.
        // The removal is real now, and the cause STILL belongs here: it is bounded to what cargo
        // calls a workspace member at one profile, so anything else in that directory can be stale
        // and removing the directory is the answer that always works. A remedy that costs a rebuild
        // is cheap next to a verdict nobody can explain.
        eprintln!("  Three causes: a test attribute `causality::scoped` does not recognise, a");
        eprintln!("  binary id or module path its file's PATH does not settle - a `[[test]]` whose");
        eprintln!("  name is not the file's stem - or a shared target directory holding something");
        eprintln!("  the per-run removal does not reach: see `causality::isolation`, and remove");
        eprintln!("  `target/causality-target`.");
    } else {
        eprintln!("xtask test-causality: FAILED - the tests this diff added are not green on HEAD");
    }
    eprintln!("{}", tail(output, 30));
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{
        Because, Coverage, Enabled, Ident, Moved, Unread, Verdict, moved_lines, no_base_behaviour, nothing_to_revert,
        report_enabled_tests, report_head_failure, report_no_base_behaviour, report_not_separable, report_nothing_to_revert,
        report_only_ignored, report_unnamed_tests, report_unread_manifests, report_unreadable, scope_lines,
    };

    /// A coverage value with nothing measured out of `total`.
    fn nothing_of(total: usize) -> Coverage {
        Coverage::Measured {
            measured: 0,
            unmeasured: (0..total).map(|n| format!("t{n}")).collect(),
            not_runnable: Vec::new(),
        }
    }

    /// A coverage value whose every added test is `#[ignore]`d: in NEITHER number, so both are 0.
    fn only_ignored(names: &[&str]) -> Coverage {
        Coverage::Measured {
            measured: 0,
            unmeasured: Vec::new(),
            not_runnable: names.iter().map(|name| String::from(*name)).collect(),
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
        assert_eq!(report_not_separable(&inseparable, &nothing_of(8), &[]), Verdict::Pass);
        assert_eq!(report_nothing_to_revert(&nothing_of(3), &[]), Verdict::Pass);
        let new_file = String::from("crates/x/src/new.rs");
        assert_eq!(
            report_no_base_behaviour("origin/main", &[&new_file], &nothing_of(2), &[]),
            Verdict::Pass
        );
        let ignored = [Ident::parse("acceptance").expect("an identifier")];
        assert_eq!(report_only_ignored(&ignored, &only_ignored(&["acceptance"])), Verdict::Pass);

        // Four FAIL, and each because the alternative is a verdict over something unmeasured:
        // running the whole suite, measuring a subset in silence, passing over a module of tests
        // nothing looked at, or comparing against a HEAD run that was not green.
        assert_eq!(report_unnamed_tests(&inseparable), Verdict::Fail);
        assert_eq!(report_unreadable(&inseparable), Verdict::Fail);
        // BOTH CAUSES, because they are one verdict and a cause with no printed sentence of its
        // own would inherit the other one's. The exhaustive match in `report_enabled_tests` is
        // what makes a third cause a compile error rather than a mislabelled line.
        let enabled = [
            Enabled {
                path: String::from("crates/x/src/lib.rs"),
                module: String::from("crates/x/src/legacy.rs"),
                because: Because::Declared,
            },
            Enabled {
                path: String::from("crates/x/src/lib.rs"),
                module: String::from("crates/x/src/legacy.rs"),
                because: Because::Feature {
                    manifest: String::from("crates/x/Cargo.toml"),
                    name: String::from("legacy"),
                },
            },
        ];
        assert_eq!(report_enabled_tests(&enabled), Verdict::Fail);
        // And the manifest this gate could not read on one side, which may not answer *nothing was
        // enabled* - all four causes, each with its own sentence.
        let unread = [
            Unread::Manifest(String::from("crates/x/Cargo.toml")),
            Unread::AtBase(String::from("crates/x/Cargo.toml")),
            Unread::Sources {
                manifest: String::from("crates/x/Cargo.toml"),
                dir: String::from("crates/x"),
            },
            Unread::Listed(String::from("crates/x/src/legacy.rs")),
        ];
        assert_eq!(report_unread_manifests(&unread), Verdict::Fail);
        assert_eq!(report_head_failure("error: no tests to run", "test(/x/)"), Verdict::Fail);
        assert_eq!(report_head_failure("assertion failed", "test(/x/)"), Verdict::Fail);
    }

    #[test]
    fn an_arm_that_returns_before_either_run_prints_a_zero_numerator() {
        // THE ARM, not a synthetic value. This test asserted `Coverage::ratio()` and
        // `Coverage::nothing_measured()` on a hand-built `Coverage` and called nothing, so it was
        // green under the one mutation it is named for: making this arm print `ratio()` - `7 of 8`,
        // the filterset's NAMES count, on a run where neither run happened. Reproduced by #306's
        // reviewer and filed as #319. `no_base_behaviour` is the arm's own lines, so the mutation
        // lands inside what this reads.
        //
        // `report_no_base_behaviour` is the only observable site: the other two zero-numerator arms
        // get a `Coverage::of(&[], ..)`, where `measured` is 0 by construction and the two wordings
        // are the same string.
        let named_seven = Coverage::Measured {
            measured: 7,
            unmeasured: vec![String::from("held")],
            not_runnable: Vec::new(),
        };
        let new_file = String::from("crates/x/src/new.rs");
        let lines = no_base_behaviour("origin/main", &[&new_file], &named_seven, &[]);
        assert!(
            lines.iter().any(|line| line.contains("0 of 8 added tests measured")),
            "an arm that ran neither run says it measured nothing: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("7 of 8")),
            "the filterset's NAMES count is not a measurement: {lines:?}"
        );
        // And the arm still prints those lines and passes. What that holds is the wording; that it
        // reached stdout is the loop's, and unobservable here - the limit `no_base_behaviour` states.
        assert_eq!(
            report_no_base_behaviour("origin/main", &[&new_file], &named_seven, &[]),
            Verdict::Pass
        );
    }

    #[test]
    fn a_manifest_only_change_is_named_on_the_arm_it_lands_on() {
        // THE DEFECT REVIEW FOUND, at the arm it lands on. `separable.build_inputs` had exactly one
        // reader - inside `super::prove`'s block - and a manifest-only implementation change returns
        // from HERE, before that block: *tests changed but no implementation did*, `Verdict::Pass`,
        // exit 0, and the `Cargo.toml` that WAS the change named nowhere. That is #343's exact shape
        // and the one arm where the naming was the point.
        //
        // Worse, the sentence this arm carried pointed the reader at *"the proof's own block"* -
        // lines this arm never prints. `super::base`'s own header records why a cross-arm citation
        // is not allowed; it is said in place now.
        let manifest = vec![String::from("crates/x/Cargo.toml")];
        let lines = nothing_to_revert(&nothing_of(1), &manifest);
        assert!(
            lines.iter().any(|line| line.contains("not reverted: crates/x/Cargo.toml")),
            "the arm names the file it could not revert: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("the proof's own block")),
            "no arm cites another arm's output: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("changes what cargo RESOLVES")),
            "and says why it is held rather than reverted: {lines:?}"
        );
        // With nothing held back the extra paragraph is absent: a tests-only branch with no build
        // input is the ordinary shape of this arm and may not be told about one.
        let plain = nothing_to_revert(&nothing_of(1), &[]);
        assert!(
            !plain.iter().any(|line| line.contains("not reverted")),
            "nothing to name: {plain:?}"
        );
        assert_eq!(report_nothing_to_revert(&nothing_of(1), &manifest), Verdict::Pass);
    }

    #[test]
    fn an_all_ignored_diff_names_them_wherever_the_ratio_is_printed() {
        // THE DEFECT, and it is a WORDING with a mechanism behind it. An `#[ignore]`d added test is
        // in neither number by design, so a diff whose added tests are all ignored printed
        // `0 of 0 added tests measured` - which reads as *this diff added no tests* - and only
        // `report_only_ignored` named them. Every arm that prints a numerator prints the names now,
        // because they come from the same shared lines the ratio's own limits do.
        let ignored = only_ignored(&["the_provisioned_surface_answers"]);
        let named_here = |lines: &[String]| {
            lines
                .iter()
                .any(|line| line.contains("not runnable here: the_provisioned_surface_answers"))
        };
        assert!(named_here(&scope_lines(&ignored)), "the pre-run scope names them");
        let new_file = String::from("crates/x/src/new.rs");
        let no_base = no_base_behaviour("7a65f1e1", &[&new_file], &ignored, &[]);
        assert!(named_here(&no_base), "the no-base arm names them: {no_base:?}");
        // And the ratio itself is untouched: putting them in the denominator is what would make
        // every acceptance-heavy branch read as partially measured forever.
        assert!(
            scope_lines(&ignored)
                .iter()
                .any(|line| line.contains("0 of 0 added tests named")),
            "{:?}",
            scope_lines(&ignored)
        );
    }

    #[test]
    fn a_scope_the_base_tree_already_had_is_named_before_either_run() {
        // The classification a reader needs BEFORE the verdict, because it decides what a green
        // base run means. Nothing is printed when every test in scope is new here - the ordinary
        // case, and a line saying so on every run is noise - and the ALL arm says out loud that
        // the run about to happen cannot prove anything.
        assert!(
            moved_lines(&Moved::Nothing).is_empty(),
            "a run that moved nothing prints no moved-line report"
        );
        let one = vec![String::from("a_moved_assertion")];
        let some = moved_lines(&Moved::Partly(one.clone()));
        assert!(
            some.iter().any(|line| line.contains("already at base: a_moved_assertion")),
            "{some:?}"
        );
        assert!(
            !some.iter().any(|line| line.contains("proves nothing")),
            "with some new tests in scope the run still proves something: {some:?}"
        );
        let all = moved_lines(&Moved::Wholly(one));
        assert!(all.iter().any(|line| line.contains("proves nothing here")), "{all:?}");
    }

    #[test]
    fn the_line_printed_before_either_run_claims_no_measurement() {
        // THE SAME DEFECT ONE CALLER OUT, and the one `base::earned` could not reach. `prove`
        // printed this line BEFORE the head run, from `Coverage::ratio`, so a run ending
        // INCONCLUSIVE carried `measured:  7 of 8 added tests measured` about twenty lines above a
        // verdict saying `0 of 8` - the same sentence, two numerators, in one paste. Reproduced by
        // review on the head that added `earned`, which is why it is worth a test rather than a
        // wording: `earned` is unreachable here, because no run has produced an outcome yet.
        let named_seven = Coverage::Measured {
            measured: 7,
            unmeasured: vec![String::from("held")],
            not_runnable: Vec::new(),
        };
        let lines = scope_lines(&named_seven);
        assert!(
            lines.iter().any(|line| line.contains("7 of 8 added tests named")),
            "the pre-run line states the SCOPE, and the count is the filterset's: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("added tests measured")),
            "nothing has run yet, so no line here may carry the measurement key: {lines:?}"
        );
        // The names are still here - they are the half a reader needs whichever line carries them.
        assert!(
            lines.iter().any(|line| line.contains("not measured: held")),
            "the filterset's omissions are named before the runs, as they were: {lines:?}"
        );
    }
}
