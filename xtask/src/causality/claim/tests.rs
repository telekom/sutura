//! The `claim` arm's fixture cells.
//!
//! For `just causality` to MEASURE this diff the assertions live in a dedicated test file
//! (`#[cfg(test)] mod tests;` in `claim.rs`, assertions here) - the same split `relocation.rs` /
//! `relocation/probe.rs` makes - so the added tests are a separable target the base run can keep
//! at HEAD and run. `probe.rs` keeps the string fixtures the pure cells read; this file builds
//! REAL throwaway git repos for the git-backed arms (`--numstat`, `git apply`, `git checkout`),
//! which a pure parser over `diff --git` headers could never reach.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::probe::{KILLED, UNRELATED, cell};
use super::{Cause, Claim, MutationKill, classify_mutation, report_accepted, report_refused};
use crate::Verdict;
use crate::causality::fixtures::{changed, manifest, tree};
use crate::causality::scoped::Scan;
use crate::causality::scoped::Scoped;

/// One unique temp directory per real-git test, so the paths never collide under nextest.
static SEQ: AtomicUsize = AtomicUsize::new(0);

/// A throwaway git repo for the git-backed arms.
///
/// `git apply --numstat`, `git apply --check` and `git checkout HEAD --` run against a REAL repo
/// here, which the string fixtures cannot exercise: a mutation parser that only read `diff --git`
/// headers would pass every one of them while missing a header-less patch that `git apply`
/// accepts.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn with(rel: &str, content: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sutura-claim-tests-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _swept = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Self { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "user.name", "test"]);
        repo.write(rel, content);
        repo.commit("init");
        repo
    }

    fn git(&self, args: &[&str]) {
        // Strip the caller's git env the way the gate does (`repo::strip_git_env`): the pre-commit
        // hook runs nextest with `GIT_INDEX_FILE`/`GIT_DIR` pointing at its OWN temp index, and a
        // tests-created repo must not inherit them - that is how clean real-git fixtures panic
        // only when a commit's hook runs them.
        let mut command = Command::new("git");
        crate::repo::strip_git_env(&mut command);
        let out = command.current_dir(&self.dir).args(args).output().expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.join(rel)).expect("file readable")
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _swept = std::fs::remove_dir_all(&self.dir);
    }
}

/// A `Scoped` whose one added test is `name` - the shape the arm's `run`/`kill_cell` read.
fn one_added_test(name: &str) -> Scoped {
    let files = vec![changed("crates/x/tests/t.rs", 1, &["#[test]", &format!("fn {name}() {{}}")])];
    let text = format!("#[test]\nfn {name}() {{}}\n");
    let read = tree(&[("crates/x/tests/t.rs", &text), ("crates/x/Cargo.toml", &manifest("x"))]);
    match Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read) {
        Scan::Runnable(scoped) => scoped,
        other => panic!("expected runnable, got {other:?}"),
    }
}

/// A header-less unified diff (no `diff --git` line): `git apply` accepts it, and it names its
/// target only through the `--- a/x` / `+++ b/x` lines.
const HEADERLESS: &str = concat!(
    "--- a/crates/x/src/lib.rs\n",
    "+++ b/crates/x/src/lib.rs\n",
    "@@ -1 +1 @@\n",
    "-pub fn f() -> u8 { 1 }\n",
    "+pub fn f() -> u8 { 2 }\n",
);

/// A full unified diff touching `crates/x/src/lib.rs`.
const PATCH_LIB: &str = concat!(
    "diff --git a/crates/x/src/lib.rs b/crates/x/src/lib.rs\n",
    "--- a/crates/x/src/lib.rs\n",
    "+++ b/crates/x/src/lib.rs\n",
    "@@ -1 +1 @@\n",
    "-pub fn f() -> u8 { 1 }\n",
    "+pub fn f() -> u8 { 2 }\n",
);

/// A full unified diff against a file this arm's repo does not carry.
const PATCH_NOMATCH: &str = concat!(
    "diff --git a/crates/x/src/other.rs b/crates/x/src/other.rs\n",
    "--- a/crates/x/src/other.rs\n",
    "+++ b/crates/x/src/other.rs\n",
    "@@ -1 +1 @@\n",
    "-pub fn f() -> u8 { 1 }\n",
    "+pub fn f() -> u8 { 2 }\n",
);

// A claim over one committed commit message, deduped and sorted, and no claim from a range
// that carries none - the mirror of the trailer-required rule over on `relocation`.
#[test]
fn the_claim_is_deduped_sorted_and_absent_without_a_trailer() {
    let log = "feat(x): subject\n\nClaim-Cell: z_third\nClaim-Cell: a_first\nClaim-Cell: z_third\n";
    assert_eq!(
        Claim::of(log),
        Some(Claim {
            cells: vec![String::from("a_first"), String::from("z_third")],
        })
    );
    // A trailer with no name is no claim: it leaves the run exactly as it was.
    assert_eq!(Claim::of("chore: split\n\nClaim-Cell:\n"), None);
    assert_eq!(Claim::of("chore: no trailer at all\n"), None);
}

// RED for the arm, half 1: a run that reports a DIFFERENT test failing does not kill the
// cell - a mutation that reddens someone else is not evidence about this one.
#[test]
fn a_run_that_names_a_different_test_is_not_a_kill() {
    assert_eq!(classify_mutation(UNRELATED, &cell(), &[]), MutationKill::NotAsserted);
}

// RED for the arm, half 2: a run that compiled and ran green names no failure - a mutation
// that leaves the cell green does not kill it. Compile errors carry no failure either.
#[test]
fn a_run_that_names_no_failure_is_not_a_kill() {
    assert_eq!(
        classify_mutation("    Summary [   0.1s] 1 test run: 1 passed\n", &cell(), &[]),
        MutationKill::NotAsserted
    );
    assert_eq!(
        classify_mutation("error[E0061]: this function takes 1 argument\n", &cell(), &[]),
        MutationKill::NotAsserted
    );
}

// GREEN for the arm: a run that reports this exact cell failing IS the kill, under the same key
// the base run uses.
#[test]
fn a_run_that_names_the_cell_is_a_kill() {
    assert_eq!(classify_mutation(KILLED, &cell(), &[]), MutationKill::Killed);
}

// BLOCKING-3 RED: a kill whose panic site is a PATCHED PRODUCTION file is not an assertion kill -
// a `panic!()` planted in production kills every cell that reaches it, proving reachability and
// not that the assertion discriminates.
#[test]
fn a_kill_by_panic_in_the_mutation_is_refused() {
    let text = concat!(
        "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "thread 'audit::tests::the_added_one' panicked at crates/x/src/lib.rs:9:14:\n",
        "error: test run failed\n",
    );
    assert_eq!(
        classify_mutation(text, &cell(), &[String::from("crates/x/src/lib.rs")]),
        MutationKill::PanicsInMutation(String::from("crates/x/src/lib.rs"))
    );
}

// BLOCKING-3 green control: a REAL assertion kill panics at the CELL'S OWN test file (never a
// patched production path, because a mutation may not touch a test file), so it still counts.
#[test]
fn a_kill_by_the_cells_own_assertion_is_the_kill() {
    let text = concat!(
        "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "thread 'audit::tests::the_added_one' panicked at crates/sutura-cli/src/audit.rs:12:9:\n",
        "error: test run failed\n",
    );
    assert_eq!(
        classify_mutation(text, &cell(), &[String::from("crates/x/src/lib.rs")]),
        MutationKill::Killed
    );
}

// The accepted verdict and its line, asserted on the compiled verdict rather than prose.
#[test]
fn the_accepted_arm_prints_and_passes() {
    assert_eq!(report_accepted(1, 1), Verdict::Pass);
    assert_eq!(report_accepted(2, 2), Verdict::Pass);
}

// The refused verdict, with a non-killing mutation refused by name. This is the shape an author
// is told about, so a test pins the wording.
#[test]
fn a_declared_but_unkilled_cell_refuses_the_whole_arm() {
    assert_eq!(
        report_refused(&[Cause::NotKilled {
            cell: String::from("the_cell"),
        }]),
        Verdict::Fail
    );
}

// RED: a declared test the diff did not add, and an added test not declared, are both refusals -
// the bijection is checked both ways.
#[test]
fn the_bijection_is_checked_both_ways() {
    let claim = Claim {
        cells: vec![String::from("declared_not_added")],
    };
    let causes = super::validate(Path::new("/nowhere"), &claim, &["added_not_declared"], &[]);
    assert!(
        causes.contains(&Cause::NotAdded(String::from("declared_not_added"))),
        "{causes:?}"
    );
    assert!(
        causes.contains(&Cause::Undeclared(String::from("added_not_declared"))),
        "{causes:?}"
    );
}

// RED: a declared cell whose mutation file is missing is refused - the git checks never run, but
// the refusal must, so a real repo is the honest stage for it.
#[test]
fn a_cell_without_a_patch_is_refused() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let claim = Claim {
        cells: vec![String::from("missing_patch")],
    };
    let causes = super::validate(&repo.dir, &claim, &["missing_patch"], &[]);
    assert!(
        causes.contains(&Cause::MissingPatch(String::from("missing_patch"))),
        "{causes:?}"
    );
}

// BLOCKING-2 RED: a HEADER-LESS patch touching a test file must be refused via git's own
// `--numstat` path set. The old `diff --git`-header parser saw no paths in it, so the test-file
// rule compared nothing and the mutation leaked into the next cell.
#[test]
fn a_headerless_patch_touching_a_test_file_is_refused() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.write("crates/x/tests/t.rs", "pub fn t() -> u8 { 1 }\n");
    let headerless = HEADERLESS
        .replace("a/crates/x/src/lib.rs", "a/crates/x/tests/t.rs")
        .replace("b/crates/x/src/lib.rs", "b/crates/x/tests/t.rs");
    repo.write("devco/claim-mutations/the_cell.patch", &headerless);
    repo.commit("patches");
    let claim = Claim {
        cells: vec![String::from("the_cell")],
    };
    let causes = super::validate(&repo.dir, &claim, &["the_cell"], &[]);
    assert!(
        causes.contains(&Cause::TouchesTests {
            cell: String::from("the_cell"),
            path: String::from("crates/x/tests/t.rs"),
        }),
        "{causes:?}"
    );
}

// M5 RED, real git: a patch touching a file THIS DIFF added as a test file - even one the repo's
// whole-file rule does not call all-test (a mixed `src/lib.rs`) - is refused.
#[test]
fn a_patch_touching_a_diff_test_file_is_refused() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.write("devco/claim-mutations/the_cell.patch", PATCH_LIB);
    repo.commit("patches");
    let claim = Claim {
        cells: vec![String::from("the_cell")],
    };
    let causes = super::validate(&repo.dir, &claim, &["the_cell"], &[String::from("crates/x/src/lib.rs")]);
    assert!(
        causes.contains(&Cause::TouchesTests {
            cell: String::from("the_cell"),
            path: String::from("crates/x/src/lib.rs"),
        }),
        "{causes:?}"
    );
}

// RED: a patch that does not `git apply` in the worktree is refused by name, through a real dry
// apply whose context matches no file.
#[test]
fn a_patch_that_does_not_apply_is_refused() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.write("devco/claim-mutations/the_cell.patch", PATCH_NOMATCH);
    repo.commit("patches");
    let claim = Claim {
        cells: vec![String::from("the_cell")],
    };
    let causes = super::validate(&repo.dir, &claim, &["the_cell"], &[]);
    assert!(
        causes
            .iter()
            .any(|c| matches!(c, Cause::DoesNotApply { cell, .. } if cell == "the_cell")),
        "{causes:?}"
    );
}

// RED: `restore` must actually put a mutated file back at HEAD - a no-op restore is the shape that
// leaks one cell's patch into the next cell's run.
#[test]
fn restore_puts_a_mutated_file_back_at_head() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let patch = repo.dir.join("devco/claim-mutations/m.patch");
    repo.write("devco/claim-mutations/m.patch", PATCH_LIB);
    repo.commit("patches");
    super::apply_git(&repo.dir, &patch, false).expect("patch applies");
    assert_eq!(repo.read("crates/x/src/lib.rs").trim(), "pub fn f() -> u8 { 2 }");
    super::restore(&repo.dir, &[String::from("crates/x/src/lib.rs")]).expect("restore succeeds");
    assert_eq!(repo.read("crates/x/src/lib.rs").trim(), "pub fn f() -> u8 { 1 }");
}

// RED for `kill_cell`'s patch arm: a declared cell with no patch file is refused before any run.
#[test]
fn kill_cell_refuses_a_cell_with_no_patch() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let scoped = one_added_test("the_cell");
    let err = super::kill_cell(&repo.dir, &repo.dir.join("target"), &scoped, "the_cell").expect_err("no patch");
    assert_eq!(err, Cause::MissingPatch(String::from("the_cell")));
}

// M4 RED: the whole arm refuses a declared cell that does not hold - the verdict that a discarded
// `claim::run(..)` return would flip to a pass.
#[test]
fn the_arm_refuses_a_declared_cell_that_has_no_patch() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let scoped = one_added_test("the_cell");
    let claim = Claim {
        cells: vec![String::from("the_cell")],
    };
    let verdict = super::run(&repo.dir, &scoped, &[], &claim);
    assert_eq!(verdict, Verdict::Fail);
}

// The numstat reader turns `git apply --numstat` rows into the repo-relative path set - several
// files and a rename's `b/` side in one patch.
#[test]
fn the_numstat_reader_names_what_git_would_rewrite() {
    let numstat = concat!("1\t1\tcrates/x/src/lib.rs\n", "0\t0\tcrates/x/tests/next.rs\n",);
    assert_eq!(
        super::numstat_paths(numstat),
        vec![String::from("crates/x/src/lib.rs"), String::from("crates/x/tests/next.rs")]
    );
}
