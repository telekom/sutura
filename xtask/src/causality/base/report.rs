//! Turning a classified [`BaseOutcome`] into the gate's verdict, and the sentence beside it.
//!
//! Split out of `super::base` at the unexemptable 1000-line cap - a concept-named module, not a
//! `part2`: `super::base` decides WHAT happened, this module decides what to TELL the reader about
//! it. The seam is the one [`super::earned`] already draws between classifying and wording.

use crate::Verdict;
use crate::causality::coverage::Coverage;
use crate::causality::provenance::Moved;
use crate::causality::reverted::{self, Reverted};

use super::{BaseOutcome, earned, reported_per_test, tests_run};

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
    let measured = state_the_gap(earned(outcome, coverage), outcome, coverage, output);
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
            // against a behaviour nobody changed, so it is not the one to go and fix. This arm is
            // now unreachable for anything but `Moved::Nothing`: a scope that is EVERY test moved
            // is `GreenAfterAMove`, and a scope that is SOME of them moved is
            // `GreenAfterAPartialMove` below, both handled separately.
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
        // #775. This used to fall into `BaseOutcome::Green` above: the moved subset passing made
        // `succeeded` true, and the genuinely new subset - which cannot have a base-side function
        // to have run at all - was silently never measured. Still FAILS, but the message now
        // tells a reader which tests to leave alone and which to fix the wiring on, rather than
        // telling them to delete a test that never ran.
        BaseOutcome::GreenAfterAPartialMove {
            ref moved,
            ref unmeasured,
        } => {
            eprintln!("xtask test-causality: FAILED - some of the tests this diff added never ran on base");
            eprintln!();
            eprintln!("The base tree already had these, so their passing is not the defect:");
            for one in moved {
                eprintln!("  already at base: {one}");
            }
            eprintln!();
            eprintln!("These are genuinely new and never ran - the base tree has no function of that");
            eprintln!("name to have run at all, so the whole run's `succeeded` came from the moved");
            eprintln!("subset alone:");
            for one in unmeasured {
                eprintln!("  never ran on base: {one}");
            }
            eprintln!();
            eprintln!("This is the ORPHANING shape `NotRun` names for a whole run, here on a subset:");
            eprintln!("the usual cause is a `mod` declaration for the new test's file sitting in a");
            eprintln!("file this diff reverted. Fix the wiring so it compiles at base, or say plainly");
            eprintln!("the named tests are not a regression test.");
            Verdict::Fail
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
            if retried {
                println!("xtask test-causality: INCONCLUSIVE - the coherent base tree ran none of the added tests");
                println!("The first reconstruction could not compile while implementation files were held at HEAD;");
                println!("after restoring those files, the scoped filter matched no base-side tests. This is a");
                println!("structural limitation of a change whose implementation and measurement target are new,");
                println!("not evidence that the tests pass against the old behavior.");
                println!("This exit is INCONCLUSIVE (code 3) rather than a pass, and {measured}.");
                return Verdict::Inconclusive;
            }
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
        BaseOutcome::DidNotResolve => {
            println!("  base: cargo could not resolve the manifest");
            println!("{}", tail(output, 12));
            println!();
            println!("xtask test-causality: INCONCLUSIVE - cargo never reached the compiler.");
            println!("A manifest cargo cannot parse, or a package with no targets, fails before the");
            println!("build is even attempted - this is neither a build failure nor a proof of");
            println!("anything about the change.");
            if retried {
                println!("This is the SECOND attempt: every changed file is at base here, so the");
                println!("manifest itself does not resolve - it is not a file this branch reverted.");
            }
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

/// Correct `measured` when nextest's own `Summary` line ran FEWER of the scope than the filter
/// named, and say so on its own line.
///
/// **THE DEFECT THIS CLOSES.** [`earned`]'s wording is honest about ZERO measured, but not about a
/// PARTIAL one: [`super::Attributed::PerTest`] says a run produced SOME per-test result, not that
/// it produced one for every name the filter carries. `Coverage::scoped_count` is baked in before
/// either run, so `10 of 13 added tests measured` reused the SCOPE's own count under the word
/// "measured" - `github.com/telekom/sutura#893`, measured on a real run: 10 names in the filter,
/// 8 of them orphaned (their file sits in `remove:`, never compiled at base), and nextest's own
/// `2 tests run` line said so twenty lines above a verdict that never read it.
///
/// Silent whenever [`tests_run`] cannot read a number, or the number it reads is not smaller than
/// the scope: an unreadable or matching summary has nothing to correct, and this function may only
/// ever narrow a claim, never widen one.
fn state_the_gap(measured: String, outcome: &BaseOutcome, coverage: &Coverage, output: &str) -> String {
    if !reported_per_test(outcome) {
        return measured;
    }
    let named = coverage.scoped_count();
    let Some(ran) = tests_run(output) else {
        return measured;
    };
    if ran >= named {
        return measured;
    }
    format!(
        "{measured}\n  named {named} into the scope filter; the base run's own summary shows only \
         {ran} of them actually ran - the rest never existed there (orphaned, not proven)"
    )
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
    use super::{BaseOutcome, Coverage, Moved, Reverted, Verdict, earned, missing_module_file, report_base, state_the_gap, tail};
    use crate::causality::base::classify_base;
    use crate::causality::fixtures::{named, scoped};
    use crate::causality::place::AddedTest;

    /// The classifier over a scope whose every test is new here and a revert that reaches them -
    /// the same wrapper `super::tests` uses, duplicated because a submodule's own tests read
    /// clearer calling the real function than reaching back across the split for a helper.
    fn classified(text: &str, succeeded: bool, scoped: &[AddedTest]) -> BaseOutcome {
        classify_base(text, succeeded, scoped, &Moved::Nothing, &Reverted::Behaviour)
    }

    /// The report for a scope whose every test is new here, for the same reason.
    fn reported(outcome: &BaseOutcome, output: &str, retried: bool, coverage: &Coverage) -> Verdict {
        report_base(outcome, output, retried, coverage, &Moved::Nothing, &Reverted::Behaviour)
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
            " --> corpus.rs:31:1\n",
            "error: could not compile `sutura-exec-bigquery` (test \"corpus\")\n",
        );
        assert!(missing_module_file(text));
        assert!(!missing_module_file("error[E0432]: unresolved import `crate::thing`"));
        let under_test = scoped("sutura-exec-bigquery", "corpus.rs", &["t"]);
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
        // #716: a manifest resolution failure is neither `Unattributed` nor `DidNotCompile`, and
        // the retry context is carried the same way `DidNotCompile` carries it.
        assert_eq!(
            reported(&BaseOutcome::DidNotResolve, "error: failed to parse manifest", false, &six),
            Verdict::Inconclusive
        );
        assert_eq!(
            reported(&BaseOutcome::DidNotResolve, "error: failed to parse manifest", true, &six),
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
            reported(&BaseOutcome::NotRun, "no tests to run", true, &six),
            Verdict::Inconclusive
        );
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
    fn a_partly_orphaned_scope_states_the_gap_between_named_and_measured() {
        // github.com/telekom/sutura#893: the filter named 10 (of 13 added), only 2 of those 10
        // existed at base - the other 8 were orphaned, each behind its own file in `remove:` - and
        // the verdict still read "10 of 13 added tests measured" over a run that measured 2.
        let coverage = Coverage::Measured {
            measured: 10,
            unmeasured: vec![String::from("held_a"), String::from("held_b"), String::from("held_c")],
            not_runnable: Vec::new(),
        };
        let outcome = BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::the_added_one")],
        };
        let output = concat!(
            "        FAIL [   0.021s] (2/2) pa tests::the_added_one\n",
            "     Summary [   0.4s] 2 tests run: 1 passed, 1 failed, 1727 skipped\n",
            "error: test run failed\n",
        );
        let stated = state_the_gap(earned(&outcome, &coverage), &outcome, &coverage, output);
        assert!(stated.starts_with("10 of 13 added tests measured"), "{stated}");
        assert!(stated.contains("named 10 into the scope filter"), "{stated}");
        assert!(stated.contains("only 2 of them actually ran"), "{stated}");
    }

    #[test]
    fn a_fully_ran_scope_states_nothing_extra() {
        // The silent case, and the one that must stay silent: every named test ran on base, so
        // there is no gap to state, and appending one anyway would print a correction nobody
        // needs beside every ordinary passing run.
        let coverage = Coverage::Measured {
            measured: 2,
            unmeasured: Vec::new(),
            not_runnable: Vec::new(),
        };
        let outcome = BaseOutcome::RedByAssertion {
            failed: vec![String::from("pa tests::t")],
        };
        let output = "     Summary [   0.1s] 2 tests run: 1 passed, 1 failed, 0 skipped\n";
        assert_eq!(
            state_the_gap(earned(&outcome, &coverage), &outcome, &coverage, output),
            "2 of 2 added tests measured"
        );
    }

    #[test]
    fn an_outcome_that_measured_nothing_is_never_corrected() {
        // `earned` already prints the honest zero for these; a `Summary` line that happens to
        // name a smaller number than the scope must not grow a second, contradictory correction
        // on an outcome that made no per-test claim in the first place.
        let coverage = Coverage::Measured {
            measured: 6,
            unmeasured: Vec::new(),
            not_runnable: Vec::new(),
        };
        let output = "     Summary [   0.1s] 1 test run: 0 passed, 1 failed\n";
        assert_eq!(
            state_the_gap(
                earned(&BaseOutcome::DidNotCompile, &coverage),
                &BaseOutcome::DidNotCompile,
                &coverage,
                output
            ),
            "0 of 6 added tests measured"
        );
    }
}
