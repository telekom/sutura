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
//! compared against the tests under test. That comparison is the SECOND mechanism, and it is not
//! redundant with scoping the run ([`super::scoped`]): a filter that stopped filtering - a
//! nextest version whose expression syntax moved, a name this tree does not spell the way the
//! extractor expects - would silently restore the wide run, and the wide run's failures are
//! exactly what this refuses to read as evidence.
//!
//! FOUR ANSWERS, NOT TWO. An unrelated base failure is neither red-by-assertion nor "the base
//! tree does not build", and calling it either one misleads in a different direction. It is
//! [`BaseOutcome::RedOutsideTheDiff`], and it FAILS: the gate's precondition - a base tree red
//! only where this diff put it - was not met, so it has no answer to give. That is the same
//! direction the HEAD half already takes when the suite is red there, and the symmetry is the
//! argument. [`BaseOutcome::NotRun`] is the other new one: nextest matched none of the scoped
//! tests, which is the ORPHANING trap (a new test module whose `mod` declaration sits in a
//! reverted file is never compiled) and used to read as *green against base behaviour*.
//!
//! WHAT IS STILL NOT SPLIT. A failure this cannot attribute to any test at all - a linker
//! signal, a runner that died before reporting - stays [`BaseOutcome::DidNotCompile`], whose
//! report says "did not build". That is inherited and it is the conservative direction (no proof
//! claimed), but the WORD is wrong for a run that got further than the compiler. Splitting it
//! needs a fifth verdict nobody has a message for yet.

use crate::Verdict;
use crate::causality::scoped::TestName;

/// What the base run actually told us.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BaseOutcome {
    /// The tests under test passed without the change: they do not test it.
    Green,
    /// A test the diff added failed an assertion. The evidence the gate exists to collect, and
    /// it carries the names so the printed verdict says what it measured.
    RedByAssertion { failed: Vec<String> },
    /// The base run failed, and none of the failures is a test this diff added. Red, and about
    /// something else.
    RedOutsideTheDiff { failed: Vec<String> },
    /// nextest matched none of the tests under test, so nothing was measured.
    NotRun,
    /// The tree did not build. Red, but it proves nothing about behaviour.
    DidNotCompile,
}

/// Classify a base test run against the tests the diff added.
///
/// Order matters twice. A tree that did not build often ALSO prints "error: test run failed", so
/// the compile check comes first or a build failure reads as a real red - that was this gate's
/// first false green. And a run that matched no test prints no failure at all, so it is settled
/// before the failure list is consulted rather than falling through as "unknown".
pub(crate) fn classify_base(text: &str, succeeded: bool, scoped: &[TestName]) -> BaseOutcome {
    if succeeded {
        return BaseOutcome::Green;
    }
    if did_not_compile(text) {
        return BaseOutcome::DidNotCompile;
    }
    if names_no_tests(text) {
        return BaseOutcome::NotRun;
    }
    let failed = failures(text);
    if failed.is_empty() {
        // Unknown failure: do not claim a proof we did not get.
        return BaseOutcome::DidNotCompile;
    }
    let under_test: Vec<String> = failed.iter().filter(|one| is_scoped(one, scoped)).cloned().collect();
    if under_test.is_empty() {
        BaseOutcome::RedOutsideTheDiff { failed }
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

/// The statuses nextest prints for a test that did not pass.
///
/// The trailing ` [` is part of each: it is what separates the status from a test whose NAME
/// begins with the same letters.
const FAILING_STATUSES: &[&str] = &["FAIL [", "TIMEOUT [", "ABORT [", "SIGSEGV [", "LEAK-FAIL ["];

/// `FAIL [   0.183s] (1/1) a::unrelated needs_state` -> `a::unrelated needs_state`.
///
/// The `(n/m)` counter is dropped: it is a property of the run's ordering rather than of the
/// test, so keeping it would make one failure in two runs look like two failures.
fn nextest_failure(trimmed: &str) -> Option<String> {
    let status_line = strip_retry(trimmed);
    if !FAILING_STATUSES.iter().any(|status| status_line.starts_with(status)) {
        return None;
    }
    let (_, named) = status_line.split_once(']')?;
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
/// The last whitespace token is the test's path, because nextest prints the binary id before it.
/// A parametrised case sits one segment BELOW the function (`tests::sums::case_1`), so the
/// comparison reaches two segments up and no further: wider would let a MODULE sharing a name
/// with an added test read as in scope, which is the generous direction this gate is here to
/// stop. Narrower would miss every `rstest` case.
fn is_scoped(failure: &str, scoped: &[TestName]) -> bool {
    let Some(path) = failure.split_whitespace().last() else {
        return false;
    };
    path.rsplit("::")
        .take(2)
        .any(|segment| scoped.iter().any(|name| name.as_str() == segment))
}

/// Turn a base run into the gate's verdict.
///
/// `retried` only changes what the operator is told: after a second attempt, "not separable at
/// file level" is no longer the likely explanation, because the tree WAS coherently at base.
pub(crate) fn report_base(outcome: &BaseOutcome, output: &str, retried: bool) -> Verdict {
    match *outcome {
        BaseOutcome::Green => {
            eprintln!("xtask test-causality: FAILED - green against base behaviour");
            eprintln!();
            eprintln!("The tests this diff added pass with the implementation reverted, so they do");
            eprintln!("not test the change. Make the test exercise the new behaviour, or say plainly");
            eprintln!("that it is not a regression test.");
            Verdict::Fail
        }
        BaseOutcome::RedByAssertion { ref failed } => {
            println!("  base: red by assertion, as required");
            for one in failed {
                println!("    red on base: {one}");
            }
            println!("{}", tail(output, 12));
            println!("xtask test-causality: ok - red on base, green on head");
            Verdict::Pass
        }
        BaseOutcome::RedOutsideTheDiff { ref failed } => {
            eprintln!("xtask test-causality: FAILED - the base tree is red outside this diff");
            for one in failed {
                eprintln!("    red on base: {one}");
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
        BaseOutcome::DidNotCompile => {
            println!("  base: did not compile");
            println!("{}", tail(output, 12));
            println!();
            println!("xtask test-causality: INCONCLUSIVE - the base tree does not build.");
            println!("That is red, but a test that never ran is not evidence about behaviour.");
            if retried {
                println!("This is the SECOND attempt: every changed file is at base here, so the");
                println!("build failure is in the changed tests themselves - they reference");
                println!("something this branch introduced. State the evidence in the handoff.");
            } else {
                println!("Usually it means the change is not separable at file level: the test and");
                println!("what it needs arrived together. State the evidence in the handoff.");
            }
            Verdict::Pass
        }
    }
}

/// The last `n` lines, so a failure shows the assertion rather than the whole compile log.
pub(crate) fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines.get(start..).map_or_else(String::new, |rest| rest.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{BaseOutcome, classify_base, names_no_tests, tail};
    use crate::causality::scoped::TestName;

    /// The tests under test, from their function names.
    fn scoped(names: &[&str]) -> Vec<TestName> {
        names
            .iter()
            .map(|name| TestName::parse(name).expect("a function name"))
            .collect()
    }

    /// The base run that reported the false green, as nextest printed it. Two tier-backed cells
    /// failed after 86 of 1810 tests and the branch's own test never ran.
    const UNRELATED_RED: &str = concat!(
        "    Starting 1810 tests across 47 binaries\n",
        "        FAIL [   0.313s] (86/1810) sutura-app::differential tests::postgres::sums_by_month\n",
        "        FAIL [   0.204s] (87/1810) sutura-app::differential tests::postgres::one_row_per_month\n",
        "  Cancelling due to test failure: \n",
        "     Summary [   4.118s] 87 tests run: 85 passed, 2 failed, 1723 skipped\n",
        "error: test run failed\n",
    );

    #[test]
    fn a_base_failure_outside_the_diff_is_not_reported_as_red_by_assertion() {
        // THE DEFECT. Both failing cells are tier-backed cells this diff did not touch, and the
        // test it did add - the audit record - never ran. The old classifier saw "FAIL [" and
        // answered red-by-assertion, which the gate printed as `ok - red on base, green on head`.
        let under_test = scoped(&["a_refused_question_is_recorded_and_names_the_refusal"]);
        let outcome = classify_base(UNRELATED_RED, false, &under_test);
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
        // This is the real base red from the branch above, with the same run around it.
        let text = concat!(
            "    Starting 3 tests across 47 binaries\n",
            "        FAIL [   0.021s] (2/3) sutura-cli::query audit::a_refused_question_is_recorded_and_names_the_refusal\n",
            "     Summary [   0.4s] 2 tests run: 1 passed, 1 failed, 0 skipped\n",
            "error: test run failed\n",
        );
        let under_test = scoped(&["a_refused_question_is_recorded_and_names_the_refusal"]);
        assert_eq!(
            classify_base(text, false, &under_test),
            BaseOutcome::RedByAssertion {
                failed: vec![String::from(
                    "sutura-cli::query audit::a_refused_question_is_recorded_and_names_the_refusal"
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
            "        FAIL [   0.021s] (87/1810) sutura-cli::query audit::the_added_one\n",
            "error: test run failed\n",
        );
        match classify_base(text, false, &scoped(&["the_added_one"])) {
            BaseOutcome::RedByAssertion { ref failed } => {
                assert_eq!(failed.len(), 1, "only the test under test: {failed:?}");
                assert!(failed.iter().all(|one| one.ends_with("audit::the_added_one")));
            }
            other => panic!("expected RedByAssertion, got {other:?}"),
        }
    }

    #[test]
    fn a_parametrised_case_belongs_to_the_test_that_declared_it() {
        // `rstest` names a case one segment BELOW the function, so a comparison on the last
        // segment alone would call every case an unrelated failure.
        let text = "        FAIL [   0.010s] (1/4) sutura-sql::golden tests::renders::case_2\nerror: test run failed\n";
        match classify_base(text, false, &scoped(&["renders"])) {
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
        match classify_base(text, false, &scoped(&["sums"])) {
            BaseOutcome::RedOutsideTheDiff { .. } => {}
            other => panic!("expected RedOutsideTheDiff, got {other:?}"),
        }
    }

    #[test]
    fn a_base_that_did_not_compile_is_not_a_proof() {
        // The false green this replaced: `cargo test` failed, so the gate said "red, as
        // required" and passed. A tree that does not build has run no tests.
        let compile = "error[E0432]: unresolved import `crate::thing`\nerror: could not compile";
        assert_eq!(classify_base(compile, false, &scoped(&["t"])), BaseOutcome::DidNotCompile);
    }

    #[test]
    fn a_build_failure_wins_over_a_test_run_failure_line() {
        // nextest prints "error: test run failed" when the build failed too. Reading that as a
        // real red is the false green this gate already had once.
        let both = "error[E0433]: failed to resolve\nerror: could not compile\nerror: test run failed";
        assert_eq!(classify_base(both, false, &scoped(&["t"])), BaseOutcome::DidNotCompile);
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
        assert_eq!(classify_base(text, false, &scoped(&["orphaned"])), BaseOutcome::NotRun);
    }

    #[test]
    fn the_cargo_test_wording_still_names_a_failure() {
        // So the classifier survives a runner swap. `cargo test` prints no binary id, so the
        // whole token is the test's path.
        let cargo = "running 3 tests\ntest suite::the_added_one ... FAILED\ntest result: FAILED. 2 passed; 1 failed";
        assert_eq!(
            classify_base(cargo, false, &scoped(&["the_added_one"])),
            BaseOutcome::RedByAssertion {
                failed: vec![String::from("suite::the_added_one")]
            }
        );
    }

    #[test]
    fn a_failure_no_line_names_claims_nothing() {
        // Deliberately conservative, and a CHANGE: this text used to be read as red-by-assertion
        // on the strength of the word "panicked", which attributes the red to nothing at all.
        let unnamed = "thread 'x' panicked at src/lib.rs:9\ntest result: FAILED. 2 passed; 1 failed";
        assert_eq!(
            classify_base(unnamed, false, &scoped(&["the_added_one"])),
            BaseOutcome::DidNotCompile
        );
    }

    #[test]
    fn an_unrecognised_failure_claims_nothing() {
        // Conservative on purpose: an unfamiliar failure is not evidence of causality.
        assert_eq!(
            classify_base("linker exited with signal 9", false, &scoped(&["t"])),
            BaseOutcome::DidNotCompile
        );
    }

    #[test]
    fn a_passing_base_means_the_test_does_not_test_the_change() {
        assert_eq!(
            classify_base("test result: ok. 12 passed; 0 failed", true, &scoped(&["t"])),
            BaseOutcome::Green
        );
    }

    #[test]
    fn a_retry_is_the_same_failure_once() {
        // nextest prints every attempt. Two lines for one test must not read as two failures.
        let text = concat!(
            "    TRY 1 FAIL [   0.010s] (1/2) sutura-http::served serves::the_added_one\n",
            "    TRY 2 FAIL [   0.010s] (1/2) sutura-http::served serves::the_added_one\n",
            "error: test run failed\n",
        );
        match classify_base(text, false, &scoped(&["the_added_one"])) {
            BaseOutcome::RedByAssertion { ref failed } => assert_eq!(failed.len(), 1, "{failed:?}"),
            other => panic!("expected RedByAssertion, got {other:?}"),
        }
    }

    #[test]
    fn the_tail_is_the_end_of_the_log() {
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        assert_eq!(tail("", 3), "");
    }
}
