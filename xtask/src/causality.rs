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
//! the file rather than by what the added set happens to contain. That second module's own doc
//! records the defect the earlier shape produced and the limits of what replaced it - a
//! tests-only branch reported as "changes behaviour and adds tests in one file", which is a
//! different message asking for a different thing.
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
//! shared test binary (see [`cargo_test`]), so one tree answered differently in two venues and
//! neither answer looked wrong. Scoping removes the first dependence and `--no-fail-fast` the
//! second.
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

mod attributes;
mod base;
mod coverage;
mod diff;
#[cfg(test)]
mod fixtures;
// `pub(crate)` rather than private: `crate::refusals` reads the same test regions this gate does,
// because "which lines of this file are test code" is one question and a second implementation of
// it would be a second thing to keep in step. Nothing else about the module moved.
pub(crate) mod regions;
mod remedies;
mod runner;
mod scoped;
mod worktree;

use attributes::{Adds, adds};
use base::{BaseOutcome, classify_base, report_base, tail};
use coverage::{Coverage, Scope};
use diff::{ChangedFile, changed_with_additions};
use regions::{PostImage, has_non_test_additions, scope};
use remedies::{report_coverage, report_head_failure, report_not_separable, report_only_ignored, report_unnamed_tests};
use runner::{Tree, cargo_test};
use scoped::{Scan, Scoped};
use worktree::{BaseState, add_worktree, apply, base_state, remove_worktree};

/// What the gate concluded, so the shape is testable without git or cargo.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// No changed tests: nothing to prove.
    NotRequired,
    /// Baseline can be reconstructed by reverting [`Separable::revert`].
    Separable(Separable),
    /// Impl and tests share a file; a human must state the evidence.
    NotSeparable { files: Vec<String> },
}

/// The four groups the reconstruction sorts a diff's Rust files into.
///
/// A struct rather than positional arguments, because two of the four are told apart only by the
/// sentence printed beside them and a reader has to be able to see which is which.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Separable {
    /// Restored to base: nothing they added is test code, so together they ARE the old behaviour
    /// a test has to be red against.
    pub(crate) revert: Vec<String>,
    /// Kept at HEAD and MEASURED: the tests these added are the proof.
    pub(crate) test_files: Vec<String>,
    /// Kept at HEAD with their own tests OUT of the proof: each adds an implementation change and
    /// a test in one file, so reverting it would remove the test along with the fix. Carried
    /// rather than dropped, because reverting the others while these stay at HEAD is what leaves
    /// a tree mixing two versions of one API - the second attempt needs them.
    pub(crate) held_back: Vec<String>,
    /// Kept at HEAD, with nothing in them to measure: everything they added is `#[cfg(test)]`
    /// code that names no test. Reverting one takes a helper the held tests call out of the base
    /// tree, and requiring it to name a test is the refusal [`attributes`] records.
    pub(crate) test_only: Vec<String>,
}

impl Separable {
    /// Every file kept at HEAD on the first attempt whose own tests the proof does not measure.
    ///
    /// One list because the second attempt restores them together: that retry exists to put the
    /// tree coherently at base, and a held test helper calling a reverted neighbour is exactly a
    /// thing that does not compile until it goes too.
    fn held(&self) -> Vec<String> {
        self.held_back.iter().chain(self.test_only.iter()).cloned().collect()
    }
}

/// Is this a Rust source path THIS workspace compiles?
///
/// Case-insensitive on the extension, since a case-sensitive test is wrong on a case-insensitive
/// filesystem. And not a path outside every workspace member: `vendor/mimalloc_rust` is
/// `exclude`d in the root manifest and carries its own `#[test]`s, so a vendor bump touching one
/// would otherwise be a changed test whose key names a package `--workspace` never builds -
/// nextest refuses an unknown `package(=..)` outright, so the gate would redden a correct change.
/// `changes::is_non_member` already answers that question for the compile-check gate.
fn is_compiled_rust(path: &str) -> bool {
    !crate::changes::is_non_member(path)
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
}

/// Split changed Rust files into "added tests" and "changed implementation only".
///
/// `read` returns a file's POST-IMAGE by repo-relative path - the working tree in the gate, a
/// fixed map in the tests. It is what makes the test-region question answerable at all.
pub(crate) fn plan(files: &[ChangedFile], read: &PostImage<'_>) -> Plan {
    let mut test_files = Vec::new();
    let mut test_only = Vec::new();
    let mut impl_only = Vec::new();

    for file in files.iter().filter(|file| is_compiled_rust(&file.path)) {
        match adds(&file.added, &file.path, read) {
            // A marker names no function, and it stays a candidate for the proof on purpose: that
            // is the shape whose unnameable test is a deliberate refusal.
            Adds::NamedTest | Adds::TestModule => test_files.push(file.path.clone()),
            Adds::TestOnlyItem => test_only.push(file.path.clone()),
            Adds::Nothing => impl_only.push(file.path.clone()),
        }
    }

    if test_files.is_empty() {
        // A `#[cfg(test)]` helper is not a test, so a diff that added none has nothing to prove -
        // the same answer this gate gives any implementation change with no new test. Calling it a
        // test file instead produced a refusal the author could not act on, because no extractor
        // improvement can read a name off an item that is not a test.
        return Plan::NotRequired;
    }

    // A file that gained BOTH a test and non-test changes cannot be split by reverting whole
    // files. Detect it by asking whether any added line lands outside the file's test regions.
    let inseparable: Vec<String> = files
        .iter()
        .filter(|file| test_files.contains(&file.path) && has_non_test_additions(&file.added, &scope(&file.path, read)))
        .map(|file| file.path.clone())
        .collect();

    // A test in an inseparable file cannot be proven by reverting OTHER files: its own
    // implementation sits in the same file, and that file is not reverted precisely because it
    // holds the tests. So those tests are excluded from the proof rather than counted in it.
    //
    // The condition here was `!inseparable.is_empty() && impl_only.is_empty()`, which was wrong
    // in a way that made the gate lie. With even one separable impl file present it took the
    // Separable path, reverted that file, ran the inseparable tests anyway - and they passed,
    // because nothing they exercise had been reverted. It then reported "green against base
    // behaviour" and blamed the author for a partition the gate itself had chosen. Adding two
    // new gate modules, each impl and tests in one new file, alongside an edit to `main.rs` is
    // exactly that shape.
    let provable: Vec<String> = test_files.iter().filter(|p| !inseparable.contains(p)).cloned().collect();

    if provable.is_empty() {
        return Plan::NotSeparable { files: inseparable };
    }

    Plan::Separable(Separable {
        revert: impl_only,
        test_files: provable,
        held_back: inseparable,
        test_only,
    })
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
fn prove(root: &Path, base: &str, separable: &Separable, scoped: &Scoped, coverage: &Coverage) -> Verdict {
    let first = base_state(root, base, &separable.revert);
    let holding = separable.held();
    let held = base_state(root, base, &holding);

    if first.restore.is_empty() {
        println!("xtask test-causality: NO BASE BEHAVIOUR TO COMPARE AGAINST");
        for f in &first.remove {
            println!("  {f} does not exist at {base}");
        }
        println!();
        println!("Every changed implementation file is new here, so there is no old behaviour");
        println!("for a test to be red against. Reverting them would leave a tree that does not");
        println!("compile, and a test that fails to compile proves nothing about behaviour.");
        println!("This gate has NOT verified causality for this change - state the evidence in");
        println!("the handoff if it is a bug fix.");
        return Verdict::Pass;
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
    for f in &separable.held_back {
        println!("  held:      {f}  (carries its own tests)");
    }
    for f in &separable.test_only {
        println!("  held:      {f}  (test-only code that names no test)");
    }

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
    report_coverage(coverage);
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
        &Scope { only: &only, coverage },
    );
    remove_worktree(root, &wt);
    verdict
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
    base: &str,
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
    let outcome = classify_base(&base_out, base_ok, scoped.tests());

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
            &classify_base(&retry_out, retry_ok, scoped.tests()),
            &retry_out,
            true,
            &scope.coverage.ratio(),
        );
    }

    report_base(&outcome, &base_out, false, &scope.coverage.ratio())
}

/// Should the proof ask a second time, with the held-back files at base too?
///
/// Only for a tree that did not COMPILE, and only while something is still held at HEAD. A base
/// run that reached an assertion has answered the question and is not retried - retrying it would
/// change the answer by reverting the very implementation whose absence the assertion measured.
const fn retry_with_held_back(outcome: &BaseOutcome, held: &BaseState<'_>) -> bool {
    matches!(outcome, BaseOutcome::DidNotCompile) && !held.is_empty()
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

    let Some(files) = changed_with_additions(&base) else {
        // Same rule as classify: an unusable base ref is not evidence of nothing to do.
        eprintln!("xtask test-causality: could not diff against `{base}`");
        return Verdict::Fail;
    };

    // WHICH BASE, named before any branch runs, so every verdict below is qualified by it. The
    // verdict is a FUNCTION of this ref and one of the ways it can be wrong is not visible in the
    // output otherwise: on the second branch of a stack the default pairs this branch's tests with
    // the parent branch's implementation, reverts that, finds the tests green and fails - a defect
    // that does not exist, reported about two halves that do not read each other. Naming the ref
    // is not a fix for that, and stating a limit is not the same as reaching it.
    println!("xtask test-causality: measuring the diff against `{base}`");

    // The POST-IMAGE of a changed file is what says which of its lines are test code, and
    // `git diff <base> --` compares base against the WORKING TREE - so the working tree is the
    // post-image, and reading it needs no second git call.
    let working_tree = |path: &str| std::fs::read_to_string(root.join(path)).ok();

    match plan(&files, &working_tree) {
        Plan::NotRequired => {
            println!("xtask test-causality: no changed tests - nothing to prove");
            Verdict::Pass
        }
        Plan::NotSeparable { files: inseparable } => {
            report_not_separable(&inseparable, &Coverage::of(&[], &files, &working_tree))
        }
        Plan::Separable(separable) => {
            if separable.revert.is_empty() {
                println!("xtask test-causality: tests changed but no implementation did");
                println!("  Nothing to revert, so there is no old behaviour to be red against.");
                println!("  If this is a new test for existing behaviour, say so; it is not a");
                println!("  regression test and this gate cannot prove it is causal.");
                return Verdict::Pass;
            }
            match Scan::of(&files, &separable.test_files, &working_tree) {
                Scan::Runnable(scoped) => {
                    let coverage = Coverage::of(scoped.tests(), &files, &working_tree);
                    prove(&root, &base, &separable, &scoped, &coverage)
                }
                Scan::OnlyIgnored(names) => report_only_ignored(&names),
                Scan::Unnamed => report_unnamed_tests(&separable.test_files),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::base::BaseOutcome;
    use super::coverage::Coverage;
    use super::fixtures::{changed, manifest, tree};
    use super::scoped::Scan;
    use super::{BaseState, Plan, plan, retry_with_held_back};

    #[test]
    fn no_changed_tests_means_nothing_to_prove() {
        let files = vec![changed("src/a.rs", 1, &["fn f() {}"])];
        assert_eq!(plan(&files, &tree(&[])), Plan::NotRequired);
    }

    #[test]
    fn separates_impl_from_test_file() {
        let files = vec![
            changed("crates/x/src/a.rs", 2, &["fn fixed() -> u8 { 2 }"]),
            changed("crates/x/tests/t.rs", 1, &["#[test]", "fn t() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/a.rs", "// header\nfn fixed() -> u8 { 2 }\n"),
            ("crates/x/tests/t.rs", "#[test]\nfn t() {}\n"),
        ]);
        match plan(&files, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/a.rs")]);
                assert_eq!(one.test_files, vec![String::from("crates/x/tests/t.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn one_file_with_both_is_reported_not_guessed() {
        // The honest case: a fix and its test in one file cannot be split by reverting the
        // file, so the gate must say so rather than pass or fail arbitrarily. Line 1 is the fix
        // and sits outside the test module that lines 2..6 hold.
        let files = vec![changed(
            "crates/x/src/a.rs",
            1,
            &["fn fixed() -> u8 { 2 }", "#[cfg(test)]", "mod tests {", "    #[test]"],
        )];
        let read = tree(&[(
            "crates/x/src/a.rs",
            "fn fixed() -> u8 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
        )]);
        match plan(&files, &read) {
            Plan::NotSeparable { files } => {
                assert_eq!(files, vec![String::from("crates/x/src/a.rs")]);
            }
            other => panic!("expected NotSeparable, got {other:?}"),
        }
    }

    #[test]
    fn an_unrelated_impl_file_does_not_make_an_inseparable_one_provable() {
        // The bug this replaced: a change that adds a new module (impl and tests in one file)
        // AND edits an unrelated impl file took the Separable path, reverted only the unrelated
        // file, then failed the author because the new module's tests still passed - which they
        // could not help doing, since their own implementation was never reverted.
        let files = vec![
            changed(
                "xtask/src/workflows.rs",
                1,
                &["fn collect() {}", "#[cfg(test)]", "mod tests {", "    #[test]"],
            ),
            changed("xtask/src/main.rs", 1, &["mod workflows;"]),
        ];
        let read = tree(&[
            (
                "xtask/src/workflows.rs",
                "fn collect() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
            ("xtask/src/main.rs", "mod workflows;\n"),
        ]);
        match plan(&files, &read) {
            Plan::NotSeparable { files } => {
                assert_eq!(files, vec![String::from("xtask/src/workflows.rs")]);
            }
            other => panic!("expected NotSeparable, got {other:?}"),
        }
    }

    #[test]
    fn a_test_only_addition_in_one_file_is_separable() {
        // Only test code added: nothing to revert in that file, so it is not "inseparable".
        let files = vec![
            changed(
                "crates/x/src/a.rs",
                2,
                &["#[cfg(test)]", "mod tests {", "    #[test]", "    fn t() {}", "}"],
            ),
            changed("crates/x/src/b.rs", 1, &["fn fixed() {}"]),
        ];
        let read = tree(&[
            (
                "crates/x/src/a.rs",
                "pub fn existing() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
            ("crates/x/src/b.rs", "fn fixed() {}\n"),
        ]);
        match plan(&files, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/b.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn tests_appended_to_existing_test_modules_are_not_an_implementation_change() {
        // THE DEFECT, at the level a caller sees. Two files gain tests INSIDE `#[cfg(test)] mod
        // tests` blocks that already existed, and no production line is touched. The markers are
        // unchanged diff context, so they never appear among the added lines - and the old
        // classifier, which read only those, called both files "changes behaviour and adds tests
        // in one file" and sent the author to the stated-evidence path. The correct answer is a
        // Separable plan with NOTHING to revert, which is `run`'s own tests-only branch: a
        // different message asking for a different thing.
        let commands = concat!(
            "pub fn open_engine() -> u8 {\n",             // 1
            "    1\n",                                    // 2
            "}\n",                                        // 3
            "#[cfg(test)]\n",                             // 4
            "mod tests {\n",                              // 5
            "    use super::open_engine;\n",              // 6
            "    #[test]\n",                              // 7
            "    fn existing() {}\n",                     // 8
            "    use sutura_domain::model::TableName;\n", // 9
            "    #[test]\n",                              // 10
            "    fn added() {\n",                         // 11
            "        let _ = TableName::parse(\"t\");\n", // 12
            "    }\n",                                    // 13
            "}\n",                                        // 14
        );
        let serve = concat!(
            "fn main() {}\n",         // 1
            "#[cfg(test)]\n",         // 2
            "mod tests {\n",          // 3
            "    #[test]\n",          // 4
            "    fn existing() {}\n", // 5
            "    #[test]\n",          // 6
            "    fn added() {}\n",    // 7
            "}\n",                    // 8
        );
        let files = vec![
            changed(
                "crates/sutura-cli/src/commands.rs",
                9,
                &[
                    "    use sutura_domain::model::TableName;",
                    "    #[test]",
                    "    fn added() {",
                    "        let _ = TableName::parse(\"t\");",
                    "    }",
                ],
            ),
            changed("crates/sutura-serve/src/main.rs", 6, &["    #[test]", "    fn added() {}"]),
        ];
        let read = tree(&[
            ("crates/sutura-cli/src/commands.rs", commands),
            ("crates/sutura-serve/src/main.rs", serve),
        ]);
        match plan(&files, &read) {
            Plan::Separable(ref one) => {
                assert!(one.revert.is_empty(), "no implementation changed: {:?}", one.revert);
                assert!(
                    one.held_back.is_empty(),
                    "nothing carries an implementation change: {:?}",
                    one.held_back
                );
                assert_eq!(
                    one.test_files.len(),
                    2,
                    "both files are provable test files: {:?}",
                    one.test_files
                );
            }
            other => panic!("expected Separable with nothing to revert, got {other:?}"),
        }
    }

    #[test]
    fn non_rust_files_are_ignored() {
        let files = vec![changed("README.md", 1, &["#[test]"])];
        assert_eq!(plan(&files, &tree(&[])), Plan::NotRequired);
    }

    #[test]
    fn a_vendored_test_is_not_a_changed_test_this_gate_can_measure() {
        // `vendor/mimalloc_rust` is `exclude`d from the root manifest and carries its own
        // `#[test]`s, so `--workspace` never builds it. Counted as a changed test, its package
        // reaches the filterset - and nextest REFUSES an unknown `package(=..)` rather than
        // matching nothing, so a vendor bump touching a `#[test]` line would redden a correct
        // change. `changes::is_non_member` is the same rule the compile-check gate uses.
        let files = vec![changed(
            "vendor/mimalloc_rust/src/lib.rs",
            1,
            &["    #[test]", "    fn allocates() {}"],
        )];
        assert_eq!(plan(&files, &tree(&[])), Plan::NotRequired);
    }

    #[test]
    fn a_file_carrying_its_own_tests_is_held_back_not_forgotten() {
        // The shape that used to end as INCONCLUSIVE in CI: one file holding an implementation
        // change AND its tests, one impl-only file to revert, and one dedicated test target to
        // prove. The plan is Separable - there is something to prove - and the file it cannot
        // prove is NAMED rather than dropped, because reconstructing a tree that compiles needs
        // it. Dropping it is what left base and HEAD versions of one API in the same tree.
        let files = vec![
            changed(
                "crates/x/src/pinned.rs",
                1,
                &[
                    "fn of(a: u8, b: u8) -> u8 { a }",
                    "#[cfg(test)]",
                    "mod tests {",
                    "    #[test]",
                ],
            ),
            changed("crates/x/src/definitions.rs", 1, &["fn changed() {}"]),
            changed("crates/x/tests/t.rs", 1, &["#[test]", "fn t() {}"]),
        ];
        let read = tree(&[
            (
                "crates/x/src/pinned.rs",
                "fn of(a: u8, b: u8) -> u8 { a }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
            ),
            ("crates/x/src/definitions.rs", "fn changed() {}\n"),
            ("crates/x/tests/t.rs", "#[test]\nfn t() {}\n"),
        ]);
        match plan(&files, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.revert, vec![String::from("crates/x/src/definitions.rs")]);
                assert_eq!(one.test_files, vec![String::from("crates/x/tests/t.rs")]);
                assert_eq!(one.held_back, vec![String::from("crates/x/src/pinned.rs")]);
                assert!(one.test_only.is_empty(), "no test-only file here: {:?}", one.test_only);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn a_cfg_test_helper_with_no_test_beside_it_adds_no_test() {
        // THE DEFECT, at the level `run` branches on. A file gains a `#[cfg(test)]` HELPER and no
        // `#[test]` anywhere in its hunk. The old classifier called it a test file on the bare
        // attribute, put it in the proof, then refused the whole diff with *the added tests could
        // not be NAMED* - which no author could act on, because the item can never name a test. A
        // helper is not a test, so there is nothing to prove: the answer this gate already gives
        // any implementation change that adds no test.
        let tasks = concat!(
            "pub(crate) fn recipe_names() -> u8 {\n",             // 1
            "    1\n",                                            // 2
            "}\n",                                                // 3
            "#[cfg(test)]\n",                                     // 4
            "pub(crate) fn recipe_body(name: &str) -> usize {\n", // 5
            "    name.len()\n",                                   // 6
            "}\n",                                                // 7
        );
        let files = vec![
            changed(
                "xtask/src/tasks.rs",
                4,
                &[
                    "#[cfg(test)]",
                    "pub(crate) fn recipe_body(name: &str) -> usize {",
                    "    name.len()",
                    "}",
                ],
            ),
            changed("xtask/src/other.rs", 1, &["fn changed() {}"]),
        ];
        let read = tree(&[
            ("xtask/src/tasks.rs", tasks),
            ("xtask/src/other.rs", "fn changed() {}\n"),
            ("xtask/Cargo.toml", &manifest("xtask")),
        ]);
        assert_eq!(plan(&files, &read), Plan::NotRequired);
    }

    #[test]
    fn a_cfg_test_helper_is_held_at_head_rather_than_reverted() {
        // The direction that matters more than the refusal it replaces, and the one a narrower
        // `adds_test` would have lost. The helper is never in `revert`: reverting it takes it out
        // of the base tree while the tests calling it are held there - `E0425`, `DidNotCompile`,
        // then a pass that proves nothing, over the COMMON case, since a new helper usually
        // exists because a new test needed it.
        let files = vec![
            changed("xtask/src/tasks.rs", 2, &["#[cfg(test)]", "fn helper() -> u8 { 1 }"]),
            changed("xtask/src/other.rs", 1, &["fn changed() {}"]),
            changed("xtask/tests/t.rs", 1, &["#[test]", "fn t() {}"]),
        ];
        let read = tree(&[
            ("xtask/src/tasks.rs", "fn kept() {}\n#[cfg(test)]\nfn helper() -> u8 { 1 }\n"),
            ("xtask/src/other.rs", "fn changed() {}\n"),
            ("xtask/tests/t.rs", "#[test]\nfn t() {}\n"),
            ("xtask/Cargo.toml", &manifest("xtask")),
        ]);
        let helper = String::from("xtask/src/tasks.rs");
        match plan(&files, &read) {
            Plan::Separable(ref one) => {
                assert_eq!(one.test_only, vec![helper.clone()]);
                assert_eq!(one.revert, vec![String::from("xtask/src/other.rs")]);
                assert_eq!(one.test_files, vec![String::from("xtask/tests/t.rs")]);
                assert!(one.held().contains(&helper), "kept at HEAD: {:?}", one.held());
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn a_cfg_test_helper_no_longer_decides_between_a_refusal_and_a_silent_pass() {
        // THE EDGE, which was sharper than the failure it caused. With the helper counted as a
        // test file the diff had one "provable" file that could name nothing and the gate FAILED;
        // without the helper the only remaining test file is inseparable, `provable` is empty and
        // the gate PASSED. One `#[cfg(test)]` attribute was the whole difference between a hard
        // refusal and a silent pass, which is why stopping the failure alone would have traded a
        // loud wrong answer for a quiet one. It moves no partition now, and the pass that remains
        // states what it did not measure.
        let inseparable_file = concat!(
            "fn fixed() -> u8 { 2 }\n", // 1
            "#[cfg(test)]\n",           // 2
            "mod tests {\n",            // 3
            "    #[test]\n",            // 4
            "    fn t() {}\n",          // 5
            "}\n",                      // 6
        );
        let added = |texts: &[&str]| changed("crates/x/src/a.rs", 1, texts);
        let hunk = [
            "fn fixed() -> u8 { 2 }",
            "#[cfg(test)]",
            "mod tests {",
            "    #[test]",
            "    fn t() {}",
            "}",
        ];
        let read = tree(&[
            ("crates/x/src/a.rs", inseparable_file),
            (
                "crates/x/src/helper.rs",
                "fn kept() {}\n#[cfg(test)]\nfn helper() -> u8 { 1 }\n",
            ),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let inseparable = vec![String::from("crates/x/src/a.rs")];

        let with_helper = vec![
            added(&hunk),
            changed("crates/x/src/helper.rs", 2, &["#[cfg(test)]", "fn helper() -> u8 { 1 }"]),
        ];
        assert_eq!(
            plan(&with_helper, &read),
            Plan::NotSeparable {
                files: inseparable.clone()
            }
        );
        // The pass carries its own limit rather than reading as a verdict about the change.
        assert_eq!(Coverage::of(&[], &with_helper, &read).ratio(), "0 of 1 added tests measured");
        // And the answer does not depend on the helper being there, which is the property that
        // was missing: the same diff without it plans identically.
        assert_eq!(plan(&[added(&hunk)], &read), Plan::NotSeparable { files: inseparable });
    }

    #[test]
    fn a_test_module_declaration_still_refuses_when_it_names_no_test() {
        // The arm that must STILL fire, and the reason the split is on `mod` rather than on
        // `#[cfg(test)]`. A file gaining `#[cfg(test)] mod tests;` gained a test MODULE, so a test
        // the extractor could not read is plausible there: the file stays in the proof, the scan's
        // refusal stays reachable, and the extractor is the fix. The helper case widens nothing.
        let files = vec![
            changed("crates/x/src/lib.rs", 2, &["#[cfg(test)]", "mod tests;"]),
            changed("crates/x/src/other.rs", 1, &["fn changed() {}"]),
        ];
        let read = tree(&[
            ("crates/x/src/lib.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n"),
            ("crates/x/src/other.rs", "fn changed() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        let planned = plan(&files, &read);
        let Plan::Separable(ref one) = planned else {
            panic!("a test module declaration is a test file, got {planned:?}");
        };
        assert_eq!(one.test_files, vec![String::from("crates/x/src/lib.rs")]);
        assert!(matches!(Scan::of(&files, &one.test_files, &read), Scan::Unnamed));
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
