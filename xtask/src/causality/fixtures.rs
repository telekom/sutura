//! The harness three of this gate's test modules were each carrying a copy of.
//!
//! A post-image reader, a line builder, and the two shapes an assertion about a RUN needs - a
//! filterset's coverage and the keys a file's tests land under. It lives in its own file for the
//! reason the gate itself teaches: a file with no `#[test]` is one the gate may revert, so a
//! harness is exactly what may move out of a test-bearing file, and assertions are exactly what
//! may not. `named` and `scoped` moved here from `super::base` when that file reached the
//! unexemptable 1000-line cap, which is that rule applied to this gate's own source.

use crate::causality::coverage::Coverage;
use crate::causality::diff::{ChangedFile, RemovedLine};
use crate::causality::place::AddedTest;
use crate::causality::regions::AddedLine;
use crate::causality::scoped::Scan;

/// A post-image reader over a fixed set of files, standing in for the working tree.
pub(crate) fn tree(files: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|&(path, text)| (String::from(path), String::from(text)))
        .collect();
    move |wanted: &str| owned.iter().find(|(path, _)| path == wanted).map(|(_, text)| text.clone())
}

/// Added lines numbered consecutively from `first`.
pub(crate) fn added_from(first: usize, texts: &[&str]) -> Vec<AddedLine> {
    texts
        .iter()
        .enumerate()
        .map(|(offset, text)| AddedLine::new(first + offset, *text))
        .collect()
}

/// A manifest declaring one package, as the post-image reader hands it back.
///
/// Three test modules were spelling this three ways, and only the shape `changes::package_name`
/// parses matters - so a fourth spelling drifting is the thing this removes.
pub(crate) fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion.workspace = true\n")
}

/// A changed file whose added lines run consecutively from `first`.
pub(crate) fn changed(path: &str, first: usize, texts: &[&str]) -> ChangedFile {
    ChangedFile {
        path: String::from(path),
        before: String::from(path),
        added: added_from(first, texts),
        removed: Vec::new(),
    }
}

/// A changed file that also REMOVED lines, added from `at` and removed from `was`.
///
/// TWO starting lines because the two sides are numbered in different images: an added line has a
/// post-image number and a removed one has a pre-image number. A fixture that shared one would be
/// asserting against a shape `super::diff` cannot produce.
pub(crate) fn changed_removing(path: &str, at: usize, added: &[&str], was: usize, removed: &[&str]) -> ChangedFile {
    ChangedFile {
        path: String::from(path),
        before: String::from(path),
        added: added_from(at, added),
        removed: removed
            .iter()
            .enumerate()
            .map(|(offset, text)| RemovedLine {
                before: was + offset,
                text: String::from(*text),
            })
            .collect(),
    }
}

/// A filterset that NAMED `count` tests, out of `count` - the shape an inconclusive run has.
pub(crate) fn named(count: usize) -> Coverage {
    Coverage::Measured {
        measured: count,
        unmeasured: Vec::new(),
        not_runnable: Vec::new(),
    }
}

/// The tests under test, as if one file in `package` had added each of `names`.
///
/// Through `Scan::of` rather than a hand-built key, so what an assertion measures is the key the
/// gate actually builds from a diff - a fixture that constructed one directly would pass while the
/// extractor produced something else.
pub(crate) fn scoped(package: &str, file: &str, names: &[&str]) -> Vec<AddedTest> {
    let lines: Vec<String> = names
        .iter()
        .flat_map(|name| [String::from("#[test]"), format!("fn {name}() {{}}")])
        .collect();
    let text = format!("{}\n", lines.join("\n"));
    let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
    let dir = file
        .split_once("/src/")
        .or_else(|| file.split_once("/tests/"))
        .expect("a package directory")
        .0;
    let read = tree(&[(file, &text), (&format!("{dir}/Cargo.toml"), &manifest(package))]);
    match Scan::of(&[changed(file, 1, &borrowed)], &[String::from(file)], &read) {
        Scan::Runnable(found) => found.tests().to_vec(),
        other => panic!("expected runnable tests, got {other:?}"),
    }
}

/// The audit record from #276, as the branch that first exposed the wide run declared it.
pub(crate) fn audit_record() -> Vec<AddedTest> {
    scoped(
        "sutura-cli",
        "crates/sutura-cli/src/audit.rs",
        &["a_refused_question_is_recorded_and_names_the_refusal"],
    )
}

/// The base run that reported the false green, as nextest printed it. Two tier-backed cells failed
/// after 86 of 1810 tests and the branch's own test never ran.
pub(crate) const UNRELATED_RED: &str = concat!(
    "    Starting 1810 tests across 47 binaries\n",
    "        FAIL [   0.313s] (86/1810) sutura-app::differential tests::postgres::sums_by_month\n",
    "        FAIL [   0.204s] (87/1810) sutura-app::differential tests::postgres::one_row_per_month\n",
    "  Cancelling due to test failure: \n",
    "     Summary [   4.118s] 87 tests run: 85 passed, 2 failed, 1723 skipped\n",
    "error: test run failed\n",
);

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// Which mutation `inseparable_claim_case` commits, if any.
#[derive(Clone, Copy)]
pub(crate) enum Mutation {
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
pub(crate) fn inseparable_claim_case(declare: bool, mutation: Mutation, beside: Option<&str>) -> Verdict {
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
    std::fs::write(dir.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\npub fn g() -> u8 { 1 }\n").unwrap();
    std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout)
        .expect("utf8")
        .trim()
        .to_owned();

    let head_content = "pub fn f() -> u8 { 2 }\npub fn g() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    use super::f;\n\n    #[test]\n    fn the_wired_one() {\n        assert_eq!(f(), 2);\n    }\n}\n";
    let head_content = beside.map_or_else(
        || String::from(head_content),
        |extra| head_content.replacen("\n}\n", &format!("\n{extra}\n}}\n"), 1),
    );
    std::fs::write(dir.join("src/lib.rs"), &head_content).unwrap();

    let mutated: Option<String> = match mutation {
        Mutation::Kills => Some(head_content.replacen("{ 2 }", "{ 9 }", 1)),
        Mutation::DoesNotKill => Some(head_content.replacen("pub fn f() -> u8 { 2 }", "pub fn f() -> u8 { 2 } // same", 1)),
        Mutation::Missing => None,
    };
    if let Some(mutated) = mutated {
        // Same technique as `run_dispatches_to_the_claim_arm`'s patch: a hand-diffed pair of
        // files renamed onto `src/lib.rs`, so the mutation's own commit stays out of the
        // measured `base..HEAD` range.
        std::fs::write(dir.join(".old.rs"), &head_content).unwrap();
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
