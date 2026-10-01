//! The `claim` arm's fixture cells.
//!
//! Assertions stay here so `just causality` keeps them while reverting production. `probe.rs`
//! holds pure fixtures; git-backed cells use real throwaway repositories.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::probe::{
    DID_NOT_COMPILE, DOWNSTREAM_EXPECT, EXIT_NO_SITE, KILLED, KILLED_BEFORE_REPORTING, NO_TESTS_TO_RUN, PRODUCTION_CALLER,
    UNRELATED, async_reader, cell, pub_fn_reader, reader,
};
use super::{Caller, Cause, Claim, MutationKill, attest, classify_mutation, refused_lines, report_accepted, report_refused};
use crate::Verdict;
use crate::causality::fixtures::{changed, manifest, tree};
use crate::causality::scoped::Scan;
use crate::causality::scoped::Scoped;

/// One unique temp directory per real-git test, so the paths never collide under nextest.
static SEQ: AtomicUsize = AtomicUsize::new(0);

/// A throwaway git repo: `git apply` and `git checkout HEAD --` against a REAL repo, which a
/// parser reading only `diff --git` headers would pass while missing a header-less patch.
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

    /// The object name of `HEAD` at this moment, for a claim scoped to a just-committed state.
    fn commit_hash(&self) -> String {
        let mut command = Command::new("git");
        crate::repo::strip_git_env(&mut command);
        let out = command
            .current_dir(&self.dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git runs");
        assert!(out.status.success(), "rev-parse failed");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
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

/// A full unified diff that CREATES a brand-new file - one `git checkout HEAD -- <path>` cannot
/// restore (the path is not in HEAD, so git answers `pathspec did not match`). That is how a
/// restore failure is provoked with a real git tree.
const PATCH_NEWFILE: &str = concat!(
    "diff --git a/crates/x/src/newfile.rs b/crates/x/src/newfile.rs\n",
    "new file mode 100644\n",
    "--- /dev/null\n",
    "+++ b/crates/x/src/newfile.rs\n",
    "@@ -0,0 +1 @@\n",
    "+pub fn n() -> u8 { 1 }\n",
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

// A claim per committed message, deduped and sorted; none from a range that carries none. Each
// declaration is paired with the commit in `worktree::messages`' NUL-delimited stream.
#[test]
fn the_claim_is_deduped_sorted_and_absent_without_a_trailer() {
    let hash = "0123456789abcdef0123456789abcdef01234567".to_owned();
    let body = "feat(x): subject\n\nClaim-Cell: z_third\nClaim-Cell: a_first\nClaim-Cell: z_third\n";
    let log = format!("{hash}\0{body}\0");
    assert_eq!(
        Claim::of(&log),
        Some(Claim {
            cells: vec![String::from("a_first"), String::from("z_third")],
            by_commit: vec![(hash, vec![String::from("a_first"), String::from("z_third")])],
        })
    );
    // A trailer with no name is no claim: it leaves the run exactly as it was.
    assert_eq!(Claim::of("\0chore: split\n\nClaim-Cell:\n\0"), None);
    assert_eq!(Claim::of("\0chore: no trailer at all\n\0"), None);
}

// RED for the arm, half 1: a run that reports a DIFFERENT test failing does not kill the
// cell - a mutation that reddens someone else is not evidence about this one.
#[test]
fn a_run_that_names_a_different_test_is_not_a_kill() {
    assert_eq!(classify_mutation(UNRELATED, &cell(), &reader()), MutationKill::NotAsserted);
}

// RED for the arm, half 2: a run that compiled and ran green names no failure - a mutation
// that leaves the cell green does not kill it. Compile errors carry no failure either.
#[test]
fn a_run_that_names_no_failure_is_not_a_kill() {
    assert_eq!(
        classify_mutation("    Summary [   0.1s] 1 test run: 1 passed\n", &cell(), &reader()),
        MutationKill::NotAsserted
    );
    assert_eq!(
        classify_mutation("error[E0061]: this function takes 1 argument\n", &cell(), &reader()),
        MutationKill::NotAsserted
    );
}

// GREEN for the arm: a run that reports this exact cell failing AND carries a `panicked at` site
// inside the cell's own test region IS the kill, under the same key the base run uses.
#[test]
fn a_run_that_names_the_cell_is_a_kill() {
    assert_eq!(classify_mutation(KILLED, &cell(), &reader()), MutationKill::Killed);
}

// A panic site in a PATCHED PRODUCTION file proves reachability, not that the assertion
// discriminates, so it is refused by SITE rather than by which file the patch touched.
#[test]
fn a_kill_by_panic_in_the_mutation_is_refused() {
    let text = concat!(
        "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "thread 'audit::tests::the_added_one' panicked at crates/x/src/lib.rs:9:14:\n",
        "error: test run failed\n",
    );
    assert_eq!(
        classify_mutation(text, &cell(), &reader()),
        MutationKill::NotByAssertion {
            site: String::from("crates/x/src/lib.rs:9")
        }
    );
}

// BLOCKING-3 green control: a REAL assertion kill panics at the CELL'S OWN test region (a line
// of the cell's own test fn), so it still counts - even inside a MIXED file whose production this
// very mutation may also touch.
#[test]
fn a_kill_by_the_cells_own_assertion_is_the_kill() {
    assert_eq!(classify_mutation(KILLED, &cell(), &reader()), MutationKill::Killed);
}

// The SAME cell, declared `#[tokio::test] async fn` (#761's shape): matching only a bare `fn`
// misread a real assertion kill as `NotByAssertion`; `scoped::function_name` carries these shapes.
#[test]
fn a_kill_by_an_async_fns_own_assertion_is_the_kill() {
    assert_eq!(classify_mutation(KILLED, &cell(), &async_reader()), MutationKill::Killed);
}

// R4 BLOCKING RED, the other shape the old matcher missed: a `pub fn` test item.
#[test]
fn a_kill_by_a_pub_fns_own_assertion_is_the_kill() {
    assert_eq!(classify_mutation(KILLED, &cell(), &pub_fn_reader()), MutationKill::Killed);
}

// BLOCKING RED (exit/abort/signal): a run reporting the cell failing with NO `panicked at` site -
// a `std::process::exit(n)` / `abort()` / signal death - kills nothing by assertion.
#[test]
fn a_kill_by_process_exit_or_abort_is_refused() {
    assert_eq!(
        classify_mutation(EXIT_NO_SITE, &cell(), &reader()),
        MutationKill::NotByAssertion { site: String::new() }
    );
}

// BLOCKING RED (downstream `.expect()`): a patch makes production return `None` and an EXISTING
// `.expect()` in an UNPATCHED production file fires. The panic site is that production file, so
// the cell died by reachability, not by its own assertion.
#[test]
fn a_kill_by_a_downstream_expect_in_production_is_refused() {
    assert_eq!(
        classify_mutation(DOWNSTREAM_EXPECT, &cell(), &reader()),
        MutationKill::NotByAssertion {
            site: String::from("crates/x/src/other.rs:1")
        }
    );
}

// A panic stopping at a PRODUCTION caller is not an assertion kill. A `#[track_caller]` panic
// called FROM the cell lands on the cell's own line; no site rule refuses that, so it is review-held.
#[test]
fn a_panic_at_a_production_caller_is_refused() {
    assert_eq!(
        classify_mutation(PRODUCTION_CALLER, &cell(), &reader()),
        MutationKill::NotByAssertion {
            site: String::from("crates/x/src/lib.rs:3")
        }
    );
}

// RED (finding-5 restriction): a panic whose site is a test region of ANOTHER file - a shared
// `tests/common` helper - is not this cell's own assertion, so it must not read as a kill. The
// cell's `AddedTest` carries its OWN file, and the site must be inside that file's own test fn.
#[test]
fn a_kill_by_another_files_test_region_is_refused() {
    let text = concat!(
        "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "thread 'audit::tests::the_added_one' panicked at crates/y/tests/helper.rs:9:5:\n",
        "error: test run failed\n",
    );
    assert_eq!(
        classify_mutation(text, &cell(), &reader()),
        MutationKill::NotByAssertion {
            site: String::from("crates/y/tests/helper.rs:9")
        }
    );
}

// The own-FN half of the kill rule: a site on a PRODUCTION line of the cell's OWN file is still
// refused, which widening `test_fn_region` to the whole file would flip to `Killed`.
#[test]
fn a_panic_on_a_production_line_of_the_cells_own_file_is_refused() {
    let text = concat!(
        "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "thread 'audit::tests::the_added_one' panicked at crates/sutura-cli/src/audit.rs:1:1:\n",
        "error: test run failed\n",
    );
    assert_eq!(
        classify_mutation(text, &cell(), &reader()),
        MutationKill::NotByAssertion {
            site: String::from("crates/sutura-cli/src/audit.rs:1")
        }
    );
}

// GREEN control for the finding-5 restriction: a DIFFERENT assertion line inside the cell's OWN
// test fn is still the cell's own kill - whatever line the assert sits on.
#[test]
fn a_different_assertion_line_in_the_same_fn_is_still_a_kill() {
    let text = concat!(
        "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "thread 'audit::tests::the_added_one' panicked at crates/sutura-cli/src/audit.rs:6:9:\n",
        "error: test run failed\n",
    );
    assert_eq!(classify_mutation(text, &cell(), &reader()), MutationKill::Killed);
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
        report_refused(
            &[Cause::NotKilled {
                cell: String::from("the_cell"),
            }],
            Caller::TEST_CAUSALITY
        ),
        Verdict::Fail
    );
}

// #855's fix. RED: a build that never compiled is not evidence the mutation fails to kill - a
// `text` this shape means the gate never asked the cell anything, so folding it into `NotKilled`
// would report *the mutation does not kill it* about a subject the run never reached.
#[test]
fn a_build_that_never_compiled_is_not_a_verdict_about_the_cell() {
    assert_eq!(
        attest(false, DID_NOT_COMPILE, "the_added_one", &cell(), &reader()),
        Err(Cause::BuildFailed {
            cell: String::from("the_added_one"),
            why: String::from("the tree did not compile"),
        })
    );
}

// The same guard's OTHER shape: a filter matching no test at all is orphaning, not a kill -
// `base::names_no_tests` is the predicate the base run already classifies `NotRun` by, and
// `could_not_attest` reuses it rather than reading only a compile failure.
#[test]
fn a_filter_matching_nothing_is_not_a_verdict_about_the_cell_either() {
    assert_eq!(
        attest(false, NO_TESTS_TO_RUN, "the_added_one", &cell(), &reader()),
        Err(Cause::BuildFailed {
            cell: String::from("the_added_one"),
            why: String::from("the filter matched no test - orphaned by the patch, not killed by it"),
        })
    );
}

// Successful process status takes precedence over compiler-looking output.
#[test]
fn a_successful_run_is_never_read_as_a_build_failure() {
    assert_eq!(
        attest(true, DID_NOT_COMPILE, "the_added_one", &cell(), &reader()),
        Err(Cause::NotKilled {
            cell: String::from("the_added_one"),
        })
    );
}

#[test]
fn a_successful_run_cannot_borrow_a_captured_failure() {
    let survived = concat!(
        "        PASS [   0.020s] (1/1) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "  stdout ───\n\n",
        "            FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "    thread 'audit::tests::the_added_one' panicked at crates/sutura-cli/src/audit.rs:7:9:\n",
        "     Summary [   0.040s] 1 test run: 1 passed\n",
        "        PASS [   0.020s] sutura-cli::bin/sutura audit::tests::the_added_one\n",
    );
    let not_killed = Err(Cause::NotKilled {
        cell: String::from("the_added_one"),
    });
    assert_eq!(attest(true, survived, "the_added_one", &cell(), &reader()), not_killed);
}

// A completed run naming an unrelated failure still reaches ordinary classification.
#[test]
fn an_ordinary_run_failure_still_reaches_the_normal_classification() {
    assert_eq!(
        attest(false, UNRELATED, "the_added_one", &cell(), &reader()),
        Err(Cause::NotKilled {
            cell: String::from("the_added_one"),
        })
    );
}

// A FAILED run naming no test at all (a signal kill before nextest printed anything) is not
// entitled to *the mutation does not kill it*, which only a SUCCESSFUL run may claim.
#[test]
fn a_failed_run_naming_no_test_at_all_is_not_a_verdict_about_the_cell() {
    assert_eq!(
        attest(false, KILLED_BEFORE_REPORTING, "the_added_one", &cell(), &reader()),
        Err(Cause::BuildFailed {
            cell: String::from("the_added_one"),
            why: String::from("the run failed but named no test failing at all - not a verdict about the declaration"),
        })
    );
}

// #1092: a cell the forced requirement ended at its tier lookup in the kill worktree never ran,
// so it is a non-verdict rather than a survived mutation. The run text is here, not in `probe`,
// because `probe` is reverted with the implementation on causality's base.
#[test]
fn a_cell_that_never_reached_its_tier_is_not_a_verdict_about_the_mutation() {
    let tier_absent = concat!(
        "        FAIL [   0.021s] (1/1) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "thread 'audit::tests::the_added_one' panicked at dev/src/provisioned.rs:413:40:\n",
        "  This run requires a provisioned service tier, so an absent one is a failure rather than a\n",
        "error: test run failed\n",
    );
    let causes = [attest(false, tier_absent, "the_added_one", &cell(), &reader()).expect_err("it never ran")];
    assert_eq!(report_refused(&causes, Caller::TEST_CAUSALITY), Verdict::Inconclusive);
    // A tier cell, not a build that "may recur": the remedy is a hand proof, not a re-run.
    let lines = refused_lines(&causes, true, Caller::TEST_CAUSALITY).join("\n");
    assert!(lines.contains("does not provision"), "{lines}");
    assert!(!lines.contains("re-run it"), "{lines}");
}

// A run that PASSED survived the mutation, even when its text echoes the tier sentence: only the
// requirement's own panic site may turn a run into a non-verdict.
#[test]
fn a_survived_run_echoing_the_tier_sentence_is_still_not_killed() {
    let survived = concat!(
        "        PASS [   0.021s] (1/1) sutura-cli::bin/sutura audit::tests::the_added_one\n",
        "note: required() says: \"This run requires a provisioned service tier\"\n",
        "     Summary [   0.4s] 1 test run: 1 passed, 0 skipped\n",
    );
    let not_killed = Err(Cause::NotKilled {
        cell: String::from("the_added_one"),
    });
    assert_eq!(attest(true, survived, "the_added_one", &cell(), &reader()), not_killed);
}

// GREEN control: a genuine kill still reaches `Ok` (`ok = false`: a killed cell is a failing run).
#[test]
fn a_genuine_kill_still_passes_through_the_guard() {
    assert_eq!(attest(false, KILLED, "the_added_one", &cell(), &reader()), Ok(()));
}

// The build-failure cause is a NON-VERDICT, the same exit code `Verdict::Inconclusive` carries
// elsewhere in this gate - not the author-actionable `Fail` every other cause here is.
#[test]
fn a_build_failure_refuses_as_inconclusive_not_failed() {
    assert_eq!(
        report_refused(
            &[Cause::BuildFailed {
                cell: String::from("the_cell"),
                why: String::from("the tree did not compile"),
            }],
            Caller::TEST_CAUSALITY
        ),
        Verdict::Inconclusive
    );
}

// A build failure mixed with an author-actionable cause stays `Fail`: the run DID measure
// something real, so the non-verdict exit is not owed just because one cause among several
// could not attest.
#[test]
fn a_build_failure_mixed_with_a_real_cause_stays_failed() {
    assert_eq!(
        report_refused(
            &[
                Cause::BuildFailed {
                    cell: String::from("cell_a"),
                    why: String::from("the tree did not compile"),
                },
                Cause::NotKilled {
                    cell: String::from("cell_b"),
                },
            ],
            Caller::TEST_CAUSALITY
        ),
        Verdict::Fail
    );
}

// #951: the refusal's wording is the CALLER's - `causality::rot::run` re-checks an old declaration,
// where "fix or drop the declaration" is wrong, so it sees its own task name and remedy only.
#[test]
fn refused_lines_use_the_callers_own_task_name_and_remedy() {
    const OTHER: Caller = Caller {
        task: "some-other-gate",
        remedy: &["a caller-specific remedy line"],
    };
    let lines = refused_lines(
        &[Cause::NotKilled {
            cell: String::from("the_cell"),
        }],
        false,
        OTHER,
    );
    assert!(lines[0].contains("some-other-gate"), "{lines:?}");
    assert!(!lines[0].contains("test-causality"), "{lines:?}");
    assert!(
        lines.iter().any(|l| l == "a caller-specific remedy line"),
        "the caller's own remedy must be printed: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("Fix or drop the declaration")),
        "a caller with its own remedy must not also print TEST_CAUSALITY's: {lines:?}"
    );
}

// RED for #954, half 1: the bijection is PER COMMIT in the direction that refuses. A declared
// cell whose DECLARING COMMIT added no such test is still `NotAdded` - the tightening survives.
// The declaring commit adds a REAL resolvable test (`sibling`), and the resolution answers
// exactly that, so `never_added` is `NotAdded` because the declaration names no test THIS
// COMMIT added - not because the diff could not be read.
#[test]
fn a_declared_cell_not_added_by_its_commit_is_refused() {
    let repo = Repo::with(
        "Cargo.toml",
        "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    // Commit 2 adds a REAL test (`sibling`) but the claim declares `never_added` ON THIS COMMIT.
    // The per-commit resolution answers `sibling`, so `never_added` is `NotAdded`: the
    // declaration may not answer for a test this commit never added.
    repo.write("tests/t.rs", "#[test]\nfn sibling() {}\n");
    repo.commit("adds sibling");
    let declaring = repo.commit_hash();
    let claim = Claim {
        cells: vec![String::from("never_added")],
        by_commit: vec![(declaring, vec![String::from("never_added")])],
    };
    let causes = super::validate(&repo.dir, &claim, &[]);
    assert!(causes.contains(&Cause::NotAdded(String::from("never_added"))), "{causes:?}");
}

// The per-commit scope, driven through the REAL `worktree::messages` stream rather than a
// hand-built `Claim`: commit A adds `the_claimed_one` with no trailer, and an EMPTY sibling commit
// B carries `Claim-Cell: the_claimed_one`. The declaration is keyed to B, and B added nothing, so
// it is `NotAdded`. With `messages` back on the un-framed `--format=%B` this is the shape the
// gate accepted: the declaration had no commit to answer for.
#[test]
fn a_declaration_answers_only_for_the_commit_that_carries_it() {
    let repo = Repo::with(
        "Cargo.toml",
        "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.commit("base");
    let base = crate::causality::provenance::Commit::parse(&repo.commit_hash()).expect("a commit");
    repo.write("tests/t.rs", "#[test]\nfn the_claimed_one() {}\n");
    repo.commit("test: adds the_claimed_one");
    repo.git(&[
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "chore: sibling\n\nClaim-Cell: the_claimed_one",
    ]);
    let sibling = repo.commit_hash();
    let claim = Claim::of(&crate::causality::worktree::messages(&repo.dir, &base)).expect("a claim");
    assert_eq!(claim.by_commit(), &vec![(sibling, vec![String::from("the_claimed_one")])]);
    let causes = super::validate(&repo.dir, &claim, &[]);
    assert!(
        causes.contains(&Cause::NotAdded(String::from("the_claimed_one"))),
        "{causes:?}"
    );
}

// RED for #954, half 2: the DIRECTION THAT REFUSES is gone - a test a NON-declaring sibling
// commit added must NOT be refused as `Undeclared`. Before this change the range-wide bijection
// read every added test in `base..HEAD` as undeclared the moment any other commit declared
// anything (measured `×135` on #929). The ordinary base/head proof, not the claim arm, proves it.
#[test]
fn an_added_test_a_sibling_commit_declared_not_for_is_not_refused() {
    let repo = Repo::with(
        "Cargo.toml",
        "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    // Commit 2 declares `declared_cell` AND adds it, and commits its killing mutation, all in the
    // SAME commit - so the declaration answers for that commit's own added tests.
    repo.write("tests/t.rs", "#[test]\nfn declared_cell() {}\n");
    repo.write(
        "devco/claim-mutations/declared_cell.patch",
        "diff --git a/src/lib.rs b/src/lib.rs\n\
--- a/src/lib.rs\n\
+++ b/src/lib.rs\n\
@@ -1 +1 @@\n\
-pub fn f() -> u8 { 1 }\n\
+pub fn f() -> u8 { 2 }\n",
    );
    repo.commit("adds declared_cell and declares it");
    let declaring = repo.commit_hash();
    // Commit 3 adds an ordinary test the declaring commit did NOT declare. The old range-wide
    // bijection would refuse it as `Undeclared`; the claim arm must let the ordinary proof have it.
    repo.write("tests/t.rs", "#[test]\nfn declared_cell() {}\n#[test]\nfn sibling() {}\n");
    repo.commit("adds sibling test");
    let claim = Claim {
        cells: vec![String::from("declared_cell")],
        by_commit: vec![(declaring, vec![String::from("declared_cell")])],
    };
    let causes = super::validate(&repo.dir, &claim, &[]);
    assert!(
        causes.is_empty(),
        "an undeclared sibling test is the ordinary proof's, not a claim refusal: {causes:?}"
    );
}
// RED: a declared cell whose mutation file is missing is refused - the git checks never run, but
// the refusal must, so a real repo is the honest stage for it.
#[test]
fn a_cell_without_a_patch_is_refused() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let claim = Claim::synthetic(["missing_patch"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[]);
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
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[]);
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
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[String::from("crates/x/src/lib.rs")]);
    assert!(
        causes.contains(&Cause::TouchesTests {
            cell: String::from("the_cell"),
            path: String::from("crates/x/src/lib.rs"),
        }),
        "{causes:?}"
    );
}

// Real git: a patch that CREATES a file - even one with its own `#[cfg(test)]` region - is refused
// as `CreatesFile` before any apply, rather than reaching a misdescribing `RestoreFailed`.
#[test]
fn a_patch_that_creates_a_file_is_refused() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.write("devco/claim-mutations/the_cell.patch", PATCH_NEWFILE);
    repo.commit("patches");
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[]);
    assert!(
        causes.contains(&Cause::CreatesFile {
            cell: String::from("the_cell"),
            path: String::from("crates/x/src/newfile.rs"),
        }),
        "{causes:?}"
    );
}

// Real git: a RENAME header (no `---`/`+++` lines) also creates a path HEAD lacks, so
// `created_paths` names the `rename to` path as `CreatesFile` before any apply.
#[test]
fn a_patch_that_renames_a_file_is_refused_as_creates_file() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.write(
        "devco/claim-mutations/the_cell.patch",
        concat!(
            "diff --git a/crates/x/src/lib.rs b/crates/x/src/renamed.rs\n",
            "similarity index 100%\n",
            "rename from crates/x/src/lib.rs\n",
            "rename to crates/x/src/renamed.rs\n",
        ),
    );
    repo.commit("patches");
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[]);
    assert!(
        causes.contains(&Cause::CreatesFile {
            cell: String::from("the_cell"),
            path: String::from("crates/x/src/renamed.rs"),
        }),
        "{causes:?}"
    );
}

// R5 NON-BLOCKING (finding 1) RED, real git: the COPY header twin of
// `a_patch_that_renames_a_file_is_refused_as_creates_file` - `copy from`/`copy to` also names a
// CREATED path HEAD does not carry, and `created_paths` must refuse it as `CreatesFile` before
// any apply, not answer `RestoreFailed`.
#[test]
fn a_patch_that_copies_a_file_is_refused_as_creates_file() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.write(
        "devco/claim-mutations/the_cell.patch",
        concat!(
            "diff --git a/crates/x/src/lib.rs b/crates/x/src/copied.rs\n",
            "similarity index 100%\n",
            "copy from crates/x/src/lib.rs\n",
            "copy to crates/x/src/copied.rs\n",
        ),
    );
    repo.commit("patches");
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[]);
    assert!(
        causes.contains(&Cause::CreatesFile {
            cell: String::from("the_cell"),
            path: String::from("crates/x/src/copied.rs"),
        }),
        "{causes:?}"
    );
}

// BLOCKING-2 green control: a MIXED file (production + an inline `#[cfg(test)] mod tests`) is a
// legitimate mutation target in its PRODUCTION lines, so a patch touching line 1 (above the test
// region) is ALLOWED - the shape #761's inline cells need to touch their own file. The old
// blanket "patch touches a diff test-file" rule refused this.
#[test]
fn a_patch_touching_a_mixed_files_production_is_allowed() {
    let repo = Repo::with(
        "crates/x/src/lib.rs",
        "pub fn f() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { assert!(false); }\n}\n",
    );
    repo.write("devco/claim-mutations/the_cell.patch", PATCH_LIB);
    repo.commit("patches");
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[String::from("crates/x/src/lib.rs")]);
    assert!(!causes.iter().any(|c| matches!(c, Cause::TouchesTests { .. })), "{causes:?}");
}

// BLOCKING-2 RED: the SAME mixed file, but the patch adds a line INSIDE the `#[cfg(test)] mod
// tests` region (line 6, `fn t`) - editing the test region is refused, exactly as a pure test
// file's edit is. The refusal is the TEXT rule's: after apply, the region is no longer
// byte-identical to HEAD, so the arm answers `PatchRewritesCell`.
#[test]
fn a_patch_with_a_hunk_inside_a_test_region_is_refused() {
    let repo = Repo::with(
        "crates/x/src/lib.rs",
        "pub fn f() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { assert!(false); }\n}\n",
    );
    let patch = concat!(
        "diff --git a/crates/x/src/lib.rs b/crates/x/src/lib.rs\n",
        "--- a/crates/x/src/lib.rs\n",
        "+++ b/crates/x/src/lib.rs\n",
        "@@ -5,3 +5,3 @@\n",
        "     #[test]\n",
        "-    fn t() { assert!(false); }\n",
        "+    fn t() { assert!(true); }\n",
        " }\n",
    );
    repo.write("devco/claim-mutations/the_cell.patch", patch);
    repo.commit("patches");
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[String::from("crates/x/src/lib.rs")]);
    assert!(
        causes.contains(&Cause::PatchRewritesCell {
            cell: String::from("the_cell"),
            path: String::from("crates/x/src/lib.rs"),
        }),
        "{causes:?}"
    );
}

// A deletion-only hunk above `#[cfg(test)]` shifts every line by 12 before a second hunk rewrites
// the cell's assertion; the region is compared by BYTES, so it is still `PatchRewritesCell`.
#[test]
fn a_deletion_above_the_region_cannot_smuggle_a_test_edit() {
    let src_leading = "// c1\n// c2\n// c3\n// c4\n// c5\n// c6\n// c7\n// c8\n// c9\n// c10\n// c11\n// c12\n";
    let mut src = String::from(src_leading);
    src.push_str("pub fn f() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { assert_eq!(1, 1); }\n}\n");
    let repo = Repo::with("crates/x/src/lib.rs", &src);
    let deletions = "-// c1\n-// c2\n-// c3\n-// c4\n-// c5\n-// c6\n-// c7\n-// c8\n-// c9\n-// c10\n-// c11\n-// c12\n";
    let mut patch = String::from(
        "diff --git a/crates/x/src/lib.rs b/crates/x/src/lib.rs\n--- a/crates/x/src/lib.rs\n+++ b/crates/x/src/lib.rs\n@@ -1,13 +1 @@\n",
    );
    patch.push_str(deletions);
    patch.push_str(" pub fn f() -> u8 { 1 }\n");
    patch.push_str("@@ -17,3 +5,3 @@\n     #[test]\n-    fn t() { assert_eq!(1, 1); }\n+    fn t() { assert_eq!(1, 2); }\n }\n");
    repo.write("devco/claim-mutations/the_cell.patch", &patch);
    repo.commit("patches");
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[String::from("crates/x/src/lib.rs")]);
    assert!(
        causes.contains(&Cause::PatchRewritesCell {
            cell: String::from("the_cell"),
            path: String::from("crates/x/src/lib.rs"),
        }),
        "{causes:?}"
    );
}

// BLOCKING-1 green control for the TEXT rule: the SAME deletion-only hunk, but WITHOUT the second
// assertion edit, is a legitimate production mutation - the post-image region is relocated by
// content and is byte-identical to HEAD's, so `validate` refuses nothing. This is the line-shift
// a real production deletion causes, and it must stay declarable.
#[test]
fn a_deletion_above_the_region_in_production_only_is_allowed() {
    let src_leading = "// c1\n// c2\n// c3\n// c4\n// c5\n// c6\n// c7\n// c8\n// c9\n// c10\n// c11\n// c12\n";
    let mut src = String::from(src_leading);
    src.push_str("pub fn f() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { assert_eq!(1, 1); }\n}\n");
    let repo = Repo::with("crates/x/src/lib.rs", &src);
    let deletions = "-// c1\n-// c2\n-// c3\n-// c4\n-// c5\n-// c6\n-// c7\n-// c8\n-// c9\n-// c10\n-// c11\n-// c12\n";
    let mut patch = String::from(
        "diff --git a/crates/x/src/lib.rs b/crates/x/src/lib.rs\n--- a/crates/x/src/lib.rs\n+++ b/crates/x/src/lib.rs\n@@ -1,13 +1 @@\n",
    );
    patch.push_str(deletions);
    patch.push_str(" pub fn f() -> u8 { 1 }\n");
    repo.write("devco/claim-mutations/the_cell.patch", &patch);
    repo.commit("patches");
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[String::from("crates/x/src/lib.rs")]);
    assert!(
        !causes.iter().any(|c| matches!(c, Cause::PatchRewritesCell { .. })),
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
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let causes = super::validate(&repo.dir, &claim, &[]);
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
    let err = super::kill_cell(&repo.dir, &repo.dir.join("target"), &scoped, "the_cell", &[]).expect_err("no patch");
    assert_eq!(err, Cause::MissingPatch(String::from("the_cell")));
}

// RED for `kill_cell`'s restore arm: a mutation that CREATES a path `git checkout HEAD --` cannot
// restore must refuse the whole arm as `RestoreFailed`, because a tree left mutated leaks into the
// next cell's run. `restore_puts_a_mutated_file_back_at_head` proves `restore` works; this proves
// `kill_cell` refuses when it does NOT - the half the round-2 review measured as green under MC.
#[test]
fn kill_cell_refuses_a_restore_failure() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.write("devco/claim-mutations/the_cell.patch", PATCH_NEWFILE);
    repo.commit("patches");
    let scoped = one_added_test("the_cell");
    let err = super::kill_cell(&repo.dir, &repo.dir.join("target"), &scoped, "the_cell", &[]).expect_err("restore fails");
    assert!(
        matches!(&err, Cause::RestoreFailed { cell, why } if cell == "the_cell" && !why.is_empty()),
        "{err:?}"
    );
}

// The whole arm refuses a declared cell that does not hold. This asserts `claim::run`'s OWN
// refusal; the site that routes a declared diff INTO `claim::run` (the `return claim::run(..)` in
// `crate::causality::run`) is one level up, in a function this unit surface does not reach, so that
// wiring is read by review, not measured here.
#[test]
fn the_arm_refuses_a_declared_cell_that_has_no_patch() {
    let repo = Repo::with("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let scoped = one_added_test("the_cell");
    let claim = Claim::synthetic(["the_cell"].into_iter()).expect("a named cell");
    let verdict = super::run(&repo.dir, &scoped, &[], &claim, Caller::TEST_CAUSALITY);
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

/// A mutation of the `src/lib.rs` the #1054 cells write, so each cell's claim has a patch to read.
const LIB_PATCH: &str = "diff --git a/src/lib.rs b/src/lib.rs\n\
--- a/src/lib.rs\n\
+++ b/src/lib.rs\n\
@@ -1 +1 @@\n\
-pub fn f() -> u8 { 1 }\n\
+pub fn f() -> u8 { 2 }\n";
const MANIFEST: &str = "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

/// `validate`'s causes for ONE cell declared on `declaring`, with its patch committed there.
fn declared_on(repo: &Repo, declaring: String, cell: &str) -> Vec<Cause> {
    let claim = Claim {
        cells: vec![cell.to_owned()],
        by_commit: vec![(declaring, vec![cell.to_owned()])],
    };
    super::validate(&repo.dir, &claim, &[])
}

/// #1054's shape, returning commit 2: it appends `added_here` below `pre_existing`, and commit 3
/// inserts two `fn`s above both (comments would not show it: the item lookup skips them).
fn shifted_below_an_earlier_test() -> (Repo, String) {
    let repo = Repo::with("Cargo.toml", MANIFEST);
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.write("tests/t.rs", "#[test]\nfn pre_existing() {}\n");
    repo.commit("adds pre_existing");
    repo.write("tests/t.rs", "#[test]\nfn pre_existing() {}\n#[test]\nfn added_here() {}\n");
    repo.write("devco/claim-mutations/added_here.patch", LIB_PATCH);
    repo.write("devco/claim-mutations/pre_existing.patch", LIB_PATCH);
    repo.commit("adds added_here");
    let declaring = repo.commit_hash();
    let shifted = "fn above_one() {}\nfn above_two() {}\n#[test]\nfn pre_existing() {}\n#[test]\nfn added_here() {}\n";
    repo.write("tests/t.rs", shifted);
    repo.commit("inserts two items above both tests");
    (repo, declaring)
}

// RED for #1054, the false refusal: read against HEAD, commit 2's added `#[test]` line lands on
// `pre_existing`, so the valid declaration of `added_here` refuses `NotAdded`.
#[test]
fn a_later_commit_shifting_the_declared_test_is_not_refused() {
    let (repo, declaring) = shifted_below_an_earlier_test();
    let causes = declared_on(&repo, declaring, "added_here");
    assert!(
        causes.is_empty(),
        "a shifted declared test is still the declaring commit's: {causes:?}"
    );
}

// RED for #1054, the false accept: the same misread names `pre_existing`, a test commit 2 never
// added, and a declaration of it would pass the per-commit bijection.
#[test]
fn a_later_commit_shifting_lines_does_not_lend_an_earlier_test_to_the_declaring_commit() {
    let (repo, declaring) = shifted_below_an_earlier_test();
    let causes = declared_on(&repo, declaring, "pre_existing");
    assert_eq!(causes, [Cause::NotAdded(String::from("pre_existing"))]);
}

// RED for #1054: a path the declaring commit does not carry answers `None`, never HEAD's copy. The
// manifest arrives only in a later commit, so the declaring commit's test has no owning package.
#[test]
fn a_manifest_a_later_commit_adds_does_not_name_the_declaring_commits_test() {
    let repo = Repo::with("README", "x\n");
    repo.write("tests/t.rs", "#[test]\nfn the_orphan() {}\n");
    repo.write("devco/claim-mutations/the_orphan.patch", LIB_PATCH);
    repo.commit("adds the_orphan before any manifest owns it, and declares it");
    let declaring = repo.commit_hash();
    repo.write("Cargo.toml", MANIFEST);
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.commit("adds the manifest");
    let causes = declared_on(&repo, declaring, "the_orphan");
    assert_eq!(causes, [Cause::NotAdded(String::from("the_orphan"))]);
}
