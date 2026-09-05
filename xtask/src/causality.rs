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
use std::process::Command;

use crate::Verdict;
use crate::repo;

mod base;
mod diff;
#[cfg(test)]
mod fixtures;
// `pub(crate)` rather than private: `crate::refusals` reads the same test regions this gate does,
// because "which lines of this file are test code" is one question and a second implementation of
// it would be a second thing to keep in step. Nothing else about the module moved.
pub(crate) mod regions;
mod scoped;
mod worktree;

use base::{BaseOutcome, classify_base, names_no_tests, report_base, tail};
use diff::{ChangedFile, changed_with_additions};
use regions::{PostImage, has_non_test_additions, scope};
use scoped::{Ident, Scan, Scoped, adds_test};
use worktree::{BaseState, add_worktree, apply, base_state, remove_worktree};

/// What the gate concluded, so the shape is testable without git or cargo.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// No changed tests: nothing to prove.
    NotRequired,
    /// Baseline can be reconstructed by reverting these files.
    ///
    /// `held_back` names the files carrying BOTH an implementation change and a test. They keep
    /// their HEAD content and their own tests are not part of the proof - but they are carried
    /// here rather than dropped, because reverting the others while these stay at HEAD is what
    /// can leave a tree mixing two versions of one API. `reconstruct_and_run` needs them to be
    /// able to ask a second time.
    Separable {
        revert: Vec<String>,
        test_files: Vec<String>,
        held_back: Vec<String>,
    },
    /// Impl and tests share a file; a human must state the evidence.
    NotSeparable { files: Vec<String> },
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
    let mut impl_only = Vec::new();

    for file in files {
        if !is_compiled_rust(&file.path) {
            continue;
        }
        if adds_test(&file.added) {
            test_files.push(file.path.clone());
        } else {
            impl_only.push(file.path.clone());
        }
    }

    if test_files.is_empty() {
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

    Plan::Separable {
        revert: impl_only,
        test_files: provable,
        held_back: inseparable,
    }
}

/// Run the test suite in `dir` with nextest, building into `target`.
///
/// nextest, not `cargo test`, because this gate compares two runs and every other test
/// invocation in the repo uses nextest: measuring "green on head" with a different runner than
/// CI trusts would make the comparison meaningless. It also gives per-test process isolation,
/// so one panicking test cannot take others down and skew the comparison.
///
/// ONE TARGET DIRECTORY FOR BOTH RUNS, and that is what keeps this gate affordable. The base
/// run happens in a worktree, so by default it gets its own `target/` and compiles the whole
/// dependency closure a second time - `DataFusion`, Arrow and the rest, none of which the base
/// commit changed. On a 14 GB runner the second copy is what exhausted the disk. Sharing the
/// directory leaves the dependencies built once and rebuilds only our own crates, which is
/// exactly the difference between the two trees.
///
/// Safe to share because both runs use the same toolchain and the same profile. Alternating
/// COMPILERS in one directory invalidates every artifact in it; alternating our own sources
/// does not, because cargo fingerprints them and the dependency graph below them is identical.
///
/// WHAT SHARING IT DOES NOT KEEP APART, measured on 2026-09-04 and NOT fixed here. The two trees
/// are one unit as far as cargo is concerned - same package names, same relative paths, so the
/// same artifact - and freshness is decided by mtime, so a build in either tree overwrites the
/// other's binaries and the next run reuses them without noticing. Reproduced from an empty
/// directory: the HEAD run built and passed, the base run in the worktree rebuilt the same test
/// binary, and the same command back at the root then rebuilt NOTHING and failed - on a file that
/// was present at the root, because the binary it executed was the worktree's.
///
/// So a run can be measuring the OTHER tree's code, and it reaches this repository twice: the
/// postgres harness resolves its endpoint by walking up from `env!("CARGO_MANIFEST_DIR")`, which
/// is baked into whichever tree compiled the binary; and the reverted source is baked in the same
/// way. It is a defect in this optimisation rather than in the classification below, and it is one
/// reason two runs of one tree can disagree - the other, and the larger one, is that only
/// `just causality` provisions a tier at all (see [`nextest`]).
///
/// The direction it fails in is what makes it survivable now: a stale binary makes a scoped test
/// pass on base ("green against base behaviour") or vanish from HEAD, both of which are loud. It
/// can only manufacture a false GREEN by executing some third tree's binary in which the same
/// test failed, which is a far narrower window than *any failure anywhere counts*. Removing it
/// needs a second target directory, whose cost is the paragraph above, or a first-party rebuild
/// forced on every run - a trade-off with its own measurement, not a detail to slip in here.
///
/// `--cargo-profile` and not `--profile`: nextest reserves `--profile` for its own profiles,
/// and passing `ci` there would select a nextest profile that does not exist rather than a
/// cargo one that does.
fn cargo_test(dir: &Path, target: &Path, only: &str, tree: Tree) -> (bool, String) {
    match nextest(dir, target, only, tree).output() {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            (o.status.success(), text)
        }
        Err(e) => (false, format!("could not run cargo: {e}")),
    }
}

/// Which tree a run happens in - which is a question about SERVICES, not about sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tree {
    /// The repo root: the tree whose service tier something provisioned before invoking the gate.
    Provisioned,
    /// The base worktree under `target/`, which no provisioner has ever seen.
    Reconstructed,
}

/// The nextest invocation for one run.
///
/// THE REQUIREMENT DOES NOT CROSS INTO A TREE ITS ENDPOINT CANNOT REACH. `nix/with-tier.sh`
/// exports `SUTURA_DEV_REQUIRE_TIER` into the recipe's whole process tree, so this gate inherits
/// it - while the endpoint that requirement belongs to is published to
/// `<root>/.sutura-dev/endpoints.json` and resolved by walking up from the harness. The base run
/// happens in a git-derived worktree whose root is a different directory, and that file is
/// gitignored, so it is not there and cannot be: the requirement crossed and the endpoint did
/// not, and every tier-backed cell failed CLOSED in a tree where nothing was provisioned.
///
/// Measured, and it is why this parameter exists rather than a comment: two such cells were the
/// whole of a base run's red, 86 tests into 1810, and the gate reported it as proof about a
/// change that touched neither. `sutura_dev::requirement`'s own rule is that only the thing which
/// provisioned a tier may declare one - so a run in a tree nothing provisioned gets the
/// developer-machine direction, which skips loudly and names what did not run.
///
/// AND IT IS WHY ONE COMMIT ANSWERED DIFFERENTLY IN TWO VENUES. `just causality` sources
/// `nix/with-tier.sh` and so provisions a tier; `just ship-check` and `nix run .#causality` do
/// not, so there those cells skipped and the verdict was about the change. The false green was
/// reachable from the one venue a person runs by hand and cites, which is the worst place for it
/// to live and the reason it went unnoticed.
///
/// WHAT THAT LEAVES: a tier-backed cell cannot be proven causal by this gate. It skips in the
/// reconstructed tree instead of running, so the base run is green and the verdict is *green
/// against base behaviour* - a false alarm rather than a false green, which is the direction to
/// be wrong in. Closing it means publishing an endpoint into a second root, and a second root for
/// the discovery file is the cross-worktree defect `dev/src/scope.rs` exists to prevent.
///
/// `--no-fail-fast` IS AFFORDABLE ONLY BECAUSE THE RUN IS SCOPED, and it is what makes the verdict
/// the same twice. Over the whole suite it would be a long bill for output nobody reads; over the
/// tests one diff added it costs almost nothing, and it removes the last ordering dependence -
/// with fail-fast, WHICH failures a verdict names is a function of nextest's scheduling, and a
/// gate whose answer moves between two runs of one tree is the property this gate exists to
/// supply.
fn nextest(dir: &Path, target: &Path, only: &str, tree: Tree) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .args(["nextest", "run", "--workspace", "--all-features", "--cargo-profile", "ci"])
        .args(["--no-fail-fast", "-E", only]);
    if tree == Tree::Reconstructed {
        command.env_remove(sutura_dev::requirement::FORCE);
    }
    command
}

/// The `--since <ref>` argument, or `None` when it was not supplied correctly.
fn base_ref(args: &[String]) -> Option<String> {
    let (flag, rest) = args.split_first()?;
    if flag != "--since" {
        return None;
    }
    rest.first().cloned()
}

/// Explain the inseparable case. Loud, and deliberately not a failure: the change may be
/// entirely legitimate, but the gate has not verified it and must not read as green.
fn report_not_separable(files: &[String]) -> Verdict {
    println!("xtask test-causality: NOT MECHANICALLY SEPARABLE");
    for f in files {
        println!("  {f} changes behaviour and adds tests in one file");
    }
    println!();
    println!("Rust keeps unit tests beside the code they test, so reverting the");
    println!("implementation would remove the test too. State the evidence in the");
    println!("handoff instead: the command you ran, the failure before the fix, and");
    println!("the pass after. This gate has NOT verified causality for this change.");
    Verdict::Pass
}

/// A plan with test files whose tests this gate could not NAME.
///
/// FAILS, and that direction is the whole point. Both runs are scoped to the tests the diff
/// added, so a scan that names none has two possible fallbacks: run nothing, or run everything.
/// Running everything is the unfiltered run whose verdict was a property of the suite - the defect
/// this scoping removes - and running nothing is a green gate over zero measurements. An empty
/// scan is therefore a refusal, and the fix is the extractor rather than the run.
fn report_unnamed_tests(test_files: &[String]) -> Verdict {
    eprintln!("xtask test-causality: FAILED - the added tests could not be NAMED");
    for f in test_files {
        eprintln!("  {f} adds a test whose name this gate could not read");
    }
    eprintln!();
    eprintln!("Both runs are scoped to the tests the diff added, so naming none of them would");
    eprintln!("leave the gate measuring the whole suite and reading any failure in it as evidence");
    eprintln!("about this change. Two causes: an attribute `causality::scoped` does not recognise,");
    eprintln!("or a file no `Cargo.toml` above it declares a package for. Fix the extractor rather");
    eprintln!("than widening the run.");
    Verdict::Fail
}

/// Every test the diff added is `#[ignore]`d, so no run in this venue reaches one.
///
/// PASSES, loudly, and the opposite direction to the refusal above for a reason: an ignored test
/// is not an extractor bug. Naming only ignored tests in a filterset matches nothing, which
/// nextest reports as `no tests to run` and this gate read as a failure - a false RED on
/// legitimate work, and a gate that reddens a correct change gets disabled. Running them instead
/// fails CLOSED in a tree nothing provisioned, the trap [`nextest`] records for tier-backed cells.
fn report_only_ignored(names: &[Ident]) -> Verdict {
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
fn report_head_failure(output: &str, only: &str) -> Verdict {
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

/// Reconstruct the baseline in a worktree and require the changed tests to fail there.
fn prove(root: &Path, base: &str, revert: &[String], test_files: &[String], held_back: &[String], scoped: &Scoped) -> Verdict {
    let first = base_state(root, base, revert);
    let held = base_state(root, base, held_back);

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
    for f in test_files {
        println!("  test file: {f}");
    }
    for f in &first.restore {
        println!("  restore:   {f}");
    }
    for f in &first.remove {
        println!("  remove:    {f}  (added in this branch)");
    }
    for f in held_back {
        println!("  held:      {f}  (carries its own tests)");
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

    let verdict = reconstruct_and_run(&wt, base, &first, &held, &shared_target, scoped, &only);
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
    only: &str,
) -> Verdict {
    if let Err(e) = apply(wt, base, first) {
        eprintln!("xtask test-causality: {e}");
        return Verdict::Fail;
    }

    let (base_ok, base_out) = cargo_test(wt, target, only, Tree::Reconstructed);
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
        let (retry_ok, retry_out) = cargo_test(wt, target, only, Tree::Reconstructed);
        return report_base(&classify_base(&retry_out, retry_ok, scoped.tests()), &retry_out, true);
    }

    report_base(&outcome, &base_out, false)
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

    // The POST-IMAGE of a changed file is what says which of its lines are test code, and
    // `git diff <base> --` compares base against the WORKING TREE - so the working tree is the
    // post-image, and reading it needs no second git call.
    let working_tree = |path: &str| std::fs::read_to_string(root.join(path)).ok();

    match plan(&files, &working_tree) {
        Plan::NotRequired => {
            println!("xtask test-causality: no changed tests - nothing to prove");
            Verdict::Pass
        }
        Plan::NotSeparable { files } => report_not_separable(&files),
        Plan::Separable {
            revert,
            test_files,
            held_back,
        } => {
            if revert.is_empty() {
                println!("xtask test-causality: tests changed but no implementation did");
                println!("  Nothing to revert, so there is no old behaviour to be red against.");
                println!("  If this is a new test for existing behaviour, say so; it is not a");
                println!("  regression test and this gate cannot prove it is causal.");
                return Verdict::Pass;
            }
            match Scan::of(&files, &test_files, &working_tree) {
                Scan::Runnable(scoped) => prove(&root, &base, &revert, &test_files, &held_back, &scoped),
                Scan::OnlyIgnored(names) => report_only_ignored(&names),
                Scan::Unnamed => report_unnamed_tests(&test_files),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::base::BaseOutcome;
    use super::fixtures::{changed, manifest, tree};
    use super::scoped::{Scan, Scoped};
    use super::{BaseState, Plan, Tree, nextest, plan, retry_with_held_back};

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
            Plan::Separable { revert, test_files, .. } => {
                assert_eq!(revert, vec![String::from("crates/x/src/a.rs")]);
                assert_eq!(test_files, vec![String::from("crates/x/tests/t.rs")]);
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
            Plan::Separable { revert, .. } => {
                assert_eq!(revert, vec![String::from("crates/x/src/b.rs")]);
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
            Plan::Separable {
                revert,
                test_files,
                held_back,
            } => {
                assert!(revert.is_empty(), "no implementation changed: {revert:?}");
                assert!(
                    held_back.is_empty(),
                    "nothing carries an implementation change: {held_back:?}"
                );
                assert_eq!(test_files.len(), 2, "both files are provable test files: {test_files:?}");
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
            Plan::Separable {
                revert,
                test_files,
                held_back,
            } => {
                assert_eq!(revert, vec![String::from("crates/x/src/definitions.rs")]);
                assert_eq!(test_files, vec![String::from("crates/x/tests/t.rs")]);
                assert_eq!(held_back, vec![String::from("crates/x/src/pinned.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
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

    /// The tests one added file declares, for the two wiring assertions below.
    fn one_added_test() -> Scoped {
        let files = vec![changed("crates/x/tests/t.rs", 1, &["#[test]", "fn the_added_one() {}"])];
        let read = tree(&[
            ("crates/x/tests/t.rs", "#[test]\nfn the_added_one() {}\n"),
            ("crates/x/Cargo.toml", &manifest("x")),
        ]);
        match Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read) {
            Scan::Runnable(scoped) => scoped,
            other => panic!("expected one added test, got {other:?}"),
        }
    }

    #[test]
    fn both_runs_are_filtered_to_the_tests_the_diff_added() {
        // The wiring, which no assertion about the filter expression alone would catch: a
        // filterset that never reaches the command line leaves the whole-suite run in place, and
        // that run's verdict is a property of the suite rather than of the change.
        let command = nextest(
            Path::new("/tmp/root"),
            Path::new("/tmp/target"),
            &one_added_test().filterset(),
            Tree::Provisioned,
        );
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args.iter().any(|arg| arg == "-E"), "{args:?}");
        // Qualified, and that is the half the wiring has to carry: an unqualified name is not a
        // key in this tree, so a filterset built from one runs tests in packages the diff never
        // touched - measured at six matches in three packages on nextest 0.9.143.
        assert!(
            args.iter()
                .any(|arg| arg == "(binary_id(=x::t) & test(/^(?:.*::)?the_added_one(?:::|$)/))"),
            "{args:?}"
        );
        // And the run completes, so which failures the verdict names is not a function of
        // nextest's scheduling. This gate was reported as answering differently for one tree in
        // two venues, and fail-fast over an unfiltered run is how that happens.
        assert!(args.iter().any(|arg| arg == "--no-fail-fast"), "{args:?}");
    }

    #[test]
    fn a_tier_backed_cell_is_not_required_in_a_worktree_the_endpoint_does_not_reach() {
        // `nix/with-tier.sh` exports the requirement into this gate's whole process tree, and the
        // endpoint it belongs to is published under the ROOT - so the base run, which happens in
        // a worktree under `target/`, inherited a requirement whose discovery file is gitignored
        // and therefore absent there. Every tier-backed cell then failed CLOSED in a tree nothing
        // had provisioned, and the gate reported that as red-on-base.
        let requirement = OsStr::new(sutura_dev::requirement::FORCE);
        let removed = |tree: Tree| {
            nextest(Path::new("/tmp/dir"), Path::new("/tmp/target"), "test(=t)", tree)
                .get_envs()
                .any(|(name, value)| name == requirement && value.is_none())
        };
        assert!(
            removed(Tree::Reconstructed),
            "a tree nothing provisioned may not inherit the requirement"
        );
        assert!(
            !removed(Tree::Provisioned),
            "the root keeps it: that is the tree whose endpoint was published"
        );
    }
}
