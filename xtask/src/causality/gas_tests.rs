//! The REAL-CARGO fixtures for `causality::run`'s claim dispatch, split out of `causality.rs`
//! at the 1000-line cap.
//!
//! `claim::tests` drives `claim::run` with string-only fixtures; these build a tiny real
//! crate and run `kill_cell`'s real `cargo test` against it, because the dispatch from
//! `causality::run` into `claim::run` (and, after `github.com/telekom/sutura#954`, the COMPOSITE
//! that also runs the ordinary proof over a range's undeclared additions) is a seam no string
//! fixture reaches. `set_current_dir` is process-global, so each fixture asserts `NEXTEST` and
//! runs alone.
//!
//! `github.com/telekom/sutura#1016` added the fourth: `causality::tests_only`'s own dispatch,
//! reached over a `Separable::revert`-empty diff rather than an inseparable one.
//!
//! `github.com/telekom/sutura#1025` added the fifth: `super::edited::touches` routing a file with
//! NO added marker into `test_files` at all, over a diff whose only change is an added line
//! inside a test that already existed.

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

/// `github.com/telekom/sutura#954`'s COMPOSITE half, proven end to end: a declaring commit
/// lands a claim cell AND an ordinary red-on-base test beside it, and `causality::run`'s
/// verdict is their AND. Before #954 the range-wide bijection refused the ordinary test as
/// `Undeclared` (`Fail`); the claim arm alone would prove only the claimed cell and silently
/// drop the ordinary one. Both must be proven for `Pass` - the claim cell's mutation must
/// kill, AND `the_ordinary_one` must be red against the reverted base (`g` returns 1 there,
/// and the test asserts 2). The fixture is the separable shape `f14e8a3a` landed on `main`:
/// an added test file, one declared cell, one undeclared test, all in one commit.
///
/// And the AND is not the claim arm alone: the same shape with the undeclared test pinning `f`
/// (which the diff never touched - green on base AND head) must be `Fail`. The claim arm answers
/// `Pass` for either shape, so only the ordinary half running over the leftovers refuses it.
#[test]
fn a_claim_cell_and_an_ordinary_test_are_both_proven() {
    // `set_current_dir` is process-global - see `falsifier`'s own use of this guard.
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "this test moves the process's current directory, so it must have the process to \
             itself: run it under `just test`."
    );
    assert_eq!(
        composite("fn the_ordinary_one() { assert_eq!(g(), 2); }"),
        Verdict::Pass,
        "the claim cell's mutation must kill AND the undeclared test must be red on base"
    );
    assert_eq!(
        composite("fn the_laundered_one() { assert_eq!(f(), 1); }"),
        Verdict::Fail,
        "an undeclared test green on base must not ride on a beside-it claim cell's kill"
    );
}

/// [`a_claim_cell_and_an_ordinary_test_are_both_proven`]'s fixture: a claimed cell plus `beside`,
/// one undeclared `#[test]` fn in the same added file, run through `causality::run`.
fn composite(beside: &str) -> Verdict {
    let dir = std::env::temp_dir().join(format!(
        "sutura-causality-composite-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _swept = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "test"]);

    // The mutation's patch TEXT is built before either commit (the `.old.rs`/`.new.rs` diff
    // technique the wiring test uses), so the patch file's own commit stays out of the
    // measured base..HEAD diff. It is diffed against the FULL head `lib.rs` (two lines), so
    // the hunk carries enough context to apply against the two-line file at HEAD - a
    // no-context single-line hunk is refused by `git apply` once a second line follows. The
    // patch flips `f` from 1 to 9 - a kill only the claimed cell's `assert_eq!(f(), 1)` can
    // redden.
    let head_lib = "pub fn f() -> u8 { 1 }\npub fn g() -> u8 { 2 }\n";
    std::fs::write(dir.join(".old.rs"), head_lib).unwrap();
    std::fs::write(
        dir.join(".new.rs"),
        head_lib.replacen("pub fn f() -> u8 { 1 }", "pub fn f() -> u8 { 9 }", 1),
    )
    .unwrap();
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
    // Both `f` and `g` live in lib.rs so an integration test can `use wired::{f, g}`.
    std::fs::write(dir.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\npub fn g() -> u8 { 1 }\n").unwrap();
    std::fs::create_dir_all(dir.join("devco/claim-mutations")).unwrap();
    std::fs::write(dir.join("devco/claim-mutations/the_claimed_one.patch"), &patch).unwrap();
    // `repo::root` requires flake.nix ALONGSIDE Cargo.toml to stop its walk (see the wiring
    // test's note).
    std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout)
        .expect("utf8")
        .trim()
        .to_owned();

    // HEAD: one added test file carrying a claimed cell (pins the BASE-provided `f() == 1`,
    // so it is green on base and only a mutation can prove it) and an ordinary test that
    // pins the HEAD value of `g` - so it is red on base and green on HEAD, the ordinary
    // proof's territory. `lib.rs`'s `g` change is the separable implementation the ordinary
    // proof reverts; `f` is untouched by the diff, exactly what a claim cell pins.
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("tests/t.rs"),
        format!("use wired::{{f, g}};\n\n#[test]\nfn the_claimed_one() {{ assert_eq!(f(), 1); }}\n#[test]\n{beside}\n"),
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\npub fn g() -> u8 { 2 }\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(
        &dir,
        &[
            "commit",
            "-q",
            "-m",
            "feat: pin f's base value\n\nClaim-Cell: the_claimed_one",
        ],
    );

    let original = std::env::current_dir().expect("a current directory");
    std::env::set_current_dir(&dir).expect("point the process at the fixture repo");
    let verdict = super::run(&[String::from("--since"), base]);
    std::env::set_current_dir(&original).expect("restore the current directory");
    drop(std::fs::remove_dir_all(&dir));
    verdict
}

/// Which patch, if any, `tests_only_claim_case` commits for `the_pinned_one`.
#[derive(Clone, Copy)]
enum Mutation {
    /// Flips `f`'s return value, killing `assert_eq!(f(), 1)`.
    Kills,
    /// Touches the production line without changing `f`'s return value: applies cleanly, kills
    /// nothing.
    DoesNotKill,
    /// No patch is committed at all.
    Missing,
}

/// `github.com/telekom/sutura#1016`'s own fixture: a NEW file, `tests/it.rs`, is the WHOLE
/// diff - `src/lib.rs`, the function it pins, is never touched at HEAD - so `plan()` finds
/// nothing to revert (`Separable::revert` is empty) and `causality::run` reaches
/// `tests_only`. Before this decision that arm passed unconditionally
/// (`report_nothing_to_revert`, deleted); `Verdict::Fail` from any case here is reachable
/// ONLY through the new dispatch.
///
/// `beside` is a second `#[test]` fn's source appended to the same added file, left out of
/// the commit's `Claim-Cell:` trailer - the partial-declaration shape, same file shape as
/// `composite` above but over an empty revert.
fn tests_only_claim_case(declare: bool, mutation: Mutation, beside: Option<&str>) -> Verdict {
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "this fixture moves the process's current directory, so it must have the process to \
         itself: run it under `just test`."
    );
    let dir = std::env::temp_dir().join(format!(
        "sutura-causality-tests-only-{}-{}",
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
    let lib_content = "pub fn f() -> u8 { 1 }\n";
    std::fs::write(dir.join("src/lib.rs"), lib_content).unwrap();
    std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();

    // THE PATCH GOES INTO THE INIT COMMIT, BEFORE `base` IS CAPTURED - and that placement is
    // load-bearing, not cosmetic. `Reach::of` reverts a non-Rust file like any other
    // implementation (`plan.rs`'s own header), so a patch committed ALONGSIDE `tests/it.rs`
    // would itself be a second changed file with something to revert - `separable.revert`
    // would no longer be empty, and the diff would route through the OTHER `Plan::Separable`
    // arm instead of `tests_only`, silently testing a different arm than the one this fixture
    // names. `claim::run` reads the patch from the checkout at HEAD, never from the diff, so
    // committing it before `base` proves nothing less.
    let mutated: Option<String> = match mutation {
        Mutation::Kills => Some(lib_content.replacen("{ 1 }", "{ 9 }", 1)),
        Mutation::DoesNotKill => Some(lib_content.replacen("pub fn f() -> u8 { 1 }", "pub fn f() -> u8 { 1 } // same", 1)),
        Mutation::Missing => None,
    };
    if let Some(mutated) = mutated {
        // Same technique as the wiring fixture's patch: a hand-diffed pair of files renamed
        // onto `src/lib.rs`.
        std::fs::write(dir.join(".old.rs"), lib_content).unwrap();
        std::fs::write(dir.join(".new.rs"), &mutated).unwrap();
        let diffed = git_output(&dir, &["diff", "--no-index", "--", ".old.rs", ".new.rs"]);
        let patch = String::from_utf8_lossy(&diffed.stdout)
            .replace(".old.rs", "src/lib.rs")
            .replace(".new.rs", "src/lib.rs");
        std::fs::remove_file(dir.join(".old.rs")).unwrap();
        std::fs::remove_file(dir.join(".new.rs")).unwrap();
        std::fs::create_dir_all(dir.join("devco/claim-mutations")).unwrap();
        std::fs::write(dir.join("devco/claim-mutations/the_pinned_one.patch"), &patch).unwrap();
    }

    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout)
        .expect("utf8")
        .trim()
        .to_owned();

    // THE WHOLE MEASURED DIFF: one new test file. `src/lib.rs` is not written again at all,
    // and nothing else changes - `separable.revert` is empty because there is truly nothing
    // else in this diff to revert.
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    let mut it = String::from("use wired::f;\n\n#[test]\nfn the_pinned_one() {\n    assert_eq!(f(), 1);\n}\n");
    if let Some(beside) = beside {
        it.push_str("\n#[test]\n");
        it.push_str(beside);
        it.push('\n');
    }
    std::fs::write(dir.join("tests/it.rs"), it).unwrap();
    git(&dir, &["add", "-A"]);
    let message = if declare {
        "test: pin f's existing return value\n\nClaim-Cell: the_pinned_one"
    } else {
        "test: pin f's existing return value"
    };
    git(&dir, &["commit", "-q", "-m", message]);

    let original = std::env::current_dir().expect("a current directory");
    std::env::set_current_dir(&dir).expect("point the process at the fixture repo");
    let verdict = super::run(&[String::from("--since"), base]);
    std::env::set_current_dir(&original).expect("restore the current directory");
    drop(std::fs::remove_dir_all(&dir));
    verdict
}

/// **THE RED-ON-BASE CELL FOR THIS ISSUE.** No `Claim-Cell:` trailer at all, over a diff whose
/// `Separable::revert` is empty - the exact shape `report_nothing_to_revert` passed
/// unconditionally before this decision. `Verdict::Fail` is NEW behaviour here, not a pin of
/// an existing one, so this cell needs no `Claim-Cell:` of its own: neutralising
/// `tests_only`'s dispatch with `&& false` on the `Scan::Runnable` arm reproduces the base
/// tree's answer, `Verdict::Pass`, confirmed by hand.
#[test]
fn a_tests_only_addition_with_no_claim_cell_is_refused() {
    assert_eq!(
        tests_only_claim_case(false, Mutation::Missing, None),
        Verdict::Fail,
        "an added test pinning existing behaviour with no `Claim-Cell:` trailer must refuse - \
         the old arm answered `Verdict::Pass` unconditionally here"
    );
}

/// The declared half: a complete declaration on a tests-only diff is EVALUATED, and a
/// mutation that kills by the cell's own assertion is accepted.
#[test]
fn a_declared_tests_only_claim_cell_is_evaluated() {
    assert_eq!(
        tests_only_claim_case(true, Mutation::Kills, None),
        Verdict::Pass,
        "a complete declaration on a tests-only diff must be consulted, and its killing \
         mutation accepted"
    );
}

/// The arm is reached, not merely declared: a mutation that applies but does not kill is
/// refused.
#[test]
fn a_tests_only_claim_cell_whose_mutation_does_not_kill_is_refused() {
    assert_eq!(
        tests_only_claim_case(true, Mutation::DoesNotKill, None),
        Verdict::Fail,
        "reaching the arm and finding the mutation does not kill must refuse"
    );
}

/// A declaration with no committed patch is `Cause::MissingPatch`, not treated as though
/// nothing were declared.
#[test]
fn a_tests_only_claim_cell_with_no_committed_patch_is_refused_as_missing() {
    assert_eq!(
        tests_only_claim_case(true, Mutation::Missing, None),
        Verdict::Fail,
        "a declared cell with no patch must refuse as missing"
    );
}

/// `github.com/telekom/sutura#1025`'s own fixture: `src/lib.rs` carries BOTH `f` and its test
/// AT INIT already, and the measured diff only ADDS a second assertion inside
/// `the_existing_one`'s body - no `#[test]`, no `mod tests`, and `f` itself never changes. Before
/// this decision `plan()` read this file's added line as `Adds::Nothing` (no marker), sent it to
/// `impl_only`, and - with nothing else in the diff - `causality::run` reached `Plan::NotRequired`
/// unconditionally: `Verdict::Pass` from any case below is reachable ONLY through
/// `edited::touches` routing the file into `test_files` instead.
///
/// The mutation targets `f`'s return value, in `src/lib.rs`'s PRODUCTION region rather than its
/// test one - `claim::rot`'s TEXT half refuses a patch that rewrites the `#[cfg(test)]` region
/// byte-for-byte, and the PATH half refuses any patch at all against a `/tests/` target, which
/// this file is not.
///
/// Every case also asserts that `Scan::of` NAMES the touched test: `plan`'s `edited::touches`
/// and `Scan::of`'s `edited::touched_in` are separately breakable, and a break of the naming
/// path alone still answers `Verdict::Fail` - from `report_unreadable`, the wrong refusal.
fn edited_assertion_case(declare: bool, mutation: Mutation) -> Verdict {
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "this fixture moves the process's current directory, so it must have the process to \
         itself: run it under `just test`."
    );
    let dir = std::env::temp_dir().join(format!(
        "sutura-causality-edited-assertion-{}-{}",
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
    let base_content = "pub fn f() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    use super::f;\n\n    #[test]\n    fn the_existing_one() {\n        assert_eq!(f(), 1);\n    }\n}\n";
    std::fs::write(dir.join("src/lib.rs"), base_content).unwrap();
    std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();

    // THE HEAD CONTENT, built here and reused below: the ONLY difference from `base_content` is
    // the added assertion line inside the existing test's body.
    let head_content = base_content.replacen(
        "        assert_eq!(f(), 1);\n",
        "        assert_eq!(f(), 1);\n        assert_ne!(f(), 0);\n",
        1,
    );

    // THE PATCH GOES INTO THE INIT COMMIT, BEFORE `base` IS CAPTURED - same placement
    // `tests_only_claim_case` uses and for the same reason: a patch committed alongside the
    // measured diff would itself be a second changed file with something to revert, and
    // `separable.revert` would no longer be empty. Diffed against `head_content`, because
    // `kill_cell` applies the mutation to the checkout AT HEAD, never at base.
    let mutated: Option<String> = match mutation {
        Mutation::Kills => Some(head_content.replacen("pub fn f() -> u8 { 1 }", "pub fn f() -> u8 { 9 }", 1)),
        Mutation::DoesNotKill => Some(head_content.replacen("pub fn f() -> u8 { 1 }", "pub fn f() -> u8 { 1 } // same", 1)),
        Mutation::Missing => None,
    };
    if let Some(mutated) = mutated {
        std::fs::write(dir.join(".old.rs"), &head_content).unwrap();
        std::fs::write(dir.join(".new.rs"), &mutated).unwrap();
        let diffed = git_output(&dir, &["diff", "--no-index", "--", ".old.rs", ".new.rs"]);
        let patch = String::from_utf8_lossy(&diffed.stdout)
            .replace(".old.rs", "src/lib.rs")
            .replace(".new.rs", "src/lib.rs");
        std::fs::remove_file(dir.join(".old.rs")).unwrap();
        std::fs::remove_file(dir.join(".new.rs")).unwrap();
        std::fs::create_dir_all(dir.join("devco/claim-mutations")).unwrap();
        std::fs::write(dir.join("devco/claim-mutations/the_existing_one.patch"), &patch).unwrap();
    }

    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout)
        .expect("utf8")
        .trim()
        .to_owned();

    // THE WHOLE MEASURED DIFF: one added line inside `the_existing_one`'s body. `f` is
    // byte-identical to base, so `separable.revert` is empty - there is truly nothing else here
    // to revert.
    std::fs::write(dir.join("src/lib.rs"), &head_content).unwrap();
    git(&dir, &["add", "-A"]);
    let message = if declare {
        "test: strengthen the existing assertion\n\nClaim-Cell: the_existing_one"
    } else {
        "test: strengthen the existing assertion"
    };
    git(&dir, &["commit", "-q", "-m", message]);

    let original = std::env::current_dir().expect("a current directory");
    std::env::set_current_dir(&dir).expect("point the process at the fixture repo");
    let named = scan_of_runnable_names(&base);
    let verdict = super::run(&[String::from("--since"), base]);
    std::env::set_current_dir(&original).expect("restore the current directory");
    drop(std::fs::remove_dir_all(&dir));
    assert_eq!(
        named,
        Some(vec![String::from("the_existing_one")]),
        "`Scan::of` must NAME the touched test, or the verdict comes from the wrong refusal"
    );
    verdict
}

/// The test names `Scan::of` finds on `run`'s own path, or `None` short of `Scan::Runnable`.
fn scan_of_runnable_names(base: &str) -> Option<Vec<String>> {
    let commit = super::provenance::Commit::parse(base)?;
    let files = super::changed_with_additions(&commit)?;
    let read = |path: &str| std::fs::read_to_string(path).ok();
    // The BASE image is the commit `--since` names, never HEAD: passing `read` for both would
    // resolve Shape A's removed-line numbers against the wrong file whenever base and HEAD
    // disagree on it, the same misalignment `commit_added_names` closed in `claim.rs`.
    let plan = super::plan::plan(&files, &read);
    let test_files = match plan {
        super::plan::Plan::Separable(sep) => sep.test_files,
        _ => return None,
    };
    match super::scoped::Scan::of(&files, &test_files, &read) {
        super::scoped::Scan::Runnable(scoped) => Some(
            scoped
                .tests()
                .iter()
                .map(super::place::AddedTest::name)
                .map(String::from)
                .collect(),
        ),
        _ => None,
    }
}

/// **THE RED-ON-BASE CELL FOR THIS ISSUE.** No `Claim-Cell:` trailer at all, over a diff whose
/// only change is an added line inside an existing test - the exact shape that reached
/// `Plan::NotRequired` unconditionally before this decision. `Verdict::Fail` is NEW behaviour
/// here, not a pin of an existing one: neutralising `edited::touches` with `&& false` in
/// `plan()`'s `Adds::Nothing` arm - keeping the field read, so the check still compiles and
/// runs - reproduces the base tree's answer, `Verdict::Pass`, confirmed by hand.
#[test]
fn an_edited_assertion_with_no_claim_cell_is_refused() {
    assert_eq!(
        edited_assertion_case(false, Mutation::Missing),
        Verdict::Fail,
        "an added line inside an existing test with no `Claim-Cell:` trailer must refuse - the \
         old classification answered `Verdict::Pass` unconditionally here"
    );
}

/// The declared half: a complete declaration over an edited existing test is EVALUATED, and a
/// mutation that kills by the cell's own assertion is accepted.
#[test]
fn a_declared_edited_assertion_claim_cell_is_evaluated() {
    assert_eq!(
        edited_assertion_case(true, Mutation::Kills),
        Verdict::Pass,
        "a complete declaration over an edited existing test must be consulted, and its killing \
         mutation accepted"
    );
}

/// The arm is reached, not merely declared: a mutation that applies but does not kill is refused.
#[test]
fn an_edited_assertion_claim_cell_whose_mutation_does_not_kill_is_refused() {
    assert_eq!(
        edited_assertion_case(true, Mutation::DoesNotKill),
        Verdict::Fail,
        "reaching the arm and finding the mutation does not kill must refuse"
    );
}

/// A declaration with no committed patch is `Cause::MissingPatch`, not treated as though
/// nothing were declared.
#[test]
fn an_edited_assertion_claim_cell_with_no_committed_patch_is_refused_as_missing() {
    assert_eq!(
        edited_assertion_case(true, Mutation::Missing),
        Verdict::Fail,
        "a declared cell with no patch must refuse as missing"
    );
}

/// THE PARTIAL DECLARATION, the third `tests_only` outcome: a commit declares ONE of the two
/// tests it added, and the refusal must name only the undeclared remainder
/// (`report_unclaimed_additions` over `scoped.minus(&declared)`), never the declared cell.
/// Same file shape as [`composite`]'s, over an empty revert.
///
/// `Mutation::Kills`, not `Missing`, is what makes this cell discriminating: a committed
/// killing patch means the declared cell WOULD prove green if the diff wrongly routed to
/// `claim::run`, so `Verdict::Fail` can come only from the remainder being refused. WHICH
/// tests the refusal names is not held: `report_unclaimed_additions` fails whatever it is
/// given and the venue captures no stdout, so passing `scoped.tests()` instead of the
/// remainder stays green here (measured) and is held by review alone. New behaviour, not a
/// pin: the base tree passed unconditionally here.
#[test]
fn a_tests_only_diff_declaring_one_of_two_additions_refuses_the_undeclared_one() {
    assert_eq!(
        tests_only_claim_case(true, Mutation::Kills, Some("fn the_undeclared_one() { assert_eq!(f(), 1); }")),
        Verdict::Fail,
        "a partial declaration must refuse over the undeclared remainder, not ride on the \
         declared cell's own passing proof"
    );
}

/// Run the public causality entry point over one changed test file in a real Git repository.
/// Both shapes below use only APIs present on base, so their cells can fail by verdict there.
fn changed_test_shape(base_content: &str, head_content: &str) -> Verdict {
    changed_tree(&[("src/lib.rs", base_content)], &[("src/lib.rs", head_content)])
}

/// [`changed_test_shape`] over any set of files: every base file is removed before the head set
/// is written, so a path in `base` and not in `head` is a rename or a deletion to git.
fn changed_tree(base_files: &[(&str, &str)], head_files: &[(&str, &str)]) -> Verdict {
    committed_tree("test: change an existing test", base_files, head_files)
}

/// [`changed_tree`] with the head commit's `message`, so a cell can carry a trailer.
fn committed_tree(message: &str, base_files: &[(&str, &str)], head_files: &[(&str, &str)]) -> Verdict {
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "this fixture changes the process directory; run it under `just test`"
    );
    let dir = std::env::temp_dir().join(format!(
        "sutura-causality-changed-test-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(dir.join("src")).expect("temp crate");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "test"]);
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[profile.ci]\ninherits = \"dev\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();
    for (path, content) in base_files {
        std::fs::write(dir.join(path), content).unwrap();
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout)
        .expect("utf8")
        .trim()
        .to_owned();
    for (path, _) in base_files {
        std::fs::remove_file(dir.join(path)).unwrap();
    }
    for (path, content) in head_files {
        std::fs::create_dir_all(dir.join(path).parent().expect("a file path")).unwrap();
        std::fs::write(dir.join(path), content).unwrap();
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", message]);

    let original = std::env::current_dir().expect("current directory");
    std::env::set_current_dir(&dir).expect("fixture directory");
    let verdict = super::run(&[String::from("--since"), base]);
    std::env::set_current_dir(&original).expect("restore directory");
    drop(std::fs::remove_dir_all(&dir));
    verdict
}

#[test]
fn deleting_an_existing_assertion_is_refused() {
    let base = "pub fn f() -> u8 { 1 }\n#[cfg(test)]\nmod tests {\n    use super::f;\n    #[test]\n    fn existing() {\n        assert_eq!(f(), 1);\n    }\n}\n";
    let head = base.replace("        assert_eq!(f(), 1);\n", "");
    assert_eq!(changed_test_shape(base, &head), Verdict::Fail);
}

#[test]
fn editing_a_called_test_helper_is_refused_without_a_claim() {
    let base = "#[cfg(test)]\nmod tests {\n    fn helper() -> u8 { 1 }\n    #[test]\n    fn existing() {\n        assert_eq!(helper(), 1);\n    }\n}\n";
    let head = base.replace("fn helper() -> u8 { 1 }", "fn helper() -> u8 { 2 }");
    assert_eq!(changed_test_shape(base, &head), Verdict::Fail);
}

#[test]
fn deleting_a_called_test_helpers_assertion_is_refused() {
    let base = "#[cfg(test)]\nmod tests {\n    fn helper() {\n        assert_eq!(2 + 2, 4);\n    }\n    #[test]\n    fn existing() {\n        helper();\n    }\n}\n";
    let head = base.replace("        assert_eq!(2 + 2, 4);\n", "");
    assert_eq!(changed_test_shape(base, &head), Verdict::Fail);
}

/// A MOVED file is read at base under the path it had there. `git diff` reports a moved-and-edited
/// file as a rename, and reading its pre-image at the new path found nothing, so every such file
/// with a removed line was refused as unreadable - a crate split could not pass at all.
#[test]
fn a_moved_file_is_read_at_base_under_its_old_path() {
    let body = "pub fn f() -> u8 {\n    // one\n    1\n}\n\npub fn g() -> u8 {\n    2\n}\n";
    let base = [("src/lib.rs", "mod a;\n"), ("src/a.rs", body)];
    let head = [("src/lib.rs", "mod b;\n"), ("src/b.rs", &*body.replace("    // one\n", ""))];
    assert_eq!(changed_tree(&base, &head), Verdict::Pass);
}

/// `github.com/telekom/sutura#1068`: a waiver excuses the deletion it names, not the range. The
/// head deletes `existing`'s assertion under a `Weakens-Test:` trailer and adds `added`, which is
/// green on base because nothing it reads changed - so the range must still fail. Before the fix
/// the fully waived deletion answered `Pass` and `added` was never measured.
#[test]
fn a_waived_deletion_still_proves_the_test_added_beside_it() {
    let base = "pub fn f() -> u8 { 1 }\n#[cfg(test)]\nmod tests {\n    use super::f;\n    #[test]\n    fn existing() {\n        assert_eq!(f(), 1);\n    }\n}\n";
    let head = [
        ("src/lib.rs", &*base.replace("        assert_eq!(f(), 1);\n", "")),
        ("tests/it.rs", "#[test]\nfn added() {\n    assert_eq!(wired::f(), 1);\n}\n"),
    ];
    let message = "test: move the pin of f\n\nWeakens-Test: existing - tests/it.rs pins f instead";
    assert_eq!(committed_tree(message, &[("src/lib.rs", base)], &head), Verdict::Fail);
}
