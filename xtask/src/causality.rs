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

use std::path::Path;
use std::process::Command;

use crate::Verdict;
use crate::repo;

mod diff;
mod regions;

use diff::{ChangedFile, changed_with_additions};
use regions::{AddedLine, PostImage, has_non_test_additions, scope};

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

/// Does this diff hunk add a test?
///
/// Deliberately syntactic and deliberately generous: `#[test]`, `#[tokio::test]`,
/// `#[rstest]`, a new `mod tests`. A false positive costs a slower gate; a false negative
/// lets a vacuous test through, so the bias goes one way on purpose.
fn adds_test(added_lines: &[AddedLine]) -> bool {
    added_lines.iter().any(|l| {
        let t = l.text.trim();
        t.starts_with("#[test]")
            || t.starts_with("#[tokio::test")
            || t.starts_with("#[rstest")
            || t.starts_with("#[test_case")
            || (t.starts_with("mod tests") && t.contains('{'))
            || t.starts_with("#[cfg(test)]")
    })
}

/// Is this a Rust source path? Case-insensitive, since a case-sensitive extension test is
/// wrong on a case-insensitive filesystem.
fn is_rust(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
}

/// Changed files split by whether they exist at the base commit.
type Partitioned<'a> = (Vec<&'a String>, Vec<&'a String>);

/// Split changed Rust files into "added tests" and "changed implementation only".
///
/// `read` returns a file's POST-IMAGE by repo-relative path - the working tree in the gate, a
/// fixed map in the tests. It is what makes the test-region question answerable at all.
pub(crate) fn plan(files: &[ChangedFile], read: &PostImage<'_>) -> Plan {
    let mut test_files = Vec::new();
    let mut impl_only = Vec::new();

    for file in files {
        if !is_rust(&file.path) {
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
/// `--cargo-profile` and not `--profile`: nextest reserves `--profile` for its own profiles,
/// and passing `ci` there would select a nextest profile that does not exist rather than a
/// cargo one that does.
fn cargo_test(dir: &Path, target: &Path) -> (bool, String) {
    let out = Command::new("cargo")
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .args(["nextest", "run", "--workspace", "--all-features", "--cargo-profile", "ci"])
        .output();
    match out {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            (o.status.success(), text)
        }
        Err(e) => (false, format!("could not run cargo: {e}")),
    }
}

/// Does `path` exist at `base`?
///
/// "Revert to base" means two different things depending on the answer. For a file that
/// existed, it means check out the old content. For a file this branch ADDED, it means the file
/// is not there - and `git checkout base -- <new file>` fails with "did not match any file(s)
/// known to git", which is how this gate first broke in CI.
fn base_has(root: &Path, base: &str, path: &str) -> bool {
    let mut command = Command::new("git");
    strip_git_env_for(&mut command);
    command
        .current_dir(root)
        .arg("cat-file")
        .arg("-e")
        .arg(format!("{base}:{path}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Git env vars that would point a subprocess at another repository.
///
/// The list moved to `repo` when a second gate needed it. This stays as the name the call sites
/// here already read by, and as the one place that would have to change if they diverged.
fn strip_git_env_for(command: &mut Command) {
    crate::repo::strip_git_env(command);
}

/// What the base run actually told us.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BaseOutcome {
    /// The changed tests passed without the change: they do not test it.
    Green,
    /// A test failed an assertion. This is the evidence the gate exists to collect.
    RedByAssertion,
    /// The tree did not build. Red, but it proves nothing about behaviour.
    DidNotCompile,
}

/// Classify a base test run.
///
/// The distinction matters more than it looks. Before this existed, a base tree that failed to
/// COMPILE counted as "red, as required" and the gate passed - a false green over exactly the
/// changes it is supposed to judge. A test that does not compile has not been run.
pub(crate) fn classify_base(text: &str, succeeded: bool) -> BaseOutcome {
    if succeeded {
        return BaseOutcome::Green;
    }
    // Order matters: a tree that did not build often ALSO prints "error: test run failed",
    // so the compile check has to come first or a build failure reads as a real red.
    let compile_failure = text.contains("could not compile")
        || text.contains("error[E")
        || text.contains("error: cannot find")
        || text.contains("unresolved import");
    if compile_failure {
        return BaseOutcome::DidNotCompile;
    }
    // nextest first, then the `cargo test` wording, so the classifier survives a runner swap.
    let assertion_failure = text.contains("error: test run failed")
        || text.contains("FAIL [")
        || text.contains("test result: FAILED")
        || text.contains("panicked at");
    if assertion_failure {
        return BaseOutcome::RedByAssertion;
    }
    // Unknown failure: do not claim a proof we did not get.
    BaseOutcome::DidNotCompile
}

/// Set up a detached worktree at HEAD under the given path.
fn add_worktree(root: &Path, dir: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["worktree", "add", "--detach", "--quiet"])
        .arg(dir)
        .arg("HEAD")
        .output()
        .map_err(|e| format!("git worktree add failed to start: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// Best-effort teardown. A leftover worktree is noise, not a correctness problem, so a
/// failure here is reported and does not change the gate's verdict.
fn remove_worktree(root: &Path, dir: &Path) {
    let outcome = Command::new("git")
        .current_dir(root)
        .args(["worktree", "remove", "--force"])
        .arg(dir)
        .output();
    if let Err(e) = outcome {
        eprintln!("xtask test-causality: could not remove the worktree: {e}");
    }
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

/// The base state to put a worktree into: files to check out at `base`, files to delete.
///
/// Two of these exist per proof. The first is the implementation change; the second is the files
/// held back for carrying their own tests, applied only if the first tree does not build.
struct BaseState<'a> {
    restore: Vec<&'a String>,
    remove: Vec<&'a String>,
}

impl BaseState<'_> {
    /// Nothing to apply, so there is no second attempt to make.
    const fn is_empty(&self) -> bool {
        self.restore.is_empty() && self.remove.is_empty()
    }
}

/// Split files into "existed at base, so check it out" and "added here, so delete it".
///
/// "Revert to base" means two different things depending on the answer, and getting it wrong is
/// how this gate first broke in CI - see [`base_has`].
fn base_state<'a>(root: &Path, base: &str, files: &'a [String]) -> BaseState<'a> {
    let (restore, remove): Partitioned<'a> = files.iter().partition(|f| base_has(root, base, f));
    BaseState { restore, remove }
}

/// Reconstruct the baseline in a worktree and require the changed tests to fail there.
fn prove(root: &Path, base: &str, revert: &[String], test_files: &[String], held_back: &[String]) -> Verdict {
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
    let (head_ok, head_out) = cargo_test(root, &shared_target);
    if !head_ok {
        eprintln!("xtask test-causality: FAILED - the tests are not green on HEAD");
        eprintln!("{}", tail(&head_out, 30));
        return Verdict::Fail;
    }
    println!("  head: green");

    let wt = root.join("target").join("causality-worktree");
    remove_worktree(root, &wt);
    if let Err(e) = add_worktree(root, &wt) {
        eprintln!("xtask test-causality: could not create a worktree: {e}");
        return Verdict::Fail;
    }

    let verdict = reconstruct_and_run(&wt, base, &first, &held, &shared_target);
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
fn reconstruct_and_run(wt: &Path, base: &str, first: &BaseState<'_>, held: &BaseState<'_>, target: &Path) -> Verdict {
    if let Err(e) = apply(wt, base, first) {
        eprintln!("xtask test-causality: {e}");
        return Verdict::Fail;
    }

    let (base_ok, base_out) = cargo_test(wt, target);
    let outcome = classify_base(&base_out, base_ok);

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
        let (retry_ok, retry_out) = cargo_test(wt, target);
        return report_base(&classify_base(&retry_out, retry_ok), &retry_out, true);
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

/// Check out the base version of the files that had one, and delete the ones this branch added.
fn apply(wt: &Path, base: &str, state: &BaseState<'_>) -> Result<(), String> {
    if !state.restore.is_empty() {
        let mut checkout = Command::new("git");
        strip_git_env_for(&mut checkout);
        checkout.current_dir(wt).args(["checkout", base, "--"]);
        for f in &state.restore {
            checkout.arg(f);
        }
        match checkout.output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Err(format!(
                    "could not restore base files: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                ));
            }
            Err(e) => return Err(format!("could not run git checkout: {e}")),
        }
    }
    for f in &state.remove {
        match std::fs::remove_file(wt.join(f)) {
            Ok(()) => {}
            // ALREADY ABSENT is the state being asked for, not a failure. The worktree is created
            // at HEAD, and a file this branch has not COMMITTED is in no commit - an
            // intent-to-add file is in the index only - so `remove` legitimately names files the
            // worktree never had. Treating that as an error failed the whole gate on any tree
            // holding a new file, which is every tree mid-change.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("could not remove {f}: {e}")),
        }
    }
    Ok(())
}

/// Turn a base run into the gate's verdict.
///
/// `retried` only changes what the operator is told: after a second attempt, "not separable at
/// file level" is no longer the likely explanation, because the tree WAS coherently at base.
fn report_base(outcome: &BaseOutcome, output: &str, retried: bool) -> Verdict {
    match *outcome {
        BaseOutcome::Green => {
            eprintln!("xtask test-causality: FAILED - green against base behaviour");
            eprintln!();
            eprintln!("The changed tests pass with the implementation reverted, so they do not");
            eprintln!("test the change. Make the test exercise the new behaviour, or say plainly");
            eprintln!("that it is not a regression test.");
            Verdict::Fail
        }
        BaseOutcome::RedByAssertion => {
            println!("  base: red by assertion, as required");
            println!("{}", tail(output, 12));
            println!("xtask test-causality: ok - red on base, green on head");
            Verdict::Pass
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
            prove(&root, &base, &revert, &test_files, &held_back)
        }
    }
}

/// The last `n` lines, so a failure shows the assertion rather than the whole compile log.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines.get(start..).map_or_else(String::new, |s| s.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::regions::AddedLine;
    use super::{BaseOutcome, BaseState, ChangedFile, Plan, adds_test, apply, plan, retry_with_held_back};

    /// A post-image reader over a fixed set of files, standing in for the working tree.
    fn tree(files: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = files
            .iter()
            .map(|&(path, text)| (String::from(path), String::from(text)))
            .collect();
        move |wanted: &str| owned.iter().find(|(path, _)| path == wanted).map(|(_, text)| text.clone())
    }

    /// A changed file whose added lines run consecutively from `first`.
    fn changed(path: &str, first: usize, texts: &[&str]) -> ChangedFile {
        ChangedFile {
            path: String::from(path),
            added: added_from(first, texts),
        }
    }

    /// Added lines numbered consecutively from `first`.
    fn added_from(first: usize, texts: &[&str]) -> Vec<AddedLine> {
        texts
            .iter()
            .enumerate()
            .map(|(offset, text)| AddedLine::new(first + offset, *text))
            .collect()
    }

    #[test]
    fn recognises_added_tests() {
        assert!(adds_test(&added_from(1, &["    #[test]"])));
        assert!(adds_test(&added_from(1, &["#[tokio::test]"])));
        assert!(adds_test(&added_from(1, &["#[cfg(test)]"])));
        assert!(adds_test(&added_from(1, &["mod tests {"])));
        assert!(!adds_test(&added_from(1, &["fn thing() {}", "// a comment"])));
    }

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
    fn a_base_that_did_not_compile_is_not_a_proof() {
        use super::{BaseOutcome, classify_base};
        // The false green this replaced: `cargo test` failed, so the gate said "red, as
        // required" and passed. A tree that does not build has run no tests.
        let compile = "error[E0432]: unresolved import `crate::thing`\nerror: could not compile";
        assert_eq!(classify_base(compile, false), BaseOutcome::DidNotCompile);
    }

    #[test]
    fn a_failed_assertion_is_the_evidence_wanted() {
        use super::{BaseOutcome, classify_base};
        // nextest's wording, which is what this now sees.
        let nextest = "    FAIL [ 0.006s] xtask::bin/xtask a::b\n Summary 42 passed, 1 failed\nerror: test run failed";
        assert_eq!(classify_base(nextest, false), BaseOutcome::RedByAssertion);
        // And `cargo test`'s, so the classifier survives a runner swap.
        let cargo = "running 3 tests\nthread 'x' panicked at src/lib.rs:9\ntest result: FAILED. 2 passed; 1 failed";
        assert_eq!(classify_base(cargo, false), BaseOutcome::RedByAssertion);
    }

    #[test]
    fn a_build_failure_wins_over_a_test_run_failure_line() {
        use super::{BaseOutcome, classify_base};
        // nextest prints "error: test run failed" when the build failed too. Reading that as a
        // real red is the false green this gate already had once.
        let both = "error[E0433]: failed to resolve\nerror: could not compile\nerror: test run failed";
        assert_eq!(classify_base(both, false), BaseOutcome::DidNotCompile);
    }

    #[test]
    fn a_passing_base_means_the_test_does_not_test_the_change() {
        use super::{BaseOutcome, classify_base};
        assert_eq!(
            classify_base("test result: ok. 12 passed; 0 failed", true),
            BaseOutcome::Green
        );
    }

    #[test]
    fn an_unrecognised_failure_claims_nothing() {
        use super::{BaseOutcome, classify_base};
        // Conservative on purpose: an unfamiliar failure is not evidence of causality.
        assert_eq!(
            classify_base("linker exited with signal 9", false),
            BaseOutcome::DidNotCompile
        );
    }

    #[test]
    fn non_rust_files_are_ignored() {
        let files = vec![changed("README.md", 1, &["#[test]"])];
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
        assert!(!retry_with_held_back(&BaseOutcome::RedByAssertion, &held));
        assert!(!retry_with_held_back(&BaseOutcome::Green, &held));
    }

    #[test]
    fn removing_a_file_the_worktree_never_had_is_not_a_failure() {
        // A file this branch has not COMMITTED is in no commit, so the worktree - created at HEAD -
        // never carried it, while `remove` names exactly the files absent at base. An intent-to-add
        // file is the everyday case, and this used to fail the whole gate rather than prove
        // anything: "could not remove xtask/src/guidance/claims.rs: No such file or directory".
        let wt = std::env::temp_dir().join(format!("sutura-causality-{}", std::process::id()));
        let _cleanup = std::fs::remove_dir_all(&wt);
        std::fs::create_dir_all(&wt).expect("a scratch worktree");
        let absent = String::from("xtask/src/guidance/claims.rs");
        let state = BaseState {
            restore: Vec::new(),
            remove: vec![&absent],
        };

        let applied = apply(&wt, "HEAD", &state);

        let _swept = std::fs::remove_dir_all(&wt);
        assert!(applied.is_ok(), "an absent file is the state asked for, got {applied:?}");
    }
}
