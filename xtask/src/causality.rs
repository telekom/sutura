//! The test-causality gate: a changed test must be **red against the base behaviour and
//! green with the change**.
//!
//! A test that passes both ways proves nothing, and is worse than no test because it reads as
//! coverage. This is the one property that distinguishes a regression test from a test that
//! happens to exercise adjacent code.
//!
//! HOW THE BASELINE IS RECONSTRUCTED.
//!
//! In a git worktree at HEAD, the base version of every changed source file that added no
//! test is restored, while files that added tests keep their HEAD content. The changed tests
//! then run against old behaviour and new tests.
//!
//! TWO ATTEMPTS. A file that gained both an implementation change and a test keeps its HEAD
//! content on the first attempt, and a file at HEAD calling an API this branch CHANGED mixes two
//! versions of it, so the tree does not compile. That used to end the run as INCONCLUSIVE after
//! paying for the whole build - 12 minutes in CI for no verdict. A failure to compile is now not
//! taken as the answer while anything is still held at HEAD: those files go to base too and the
//! question is asked once more, on a tree that is coherently at base.
//!
//! Where that does NOT work, stated plainly rather than papered over: Rust keeps unit tests
//! in `mod tests` inside the file they test, so a fix and its test frequently live in ONE
//! file. Reverting it removes the test; keeping it keeps the fix. Such a change is not
//! mechanically separable, and this gate says so and requires the author to state the
//! evidence instead. It does not silently pass, and it does not pretend to have checked.
//!
//! A gate that quietly downgraded to "no opinion" here would be worse than no gate: it would
//! report green over exactly the cases most likely to hide a vacuous test.
//!
//! WHICH FILE IS WHICH. [`diff`] reads the diff into added lines that carry their post-image
//! line numbers; [`regions`] decides which of those lines are test code, by where they sit in
//! the file rather than by what the added set happens to contain; [`plan()`] sorts the result into
//! what to revert, what to hold and what to measure. That middle module's own doc records the
//! defect the earlier shape produced and the limits of what replaced it - a tests-only branch
//! reported as "changes behaviour and adds tests in one file", which is a different message asking
//! for a different thing.
//!
//! AND A GREEN BASE RUN IS A FACT ABOUT THE PARTITION until something says otherwise, which is
//! [`reverted`]. *These tests pass with the implementation reverted* reads as a statement about the
//! tests and is first a statement about which files were put back: a revert that restores no line
//! a measured test can execute proves nothing either way, which is `provenance::Reach::BuildInput`'s
//! argument reaching a file that IS reverted. That module carries the two excuses, the one case it
//! deliberately leaves in reach - test code in the package a measured test is in - and the
//! falsifier [`base`] prints when a run contradicts it.
//!
//! **This paragraph belongs in `.agents/skills/sutura/gates/SKILL.md`'s causality section**, whose
//! whole subject is how this gate can lie; it is here because that page was held by another change
//! when this landed.
//!
//! AND WHAT A DIFF CANNOT SAY IS ITS OWN MODULE. [`provenance`] holds the three facts that are
//! true of a diff and are not in it: the COMMIT it is measured against rather than a ref that
//! moves, what the reconstruction may do with a changed path that is not Rust, and whether a test
//! the added lines name was already in the base tree - a MOVE, which a diff and an addition look
//! identical to. Each of those was a verdict the gate reached about something nothing had
//! established. [`isolation`] is the fourth: both runs share one target directory, and until a
//! removal preceded each of them a run could execute the other tree's binaries and replay its
//! saved diagnostics.
//!
//! WHICH TESTS ARE PROVEN, and this is the part that used to be missing. Both runs are scoped to
//! the tests the diff ADDED ([`scoped`]), and the base run's verdict compares every failure it
//! reported against that same set ([`base`]). Before that, the run was `--workspace` unfiltered
//! and any assertion failure anywhere counted as red-by-assertion, so one unrelated failing cell
//! early in the run answered "red on base" while the tests under test never ran at all.
//!
//! IT ALL RESTS ON THE KEY, and keyed on the bare function name it was too weak to identify a
//! test at all - so a collided failure passed both halves and a vacuous test got *ok - red on
//! base, green on head* off a pre-existing one elsewhere. [`scoped`] owns the key, the
//! measurement behind it, and the collisions it still does not separate; [`base`] owns why a
//! second check on ONE key is not a second mechanism.
//!
//! THE OLD ANSWER WAS NOT EVEN STABLE, which is the part that made it hard to see. Whether an
//! unrelated cell failed BEFORE the tests under test - and so, under fail-fast, whether the wide
//! run ever reached them - depended on nextest's scheduling and on which tree last compiled a
//! shared test binary (see [`runner::cargo_test`]), so one tree answered differently in two
//! venues and neither answer looked wrong. Scoping removes the first dependence and
//! `--no-fail-fast` the second.
//!
//! SCOPING THE HEAD RUN NARROWS A CLAIM, and the narrowing is on purpose. "The suite is green on
//! HEAD" was never this gate's property - it is what `just test` and the nix `nextest` check are
//! for - and holding it here meant the gate reported nothing about a change whenever anything
//! else in the tree was red. What it asserts now is exactly what it needs: *these tests are green
//! on HEAD and red on base*. Because nextest fails when a filter matches nothing, a test this
//! gate cannot name is a loud failure on the HEAD run rather than a quiet pass.

use std::path::Path;

use crate::Verdict;
use crate::repo;

// `pub(crate)` for the reason `regions` below is: `crate::examples` asks which of a file's tests
// a run in this venue reaches, and the attribute vocabulary that answers it already lives here.
// A second copy of "what declares a test, and what makes one `#[ignore]`d" is a second thing to
// keep true, and this module's own header is about what one such disagreement already cost.
pub(crate) mod attributes;
mod base;
mod claim;
mod coverage;
mod diff;
mod features;
#[cfg(test)]
mod fixtures;
mod isolation;
mod membership;
mod names;
mod place;
mod plan;
mod provenance;
// `pub(crate)` rather than private: `crate::refusals` reads the same test regions this gate does,
// because "which lines of this file are test code" is one question and a second implementation of
// it would be a second thing to keep in step. Nothing else about the module moved.
pub(crate) mod regions;
mod relocation;
mod remedies;
// `reverted` sits after `remedies` alphabetically and after `regions` conceptually: it reads the
// same post-image regions, and it is the last question asked before a green run becomes a verdict.
mod reverted;
// `pub(crate)` rather than private, like `attributes` and `regions` above and for the same
// reason: `task_table` registers its two gates directly by fn pointer, and a second copy of "which
// committed patch names which cell" inside `task_table` would be a second thing to keep in step
// with `claim::MUTATIONS_DIR` - `github.com/telekom/sutura#950`.
pub(crate) mod rot;
mod runner;
mod scoped;
mod stack;
mod worktree;

use base::{BaseOutcome, classify_base, report_base, tail};
use coverage::{Coverage, Scope};
use diff::changed_with_additions;
use features::{Activation, BaseText, Trees};
use place::AddedTest;
use plan::{Plan, Separable, plan};
use provenance::{Commit, Moved, Reach};
use relocation::{Claim, Images, Relocation};
use remedies::{
    report_enabled_tests, report_head_failure, report_moved, report_no_base_behaviour, report_not_separable,
    report_nothing_to_revert, report_only_ignored, report_orphaned_modules, report_scope, report_silent, report_unnamed_tests,
    report_unread_manifests, report_unreadable, report_unreverted,
};
use reverted::Attempts;
use runner::{Tree, cargo_test};
use scoped::{Scan, Scoped};
use stack::{Base, Parent};
use worktree::{BaseState, add_worktree, apply, base_state, remove_worktree};

/// Is this a Rust source path THIS workspace compiles?
///
/// Case-insensitive on the extension, since a case-sensitive test is wrong on a case-insensitive
/// filesystem. And not a path outside every workspace member: `vendor/mimalloc_rust` is
/// `exclude`d in the root manifest and carries its own `#[test]`s, so a vendor bump touching one
/// would otherwise be a changed test whose key names a package `--workspace` never builds -
/// nextest refuses an unknown `package(=..)` outright, so the gate would redden a correct change.
///
/// [`provenance::Reach`] owns the whole question now, because "not compiled" stopped meaning "not
/// interesting": a page, a recipe or a nix file is the implementation of every test that reads it.
/// This is the one arm of it that decides whether a file's ADDED LINES get read.
fn is_compiled_rust(path: &str) -> bool {
    Reach::of(path) == Reach::Compiled
}

/// The `--since <ref>` argument, or `None` when it was not supplied correctly.
fn base_ref(args: &[String]) -> Option<String> {
    let (flag, rest) = args.split_first()?;
    if flag != "--since" {
        return None;
    }
    rest.first().cloned()
}

/// Reconstruct the baseline in a worktree and require the changed tests to fail there.
fn prove(
    root: &Path,
    base: &Commit,
    separable: &Separable,
    scoped: &Scoped,
    coverage: &Coverage,
    reverted: &Attempts,
    files: &[diff::ChangedFile],
    read: &regions::PostImage<'_>,
) -> Verdict {
    // A CRATE THIS BRANCH ADDED IS ITS MANIFEST TOO, and reverting the sources alone left a
    // workspace member with no targets - cargo refuses before the compiler and the gate had no
    // verdict to give any branch that adds a crate (`membership`, #936).
    //
    // ONE WITHDRAWAL PER ATTEMPT, keyed on what that attempt keeps at HEAD. The first keeps the
    // held-back files AND the test files, so a new crate carrying either stays whole; the retry
    // keeps only the test files, which are never reverted at all - the proof needs them present -
    // so a provable test inside the new crate keeps the crate in the workspace on both attempts.
    // Withdrawing it there would leave that test in a directory cargo no longer reads, and nextest
    // failing an empty filterset is a red about this partition rather than about the change.
    let inputs = &separable.build_inputs;
    let membership = membership::Membership::of(root, base, inputs);
    let at_head = separable.at_head_first_attempt();
    let reverting = membership.reverting(&separable.revert, inputs, &at_head);
    let holding = membership.reverting(&separable.held(), inputs, &separable.test_files);
    let first = base_state(root, base, &reverting);
    let held = base_state(root, base, &holding);
    let unreverted = separable.unreverted_from(&reverting);

    if first.restore.is_empty() {
        return report_no_base_behaviour(base.short(), &first.remove, coverage, &unreverted);
    }

    println!("xtask test-causality: proving red-before-green");
    for f in &separable.test_files {
        println!("  test file: {f}");
    }
    for f in &first.restore {
        println!("  restore:   {f}");
    }
    for f in &first.remove {
        println!("  remove:    {f}  (added in this branch)");
    }
    report_orphaned_modules(&first.remove, files, read);
    for f in &separable.held_back {
        println!("  held:      {f}  (carries its own tests)");
    }
    for f in &separable.test_only {
        println!("  held:      {f}  (test-only code that names no test)");
    }
    report_unreverted(&unreverted);

    // HEAD must be green, or "red on base" means nothing.
    // One directory for both runs. Beside the worktree under `target/`, so a `cargo clean`
    // or a fresh checkout takes it with everything else rather than leaving it behind.
    //
    // THIS PATH IS SPELLED TWICE. `nix/cargo-env.nix` unpacks the closure the checks already
    // built into the same directory, and a rename on either side would silently stop the reuse
    // rather than fail - the gate would still answer, minutes later. `check-warm-start` reads
    // both files and fails if they differ; it finds this binding by name, so a rename here is a
    // red gate rather than a silent one.
    let shared_target = root.join("target").join("causality-target");
    let only = scoped.filterset();
    println!("  filter:    {only}");
    report_silent(scoped.silent());
    // THE SCOPE, NOT A MEASUREMENT, and it printed the ratio here until review reproduced what
    // that costs: on a run that ends INCONCLUSIVE the honest `0 of M` lands twenty lines below
    // `measured:  M of M added tests measured`, one output, one sentence, two numerators. Nothing
    // has run yet, so `base::earned` cannot be consulted and there is no outcome to earn a
    // numerator from - `causality::coverage` makes that a compile-time fact rather than a wording.
    report_scope(coverage);
    // WHICH OF THESE THE BASE TREE ALREADY HAD, asked before either run because it decides what
    // *green on base* MEANS. A diff cannot tell a moved test from an added one, and the gate's
    // premise is about tests that were added: `provenance::Moved` asks the base commit instead.
    let named: Vec<&str> = scoped.tests().iter().map(AddedTest::name).collect();
    let moved = already_there(root, base, &named);
    report_moved(&moved);
    let (head_ok, head_out) = cargo_test(root, &shared_target, &only, Tree::Provisioned);
    if !head_ok {
        return report_head_failure(&head_out, &only);
    }
    println!("  head: green");

    let wt = root.join("target").join("causality-worktree");
    remove_worktree(root, &wt);
    if let Err(e) = add_worktree(root, &wt) {
        eprintln!("xtask test-causality: could not create a worktree: {e}");
        return Verdict::Fail;
    }

    let verdict = reconstruct_and_run(
        &wt,
        base,
        &first,
        &held,
        &shared_target,
        scoped,
        &Scope {
            only: &only,
            coverage,
            moved: &moved,
            reverted,
        },
    );
    remove_worktree(root, &wt);
    verdict
}

/// Which of the tests `named` the base commit already had, asked of git.
///
/// TWO CALLS AND NO DECISION HERE. The search is restricted to the `.rs` files THIS DIFF TOUCHED -
/// deletions included, which is why the path list comes from git rather than from the parsed diff -
/// because that is what a move looks like from the base side: the file the test came from lost
/// those lines, so it is in the diff. Asking the whole tree instead would let one of this tree's
/// duplicated test names answer yes about a genuinely new test. `provenance` owns both the patterns
/// and the classification, so the direction it fails in is stated where it is decided.
fn already_there(root: &Path, base: &Commit, named: &[&str]) -> Moved {
    let paths: Vec<String> = worktree::touched(root, base)
        .into_iter()
        .filter(|path| is_compiled_rust(path))
        .collect();
    let found = worktree::search(root, base, &provenance::needles(named), &paths);
    Moved::of(named, &found)
}

/// Put the worktree into the base state for the implementation, then run the tests.
///
/// TWO ATTEMPTS, and the second is the difference between a verdict and no answer. The first
/// leaves the held-back files at HEAD, because that is what lets their neighbours' tests run
/// against old behaviour. But a file held at HEAD calling an API this branch CHANGED mixes two
/// versions of that API, and the tree does not compile - which used to end the run as
/// INCONCLUSIVE after paying for the whole build. Observed in CI: base
/// `DefinitionDigest::of(&definitions)` against a held-back caller passing
/// `(&definitions, &knowledge)`, E0061, 12 minutes, no verdict.
///
/// So a failure to COMPILE is not taken as the answer while anything is still held at HEAD.
/// Restoring those too costs one more incremental build and yields a tree that is coherently at
/// base, where the tests that ARE separable get the verdict they came for. Their own tests go
/// with them, which is exactly what `plan` already excluded from the proof.
fn reconstruct_and_run(
    wt: &Path,
    base: &Commit,
    first: &BaseState<'_>,
    held: &BaseState<'_>,
    target: &Path,
    scoped: &Scoped,
    scope: &Scope<'_>,
) -> Verdict {
    if let Err(e) = apply(wt, base, first) {
        eprintln!("xtask test-causality: {e}");
        return Verdict::Fail;
    }

    let (base_ok, base_out) = cargo_test(wt, target, scope.only, Tree::Reconstructed);
    let outcome = classify_base(&base_out, base_ok, scoped.tests(), scope.moved, scope.reverted.attempt(false));

    if retry_with_held_back(&outcome, held) {
        println!("  base: did not compile with the held-back file(s) still at HEAD");
        println!("{}", tail(&base_out, 8));
        println!("  retrying with those at base as well:");
        for f in &held.restore {
            println!("    restore:   {f}");
        }
        for f in &held.remove {
            println!("    remove:    {f}");
        }
        if let Err(e) = apply(wt, base, held) {
            eprintln!("xtask test-causality: {e}");
            return Verdict::Fail;
        }
        let (retry_ok, retry_out) = cargo_test(wt, target, scope.only, Tree::Reconstructed);
        return report_base(
            &classify_base(
                &retry_out,
                retry_ok,
                scoped.tests(),
                scope.moved,
                scope.reverted.attempt(true),
            ),
            &retry_out,
            true,
            scope.coverage,
            scope.moved,
            scope.reverted.attempt(true),
        );
    }

    report_base(
        &outcome,
        &base_out,
        false,
        scope.coverage,
        scope.moved,
        scope.reverted.attempt(false),
    )
}

/// Should the proof ask a second time, with the held-back files at base too?
///
/// Only for a tree that did not COMPILE, and only while something is still held at HEAD. A base
/// run that reached an assertion has answered the question and is not retried - retrying it would
/// change the answer by reverting the very implementation whose absence the assertion measured.
const fn retry_with_held_back(outcome: &BaseOutcome, held: &BaseState<'_>) -> bool {
    matches!(outcome, BaseOutcome::DidNotCompile) && !held.is_empty()
}

/// The three git reads `stack::parent_of` needs, bound to this repository.
///
/// GLUE AND NO DECISION, and it is now glue with nothing in it to get wrong: the composition and
/// every fallback moved into `stack::parent_of`, where fakes can drive them, and the choice is
/// `stack::Base::of`. **Review found why that mattered.** This function used to carry the guard
/// against a parent that is really this branch, spelled `parent == branch` - a NAME comparison for
/// a hazard that is a COMMIT - and being untested is what let one spelling stand in for the input.
/// It is `stack::Origin::Contains` now, keyed on HEAD's commit.
fn stack_parent(root: &Path, asked_for: &Commit) -> Option<Parent> {
    let metadata = |branch: &stack::BranchRef| worktree::branch_metadata(root, branch);
    let merge_base = |earlier: &str, later: &str| worktree::merge_base(root, earlier, later);
    stack::parent_of(
        asked_for,
        &stack::Reads {
            branch: worktree::head_branch(root),
            metadata: &metadata,
            merge_base: &merge_base,
        },
    )
}

/// What the diff's manifest changes put into the build, asked of the two trees.
///
/// The three readers [`features::Trees`] wants, and the pairing that matters is the first: a path
/// the base does not HAVE is a new package whose base feature table is legitimately empty, while a
/// path the base has and whose content did not come back is a refusal. `cat-file -e` and `show` are
/// separate calls so those two are separate answers.
///
/// The source listing is behind a `OnceCell` because most runs never need it: it is consulted only
/// for a manifest that declares a feature name the base did not, which is rare, and
/// `repo::all_files` shells out to git twice.
fn feature_activation(root: &Path, at: &Commit, files: &[diff::ChangedFile], read: &regions::PostImage<'_>) -> Activation {
    let base = |path: &str| {
        if worktree::base_has(root, at, path) {
            worktree::at_base(root, at, path).map_or(BaseText::Unreadable, BaseText::Text)
        } else {
            BaseText::Absent
        }
    };
    let listing: std::cell::OnceCell<Vec<String>> = std::cell::OnceCell::new();
    let sources = |dir: &str| {
        listing
            // FAIL OPEN, unchanged and now visible: a refusal yields an EMPTY listing and the
            // feature-activation walk proceeds over nothing. Narrow - this is consulted only for a
            // manifest declaring a feature name the base did not - and it is
            // `github.com/telekom/sutura#414`'s own finding on this file, left to the PR that owns
            // this gate rather than folded into a mechanical one.
            .get_or_init(|| {
                repo::all_files()
                    .and_then(|census| census.into_listing(repo::Unmigrated::Causality))
                    .map(|(_root, files)| files)
                    .unwrap_or_default()
            })
            .iter()
            // `is_compiled_rust` rather than an extension test, so the one rule that decides
            // what this workspace compiles decides here too - a vendored path is excluded by it.
            .filter(|path| is_compiled_rust(path) && (dir.is_empty() || path.starts_with(&format!("{dir}/"))))
            .cloned()
            .collect()
    };
    Activation::of(
        files,
        &Trees {
            head: read,
            base: &base,
            sources: &sources,
        },
    )
}

/// `xtask test-causality --since <base>` - the ship-check and CI entry point.
pub(crate) fn run(args: &[String]) -> Verdict {
    let Some(base) = base_ref(args) else {
        eprintln!("xtask test-causality: usage: --since <base-ref>");
        return Verdict::Usage;
    };

    let Some(root) = repo::root() else {
        eprintln!("xtask test-causality: could not determine the repo root");
        return Verdict::Fail;
    };

    // WHICH COMMIT, and it is DERIVED rather than taken. `git diff <ref>` compares the tree at
    // whatever that ref points to NOW, so a base branch that has moved puts commits this branch
    // never made into the diff - the gate reverts them and measures a tree nobody proposed. Both
    // other venues resolved a merge base before calling; this recipe passed the ref through, so
    // the one venue a person runs by hand was the one that could measure the wrong commits.
    // Idempotent for a commit already behind HEAD, which is how scoping the gate per commit works.
    let Some(asked_for) = Commit::parse(&worktree::merge_base(&root, &base, "HEAD")) else {
        eprintln!("xtask test-causality: could not resolve a merge base between `{base}` and HEAD");
        eprintln!("  Fetch that ref, or name a commit this branch descends from.");
        return Verdict::Fail;
    };
    // HEAD'S OWN COMMIT, for the guard that needs it and for nothing else. A recorded parent that
    // already CONTAINS this branch forks at HEAD, and a base equal to HEAD makes the diff the
    // uncommitted working tree alone - an exit-0 pass over every file the branch changed, which
    // review reproduced twice here. `merge_base` above already resolved HEAD, so this cannot
    // realistically fail; refusing is the fail-closed direction if it ever does, because the
    // alternative is deriving a base with the one guard that matters unable to run.
    let Some(head) = Commit::parse(&worktree::head_commit(&root)) else {
        eprintln!("xtask test-causality: could not resolve HEAD to one commit");
        return Verdict::Fail;
    };
    let parent = stack_parent(&root, &asked_for);
    let measured = Base::of(asked_for, &head, parent);
    let at = measured.at().clone();

    let Some(files) = changed_with_additions(&at) else {
        // Same rule as classify: an unusable base ref is not evidence of nothing to do.
        eprintln!("xtask test-causality: could not diff against `{base}`");
        return Verdict::Fail;
    };

    // NAMED BEFORE ANY BRANCH RUNS, so every verdict below is qualified by it - the ref asked for,
    // the commit it resolved to, and which of those two the derivation chose. `stack` owns why the
    // ref's own merge base is the WRONG default on the second branch of a stack, and the sentence
    // comes out of the same value the commit does, so it cannot claim a narrowing that did not
    // happen.
    println!("{}", measured.measured(&base));

    // The POST-IMAGE of a changed file is what says which of its lines are test code, and
    // `git diff <base> --` compares base against the WORKING TREE - so the working tree is the
    // post-image, and reading it needs no second git call.
    let working_tree = |path: &str| std::fs::read_to_string(root.join(path)).ok();

    // WHAT A MANIFEST DIFF PUT INTO THE BUILD, asked before the plan because the plan cannot see
    // it: a `Cargo.toml`-only diff has no changed test file, so `Plan::NotRequired` used to pass
    // with *nothing to prove* over a feature declaration that compiled a whole module of
    // pre-existing tests. `features` reads the tables on both sides rather than the diff's lines.
    //
    // BEFORE the plan is a DECISION, so it is stated: `Scan::Unreadable` refuses ahead of every
    // other `.rs` answer for its own reason, and this now sits ahead of that. Both are
    // `Verdict::Fail` and each prints its own cause, so the order decides which cause an author is
    // shown and not the verdict. It goes first because a `Cargo.toml`-only diff reaches no other
    // refusal at all, which is the finding.
    match feature_activation(&root, &at, &files, &working_tree) {
        Activation::Nothing => {}
        Activation::Enables(refused) => return report_enabled_tests(&refused),
        Activation::Unread(unread) => return report_unread_manifests(&unread),
    }

    // THE BASE IMAGE, because a REMOVED line exists in no other one. Two consumers: `reverted`
    // asks whether a reverted line sat inside a test region of the tree it restores, and
    // `relocation` asks the same of every line a declared cleanup took out.
    let base_tree = |path: &str| worktree::at_base(&root, &at, path);

    // A COMMIT MAY DECLARE ITSELF A TEST-FILE CLEANUP, and the trailer is a CLAIM this CHECKS
    // rather than a permission that replaces the check - a trailer nothing verifies is the gate
    // that has quietly stopped gating. Ahead of the plan because a pure relocation has nothing for
    // the plan to partition: it added no test that any tree could be red against, so refusing is
    // the gate answering a question the diff does not ask.
    //
    // AFTER the feature refusal above and not before it, which is a decision and so is stated:
    // both are `Verdict::Fail` and the order only decides which cause an author is shown, and a
    // manifest in the diff is a `Broken::NotRust` here - so putting this first would shadow the
    // narrower, earlier-established answer with a broader one.
    //
    // `relocation` carries the four conditions, why the carrier is a commit trailer, and what a
    // pure-relocation verdict does NOT prove.
    //
    // GIT'S OWN PATH LIST IS A SECOND INPUT, and not a convenience: `changed_with_additions` builds
    // from the POST-image, so a file this branch DELETED has no entry in it at all. Without the
    // list a `git rm` of a test file rides a claim that every changed line was accounted for, over
    // lines nothing ever read.
    match relocation::decide(
        Claim::of(&worktree::messages(&root, &at)).as_ref(),
        &relocation::Changed {
            files: &files,
            touched: &worktree::touched(&root, &at),
        },
        &Images {
            head: &working_tree,
            base: &base_tree,
        },
    ) {
        Relocation::Unclaimed => {}
        Relocation::Pure { paths, exempt } => {
            return relocation::report_pure(&paths, &exempt, &Coverage::of(&[], &files, &working_tree));
        }
        Relocation::Refused(broken) => return relocation::report_refused(&broken),
    }

    match plan(&files, &working_tree) {
        Plan::NotRequired => {
            println!("xtask test-causality: no changed tests - nothing to prove");
            Verdict::Pass
        }
        Plan::NotSeparable {
            files: inseparable,
            build_inputs,
        } => {
            // `github.com/telekom/sutura#837` DIRECTION 2: a complete declaration is CONSULTED
            // even here, not merely mentioned. The claim arm needs no revert at all - `claim::run`
            // mutates and re-runs at HEAD, never at base - so inseparability does not disqualify
            // it; only whether the diff's own added tests can be NAMED does, which is the same
            // `Scan::of` the `Plan::Separable` arm below already asks of its own test files.
            //
            // A Scan outcome other than `Runnable` (unnameable, an enabling declaration, all
            // ignored) falls through to the unconsulted-declaration line unchanged: this arm
            // cannot build the `Scoped` value `claim::run` needs, so it has not reached the claim
            // arm either, and says so exactly as it did before this decision.
            let claim = claim::Claim::of(&worktree::messages(&root, &at));
            if let Some(ref declared) = claim
                && let Scan::Runnable(scoped) = Scan::of(&files, &inseparable, &working_tree)
            {
                return claim::run(&root, &scoped, &inseparable, declared, claim::Caller::TEST_CAUSALITY);
            }
            report_not_separable(
                &inseparable,
                &Coverage::of(&[], &files, &working_tree),
                &build_inputs,
                claim.is_some(),
            )
        }
        Plan::Separable(separable) => {
            if separable.revert.is_empty() {
                // Same string check as the arm above: an incomplete claim declaration (trailer
                // committed, patch not) has an empty `revert` and lands here rather than at
                // `Plan::NotSeparable`, so it needs the same unconsulted-declaration line.
                let claim_declared = claim::Claim::of(&worktree::messages(&root, &at)).is_some();
                return report_nothing_to_revert(
                    &Coverage::of(&[], &files, &working_tree),
                    &separable.build_inputs,
                    claim_declared,
                );
            }
            match Scan::of(&files, &separable.test_files, &working_tree) {
                Scan::Runnable(scoped) => {
                    // A COMMIT MAY DECLARE A CLAIM CELL, and the trailer is a CLAIM this CHECKS
                    // rather than a permission that replaces the check. A claim cell is an added
                    // test that PINNS behaviour the base tree already provides, so the base run is
                    // green and the only honest proof is a mutation: break the behaviour, one cell
                    // at a time, and require the test to redden. The declaration is a bijection
                    // with the diff's added tests, each cell needs a committed killing mutation at
                    // `devco/claim-mutations/<test-fn-name>.patch`, and the whole arm takes the
                    // place of the normal proof - the declared set is what may pass, and nothing
                    // undeclared rides along. `claim` carries the conditions, the patched runs and
                    // what an accepted arm does and does not prove.
                    //
                    // After feature-activation and relocation and nowhere before, for the same
                    // reason both were: a manifest in the diff or a conflicting trailer is a
                    // narrower, earlier-established answer and a claim-cell here would shadow it.
                    // A claim-cell diff is `Relocation::Unclaimed` - the two trailers are mutually
                    // exclusive, and the range reads one carrier.
                    if let Some(claim) = claim::Claim::of(&worktree::messages(&root, &at)) {
                        return claim::run(&root, &scoped, &separable.test_files, &claim, claim::Caller::TEST_CAUSALITY);
                    }
                    let coverage = Coverage::of(scoped.tests(), &files, &working_tree);
                    // WHAT A GREEN BASE RUN WOULD MEAN, decided from the partition before either
                    // run rather than read off the run. `reverted` owns the argument; the point of
                    // asking here is that `separable` is the last place both halves of it exist -
                    // what is being put back, and which files' tests are being measured.
                    // THE BASE IMAGE IS A SECOND READER, because a removed line exists only
                    // there: `reverted` asks whether it sat inside a test region of the tree the
                    // revert restores, and the post-image cannot answer a question about a line it
                    // does not contain. Bound once above, where `relocation` needs the same reader.
                    let reach = Attempts::of(
                        &separable.revert,
                        &separable.held(),
                        &separable.test_files,
                        &files,
                        &working_tree,
                        &base_tree,
                    );
                    prove(&root, &at, &separable, &scoped, &coverage, &reach, &files, &working_tree)
                }
                Scan::Unreadable(files) => report_unreadable(&files),
                Scan::Enabled(refused) => report_enabled_tests(&refused),
                // The names come from this arm and the RATIO from the whole-diff scan, which is
                // the half `github.com/telekom/sutura#314` was about: this arm formatted its own
                // `0 of N` while every other arm printed `0 of 0` and named none of them.
                Scan::OnlyIgnored(names) => report_only_ignored(&names, &Coverage::of(&[], &files, &working_tree)),
                Scan::Unnamed => report_unnamed_tests(&separable.test_files),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::base::BaseOutcome;
    use super::{BaseState, retry_with_held_back};
    use crate::Verdict;

    /// One unique temp directory per real-git test, so paths never collide under nextest.
    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn git(dir: &std::path::Path, args: &[&str]) {
        let out = git_output(dir, args);
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A `git` invocation in `dir`, with the caller's git environment stripped.
    ///
    /// `strip_git_env` is not optional here: the pre-commit hook runs `just test` with
    /// `GIT_DIR`/`GIT_INDEX_FILE` pointing at the OUTER repo's own commit, and an unstripped
    /// `rev-parse`/`diff` inside this fixture reads THAT repo instead of `dir` - measured live,
    /// where `base` came back as the outer repo's real HEAD and `run` refused to resolve a merge
    /// base against it.
    fn git_output(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
        let mut command = Command::new("git");
        crate::repo::strip_git_env(&mut command);
        command.current_dir(dir).args(args).output().expect("git runs")
    }

    /// `causality::run` DISPATCHES to `claim::run` rather than merely being able to. Nothing else
    /// in this module exercises that: `claim::tests` calls `claim::run` directly, and the round
    /// reviews of #790 name this exact seam as "read by review, not measured by a cell". Proven
    /// by mutation, not merely by asserting `Verdict::Pass`: with the `if let ... { return
    /// claim::run(..) }` neutralised by `&& false`, this same fixture answers `Inconclusive`
    /// instead (the ordinary proof has nothing in reach to revert against `the_wired_one`) - a
    /// tiny real cargo crate, not the string-only fixtures `claim::tests` uses, because
    /// `kill_cell` runs a real `cargo test`.
    #[test]
    fn run_dispatches_to_the_claim_arm() {
        // `set_current_dir` is process-global - see `falsifier`'s own use of this guard.
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this test moves the process's current directory, so it must have the process to \
             itself: run it under `just test`."
        );

        let dir = std::env::temp_dir().join(format!(
            "sutura-causality-wiring-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _swept = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "test"]);

        let head_content = "pub fn f() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    use super::f;\n\n    #[test]\n    fn the_wired_one() {\n        assert_eq!(f(), 1);\n    }\n}\n";
        // `git diff --no-index` needs no repo and no commits, so the mutation's PATCH TEXT is
        // built before either commit - a hand-written single-line hunk (`@@ -1 +1 @@`, no
        // context) only applies when the whole file IS that one line, and `git apply` refuses it
        // once the test module's lines follow. This keeps the patch file's own commit OUT of the
        // measured base..HEAD diff (only `src/lib.rs`'s test addition is in it): a NEW non-`.rs`
        // file in that diff has no base image to restore, which is a DIFFERENT Pass arm
        // (`NO BASE BEHAVIOUR TO COMPARE AGAINST`) that would make this test pass for the wrong
        // reason once the wiring is removed.
        std::fs::write(dir.join(".old.rs"), head_content).unwrap();
        std::fs::write(dir.join(".new.rs"), head_content.replacen("{ 1 }", "{ 2 }", 1)).unwrap();
        let diffed = git_output(&dir, &["diff", "--no-index", "--", ".old.rs", ".new.rs"]);
        let patch = String::from_utf8_lossy(&diffed.stdout)
            .replace(".old.rs", "src/lib.rs")
            .replace(".new.rs", "src/lib.rs");
        std::fs::remove_file(dir.join(".old.rs")).unwrap();
        std::fs::remove_file(dir.join(".new.rs")).unwrap();

        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[profile.ci]\ninherits = \"dev\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
        // An UNRELATED impl-only file, changed alongside the claim cell: `run` returns
        // `Verdict::Pass` before ever consulting `Claim::of` when `Plan::Separable`'s `revert`
        // set is empty (a pure test-only diff has nothing else to prove causal, by design), so a
        // diff with no OTHER changed file never reaches the claim arm at all. A real branch that
        // declares a claim cell is never JUST that test; this fixture keeps that true too.
        std::fs::write(dir.join("src/other.rs"), "pub fn g() -> u8 { 9 }\n").unwrap();
        std::fs::create_dir_all(dir.join("devco/claim-mutations")).unwrap();
        std::fs::write(dir.join("devco/claim-mutations/the_wired_one.patch"), &patch).unwrap();
        // `repo::root` requires flake.nix ALONGSIDE Cargo.toml to stop its walk here rather than
        // falling back to this process's own compile-time manifest dir - the real sutura repo.
        std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout)
            .expect("utf8")
            .trim()
            .to_owned();

        std::fs::write(dir.join("src/lib.rs"), head_content).unwrap();
        std::fs::write(dir.join("src/other.rs"), "// unrelated\npub fn g() -> u8 { 9 }\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(
            &dir,
            &[
                "commit",
                "-q",
                "-m",
                "feat: pin f's existing return value\n\nClaim-Cell: the_wired_one",
            ],
        );

        let original = std::env::current_dir().expect("a current directory");
        std::env::set_current_dir(&dir).expect("point the process at the fixture repo");
        let verdict = super::run(&[String::from("--since"), base]);
        std::env::set_current_dir(&original).expect("restore the current directory");
        drop(std::fs::remove_dir_all(&dir));

        // `Pass` here is reachable only through `claim::run`'s own accepted arm - see the
        // mutation quoted on this test's own doc comment for the other side.
        assert_eq!(
            verdict,
            Verdict::Pass,
            "causality::run must dispatch a declared claim cell to claim::run, which accepts it"
        );
    }

    /// Which mutation `inseparable_claim_case` commits, if any.
    #[derive(Clone, Copy)]
    enum Mutation {
        /// Changes `f`'s return value again, so the cell's own `assert_eq!(f(), 2)` fails.
        Kills,
        /// Touches the production line without changing `f`'s return value, so the cell stays
        /// green - applies cleanly and kills nothing.
        DoesNotKill,
        /// No patch is committed at all.
        Missing,
    }

    /// `github.com/telekom/sutura#837` direction 2's own fixture: ONE file, `src/lib.rs`, carries
    /// both an implementation change (`f`'s return value moves from 1 to 2) and its own
    /// `#[cfg(test)] mod tests` in the SAME commit - no other file changes at all - so `plan()`
    /// has no separable test file and `causality::run` reaches `Plan::NotSeparable`. Before this
    /// decision that arm returned `Verdict::Pass` unconditionally, so `Verdict::Fail` from any
    /// case here is reachable ONLY through the new dispatch into `claim::run`.
    fn inseparable_claim_case(declare: bool, mutation: Mutation) -> Verdict {
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this fixture moves the process's current directory, so it must have the process to \
             itself: run it under `just test`."
        );
        let dir = std::env::temp_dir().join(format!(
            "sutura-causality-inseparable-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _swept = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "test"]);

        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[profile.ci]\ninherits = \"dev\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
        std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout)
            .expect("utf8")
            .trim()
            .to_owned();

        let head_content = "pub fn f() -> u8 { 2 }\n\n#[cfg(test)]\nmod tests {\n    use super::f;\n\n    #[test]\n    fn the_wired_one() {\n        assert_eq!(f(), 2);\n    }\n}\n";
        std::fs::write(dir.join("src/lib.rs"), head_content).unwrap();

        let mutated: Option<String> = match mutation {
            Mutation::Kills => Some(head_content.replacen("{ 2 }", "{ 9 }", 1)),
            Mutation::DoesNotKill => Some(head_content.replacen("pub fn f() -> u8 { 2 }", "pub fn f() -> u8 { 2 } // same", 1)),
            Mutation::Missing => None,
        };
        if let Some(mutated) = mutated {
            // Same technique as `run_dispatches_to_the_claim_arm`'s patch: a hand-diffed pair of
            // files renamed onto `src/lib.rs`, so the mutation's own commit stays out of the
            // measured `base..HEAD` range.
            std::fs::write(dir.join(".old.rs"), head_content).unwrap();
            std::fs::write(dir.join(".new.rs"), &mutated).unwrap();
            let diffed = git_output(&dir, &["diff", "--no-index", "--", ".old.rs", ".new.rs"]);
            let patch = String::from_utf8_lossy(&diffed.stdout)
                .replace(".old.rs", "src/lib.rs")
                .replace(".new.rs", "src/lib.rs");
            std::fs::remove_file(dir.join(".old.rs")).unwrap();
            std::fs::remove_file(dir.join(".new.rs")).unwrap();
            std::fs::create_dir_all(dir.join("devco/claim-mutations")).unwrap();
            std::fs::write(dir.join("devco/claim-mutations/the_wired_one.patch"), &patch).unwrap();
        }

        git(&dir, &["add", "-A"]);
        let message = if declare {
            "feat: pin f's changed return value\n\nClaim-Cell: the_wired_one"
        } else {
            "feat: change f's return value and add its own test"
        };
        git(&dir, &["commit", "-q", "-m", message]);

        let original = std::env::current_dir().expect("a current directory");
        std::env::set_current_dir(&dir).expect("point the process at the fixture repo");
        let verdict = super::run(&[String::from("--since"), base]);
        std::env::set_current_dir(&original).expect("restore the current directory");
        drop(std::fs::remove_dir_all(&dir));
        verdict
    }

    /// `github.com/telekom/sutura#837` direction 2, half one: a complete declaration on an
    /// inseparable diff is EVALUATED, and a mutation that kills by the cell's own assertion is
    /// accepted.
    ///
    /// NOT DISCRIMINATING ALONE, stated rather than hidden: the old `Plan::NotSeparable`
    /// fallback (`report_not_separable`) also answers `Verdict::Pass`, unconditionally, so this
    /// assertion by itself cannot be shown red-on-base - confirmed by hand: neutralising the
    /// dispatch guard below with `&& false` leaves this test green. The causal evidence that the
    /// NEW dispatch, not the old fallback, is what answers here is the CONTRAST with the two
    /// tests after it: the same fixture shape with an invalid declaration
    /// (`Mutation::DoesNotKill`, `Mutation::Missing`) answers `Verdict::Fail` under the same
    /// neutralising mutation - reversed to green - which an unconditional fallback could never
    /// do. A single arm sensitive to the patch's validity is not reachable through one that is
    /// not.
    #[test]
    fn a_declared_claim_cell_is_evaluated_from_an_inseparable_plan() {
        assert_eq!(
            inseparable_claim_case(true, Mutation::Kills),
            Verdict::Pass,
            "a complete declaration on an inseparable diff must be consulted, and its killing \
             mutation accepted"
        );
    }

    /// The arm is reached, not merely declared: a mutation that applies but does not kill is
    /// refused rather than passing by the old unconsulted route (`Verdict::Pass`, unconditionally,
    /// before this decision). CONFIRMED BY HAND: neutralising the dispatch guard with `&& false`
    /// turns this back into `Verdict::Pass` - this is one of the two tests that discriminate the
    /// new dispatch from the old fallback.
    #[test]
    fn an_inseparable_claim_cell_whose_mutation_does_not_kill_is_refused() {
        assert_eq!(
            inseparable_claim_case(true, Mutation::DoesNotKill),
            Verdict::Fail,
            "reaching the arm and finding the mutation does not kill must refuse - the old route \
             answered `Verdict::Pass` unconditionally here"
        );
    }

    /// `github.com/telekom/sutura#837` direction 2's own guard: a declaration with no committed
    /// patch is `Cause::MissingPatch`, not treated as though nothing were declared. CONFIRMED BY
    /// HAND: neutralising the dispatch guard with `&& false` turns this back into
    /// `Verdict::Pass` too - the second of the two discriminating tests.
    #[test]
    fn an_inseparable_claim_cell_with_no_committed_patch_is_refused_as_missing() {
        assert_eq!(
            inseparable_claim_case(true, Mutation::Missing),
            Verdict::Fail,
            "a declared cell with no patch must refuse as missing - the old route answered \
             `Verdict::Pass` unconditionally here"
        );
    }

    /// Half two, and UNCHANGED behaviour: with no `Claim-Cell:` trailer at all, an inseparable
    /// plan still answers exactly the non-verdict it did before this decision. Not provable by
    /// causality - the base tree already gives this same answer, by construction - so this pins
    /// by inspection rather than by a mutation: `causality::run`'s `if let Some(..) = claim` guard
    /// is `None` here and the new code path is never entered at all.
    #[test]
    fn an_inseparable_plan_with_no_declaration_stays_the_same_non_verdict() {
        assert_eq!(
            inseparable_claim_case(false, Mutation::Missing),
            Verdict::Pass,
            "no `Claim-Cell:` trailer at all must still be the loud NOT MECHANICALLY SEPARABLE pass"
        );
    }

    #[test]
    fn only_a_tree_that_did_not_compile_is_asked_twice() {
        let one = String::from("crates/x/src/pinned.rs");
        let held = BaseState {
            restore: vec![&one],
            remove: Vec::new(),
        };
        let nothing_held = BaseState {
            restore: Vec::new(),
            remove: Vec::new(),
        };
        // The case the second attempt exists for.
        assert!(retry_with_held_back(&BaseOutcome::DidNotCompile, &held));
        // Nothing is held at HEAD, so a second attempt would reconstruct the same tree and fail
        // the same way. One build, not two.
        assert!(!retry_with_held_back(&BaseOutcome::DidNotCompile, &nothing_held));
        // A run that reached an assertion has ANSWERED. Retrying would revert the implementation
        // whose absence that assertion just measured, turning evidence into a different question.
        assert!(!retry_with_held_back(
            &BaseOutcome::RedByAssertion { failed: Vec::new() },
            &held
        ));
        assert!(!retry_with_held_back(&BaseOutcome::Green, &held));
    }
}
