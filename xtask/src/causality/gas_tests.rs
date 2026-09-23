//! The REAL-CARGO fixtures for `causality::run`'s claim dispatch, split out of `causality.rs`
//! at the 1000-line cap.
//!
//! `claim::tests` drives `claim::run` with string-only fixtures; these two build a tiny real
//! crate and run `kill_cell`'s real `cargo test` against it, because the dispatch from
//! `causality::run` into `claim::run` (and, after `github.com/telekom/sutura#954`, the COMPOSITE
//! that also runs the ordinary proof over a range's undeclared additions) is a seam no string
//! fixture reaches. `set_current_dir` is process-global, so each fixture asserts `NEXTEST` and
//! runs alone.

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
