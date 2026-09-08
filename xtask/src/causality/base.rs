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
//! compared against the tests under test, through [`super::scoped::AddedTest::claims`] - the same
//! key the filter is built from, applied here by us instead of by nextest. That makes this a
//! second ENFORCER of one key rather than a second key, and the distinction is load-bearing: it
//! catches a filter that stopped filtering (a nextest version whose expression syntax moved, a
//! predicate this tree spells differently) and it cannot catch a key that is too weak. The first
//! version of this comparison was on the bare function name, which is not a key in this tree at
//! all - see [`super::scoped`] for the measurement - so a name-collided failure passed both
//! halves and the false green survived the fix that was supposed to remove it.
//!
//! FIVE ANSWERS, NOT TWO. An unrelated base failure is neither red-by-assertion nor "the base
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
//!
//! AND A GREEN RUN IS THREE ANSWERS, none of which this classifier could reach on its own.
//! *These tests pass without the change* is the defect the gate exists for **only if the diff added
//! them**, and a diff cannot tell an added test from a MOVED one - so moving a test into a new file,
//! which is what this repository's own guidance asks for when a file hits the line cap, produced
//! *FAILED - green against base behaviour* about a change with no defect in it.
//! [`BaseOutcome::GreenAfterAMove`] is the second answer and it is INCONCLUSIVE;
//! `super::provenance::Moved` is the input, because the base TREE is the only place that fact
//! lives. A mix still fails: the tests that were new passed both ways.
//!
//! THE THIRD IS ABOUT THE OTHER HALF OF THAT PREMISE - what was REVERTED rather than what was
//! measured. [`BaseOutcome::GreenOverAnUnreachableRevert`] is also INCONCLUSIVE and
//! `super::reverted` is both the input and the sentence; that module carries the argument, the
//! case it deliberately leaves in reach, and why the defect this gate exists for still FAILS.
//!
//! AND NEITHER INCONCLUSIVE ANSWER IS A PASS ANY MORE, which is `github.com/telekom/sutura#307`.
//! Both returned [`Verdict::Pass`] - exit 0 - so a required CI step whose whole input is the exit
//! code read *measured nothing* exactly as it reads *red on base, green on head*. Measured twice on
//! finished branches: one green `ci` step over `the base tree does not build` (a `-D dead-code`
//! error) while the same commit refused locally, and one branch-level `INCONCLUSIVE` because a
//! changed public signature left the held-at-HEAD test files unable to compile against base. They
//! return [`Verdict::Inconclusive`] now, whose exit code is 3, so the default for any consumer is
//! CLOSED and a venue that means to continue over it has to say so. Failing was weighed and
//! rejected: the harness move lands on `DidNotCompile` every time and a gate that reddens correct
//! work gets disabled. What each venue does with the 3 is in `ci.yml` and `devenv.nix`.
//!
//! WHAT THE SENTENCE BESIDE IT MAY CLAIM, the same defect one level down. Both arms printed
//! `Coverage::ratio()` - `6 of 6 added tests measured` on a run where the base tree never built, so
//! the six were NAMED by the filterset and not one of them ran. [`earned`] is the choice, in one
//! place and pure, because it was made at the call site and pinned by review only.

use crate::Verdict;
use crate::causality::coverage::{Attributed, Coverage};
use crate::causality::place::AddedTest;
use crate::causality::provenance::Moved;
use crate::causality::reverted::{self, Excused, Reverted};

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
/// A GREEN RUN IS THREE ANSWERS, and telling them apart takes two inputs the run does not carry.
/// The premise - *a test the diff ADDED must be red against the base behaviour* - has a subject
/// and an object. `moved` settles the subject: were these tests added here. `reverted` settles the
/// object: is what was put back something they could have executed. Everything else `moved` says
/// is left to the report - with SOME of them moved the failure stands, the rest passed both ways.
///
/// **THE ORDER IS A MESSAGE AND NOT A VERDICT.** Both non-defect answers are
/// [`Verdict::Inconclusive`], so a diff that is both exits 3 either way; `moved` goes first
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
            (_, Reverted::Nothing(excused)) => BaseOutcome::GreenOverAnUnreachableRevert {
                excused: excused.clone(),
            },
            (Moved::Nothing | Moved::Partly(_), Reverted::Behaviour) => BaseOutcome::Green,
        };
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
/// Five of the eight print it: [`BaseOutcome::RedByAssertion`] in its verdict line, both
/// inconclusive arms in theirs, and the two green arms that are not the defect
/// ([`BaseOutcome::GreenAfterAMove`], [`BaseOutcome::GreenOverAnUnreachableRevert`]) - whose tests
/// DID run in both trees, so the measurement is real there and what it is not is evidence about
/// the change. The other three are asserted anyway, because the mapping is what a mutation moves
/// and an arm that starts printing must not have to re-derive it.
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
        // none of them, no failure was attributable, or the run stopped on something else.
        BaseOutcome::RedOutsideTheDiff { .. } | BaseOutcome::NotRun | BaseOutcome::Unattributed | BaseOutcome::DidNotCompile => {
            Attributed::Nothing
        }
    })
}

/// Turn a base run into the gate's verdict.
///
/// `retried` only changes what the operator is told: after a second attempt, "not separable at
/// file level" is no longer the likely explanation, because the tree WAS coherently at base.
///
/// **NO ARM HERE CITES ANOTHER ARM'S OUTPUT.** The retry branch used to say *the remedy is the same
/// as for the harness move below*, and the harness-move paragraph is the `else if` this branch
/// excludes - so the reader was pointed at lines that run never printed. It restates the remedy
/// instead, which costs two lines and contains no fact no code produced. This module's whole subject
/// is printed claims, so it is the last place to keep one.
///
/// The coverage sentence rides on the PASSING verdict specifically: that is the line a handoff
/// cites, and it read as a statement about the change while the run covered a subset of the tests
/// the branch added. [`earned`] decides which wording each outcome may print; `prove`'s own arms
/// print theirs before either run, where they are asking for something rather than reporting
/// coverage.
pub(crate) fn report_base(
    outcome: &BaseOutcome,
    output: &str,
    retried: bool,
    coverage: &Coverage,
    moved: &Moved,
    reverted: &Reverted,
) -> Verdict {
    let measured = earned(outcome, coverage);
    // ONE CALL SITE, and the DECISION beside it is pure. It was a loop in each red arm, and review
    // measured what that cost: deleting the one in `RedOutsideTheDiff` reddened nothing, because
    // every test of that arm goes through a wrapper passing `Reverted::Behaviour`. The choice of
    // which outcomes contradict is now [`contradicts`] and has its own assertions; the `println!`
    // itself is still uncovered, which is true of every printed line here and is why `remedies`
    // keeps its wording in pure functions.
    reverted::verdict::emit_contradiction(outcome, reverted, &mut reverted::verdict::to_stdout);
    match *outcome {
        BaseOutcome::Green => {
            eprintln!("xtask test-causality: FAILED - green against base behaviour");
            eprintln!();
            eprintln!("The tests this diff added pass with the implementation reverted, so they do");
            eprintln!("not test the change. Make the test exercise the new behaviour, or say plainly");
            eprintln!("that it is not a regression test.");
            // WHICH OF THEM THIS IS NOT ABOUT. A test the base tree already had cannot be red
            // against a behaviour nobody changed, so it is not the one to go and fix - and the
            // failure stands anyway, because the others passed both ways. When EVERY test in scope
            // was already there this arm is unreachable: that is `GreenAfterAMove`.
            for one in moved.names() {
                eprintln!("  already at base: {one}  (this failure is not about that one)");
            }
            Verdict::Fail
        }
        BaseOutcome::GreenAfterAMove { ref moved } => {
            println!("  base: green - and the base tree already had every test in scope");
            for one in moved {
                println!("    already at base: {one}");
            }
            println!();
            println!("xtask test-causality: INCONCLUSIVE - these tests were not ADDED here.");
            println!("Every test in scope exists at the base commit in a `.rs` file this diff also");
            println!("touched, which is what a MOVED test looks like from the base side - so passing");
            println!("there is not the defect this gate names, and reporting one would blame the");
            println!("refactor this repository asks for when a file hits the line cap.");
            println!("Two readings this cannot separate, so state which: if they moved, nothing is");
            println!("wrong with the change; if they are genuinely new and their names collide with");
            println!("something this diff also touched, they DO pass both ways - prove it by MUTATION.");
            println!("This exit is INCONCLUSIVE (code 3) rather than a pass, and {measured}.");
            Verdict::Inconclusive
        }
        // The SENTENCE is `super::reverted`'s, beside the rule that decides it: an excuse and the
        // words that explain it are one thing to keep true rather than two.
        BaseOutcome::GreenOverAnUnreachableRevert { ref excused } => {
            reverted::verdict::explain(excused, &measured, &mut reverted::verdict::to_stdout)
        }
        BaseOutcome::RedByAssertion { ref failed } => {
            println!("  base: red by assertion, as required");
            for one in failed {
                println!("    red on base: {one}");
            }
            println!("{}", tail(output, 12));
            println!("xtask test-causality: ok - red on base, green on head ({measured})");
            Verdict::Pass
        }
        BaseOutcome::RedOutsideTheDiff { ref failed } => {
            eprintln!("xtask test-causality: FAILED - the base tree is red outside this diff");
            for one in failed {
                eprintln!("    outside the diff: {one}");
            }
            eprintln!();
            eprintln!("Not one of those is a test this diff added, so the run says nothing about");
            eprintln!("whether the change is causal - and reading it as evidence is the defect this");
            eprintln!("check exists to stop. The base tree has to be green where the diff is not:");
            eprintln!("provision what those tests need, or fix them, and ask again.");
            Verdict::Fail
        }
        BaseOutcome::NotRun => {
            eprintln!("xtask test-causality: FAILED - the tests this diff added did not run on base");
            eprintln!("{}", tail(output, 8));
            eprintln!();
            eprintln!("nextest matched none of them in the reconstructed tree, so nothing was");
            eprintln!("measured. The usual cause is ORPHANING: a new test module whose `mod`");
            eprintln!("declaration lives in a file that was reverted is never compiled, and Rust");
            eprintln!("builds no unreferenced file. When a file with tests hits the line cap, move");
            eprintln!("the harness out of it and leave every assertion where it is.");
            Verdict::Fail
        }
        BaseOutcome::Unattributed => {
            println!("  base: red, and no line attributes it to a test");
            println!("{}", tail(output, 12));
            println!();
            println!("xtask test-causality: INCONCLUSIVE - the base run named no failure.");
            println!("It got past the compiler, so this is not a build failure: a runner that died");
            println!("before reporting, a linker signal, or a status this gate does not recognise.");
            println!("Nothing is proven either way - state the evidence in the handoff.");
            println!("This exit is INCONCLUSIVE (code 3) rather than a pass, and {measured}.");
            Verdict::Inconclusive
        }
        BaseOutcome::DidNotCompile => {
            println!("  base: did not compile");
            println!("{}", tail(output, 12));
            println!();
            println!("xtask test-causality: INCONCLUSIVE - the base tree does not build.");
            println!("That is red, but a test that never ran is not evidence about behaviour.");
            if retried {
                println!("This is the SECOND attempt: every changed file is at base here, so the");
                println!("build failure is in the changed tests themselves - they reference");
                println!("something this branch introduced, which is usually a public signature");
                println!("this change altered: a test file kept at HEAD cannot compile against the");
                println!("base implementation. Nothing is wrong with the change. Scope this gate");
                println!("PER COMMIT against the commit before the signature change, or prove by");
                println!("MUTATION.");
            } else if missing_module_file(output) {
                println!("A `mod` here points at a file the base tree does not have, which is the");
                println!("HARNESS MOVE shape and the EXPECTED answer to it: a file with no");
                println!("`#[test]` is revertible, the file declaring it is held for adding tests,");
                println!("so the declaration outlives its target. Nothing is wrong with the change.");
                println!("What it costs is the proof, so a MUTATION takes its place: break what");
                println!("each new test claims, one at a time, and paste the test that reddens.");
                println!("Scope that to a whole test binary, never a name pattern - a filter that");
                println!("omits the guarding test reports green and proves nothing.");
            } else {
                // MEASURED, and it is why this fork gained a third cause: driven end to end over a
                // diff whose only change was a public function's ARITY plus a test calling it, this
                // arm printed *not separable at file level* - the wrong fix, sending the author to
                // split a change that is already split. Nothing was held back there, so neither the
                // retry above nor `missing_module_file` could catch it.
                println!("Two causes reach here and they ask for different things. Either the change");
                println!("is not separable at file level - the test and what it needs arrived");
                println!("together - or this change altered a PUBLIC SIGNATURE that a test file kept");
                println!("at HEAD calls, so the base implementation cannot compile against it: look");
                println!("for E0061 or E0308 above naming one of your own items. For that second");
                println!("cause nothing is wrong with the change; scope this gate PER COMMIT against");
                println!("the commit before the signature change, or prove by MUTATION. Either way,");
                println!("state the evidence in the handoff.");
            }
            // WHY THIS IS NOT A PASS AND NOT A FAILURE. It returned `Verdict::Pass` - exit 0 - and
            // a required CI step reads the exit code and nothing else, so this shape was cited as
            // red-before-green evidence on a finished branch that had none. Failing instead is the
            // decision that was rejected: the HARNESS MOVE above and the changed-signature retry
            // both land here, both are legitimate, and a gate that reddens correct work gets
            // disabled. Exit 3 puts the choice in the venue with the default closed.
            println!("This exit is INCONCLUSIVE (code 3) rather than a pass, and {measured}:");
            println!("the base tree ran nothing, whatever the filterset was able to name.");
            Verdict::Inconclusive
        }
    }
}

/// Did the base tree fail because a module's FILE is not there?
///
/// `error[E0583]` is the HARNESS-MOVE shape, and it is the EXPECTED outcome of following this
/// gate's own advice rather than a mistake: when a test file hits the line cap the harness moves
/// out, a file with no `#[test]` is revertible while the file declaring `mod <harness>;` is held
/// for adding tests, so the base tree has a declaration whose target is gone. Seen on #119 and
/// again on #283. Telling that author "the change is not separable at file level" sends them to
/// the wrong fix, which is why the message forks here rather than reading the same for both.
fn missing_module_file(text: &str) -> bool {
    text.contains("[E0583]") || text.contains("file not found for module")
}

/// The last `n` lines, so a failure shows the assertion rather than the whole compile log.
pub(crate) fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines.get(start..).map_or_else(String::new, |rest| rest.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{
        BaseOutcome, Coverage, Moved, Reverted, Verdict, classify_base, earned, missing_module_file, names_no_tests, report_base,
        tail,
    };
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

    /// The report for a scope whose every test is new here, for the same reason.
    fn reported(outcome: &BaseOutcome, output: &str, retried: bool, coverage: &Coverage) -> Verdict {
        report_base(outcome, output, retried, coverage, &Moved::Nothing, &Reverted::Behaviour)
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
        // THE FINDING THIS CLOSES. `ABORT [` is the Windows word; Unix nextest 0.9.143 prints
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

        // AND A MIX STILL FAILS, which a blanket rule would have lost: one test moved, one is new,
        // and the new one passed both ways - which is exactly the defect this gate exists for.
        let mixed = Moved::Partly(vec![String::from("a_moved_assertion")]);
        let two = scoped("pa", "pa/src/lib.rs", &["a_moved_assertion", "genuinely_new"]);
        assert_eq!(
            classify_base(green, true, &two, &mixed, &Reverted::Behaviour),
            BaseOutcome::Green
        );
        assert_eq!(
            report_base(&BaseOutcome::Green, green, false, &named(2), &mixed, &Reverted::Behaviour),
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
    fn a_declaration_outliving_its_file_is_the_harness_move_rather_than_an_author_error() {
        // Measured twice in this repository - #119 and #283 - and it is what following the
        // line-cap advice produces: the harness file added no `#[test]` so it is reverted, the
        // test file declaring `mod <harness>;` is held, and the base tree cannot build. Still
        // `DidNotCompile`, because it genuinely did not - but the operator is told the shape
        // rather than "the change is not separable at file level", which is a different fix.
        let text = concat!(
            "error[E0583]: file not found for module `naming`\n",
            " --> crates/sutura-exec-bigquery/tests/corpus.rs:31:1\n",
            "error: could not compile `sutura-exec-bigquery` (test \"corpus\")\n",
        );
        assert!(missing_module_file(text));
        assert!(!missing_module_file("error[E0432]: unresolved import `crate::thing`"));
        let under_test = scoped("sutura-exec-bigquery", "crates/sutura-exec-bigquery/tests/corpus.rs", &["t"]);
        assert_eq!(classified(text, false, &under_test), BaseOutcome::DidNotCompile);
    }

    #[test]
    fn the_tail_is_the_end_of_the_log() {
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        assert_eq!(tail("", 3), "");
    }

    #[test]
    fn an_inconclusive_base_is_neither_a_pass_nor_a_failure() {
        // #307. Both of these returned `Verdict::Pass`, so a required CI step - whose whole input
        // is the exit code - read *measured nothing* exactly as it read a proof. Neither may be
        // exit 0 again, and neither may be a FAILURE either: the harness move lands on
        // `DidNotCompile` every time and so does a changed public signature on the retry, both
        // legitimate, and a gate that reddens correct work gets disabled.
        let six = named(6);
        assert_eq!(
            reported(&BaseOutcome::DidNotCompile, "error[E0583]", false, &six),
            Verdict::Inconclusive
        );
        assert_eq!(
            reported(&BaseOutcome::DidNotCompile, "error[E0061]", true, &six),
            Verdict::Inconclusive
        );
        assert_eq!(
            reported(&BaseOutcome::Unattributed, "linker exited with signal 9", false, &six),
            Verdict::Inconclusive
        );

        // And the other four keep the direction they had, because this change is about the two
        // answers that measured nothing and not about the ones that measured something.
        assert_eq!(
            reported(
                &BaseOutcome::RedByAssertion {
                    failed: vec![String::from("pa tests::t")]
                },
                "FAIL [ 0.0s ] pa tests::t",
                false,
                &six
            ),
            Verdict::Pass
        );
        assert_eq!(reported(&BaseOutcome::Green, "ok", false, &six), Verdict::Fail);
        assert_eq!(reported(&BaseOutcome::NotRun, "no tests to run", false, &six), Verdict::Fail);
        assert_eq!(
            reported(
                &BaseOutcome::RedOutsideTheDiff {
                    failed: vec![String::from("pb tests::other")]
                },
                "FAIL [ 0.0s ] pb tests::other",
                false,
                &six
            ),
            Verdict::Fail
        );
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
