//! What the base run said, and WHICH test said it.
//!
//! THE DEFECT THIS REPLACES. `classify_base` accepted any assertion failure anywhere in the run
//! as red-by-assertion, so the verdict was "something was red" wearing the words "these tests
//! were red". Measured on a real branch: the base run failed on two tier-backed cells after 86 of
//! 1810 tests, nextest's fail-fast stopped there, the tests the diff added never ran, and the
//! gate printed *ok - red on base, green on head*. A control weaker than its claim, in the gate
//! whose whole job is to catch exactly that.
//!
//! SO A VERDICT NAMES WHAT REDDENED. Every failure the run reports is read back by name and
//! compared against the tests under test, through [`super::place::AddedTest::claims`] - the same
//! key the filter is built from, applied here by us instead of by nextest. That makes this a
//! second ENFORCER of one key rather than a second key, and the distinction is load-bearing: it
//! catches a filter that stopped filtering (a nextest version whose expression syntax moved, a
//! predicate this tree spells differently) and it cannot catch a key that is too weak. The first
//! version of this comparison was on the bare function name, which is not a key in this tree at
//! all - see [`super::scoped`] for the measurement - so a name-collided failure passed both
//! halves and the false green survived the fix that was supposed to remove it.
//!
//! SIX ANSWERS, NOT TWO. An unrelated base failure is neither red-by-assertion nor "the base
//! tree does not build", and calling it either one misleads in a different direction. It is
//! [`BaseOutcome::RedOutsideTheDiff`], and it FAILS: the gate's precondition - a base tree red
//! only where this diff put it - was not met, so it has no answer to give. That is the same
//! direction the HEAD half already takes when the suite is red there, and the symmetry is the
//! argument. [`BaseOutcome::NotRun`] is nextest matching none of the scoped tests, which is the
//! ORPHANING trap (a new test module whose `mod` declaration sits in a reverted file is never
//! compiled) and used to read as *green against base behaviour*. And
//! [`BaseOutcome::Unattributed`] is a run that got PAST the compiler and reported no failure this
//! can name: it used to be folded into `DidNotCompile` and printed *the base tree does not build*
//! about a tree that built fine, which is how a missing status word turned into a wrong sentence.
//! [`BaseOutcome::DidNotResolve`] is the mirror mistake, one step earlier: cargo failing to parse
//! the manifest or find a target never reaches the compiler at all, so it used to fall into
//! `Unattributed` and print *it got past the compiler* about a run that never got there -
//! `github.com/telekom/sutura#716`.
//!
//! AND A GREEN RUN IS FOUR ANSWERS, none of which this classifier could reach on its own.
//! *These tests pass without the change* is the defect the gate exists for **only if the diff added
//! them**, and a diff cannot tell an added test from a MOVED one - so moving a test into a new file,
//! which is what this repository's own guidance asks for when a file hits the line cap, produced
//! *FAILED - green against base behaviour* about a change with no defect in it.
//! [`BaseOutcome::GreenAfterAMove`] is the second answer and it is INCONCLUSIVE;
//! `super::provenance::Moved` is the input, because the base TREE is the only place that fact
//! lives.
//!
//! THE THIRD IS ABOUT THE OTHER HALF OF THAT PREMISE - what was REVERTED rather than what was
//! measured. [`BaseOutcome::GreenOverAnUnreachableRevert`] is also INCONCLUSIVE and
//! `super::reverted` is both the input and the sentence; that module carries the argument, the
//! case it deliberately leaves in reach, and why the defect this gate exists for still FAILS.
//!
//! THE FOURTH IS A MIX, AND IT USED TO COLLAPSE INTO THE FIRST. Some of the scope moved and the
//! rest is genuinely new in the same file - the shape a file hitting the line cap produces when
//! its harness carries both a relocated test and a new one. The moved names passing tells you
//! nothing; but the new names cannot have a base-side function to have run at all, so a whole-run
//! `succeeded` came from the moved subset alone and the new tests were never measured, not proven
//! green. [`BaseOutcome::GreenAfterAPartialMove`] says so and still FAILS -
//! `github.com/telekom/sutura#775`: the old answer here was [`BaseOutcome::Green`], which is the
//! right diagnosis only for [`super::provenance::Moved::Nothing`] and told a reader to delete a
//! test that was never run rather than to fix its wiring.
//!
//! AND NEITHER INCONCLUSIVE ANSWER IS A PASS ANY MORE, which is `github.com/telekom/sutura#307`.
//! Both returned [`crate::Verdict::Pass`] - exit 0 - so a required CI step whose whole input is the exit
//! code read *measured nothing* exactly as it reads *red on base, green on head*. Measured twice on
//! finished branches: one green `ci` step over `the base tree does not build` (a `-D dead-code`
//! error) while the same commit refused locally, and one branch-level `INCONCLUSIVE` because a
//! changed public signature left the held-at-HEAD test files unable to compile against base. They
//! return [`crate::Verdict::Inconclusive`] now, whose exit code is 3, so the default for any consumer is
//! CLOSED and a venue that means to continue over it has to say so. Failing was weighed and
//! rejected: the harness move lands on `DidNotCompile` every time and a gate that reddens correct
//! work gets disabled. What each venue does with the 3 is in `ci.yml` and `devenv.nix`.
//!
//! WHAT THE SENTENCE BESIDE IT MAY CLAIM, the same defect one level down. Both arms printed
//! `Coverage::ratio()` - `6 of 6 added tests measured` on a run where the base tree never built, so
//! the six were NAMED by the filterset and not one of them ran. [`earned`] is the choice, in one
//! place and pure, because it was made at the call site and pinned by review only.

use crate::causality::coverage::{Attributed, Coverage};
use crate::causality::place::AddedTest;
use crate::causality::provenance::Moved;
use crate::causality::reverted::{Excused, Reverted};

/// Evidence that a base run REPORTED PER-TEST RESULTS, and therefore that the filterset's names
/// are also what was measured.
///
/// **A sealed witness: the field is private to this module, so nothing outside it can build one.**
/// That is the mechanism, and #307's fix did not have it. [`earned`] was pure, in one place and
/// under test - and none of that reached `super::prove`, which prints before either run and had
/// `Coverage::ratio` in reach, so an inconclusive run carried `N of N added tests measured` twenty
/// lines above its own `0 of N`. A non-zero measured numerator now requires one of these, this
/// module is the only place that mints one, and it mints one from the [`BaseOutcome`] variants that
/// reported per-test results - every one of which exists only after a base run has been classified. **So the overstating
/// direction does not compile**; the understating one - printing the zero numerator before the runs
/// - is held by `super::remedies`' `the_line_printed_before_either_run_claims_no_measurement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PerTestResults(());

/// What the base run actually told us.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BaseOutcome {
    /// The tests under test passed without the change: they do not test it.
    Green,
    /// They passed, and the base tree ALREADY HAD every one of them. Not a defect: a test that
    /// moved was not added behaviourally, so there is no old behaviour it could have been red
    /// against. `super::provenance::Moved` is the classification and carries the direction it
    /// fails in; this variant is why *green against base behaviour* no longer blames the refactor
    /// this repository's own guidance asks for when a file hits the line cap.
    GreenAfterAMove { moved: Vec<String> },
    /// Some of them moved, the rest is genuinely new in the same scope, and this is NOT the same
    /// answer as [`Self::Green`]. `moved` passing tells you nothing; the rest cannot have a
    /// function of that name at base to have run at all, so a whole-run `succeeded` came only
    /// from the moved subset. Still FAILS, because the new subset is unmeasured rather than
    /// proven green - the diagnosis a reader needs is "fix the wiring", not "delete the test".
    GreenAfterAPartialMove { moved: Vec<String>, unmeasured: Vec<String> },
    /// They passed, and NOTHING THAT WAS REVERTED could have made them fail. Not a defect either:
    /// a green run over a partition that restores no line the tests in scope can execute is a
    /// fact about the partition. `super::reverted::Reverted` is the classification and its header
    /// carries the two arguments and what neither reaches.
    GreenOverAnUnreachableRevert { excused: Vec<Excused> },
    /// A test the diff added failed an assertion. The evidence the gate exists to collect, and
    /// it carries the names so the printed verdict says what it measured.
    RedByAssertion { failed: Vec<String> },
    /// The base run failed, and none of the failures is a test this diff added. Red, and about
    /// something else.
    RedOutsideTheDiff { failed: Vec<String> },
    /// nextest matched none of the tests under test, so nothing was measured.
    NotRun,
    /// Red past the compiler, with no failure any line attributes to a test.
    Unattributed,
    /// Cargo could not resolve the manifest, so the compiler never ran at all. Distinct from
    /// [`Self::DidNotCompile`]: that is a build that failed, this is a build that was never
    /// attempted, and the two sentences are not interchangeable.
    DidNotResolve,
    /// The tree did not build. Red, but it proves nothing about behaviour.
    DidNotCompile,
}

/// Classify a base test run against the tests the diff added.
///
/// Order matters twice. A tree that did not build often ALSO prints "error: test run failed", so
/// the compile check comes first or a build failure reads as a real red - that was this gate's
/// first false green. And a run that matched no test prints no failure at all, so it is settled
/// before the failure list is consulted rather than falling through as "unknown".
///
/// A GREEN RUN IS FOUR ANSWERS, and telling them apart takes two inputs the run does not carry.
/// The premise - *a test the diff ADDED must be red against the base behaviour* - has a subject
/// and an object. `moved` settles the subject: were these tests added here. `reverted` settles the
/// object: is what was put back something they could have executed. A SCOPE moved is
/// [`BaseOutcome::GreenAfterAMove`]; a scope PARTLY moved is [`BaseOutcome::GreenAfterAPartialMove`]
/// and stays a failure, but the report now says why: the moved names are not the defect and the
/// rest never had a base-side function to have run at all, so `succeeded` came only from the
/// moved subset and nothing measured the new one.
///
/// **THE ORDER IS A MESSAGE AND NOT A VERDICT.** Both non-defect answers are
/// [`crate::Verdict::Inconclusive`], so a diff that is both exits 3 either way; `moved` goes first
/// because *these tests were not added here* asks for less.
pub(crate) fn classify_base(
    text: &str,
    succeeded: bool,
    scoped: &[AddedTest],
    moved: &Moved,
    reverted: &Reverted,
) -> BaseOutcome {
    if succeeded {
        return match (moved, reverted) {
            (Moved::Wholly(names), _) => BaseOutcome::GreenAfterAMove { moved: names.clone() },
            // The witness is spent HERE, and the limit is worth the line: what it held is that
            // every reverted file earned an excuse, which is the decision. From this point on the
            // list is a message, and a narrowing of the message prints fewer lines than the
            // decision covered - visible, and not the classification `#414` was about.
            (_, Reverted::Nothing(excused)) => BaseOutcome::GreenOverAnUnreachableRevert {
                excused: excused.outcomes().to_vec(),
            },
            // `Moved::Partly` names the subset the base tree already had; the rest of `scoped`
            // is genuinely new and has no base-side function to have run at all, so it is
            // `unmeasured` rather than folded into the same `Green` this diff exists to catch.
            (Moved::Partly(names), Reverted::Behaviour) => BaseOutcome::GreenAfterAPartialMove {
                moved: names.clone(),
                unmeasured: scoped
                    .iter()
                    .map(AddedTest::name)
                    .filter(|name| !names.iter().any(|one| one == name))
                    .map(String::from)
                    .collect(),
            },
            (Moved::Nothing, Reverted::Behaviour) => BaseOutcome::Green,
        };
    }
    // Checked before `did_not_compile`: a manifest cargo cannot resolve never reaches the
    // compiler, so it is not that outcome's build failure either.
    if failed_to_resolve(text) {
        return BaseOutcome::DidNotResolve;
    }
    if did_not_compile(text) {
        return BaseOutcome::DidNotCompile;
    }
    if names_no_tests(text) {
        return BaseOutcome::NotRun;
    }
    let failed = failures(text);
    if failed.is_empty() {
        // Red, and about nothing this can name: do not claim a proof, and do not blame the build.
        return BaseOutcome::Unattributed;
    }
    let (under_test, outside) = failed.into_iter().partition::<Vec<String>, _>(|one| is_scoped(one, scoped));
    if under_test.is_empty() {
        BaseOutcome::RedOutsideTheDiff { failed: outside }
    } else {
        BaseOutcome::RedByAssertion { failed: under_test }
    }
}

/// Did cargo fail to resolve the manifest, never reaching the compiler?
fn failed_to_resolve(text: &str) -> bool {
    text.contains("failed to parse manifest") || text.contains("no targets specified in the manifest")
}

/// Did the tree fail to build?
fn did_not_compile(text: &str) -> bool {
    text.contains("could not compile")
        || text.contains("error[E")
        || text.contains("error: cannot find")
        || text.contains("unresolved import")
}

/// Did the filter match nothing?
///
/// nextest's `--no-tests` defaults to failing, which is what makes a scoped run fail CLOSED when
/// the tests it named are absent: measured on 0.9.143, a filter matching nothing prints this and
/// exits 4 rather than reporting a green run over zero tests.
pub(crate) fn names_no_tests(text: &str) -> bool {
    text.contains("error: no tests to run")
}

/// The failing tests a run reported.
///
/// nextest's wording first, then `cargo test`'s, so the classifier survives a runner swap - and
/// only one of the two, because nextest prints the inner `cargo test` line as well when it shows
/// a failure's captured output, and reading both would report one failure twice.
fn failures(text: &str) -> Vec<String> {
    let reported = collect(text, nextest_failure);
    if reported.is_empty() {
        return collect(text, cargo_test_failure);
    }
    reported
}

/// One runner's wording: a printed line in, the failure it names out.
type ReadFailure = fn(&str) -> Option<String>;

/// Every distinct failure `read` finds, in the order the run printed them.
fn collect(text: &str, read: ReadFailure) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for named in text.lines().filter_map(|line| read(line.trim())) {
        if !found.contains(&named) {
            found.push(named);
        }
    }
    found
}

/// The non-signal statuses nextest prints for a test that did not pass, as of 0.9.143.
///
/// `ABORT` is the WINDOWS spelling and never matches on this platform, which is the whole reason
/// this list is measured rather than read off a status table: it was carried alone while an
/// aborting base parsed to zero failures. Measured side by side on 0.9.143:
///
/// ```text
///     SIGSEGV [   0.282s] (3/4) pa tests::segfaults
///     SIGABRT [   0.265s] (2/4) pa tests::aborts
///        FAIL [   0.255s] (1/4) pa tests::plain_fail
///     TIMEOUT [   1.005s] (4/4) pa tests::hangs
/// ```
///
/// The two this tree cannot provoke on demand - `FAIL + LEAK` and `LEAK-FAIL`, one for a failing
/// test that also leaked and one for a leak the profile fails on - are read off the pinned
/// binary's own string table rather than guessed, alongside `ABORT` and `XFAIL`. `XFAIL` is
/// deliberately absent: an expected failure that failed is a PASS.
const FAILING_STATUSES: &[&str] = &["FAIL", "FAIL + LEAK", "TIMEOUT", "ABORT", "LEAK-FAIL"];

/// Is this status one that a test did not pass under?
///
/// Abnormal termination is one status PER SIGNAL, so the rule for those is the SHAPE rather than
/// a list that goes stale on the next platform or the next signal. `PASS`, `LEAK` and `XFAIL` are
/// the statuses 0.9.143 prints that are not failures, and the direction of a status this does not
/// recognise is [`BaseOutcome::Unattributed`] - no proof claimed - rather than a red attributed
/// to whichever test the line happened to name.
fn is_failing(status: &str) -> bool {
    status.starts_with("SIG") || FAILING_STATUSES.contains(&status)
}

/// `FAIL [   0.183s] (1/1) a::unrelated needs_state` -> `a::unrelated needs_state`.
///
/// The status is everything before the ` [` that opens the duration, so a two-word one like
/// `FAIL + LEAK` is read whole rather than matched by its first letters. The `(n/m)` counter is
/// dropped: it is a property of the run's ordering rather than of the test, so keeping it would
/// make one failure in two runs look like two failures.
fn nextest_failure(trimmed: &str) -> Option<String> {
    let (status, rest) = strip_retry(trimmed).split_once(" [")?;
    if !is_failing(status) {
        return None;
    }
    let (_, named) = rest.split_once(']')?;
    let words: Vec<&str> = named.split_whitespace().filter(|word| !is_progress(word)).collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// `TRY 2 FAIL [ .. ]` is a retry of one test; the retry count is not part of its name.
fn strip_retry(trimmed: &str) -> &str {
    trimmed.strip_prefix("TRY ").map_or(trimmed, |rest| {
        rest.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start()
    })
}

/// `(3/17)` - nextest's progress counter, which no test name can look like.
fn is_progress(word: &str) -> bool {
    word.starts_with('(') && word.ends_with(')') && word.contains('/')
}

/// `test needs_state ... FAILED` - the `cargo test` wording.
fn cargo_test_failure(trimmed: &str) -> Option<String> {
    let named = trimmed.strip_prefix("test ")?.strip_suffix(" ... FAILED")?;
    (!named.is_empty()).then(|| String::from(named))
}

/// Does this failure name a test the diff added?
///
/// nextest prints `<binary id> <test path>` and `cargo test` prints a bare `<test path>`, so the
/// last two words are the key's two halves and a one-word failure supplies only the second.
/// [`AddedTest::claims`] holds the comparison itself, next to the filter it mirrors - one place
/// for the key, because two spellings of it is how the filter and the check came to disagree.
fn is_scoped(failure: &str, scoped: &[AddedTest]) -> bool {
    let mut words = failure.split_whitespace().rev();
    let Some(path) = words.next() else {
        return false;
    };
    let binary_id = words.next();
    scoped.iter().any(|one| one.claims(binary_id, path))
}

/// Which coverage wording `outcome` has EARNED.
///
/// A numerator is what the filterset NAMED, and it equals what was measured only for a base run
/// that produced a test result. Two outcomes produced none - a tree that did not build ran nothing
/// at all, and a run no line attributes to a test attributes nothing - so `6 of 6 added tests
/// measured` beside either one is a claim about six runs that did not happen. That is the wording
/// half of #307, and it was one `.ratio()` at the call site: pure and in one place here, so a test
/// can read the CHOICE rather than a synthetic [`Coverage`].
///
/// Six of the ten print it: [`BaseOutcome::RedByAssertion`] in its verdict line, all three
/// always-inconclusive arms in theirs ([`BaseOutcome::Unattributed`], [`BaseOutcome::DidNotResolve`],
/// [`BaseOutcome::DidNotCompile`]), and the two green arms that are not the defect
/// ([`BaseOutcome::GreenAfterAMove`], [`BaseOutcome::GreenOverAnUnreachableRevert`]) - whose tests
/// DID run in both trees, so the measurement is real there and what it is not is evidence about
/// the change. The other four are asserted anyway, because the mapping is what a mutation moves
/// and an arm that starts printing must not have to re-derive it -
/// [`BaseOutcome::GreenAfterAPartialMove`] joins them rather than the six: its moved subset ran,
/// but the scope it is about includes tests that did not, so the numerator stays understated.
///
/// **AND IT IS NOW THE ONLY PLACE THAT CAN MINT [`PerTestResults`]**, which is the half review found
/// missing: being pure and in one place did not stop a caller with no outcome at all printing the
/// ratio, and `super::prove` did, ahead of the head run. `Coverage`'s measured wording takes the
/// evidence rather than a preference, so a numerator no run earned does not compile.
fn earned(outcome: &BaseOutcome, coverage: &Coverage) -> String {
    coverage.measured(match *outcome {
        // Both runs happened and reported per-test results, so the filterset's names are also
        // what was measured. `PerTestResults(())` is spellable here and nowhere else.
        BaseOutcome::Green
        | BaseOutcome::GreenAfterAMove { .. }
        | BaseOutcome::GreenOverAnUnreachableRevert { .. }
        | BaseOutcome::RedByAssertion { .. } => Attributed::PerTest(PerTestResults(())),
        // Nothing this diff added was measured on base: the tree did not build, the filter matched
        // none of them, no failure was attributable, or the run stopped on something else. A
        // partial move is here too, understated on purpose: its moved subset did run, but the
        // scope this outcome is about is the one that did not, and this module's own rule is that
        // the overstating direction must not compile.
        BaseOutcome::RedOutsideTheDiff { .. }
        | BaseOutcome::NotRun
        | BaseOutcome::Unattributed
        | BaseOutcome::DidNotResolve
        | BaseOutcome::DidNotCompile
        | BaseOutcome::GreenAfterAPartialMove { .. } => Attributed::Nothing,
    })
}

// Split at the unexemptable 1000-line cap: `report` owns turning a classified `BaseOutcome` into
// the gate's verdict and printed explanation, this module owns classifying one. See its own
// header for why that seam and not a `part2`.
mod report;
pub(crate) use report::{report_base, tail};

#[cfg(test)]
mod tests {
    use super::{BaseOutcome, Moved, Reverted, classify_base, earned, names_no_tests, report_base};
    use crate::Verdict;
    use crate::causality::fixtures::{UNRELATED_RED, audit_record, named, scoped};
    use crate::causality::place::AddedTest;

    /// The classifier over a scope whose every test is NEW here and a revert that reaches them.
    ///
    /// A wrapper, because two inputs change what exactly one answer means - a GREEN base run - and
    /// every assertion below is about the other six. Their own arms are
    /// `a_scope_the_base_tree_already_had_is_not_a_defect` and
    /// `a_green_run_over_a_revert_nothing_in_scope_could_read_is_not_that_defect`, both of which
    /// call the real function.
    fn classified(text: &str, succeeded: bool, scoped: &[AddedTest]) -> BaseOutcome {
        classify_base(text, succeeded, scoped, &Moved::Nothing, &Reverted::Behaviour)
    }

    #[test]
    fn a_base_failure_outside_the_diff_is_not_reported_as_red_by_assertion() {
        // THE DEFECT. Both failing cells are tier-backed cells this diff did not touch, and the
        // test it did add - the audit record - never ran. The old classifier saw "FAIL [" and
        // answered red-by-assertion, which the gate printed as `ok - red on base, green on head`.
        let outcome = classified(UNRELATED_RED, false, &audit_record());
        match outcome {
            BaseOutcome::RedOutsideTheDiff { ref failed } => {
                assert_eq!(failed.len(), 2, "both cells are named: {failed:?}");
                assert!(
                    failed.iter().all(|one| one.contains("tests::postgres::")),
                    "the verdict names what reddened: {failed:?}"
                );
            }
            other => panic!("an unrelated red is not evidence, got {other:?}"),
        }
    }

    #[test]
    fn a_failed_test_the_diff_added_is_the_evidence_wanted() {
        // The other direction, which matters as much: a genuinely causal change still passes.
        // This is the real base red from #276, with the same run around it.
        let text = concat!(
            "    Starting 3 tests across 47 binaries\n",
            "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::a_refused_question_is_recorded_and_names_the_refusal\n",
            "     Summary [   0.4s] 2 tests run: 1 passed, 1 failed, 0 skipped\n",
            "error: test run failed\n",
        );
        assert_eq!(
            classified(text, false, &audit_record()),
            BaseOutcome::RedByAssertion {
                failed: vec![String::from(
                    "sutura-cli::bin/sutura audit::tests::a_refused_question_is_recorded_and_names_the_refusal"
                )]
            }
        );
    }

    #[test]
    fn a_scoped_failure_among_unrelated_ones_is_still_the_evidence() {
        // Only the scoped failures are carried into the verdict: the printed line has to be about
        // the change, and the operator gets the run's own tail underneath it for the rest.
        let text = concat!(
            "        FAIL [   0.313s] (86/1810) sutura-app::differential tests::postgres::sums_by_month\n",
            "        FAIL [   0.021s] (87/1810) sutura-cli::bin/sutura audit::tests::the_added_one\n",
            "error: test run failed\n",
        );
        let under_test = scoped("sutura-cli", "crates/sutura-cli/src/audit.rs", &["the_added_one"]);
        match classified(text, false, &under_test) {
            BaseOutcome::RedByAssertion { ref failed } => {
                assert_eq!(failed.len(), 1, "only the test under test: {failed:?}");
                assert!(failed.iter().all(|one| one.ends_with("audit::tests::the_added_one")));
            }
            other => panic!("expected RedByAssertion, got {other:?}"),
        }
    }

    #[test]
    fn a_failure_in_another_package_is_never_this_diffs_test() {
        // THE FINDING THIS CLOSES. With the bare function name as the key, a failure anywhere in
        // the workspace that happened to share a name was accepted - so a vacuous added test got
        // `ok - red on base, green on head` on the strength of a pre-existing test in another
        // crate. Both lines below name `sums`; neither is in `pa`.
        let text = concat!(
            "        FAIL [   0.313s] (1/6) pb other::sums\n",
            "        FAIL [   0.204s] (2/6) pb sums::inner\n",
            "error: test run failed\n",
        );
        match classified(text, false, &scoped("pa", "pa/src/lib.rs", &["sums"])) {
            BaseOutcome::RedOutsideTheDiff { ref failed } => assert_eq!(failed.len(), 2, "{failed:?}"),
            other => panic!("another package's failure is not evidence, got {other:?}"),
        }
    }

    #[test]
    fn a_failure_in_a_sibling_module_of_the_same_package_is_not_this_diffs_test() {
        // The same collision inside ONE binary, which no package or binary qualifier separates:
        // `deserialization_goes_through_the_constructor` occurs four times in `sutura-domain`'s
        // own lib. The module path the file contributes is what tells them apart.
        let text = concat!(
            "        FAIL [   0.010s] (1/4) sutura-domain model::tests::deserialization_goes_through_the_constructor\n",
            "error: test run failed\n",
        );
        let under_test = scoped(
            "sutura-domain",
            "crates/sutura-domain/src/calendar.rs",
            &["deserialization_goes_through_the_constructor"],
        );
        match classified(text, false, &under_test) {
            BaseOutcome::RedOutsideTheDiff { .. } => {}
            other => panic!("a sibling module's failure is not evidence, got {other:?}"),
        }
    }

    #[test]
    fn a_parametrised_case_belongs_to_the_test_that_declared_it() {
        // `rstest` names a case one segment BELOW the function, so a comparison on the last
        // segment alone would call every case an unrelated failure.
        let text = "        FAIL [   0.010s] (1/4) sutura-app::golden tests::renders::case_2\nerror: test run failed\n";
        match classified(
            text,
            false,
            &scoped("sutura-app", "crates/sutura-app/tests/golden.rs", &["renders"]),
        ) {
            BaseOutcome::RedByAssertion { .. } => {}
            other => panic!("a case is its function's failure, got {other:?}"),
        }
    }

    #[test]
    fn a_test_whose_name_merely_contains_a_scoped_one_is_outside_the_diff() {
        // The segment comparison is exact. A substring one would read `sums_by_month` as the
        // added `sums`, which is the generous direction that produced the false green.
        let text =
            "        FAIL [   0.313s] (1/9) sutura-app::differential tests::postgres::sums_by_month\nerror: test run failed\n";
        match classified(
            text,
            false,
            &scoped("sutura-app", "crates/sutura-app/tests/differential.rs", &["sums"]),
        ) {
            BaseOutcome::RedOutsideTheDiff { .. } => {}
            other => panic!("expected RedOutsideTheDiff, got {other:?}"),
        }
    }

    #[test]
    fn an_aborting_base_is_a_failure_rather_than_a_build_that_did_not_happen() {
        // THE FINDING THIS CLOSES. `ABORT [` is the Windows word; the pinned Unix nextest prints
        // `SIGABRT [`, so a base run whose only failure was an abort - a double panic, a panic in
        // `Drop`, a stack overflow - parsed to ZERO failures and printed *the base tree does not
        // build* about a tree that built fine. Every signal status is one word per signal, so the
        // rule is the shape.
        let text = concat!(
            "     SIGABRT [   0.265s] (1/2) sutura-domain calendar::tests::the_added_one\n",
            "     SIGSEGV [   0.282s] (2/2) sutura-domain calendar::tests::neighbour\n",
            "error: test run failed\n",
        );
        let under_test = scoped("sutura-domain", "crates/sutura-domain/src/calendar.rs", &["the_added_one"]);
        match classified(text, false, &under_test) {
            BaseOutcome::RedByAssertion { ref failed } => {
                assert_eq!(failed.len(), 1, "only the test under test: {failed:?}");
            }
            other => panic!("an abort is a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_leaking_failure_and_a_timeout_are_failures_and_a_pass_is_not() {
        // The status is everything before the ` [`, so a two-word one is read whole. `PASS`,
        // `LEAK` and `XFAIL` are the statuses 0.9.143 prints that are NOT failures, and reading
        // one as a failure would attribute a red to a test that passed.
        let failing = concat!(
            "     FAIL + LEAK [   0.010s] (1/3) pa tests::leaks\n",
            "     TIMEOUT [   1.005s] (2/3) pa tests::hangs\n",
            "error: test run failed\n",
        );
        match classified(failing, false, &scoped("pa", "pa/src/lib.rs", &["leaks", "hangs"])) {
            BaseOutcome::RedByAssertion { ref failed } => assert_eq!(failed.len(), 2, "{failed:?}"),
            other => panic!("expected both, got {other:?}"),
        }
        let passing = concat!(
            "        PASS [   0.010s] (1/2) pa tests::leaks\n",
            "        LEAK [   0.010s] (2/2) pa tests::hangs\n",
            "error: test run failed\n",
        );
        assert_eq!(
            classified(passing, false, &scoped("pa", "pa/src/lib.rs", &["leaks", "hangs"])),
            BaseOutcome::Unattributed
        );
    }

    #[test]
    fn a_base_that_did_not_compile_is_not_a_proof() {
        // The false green this replaced: `cargo test` failed, so the gate said "red, as
        // required" and passed. A tree that does not build has run no tests.
        let compile = "error[E0432]: unresolved import `crate::thing`\nerror: could not compile";
        let under_test = scoped("pa", "pa/src/lib.rs", &["t"]);
        assert_eq!(classified(compile, false, &under_test), BaseOutcome::DidNotCompile);
    }

    #[test]
    fn a_manifest_that_never_resolved_never_reached_the_compiler() {
        // #716. The classifier's four `did_not_compile` substrings never appear in a cargo
        // resolution failure, so this used to fall through to `Unattributed` and print "it got
        // past the compiler" about a run that never got there.
        let manifest = concat!(
            "error: failed to parse manifest at `/x/Cargo.toml`\n",
            "\n",
            "Caused by:\n",
            "  no targets specified in the manifest\n",
        );
        let under_test = scoped("pa", "pa/src/lib.rs", &["t"]);
        assert_eq!(classified(manifest, false, &under_test), BaseOutcome::DidNotResolve);
    }

    #[test]
    fn a_build_failure_wins_over_a_test_run_failure_line() {
        // nextest prints "error: test run failed" when the build failed too. Reading that as a
        // real red is the false green this gate already had once.
        let both = "error[E0433]: failed to resolve\nerror: could not compile\nerror: test run failed";
        let under_test = scoped("pa", "pa/src/lib.rs", &["t"]);
        assert_eq!(classified(both, false, &under_test), BaseOutcome::DidNotCompile);
    }

    #[test]
    fn a_filter_matching_nothing_is_not_a_pass_and_not_a_build_failure() {
        // The orphaning trap, and nextest's own wording for it - measured on 0.9.143, which exits
        // 4 rather than reporting a green run over zero tests.
        let text = concat!(
            "    Starting 0 tests across 47 binaries (1810 tests skipped)\n",
            "     Summary [   0.000s] 0 tests run: 0 passed, 1810 skipped\n",
            "error: no tests to run\n",
            "(hint: use `--no-tests` to customize)\n",
        );
        assert!(names_no_tests(text));
        let under_test = scoped("pa", "pa/src/lib.rs", &["orphaned"]);
        assert_eq!(classified(text, false, &under_test), BaseOutcome::NotRun);
    }

    #[test]
    fn the_cargo_test_wording_still_names_a_failure() {
        // So the classifier survives a runner swap. `cargo test` prints no binary id, so the
        // whole token is the test's path - and the coarse half of the key cannot be checked.
        let cargo = "running 3 tests\ntest suite::the_added_one ... FAILED\ntest result: FAILED. 2 passed; 1 failed";
        let under_test = scoped("pa", "pa/src/lib.rs", &["the_added_one"]);
        assert_eq!(
            classified(cargo, false, &under_test),
            BaseOutcome::RedByAssertion {
                failed: vec![String::from("suite::the_added_one")]
            }
        );
    }

    #[test]
    fn a_failure_no_line_names_claims_nothing() {
        // Deliberately conservative, and a CHANGE: this text used to be read as red-by-assertion
        // on the strength of the word "panicked", which attributes the red to nothing at all. It
        // is not a build failure either, and saying so was the wrong sentence.
        let unnamed = "thread 'x' panicked at src/lib.rs:9\ntest result: FAILED. 2 passed; 1 failed";
        let under_test = scoped("pa", "pa/src/lib.rs", &["the_added_one"]);
        assert_eq!(classified(unnamed, false, &under_test), BaseOutcome::Unattributed);
    }

    #[test]
    fn an_unrecognised_failure_claims_nothing() {
        // Conservative on purpose: an unfamiliar failure is not evidence of causality, and it is
        // not a compile failure just because nothing else fits.
        let under_test = scoped("pa", "pa/src/lib.rs", &["t"]);
        assert_eq!(
            classified("linker exited with signal 9", false, &under_test),
            BaseOutcome::Unattributed
        );
    }

    #[test]
    fn a_passing_base_means_the_test_does_not_test_the_change() {
        let under_test = scoped("pa", "pa/src/lib.rs", &["t"]);
        assert_eq!(
            classified("test result: ok. 12 passed; 0 failed", true, &under_test),
            BaseOutcome::Green
        );
    }

    #[test]
    fn a_scope_the_base_tree_already_had_is_not_a_defect() {
        // THE DEFECT this variant removes. Move a test into a new file - the refactor this
        // repository's own guidance asks for when a file hits the line cap - and the added lines
        // name a test whose subject nobody changed: green on base, and the gate printed
        // *FAILED - green against base behaviour* about a defect that does not exist. A green run
        // is two answers, and which one it is comes from an input the run does not carry.
        let under_test = scoped("pa", "pa/src/lib.rs", &["a_moved_assertion"]);
        let green = "test result: ok. 12 passed; 0 failed";
        let all = Moved::Wholly(vec![String::from("a_moved_assertion")]);
        assert_eq!(
            classify_base(green, true, &under_test, &all, &Reverted::Behaviour),
            BaseOutcome::GreenAfterAMove {
                moved: vec![String::from("a_moved_assertion")]
            }
        );
        // NEITHER a pass nor a failure: nothing was proven, and blaming the author would redden
        // correct work - the direction that gets a gate disabled.
        assert_eq!(
            report_base(
                &BaseOutcome::GreenAfterAMove {
                    moved: vec![String::from("a_moved_assertion")]
                },
                green,
                false,
                &named(1),
                &all,
                &Reverted::Behaviour
            ),
            Verdict::Inconclusive
        );

        // AND A MIX STILL FAILS, but #775 is why it may no longer say `Green`: one test moved,
        // one is genuinely new, and the new one has no base-side function to have run at all - it
        // was NEVER MEASURED, not "passed both ways". Folding it into `Green` told a reader to
        // delete a test that was never run.
        let mixed = Moved::Partly(vec![String::from("a_moved_assertion")]);
        let two = scoped("pa", "pa/src/lib.rs", &["a_moved_assertion", "genuinely_new"]);
        let partial = BaseOutcome::GreenAfterAPartialMove {
            moved: vec![String::from("a_moved_assertion")],
            unmeasured: vec![String::from("genuinely_new")],
        };
        assert_eq!(classify_base(green, true, &two, &mixed, &Reverted::Behaviour), partial);
        assert_eq!(
            report_base(&partial, green, false, &named(2), &mixed, &Reverted::Behaviour),
            Verdict::Fail
        );
    }

    #[test]
    fn a_run_that_measured_a_moved_test_measured_it() {
        // The wording half, and it is the opposite call from the inconclusive arms above: this
        // outcome's tests RAN, in both trees, and reported per-test results. What the run is not
        // is evidence about the change - which the verdict line says in words, next to a numerator
        // that is true. `Attributed::PerTest` is only mintable from an outcome, and this is one.
        assert_eq!(
            earned(
                &BaseOutcome::GreenAfterAMove {
                    moved: vec![String::from("a_moved_assertion")]
                },
                &named(6)
            ),
            "6 of 6 added tests measured"
        );
        // The same call for the same reason, on the arm `super::reverted` introduced: those tests
        // ran in both trees too, and what the run is not is evidence about the CHANGE.
        assert_eq!(
            earned(&BaseOutcome::GreenOverAnUnreachableRevert { excused: Vec::new() }, &named(6)),
            "6 of 6 added tests measured"
        );
    }

    #[test]
    fn a_retry_is_the_same_failure_once() {
        // nextest prints every attempt. Two lines for one test must not read as two failures.
        let text = concat!(
            "    TRY 1 FAIL [   0.010s] (1/2) sutura-serve::served serves::the_added_one\n",
            "    TRY 2 FAIL [   0.010s] (1/2) sutura-serve::served serves::the_added_one\n",
            "error: test run failed\n",
        );
        let under_test = scoped("sutura-serve", "crates/sutura-serve/tests/served.rs", &["the_added_one"]);
        match classified(text, false, &under_test) {
            BaseOutcome::RedByAssertion { ref failed } => assert_eq!(failed.len(), 1, "{failed:?}"),
            other => panic!("expected RedByAssertion, got {other:?}"),
        }
    }

    #[test]
    fn an_outcome_that_ran_nothing_has_earned_no_numerator() {
        // THE WORDING HALF of #307, and the mutation this exists to catch is one method call:
        // `6 of 6 added tests measured` beside *the base tree does not build* says six tests ran
        // against base, when the filterset merely NAMED six and the tree never compiled. The
        // synthetic `Coverage` here is the input, not the subject - what is asserted is the arm's
        // own choice of wording, which was made at the call site and pinned by review only.
        let six = named(6);
        assert_eq!(
            earned(&BaseOutcome::DidNotCompile, &six),
            "0 of 6 added tests measured",
            "a tree that did not build ran nothing"
        );
        assert_eq!(
            earned(&BaseOutcome::Unattributed, &six),
            "0 of 6 added tests measured",
            "a run with no attributable failure attributed nothing"
        );
        assert_eq!(
            earned(&BaseOutcome::DidNotResolve, &six),
            "0 of 6 added tests measured",
            "a manifest cargo never resolved ran nothing"
        );
        assert_eq!(earned(&BaseOutcome::NotRun, &six), "0 of 6 added tests measured");
        assert_eq!(
            earned(
                &BaseOutcome::RedOutsideTheDiff {
                    failed: vec![String::from("pb tests::other")]
                },
                &six
            ),
            "0 of 6 added tests measured"
        );

        // The two that DID produce per-test results keep the ratio: this must not become a gate
        // that under-reports every branch it does measure.
        assert_eq!(
            earned(
                &BaseOutcome::RedByAssertion {
                    failed: vec![String::from("pa tests::t")]
                },
                &six
            ),
            "6 of 6 added tests measured"
        );
        assert_eq!(earned(&BaseOutcome::Green, &six), "6 of 6 added tests measured");
    }
}
