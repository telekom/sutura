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
//! Where that does NOT work, stated plainly rather than papered over: Rust keeps unit tests
//! in `mod tests` inside the file they test, so a fix and its test frequently live in ONE
//! file. Reverting it removes the test; keeping it keeps the fix. Such a change is not
//! mechanically separable, and this gate says so and requires the author to state the
//! evidence instead. It does not silently pass, and it does not pretend to have checked.
//!
//! A gate that quietly downgraded to "no opinion" here would be worse than no gate: it would
//! report green over exactly the cases most likely to hide a vacuous test.

use std::path::Path;
use std::process::Command;

use crate::Verdict;
use crate::repo;

/// What the gate concluded, so the shape is testable without git or cargo.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// No changed tests: nothing to prove.
    NotRequired,
    /// Baseline can be reconstructed by reverting these files.
    Separable { revert: Vec<String>, test_files: Vec<String> },
    /// Impl and tests share a file; a human must state the evidence.
    NotSeparable { files: Vec<String> },
}

/// Does this diff hunk add a test?
///
/// Deliberately syntactic and deliberately generous: `#[test]`, `#[tokio::test]`,
/// `#[rstest]`, a new `mod tests`. A false positive costs a slower gate; a false negative
/// lets a vacuous test through, so the bias goes one way on purpose.
fn adds_test(added_lines: &[String]) -> bool {
    added_lines.iter().any(|l| {
        let t = l.trim();
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

/// A changed file and the lines the diff added to it.
pub(crate) type ChangedFile = (String, Vec<String>);

/// Split changed Rust files into "added tests" and "changed implementation only".
pub(crate) fn plan(files: &[ChangedFile]) -> Plan {
    let mut test_files = Vec::new();
    let mut impl_only = Vec::new();

    for (path, added) in files {
        if !is_rust(path) {
            continue;
        }
        if adds_test(added) {
            test_files.push(path.clone());
        } else {
            impl_only.push(path.clone());
        }
    }

    if test_files.is_empty() {
        return Plan::NotRequired;
    }

    // A file that gained BOTH a test and non-test changes cannot be split by reverting whole
    // files. Detect it by asking whether the file has added lines outside a test context.
    let inseparable: Vec<String> = files
        .iter()
        .filter(|(path, added)| test_files.contains(path) && has_non_test_additions(added))
        .map(|(path, _)| path.clone())
        .collect();

    if !inseparable.is_empty() && impl_only.is_empty() {
        return Plan::NotSeparable { files: inseparable };
    }

    Plan::Separable {
        revert: impl_only,
        test_files,
    }
}

/// Are there added lines that are plainly not test code?
///
/// Heuristic and conservative: an added line inside a `#[cfg(test)]` region is test code, and
/// anything before the first such marker is not. It cannot be exact without parsing, so it
/// errs toward "not separable", which asks a human rather than guessing.
fn has_non_test_additions(added: &[String]) -> bool {
    let mut seen_test_marker = false;
    for line in added {
        let t = line.trim();
        if t.starts_with("#[cfg(test)]") || t.starts_with("mod tests") {
            seen_test_marker = true;
            continue;
        }
        if seen_test_marker {
            continue;
        }
        // Blank lines, comments and attributes carry no behaviour.
        if t.is_empty() || t.starts_with("//") || t.starts_with("#[") || t.starts_with("#!") {
            continue;
        }
        return true;
    }
    false
}

/// Added lines per changed file, from `git diff`.
fn changed_with_additions(base: &str) -> Option<Vec<ChangedFile>> {
    let out = Command::new("git")
        .args(["diff", "--unified=0", "--no-color", base, "--"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);

    let mut files: Vec<ChangedFile> = Vec::new();
    let mut current: Option<ChangedFile> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            if let Some(done) = current.take() {
                files.push(done);
            }
            current = Some((String::from(rest), Vec::new()));
            continue;
        }
        if let Some(added) = line.strip_prefix('+')
            && !line.starts_with("+++")
            && let Some((_, lines)) = current.as_mut()
        {
            lines.push(String::from(added));
        }
    }
    if let Some(done) = current.take() {
        files.push(done);
    }
    Some(files)
}

/// Run cargo in `dir`, returning whether it succeeded plus its combined output.
/// Run the test suite in `dir` with nextest.
///
/// nextest, not `cargo test`, because this gate compares two runs and every other test
/// invocation in the repo uses nextest: measuring "green on head" with a different runner than
/// CI trusts would make the comparison meaningless. It also gives per-test process isolation,
/// so one panicking test cannot take others down and skew the comparison.
fn cargo_test(dir: &Path) -> (bool, String) {
    let out = Command::new("cargo")
        .current_dir(dir)
        .args(["nextest", "run", "--workspace", "--all-features"])
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
fn strip_git_env_for(command: &mut Command) {
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(name);
    }
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

/// Reconstruct the baseline in a worktree and require the changed tests to fail there.
fn prove(root: &Path, base: &str, revert: &[String], test_files: &[String]) -> Verdict {
    // Split by what "revert" means for each file. A file this branch added is not restored -
    // it is removed, because absent is what the base state was.
    let (restore, remove): Partitioned<'_> = revert.iter().partition(|f| base_has(root, base, f));

    if restore.is_empty() {
        println!("xtask test-causality: NO BASE BEHAVIOUR TO COMPARE AGAINST");
        for f in remove {
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
    for f in &restore {
        println!("  restore:   {f}");
    }
    for f in &remove {
        println!("  remove:    {f}  (added in this branch)");
    }

    // HEAD must be green, or "red on base" means nothing.
    let (head_ok, head_out) = cargo_test(root);
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

    let verdict = reconstruct_and_run(&wt, base, &restore, &remove);
    remove_worktree(root, &wt);
    verdict
}

/// Put the worktree into the base state for the implementation, then run the tests.
fn reconstruct_and_run(wt: &Path, base: &str, restore: &[&String], remove: &[&String]) -> Verdict {
    let mut checkout = Command::new("git");
    strip_git_env_for(&mut checkout);
    checkout.current_dir(wt).args(["checkout", base, "--"]);
    for f in restore {
        checkout.arg(f);
    }
    match checkout.output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!(
                "xtask test-causality: could not restore base files: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            return Verdict::Fail;
        }
        Err(e) => {
            eprintln!("xtask test-causality: could not run git checkout: {e}");
            return Verdict::Fail;
        }
    }
    for f in remove {
        if let Err(e) = std::fs::remove_file(wt.join(f)) {
            eprintln!("xtask test-causality: could not remove {f}: {e}");
            return Verdict::Fail;
        }
    }

    let (base_ok, base_out) = cargo_test(wt);
    match classify_base(&base_out, base_ok) {
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
            println!("{}", tail(&base_out, 12));
            println!("xtask test-causality: ok - red on base, green on head");
            Verdict::Pass
        }
        BaseOutcome::DidNotCompile => {
            println!("  base: did not compile");
            println!("{}", tail(&base_out, 12));
            println!();
            println!("xtask test-causality: INCONCLUSIVE - the base tree does not build.");
            println!("That is red, but a test that never ran is not evidence about behaviour.");
            println!("Usually it means the change is not separable at file level: the test and");
            println!("what it needs arrived together. State the evidence in the handoff.");
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

    match plan(&files) {
        Plan::NotRequired => {
            println!("xtask test-causality: no changed tests - nothing to prove");
            Verdict::Pass
        }
        Plan::NotSeparable { files } => report_not_separable(&files),
        Plan::Separable { revert, test_files } => {
            if revert.is_empty() {
                println!("xtask test-causality: tests changed but no implementation did");
                println!("  Nothing to revert, so there is no old behaviour to be red against.");
                println!("  If this is a new test for existing behaviour, say so; it is not a");
                println!("  regression test and this gate cannot prove it is causal.");
                return Verdict::Pass;
            }
            prove(&root, &base, &revert, &test_files)
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
    use super::{Plan, adds_test, has_non_test_additions, plan};

    fn lines(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn recognises_added_tests() {
        assert!(adds_test(&lines(&["    #[test]"])));
        assert!(adds_test(&lines(&["#[tokio::test]"])));
        assert!(adds_test(&lines(&["#[cfg(test)]"])));
        assert!(adds_test(&lines(&["mod tests {"])));
        assert!(!adds_test(&lines(&["fn thing() {}", "// a comment"])));
    }

    #[test]
    fn no_changed_tests_means_nothing_to_prove() {
        let files = vec![(String::from("src/a.rs"), lines(&["fn f() {}"]))];
        assert_eq!(plan(&files), Plan::NotRequired);
    }

    #[test]
    fn separates_impl_from_test_file() {
        let files = vec![
            (String::from("crates/x/src/a.rs"), lines(&["fn fixed() -> u8 { 2 }"])),
            (String::from("crates/x/tests/t.rs"), lines(&["#[test]", "fn t() {}"])),
        ];
        match plan(&files) {
            Plan::Separable { revert, test_files } => {
                assert_eq!(revert, vec![String::from("crates/x/src/a.rs")]);
                assert_eq!(test_files, vec![String::from("crates/x/tests/t.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn one_file_with_both_is_reported_not_guessed() {
        // The honest case: a fix and its test in one file cannot be split by reverting the
        // file, so the gate must say so rather than pass or fail arbitrarily.
        let files = vec![(
            String::from("crates/x/src/a.rs"),
            lines(&["fn fixed() -> u8 { 2 }", "#[cfg(test)]", "mod tests {", "    #[test]"]),
        )];
        match plan(&files) {
            Plan::NotSeparable { files } => {
                assert_eq!(files, vec![String::from("crates/x/src/a.rs")]);
            }
            other => panic!("expected NotSeparable, got {other:?}"),
        }
    }

    #[test]
    fn a_test_only_addition_in_one_file_is_separable() {
        // Only test code added: nothing to revert in that file, so it is not "inseparable".
        let files = vec![
            (
                String::from("crates/x/src/a.rs"),
                lines(&["#[cfg(test)]", "    #[test]", "    fn t() {}"]),
            ),
            (String::from("crates/x/src/b.rs"), lines(&["fn fixed() {}"])),
        ];
        match plan(&files) {
            Plan::Separable { revert, .. } => {
                assert_eq!(revert, vec![String::from("crates/x/src/b.rs")]);
            }
            other => panic!("expected Separable, got {other:?}"),
        }
    }

    #[test]
    fn non_test_additions_are_detected_before_the_test_marker() {
        assert!(has_non_test_additions(&lines(&["fn f() {}", "#[cfg(test)]"])));
        assert!(!has_non_test_additions(&lines(&["#[cfg(test)]", "fn t() {}"])));
        // Comments and attributes carry no behaviour.
        assert!(!has_non_test_additions(&lines(&[
            "// note",
            "#[derive(Debug)]",
            "#[cfg(test)]"
        ])));
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
        let files = vec![(String::from("README.md"), lines(&["#[test]"]))];
        assert_eq!(plan(&files), Plan::NotRequired);
    }
}
