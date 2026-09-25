//! What the gate PASSES on, loudly, before either run has happened or in place of one - and the
//! shared lines a handful of these answers print beside their own.
//!
//! `super::base` owns the verdict once BOTH runs have happened. `super::refusals` holds the
//! opposite direction, `Verdict::Fail` for the same shape of reason each time: the scan could not
//! name a test, a declaration put tests into the build this diff never named, or the HEAD run
//! itself was not green. Split there at the unexemptable 1000-line cap - a printed remedy is
//! prose, nothing derives it, and the one thing that kept it honest was reading every answer side
//! by side, which the split keeps true PER DIRECTION rather than for the whole gate.
//!
//! EACH ONE DECLARES ITS OWN DIRECTION at its own decision, because what a wrong answer costs
//! differs per answer: an unnameable test is a REFUSAL (`super::refusals::report_unnamed_tests`;
//! running everything instead is the unfiltered run whose verdict was a property of the suite), an
//! inseparable file is a loud PASS (the change is usually legitimate and a gate that reddens
//! correct work gets disabled), and an all-ignored diff is a loud PASS for a third reason again
//! (nothing here can execute the tests, so there is no measurement to refuse over).
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

use crate::Verdict;
use crate::causality::coverage::{Attributed, Coverage};
use crate::causality::diff::ChangedFile;
use crate::causality::names::Ident;
use crate::causality::place::{Declares, accounted_for};
use crate::causality::provenance::Moved;
use crate::causality::regions::PostImage;
use crate::causality::scoped::Silent;

/// Explain the inseparable case. Loud, and deliberately not a failure: the change may be
/// entirely legitimate, but the gate has not verified it and must not read as green.
///
/// `claim_declared` says whether the commit range carries a `Claim-Cell:` trailer - this arm
/// returns before `Plan::Separable`'s branch ever reaches `claim::Claim::of`, so a declaration
/// made here is silent otherwise: an author who committed both the trailer and its killing
/// mutation would see this exact verdict with nothing telling them the declaration was never
/// read. `github.com/telekom/sutura#837` is the direction decision this does not make; it only
/// stops the silence.
pub(super) fn report_not_separable(
    files: &[String],
    coverage: &Coverage,
    build_inputs: &[String],
    claim_declared: bool,
) -> Verdict {
    for line in not_separable_lines(files, coverage, build_inputs, claim_declared) {
        println!("{line}");
    }
    Verdict::Pass
}

/// Every line the arm above prints, in order.
///
/// PURE, for the reason every other line in this module is: nothing in this venue captures
/// stdout, so the assertion has to read the lines rather than the print.
fn not_separable_lines(files: &[String], coverage: &Coverage, build_inputs: &[String], claim_declared: bool) -> Vec<String> {
    let mut lines = vec![String::from("xtask test-causality: NOT MECHANICALLY SEPARABLE")];
    for f in files {
        lines.push(format!("  {f} changes behaviour and adds tests in one file"));
    }
    // The count, so the pass carries its own limit. This branch measures nothing, and saying
    // `0 of 8` beside the prose is the difference between a reader inferring that and being told.
    //
    // [`Attributed::Nothing`] is stated here rather than arriving from the caller. It USED to be
    // honest by accident: `plan`'s inseparable arm hands over a `Coverage::of(&[], ..)`, whose
    // numerator is zero by construction, so the ratio and the zero numerator were the same string
    // and a caller passing a populated `Coverage` would have printed a claim about runs that never
    // happened.
    lines.push(format!("  measured:  {}", coverage.measured(Attributed::Nothing)));
    lines.extend(unmeasured_lines(coverage));
    lines.extend(unreverted_lines(build_inputs));
    lines.push(String::new());
    lines.push(String::from(
        "Rust keeps unit tests beside the code they test, so reverting the",
    ));
    lines.push(String::from(
        "implementation would remove the test too. State the evidence in the",
    ));
    lines.push(String::from(
        "handoff instead: the command you ran, the failure before the fix, and",
    ));
    lines.push(String::from(
        "the pass after. This gate has NOT verified causality for this change.",
    ));
    if claim_declared {
        lines.push(String::new());
        lines.push(String::from(
            "A `Claim-Cell:` declaration is present in this range and was NOT consulted: this",
        ));
        lines.push(String::from(
            "arm returns before the claim arm is reached, so nothing here read it. It is not a",
        ));
        lines.push(String::from("verdict on the declaration - state the evidence above instead."));
    }
    lines
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
/// **ONE FUNCTION, THREE CALLERS, because one of them used to have none.** `separable.build_inputs`
/// was printed at exactly one place - inside `super::prove`'s block - and the tests-only arm that
/// returns BEFORE that block is precisely where a manifest is the whole implementation change:
/// `Cargo.toml` plus a test that needs the dependency it adds gives an empty `revert`, and that arm
/// answered *tests changed but no implementation did* at exit 0 without ever naming the manifest.
/// Naming it is all this does; what a manifest diff PUT INTO THE BUILD is `super::features`' own
/// question and its own refusal. `github.com/telekom/sutura#1016` moved that arm onto this same
/// printer directly (`causality::tests_only`) rather than leaving it a private copy of the same
/// list, which is the caller this paragraph used to call out as missing one.
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

/// Say which `remove:` file declares a test module whose own file is ALSO in this diff.
///
/// **THE DEFECT THIS NAMES.** `github.com/telekom/sutura#893`'s second finding: eight cells the
/// base run never even attempted were unmeasured for a structural reason nothing printed. The
/// parent is NEW here - `remove:` means the base tree has no version of it at all - so a
/// `#[cfg(test)] mod tests;` inside it has nothing to attach the child's tests to at base: not
/// held at HEAD, not restored, simply never compiled. That is detectable from the diff alone and
/// before either run: the parent is in `remove:`, [`accounted_for`] resolves its declaration to a
/// candidate file, and that candidate is itself in this diff.
///
/// **What this does NOT cover.** A parent in `restore:` - an EXISTING file whose base version may
/// already declare the same `mod`, so nothing here is orphaned by that reversion alone - and an
/// INLINE `mod tests { .. }`, whose body arrives with the parent and needs no second file to
/// exist. Both are silently skipped rather than guessed at.
pub(super) fn report_orphaned_modules(remove: &[&String], files: &[ChangedFile], read: &PostImage<'_>) {
    for line in orphaned_lines(remove, files, read) {
        println!("{line}");
    }
}

/// Every line [`report_orphaned_modules`] prints, in order.
///
/// PURE for the reason every other line in this module is: a mutation of the `println!` above
/// reddens nothing, and review is what holds it.
fn orphaned_lines(remove: &[&String], files: &[ChangedFile], read: &PostImage<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    for &path in remove {
        let Some(file) = files.iter().find(|f| &f.path == path) else {
            continue;
        };
        let Some(text) = read(path) else {
            continue;
        };
        let post: Vec<&str> = text.lines().collect();
        let Some(Declares::OutOfLine(candidates)) = accounted_for(file, &post) else {
            continue;
        };
        if let Some(child) = candidates
            .into_iter()
            .find(|candidate| files.iter().any(|f| &f.path == candidate))
        {
            lines.push(format!(
                "  orphaned:  {child}  (its `mod` is declared in {path}, which is new here - nothing to attach to at base)"
            ));
        }
    }
    lines
}

/// What the filterset left out, and what stopped the total being established.
///
/// Shared by the pre-run scope line and the inseparable verdict, because the LIST is the same
/// question in both and only the numerator's claim differs. One place, so a reader gets the names
/// whichever line they are reading.
fn unmeasured_lines(coverage: &Coverage) -> Vec<String> {
    let mut lines = Vec::new();
    for name in coverage.unmeasured() {
        // NOT "its file is kept at HEAD" - `github.com/telekom/sutura#893` found that claim false
        // for a file `plan` pulls into `restore:` anyway, when a scoped test elsewhere reaches
        // it: the file IS reverted, only this name never entered the filter that reversion is
        // measured against. `Coverage` only ever checks scope membership, never file state, so
        // the sentence says exactly that and nothing plan's later reach analysis could contradict.
        lines.push(format!(
            "    not measured: {name}  (never named into the scope filter, so it has no base result to compare)"
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

#[cfg(test)]
mod tests {
    use super::{
        Coverage, Ident, Moved, Verdict, moved_lines, no_base_behaviour, not_separable_lines, orphaned_lines,
        report_no_base_behaviour, report_not_separable, report_only_ignored, scope_lines, unreverted_lines,
    };
    use crate::causality::fixtures::{changed, tree};

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
    fn each_pass_answer_keeps_the_direction_its_own_doc_argues_for() {
        // THE ONE MECHANICAL THING IN THIS MODULE, and it had no test at all - which matters more
        // here than elsewhere, because the module's whole argument is that the direction is
        // decided PER ANSWER and for a different reason each time. Nothing captures stdout in this
        // venue, so the sentences stay held by review; the direction does not have to be.
        //
        // `super::refusals::tests` holds the FAIL half of this same argument now, split there at
        // the 1000-line cap - each answer in EITHER file still keeps its own direction for its own
        // reason, not because the two files agree.
        //
        // Three PASS, and each for its own reason: an inseparable file is usually legitimate work,
        // a changed file that is wholly new here has no old behaviour to compile, and an
        // all-ignored diff has nothing this venue can execute. Flipping any of them to a failure
        // reddens correct work, which is how a gate gets disabled.
        //
        // A TESTS-ONLY branch (`Separable::revert` empty) used to be a fourth PASS here
        // unconditionally - `report_nothing_to_revert`, deleted by `github.com/telekom/sutura#1016`
        // - and is not one any more: `causality::tests_only` refuses an undeclared addition now,
        // so that shape moved to `super::refusals::report_unclaimed_additions` and is no longer
        // this module's to hold.
        let inseparable = vec![String::from("crates/x/src/a.rs")];
        assert_eq!(report_not_separable(&inseparable, &nothing_of(8), &[], false), Verdict::Pass);
        let new_file = String::from("crates/x/src/new.rs");
        assert_eq!(
            report_no_base_behaviour("origin/main", &[&new_file], &nothing_of(2), &[]),
            Verdict::Pass
        );
        let ignored = [Ident::parse("acceptance").expect("an identifier")];
        assert_eq!(report_only_ignored(&ignored, &only_ignored(&["acceptance"])), Verdict::Pass);
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
    fn unreverted_lines_names_a_build_input_and_why_it_is_held() {
        // THE ONLY DIRECT COVERAGE OF THIS SHARED PRINTER: `report_nothing_to_revert` used to be
        // the one arm that exercised it with a non-empty list (#343's exact shape - a
        // manifest-only implementation change - and the one arm where the naming was the point),
        // and `github.com/telekom/sutura#1016` deleted that arm along with its own test. The
        // behaviour it pinned still has three callers (`not_separable_lines`, `no_base_behaviour`,
        // `report_unreverted`), so it keeps a test of its own rather than losing coverage with the
        // arm.
        let lines = unreverted_lines(&[String::from("crates/x/Cargo.toml")]);
        assert!(
            lines.iter().any(|line| line.contains("not reverted: crates/x/Cargo.toml")),
            "names the file it could not revert: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("changes what cargo RESOLVES")),
            "and says why it is held rather than reverted: {lines:?}"
        );
        assert_eq!(
            unreverted_lines(&[]),
            Vec::<String>::new(),
            "nothing to name: nothing printed"
        );
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

    #[test]
    fn a_remove_files_out_of_line_module_orphans_its_own_child_file() {
        // #893's second defect: the parent is NEW here (`remove:`), so its `mod tests;` has
        // nothing to attach to at base - the child never compiles there, whatever the base run's
        // filter names. Detectable from the diff alone, with no run at all.
        let parent = changed("xtask/src/fuzz/hook_paths.rs", 1, &["#[cfg(test)]", "mod tests;"]);
        let child = changed("xtask/src/fuzz/hook_paths/tests.rs", 1, &["#[test]", "fn a_new_cell() {}"]);
        let path = String::from("xtask/src/fuzz/hook_paths.rs");
        let files = vec![parent, child];
        let read = tree(&[
            ("xtask/src/fuzz/hook_paths.rs", "#[cfg(test)]\nmod tests;\n"),
            ("xtask/src/fuzz/hook_paths/tests.rs", "#[test]\nfn a_new_cell() {}\n"),
        ]);
        let lines = orphaned_lines(&[&path], &files, &read);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("xtask/src/fuzz/hook_paths/tests.rs"), "{lines:?}");
        assert!(lines[0].contains("xtask/src/fuzz/hook_paths.rs"), "{lines:?}");
    }

    #[test]
    fn a_remove_file_whose_declared_module_is_not_in_the_diff_orphans_nothing() {
        // The child is not part of this diff at all - an ordinary `mod tests;` reaching a file
        // this branch never touched - so there is nothing here to name as orphaned.
        let parent = changed("crates/x/src/a.rs", 1, &["#[cfg(test)]", "mod tests;"]);
        let path = String::from("crates/x/src/a.rs");
        let files = vec![parent];
        let read = tree(&[("crates/x/src/a.rs", "#[cfg(test)]\nmod tests;\n")]);
        assert_eq!(orphaned_lines(&[&path], &files, &read), Vec::<String>::new());
    }

    #[test]
    fn an_inline_test_module_in_a_removed_file_orphans_nothing() {
        // The body arrives WITH the parent, so there is no second file to be missing at base.
        let parent = changed(
            "crates/x/src/a.rs",
            1,
            &["#[cfg(test)]", "mod tests {", "    #[test]", "    fn t() {}", "}"],
        );
        let path = String::from("crates/x/src/a.rs");
        let files = vec![parent];
        let read = tree(&[(
            "crates/x/src/a.rs",
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
        )]);
        assert_eq!(orphaned_lines(&[&path], &files, &read), Vec::<String>::new());
    }

    #[test]
    fn an_inseparable_diff_says_a_declared_claim_was_not_consulted() {
        // #837 DIRECTION 4: this arm returns before `claim::Claim::of` is ever reached, so a
        // `Claim-Cell:` trailer committed alongside an inseparable diff used to disappear with no
        // trace - `NOT MECHANICALLY SEPARABLE` at exit 0, and nothing said the declaration existed.
        // PR #899 lived this: the trailer and its killing patch were both committed and the gate
        // never mentioned either.
        let inseparable = vec![String::from("crates/x/src/a.rs")];
        let declared = not_separable_lines(&inseparable, &nothing_of(8), &[], true);
        assert!(
            declared.iter().any(|line| line.contains("was NOT consulted")),
            "a declared trailer is named as unconsulted, not silently dropped: {declared:?}"
        );
        assert_eq!(report_not_separable(&inseparable, &nothing_of(8), &[], true), Verdict::Pass);
    }

    #[test]
    fn an_inseparable_diff_with_no_claim_says_nothing_about_one() {
        // THE REFUSAL'S OWN CELL, not just the predicate: with no trailer declared this arm must
        // stay silent about one, and the field it reads has to still be read - `dead_code` cannot
        // catch a `claim_declared` that is ignored, only a mutation that neutralises the branch can.
        let inseparable = vec![String::from("crates/x/src/a.rs")];
        let undeclared = not_separable_lines(&inseparable, &nothing_of(8), &[], false);
        assert!(
            !undeclared.iter().any(|line| line.contains("Claim-Cell")),
            "no trailer, no claim; the arm says nothing about one: {undeclared:?}"
        );
    }
}
