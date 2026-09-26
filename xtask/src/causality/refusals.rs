//! The answers this gate gives when it FAILS before either run settles anything, or when the
//! HEAD run itself was not green.
//!
//! Split out of `super::remedies` at the unexemptable 1000-line cap, along the seam that module's
//! own header already argued for: `report_scope`'s "EACH ONE DECLARES ITS OWN DIRECTION" applies
//! here as much as there, but every answer in THIS file is `Verdict::Fail` for the same shape of
//! reason - the scan could not read its own inputs, a declaration put tests into the build this
//! diff never named, or the HEAD run itself was red - and every answer in `remedies` is a loud
//! `Verdict::Pass` instead. Nothing here is called from `remedies`, or the reverse: the only
//! caller of either is `causality::run`.
//!
//! A PRINTED CAUSE IS A CLAIM, and it is the claim these functions get wrong most easily -
//! [`report_unnamed_tests`] has been rewritten twice for exactly that, because the two causes it
//! first printed were both wrong for the shape that reached it most often. So each refusing answer
//! names ONE cause per file, taken from the value `super::scoped` classified, rather than
//! offering a reader a list to pick from.

use crate::Verdict;
use crate::causality::base::{names_no_tests, tail};
use crate::causality::features::{Because, Enabled, Unread};
use crate::causality::place::AddedTest;

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

/// A tests-only diff (`Separable::revert` is empty, so nothing here differs to revert) added a
/// test this gate cannot pin: no `Claim-Cell:` trailer names it.
///
/// FAILS. `super::remedies::report_nothing_to_revert` is the arm this used to fall through to
/// unconditionally, at `Verdict::Pass`: nothing was reverted, so the gate had no base run to ask
/// and asked none - which is right for a test genuinely pinning existing behaviour, and wrong for
/// every OTHER test this shape allows through unproven, because AGENTS.md requires a
/// `Claim-Cell:` declaration plus its killing mutation for exactly that pin, and this arm never
/// looked for one (`github.com/telekom/sutura#1016`; measured on #1010: two added tests, no
/// trailer, `measured: 0 of 2`, exit 0).
///
/// NAMES EACH UNDECLARED ADDITION, never the whole diff's tests: a diff may declare some of its
/// added tests and not others, and only the undeclared ones are unproven.
///
/// **THE LIMIT, next to the claim it corrects.** `Scan::of` used to read only ADDED tests -
/// an added `#[test]` attribute or test-module declaration - so an assertion edited inside an
/// EXISTING test, in a file whose production code did not change, reached `Plan::NotRequired`
/// and never reached this arm (`github.com/telekom/sutura#1016`'s own second finding).
/// `super::edited` now names that test too, from the PRE-existing attribute down to the item an
/// added line lands inside, so the edited-assertion shape reaches this same arm and is refused
/// the same way an undeclared addition is. Two shapes still name nothing and still reach
/// `Plan::NotRequired`: a pure DELETION of an assertion, which adds no line either extractor can
/// find; and an edit inside a `#[cfg(test)]` helper fn that a `#[test]` calls but whose own
/// attributed item is not the test's - `touched_in` walks only the `#[test]`-declaring item's own
/// brace span, never a sibling item a test calls. `super::edited`'s own header states both.
pub(super) fn report_unclaimed_additions(tests: &[AddedTest]) -> Verdict {
    eprintln!("xtask test-causality: FAILED - a tests-only diff added a test with no `Claim-Cell:` declaration");
    for test in tests {
        eprintln!(
            "  {} in {}  (pins behaviour the base tree already has - nothing here was reverted to prove it against)",
            test.name(),
            test.file()
        );
    }
    eprintln!();
    eprintln!("Nothing in this diff was reverted, so this gate has no base run to compare any of");
    eprintln!("these against. The only proof it can accept for a test pinning existing behaviour is");
    eprintln!("a mutation: declare `Claim-Cell: <test-fn-name>` on the commit that added it, and");
    eprintln!("commit the killing mutation at `devco/claim-mutations/<test-fn-name>.patch`.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::{
        Because, Enabled, Unread, Verdict, report_enabled_tests, report_head_failure, report_unclaimed_additions,
        report_unnamed_tests, report_unread_manifests, report_unreadable,
    };
    use crate::causality::fixtures::scoped;

    #[test]
    fn each_refusal_keeps_the_direction_its_own_doc_argues_for() {
        // THE ONE MECHANICAL THING IN THIS MODULE, and it had no test at all - which matters more
        // here than elsewhere, because the module's whole argument is that the direction is
        // decided PER ANSWER and for a different reason each time. Nothing captures stdout in this
        // venue, so the sentences stay held by review; the direction does not have to be.
        //
        // Every arm here is a refusal: running everything instead is the unfiltered run whose
        // verdict was a property of the suite, measuring a subset in silence is the defect the
        // scope removes, and comparing against a HEAD run that was not green is not a comparison
        // at all.
        let inseparable = vec![String::from("crates/x/src/a.rs")];
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
        let pinned = scoped("wired", "crates/x/tests/it.rs", &["the_pinned_one"]);
        assert_eq!(report_unclaimed_additions(&pinned), Verdict::Fail);
    }
}
