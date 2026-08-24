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
use std::process::{Command, ExitCode};

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
fn cargo_test(dir: &Path) -> (bool, String) {
    let out = Command::new("cargo")
        .current_dir(dir)
        .args(["test", "--workspace", "--all-features"])
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
fn report_not_separable(files: &[String]) -> ExitCode {
    println!("xtask test-causality: NOT MECHANICALLY SEPARABLE");
    for f in files {
        println!("  {f} changes behaviour and adds tests in one file");
    }
    println!();
    println!("Rust keeps unit tests beside the code they test, so reverting the");
    println!("implementation would remove the test too. State the evidence in the");
    println!("handoff instead: the command you ran, the failure before the fix, and");
    println!("the pass after. This gate has NOT verified causality for this change.");
    ExitCode::SUCCESS
}

/// Reconstruct the baseline in a worktree and require the changed tests to fail there.
fn prove(root: &Path, base: &str, revert: &[String], test_files: &[String]) -> ExitCode {
    println!("xtask test-causality: proving red-before-green");
    for f in test_files {
        println!("  test file: {f}");
    }
    for f in revert {
        println!("  revert:    {f}");
    }

    // HEAD must be green, or "red on base" means nothing.
    let (head_ok, head_out) = cargo_test(root);
    if !head_ok {
        eprintln!("xtask test-causality: FAILED - the tests are not green on HEAD");
        eprintln!("{}", tail(&head_out, 30));
        return ExitCode::FAILURE;
    }
    println!("  head: green");

    let wt = root.join("target").join("causality-worktree");
    remove_worktree(root, &wt);
    if let Err(e) = add_worktree(root, &wt) {
        eprintln!("xtask test-causality: could not create a worktree: {e}");
        return ExitCode::FAILURE;
    }

    let mut restore = Command::new("git");
    restore.current_dir(&wt).args(["checkout", base, "--"]);
    for f in revert {
        restore.arg(f);
    }

    let verdict = match restore.output() {
        Ok(o) if o.status.success() => {
            let (base_ok, base_out) = cargo_test(&wt);
            if base_ok {
                eprintln!("xtask test-causality: FAILED - green against base behaviour");
                eprintln!();
                eprintln!("The changed tests pass with the implementation reverted, so they");
                eprintln!("do not test the change. Make the test exercise the new behaviour,");
                eprintln!("or say plainly that it is not a regression test.");
                ExitCode::FAILURE
            } else {
                println!("  base: red, as required");
                println!("{}", tail(&base_out, 12));
                println!("xtask test-causality: ok - red on base, green on head");
                ExitCode::SUCCESS
            }
        }
        Ok(o) => {
            eprintln!(
                "xtask test-causality: could not restore base files: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("xtask test-causality: could not run git checkout: {e}");
            ExitCode::FAILURE
        }
    };

    remove_worktree(root, &wt);
    verdict
}

/// `xtask test-causality --since <base>` - the ship-check and CI entry point.
pub(crate) fn run(args: &[String]) -> ExitCode {
    let Some(base) = base_ref(args) else {
        eprintln!("xtask test-causality: usage: --since <base-ref>");
        return ExitCode::from(2);
    };

    let Some(root) = repo::root() else {
        eprintln!("xtask test-causality: could not determine the repo root");
        return ExitCode::FAILURE;
    };

    let Some(files) = changed_with_additions(&base) else {
        // Same rule as classify: an unusable base ref is not evidence of nothing to do.
        eprintln!("xtask test-causality: could not diff against `{base}`");
        return ExitCode::FAILURE;
    };

    match plan(&files) {
        Plan::NotRequired => {
            println!("xtask test-causality: no changed tests - nothing to prove");
            ExitCode::SUCCESS
        }
        Plan::NotSeparable { files } => report_not_separable(&files),
        Plan::Separable { revert, test_files } => {
            if revert.is_empty() {
                println!("xtask test-causality: tests changed but no implementation did");
                println!("  Nothing to revert, so there is no old behaviour to be red against.");
                println!("  If this is a new test for existing behaviour, say so; it is not a");
                println!("  regression test and this gate cannot prove it is causal.");
                return ExitCode::SUCCESS;
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
    fn non_rust_files_are_ignored() {
        let files = vec![(String::from("README.md"), lines(&["#[test]"]))];
        assert_eq!(plan(&files), Plan::NotRequired);
    }
}
