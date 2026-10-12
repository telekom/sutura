//! `causality::run` over a range whose cell trailer sits on a MERGE commit. The squash keeps only
//! the non-merge commits' messages, so that trailer is refused; on the merged branch's own commit
//! the same trailer is not.

use std::sync::atomic::Ordering;

use super::{SEQ, git, git_output};
use crate::Verdict;

/// The gate's verdict over a docs-only branch merged with `--no-ff`, with `trailer` in the merge
/// commit's message when `on_merge`, else in the branch commit's.
fn verdict(trailer: &str, on_merge: bool) -> Verdict {
    let dir = std::env::temp_dir().join(format!(
        "sutura-causality-merge-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _swept = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "test"]);
    // Both of `repo::root`'s markers, or it falls back to the repository this test was built in.
    std::fs::write(dir.join("Cargo.toml"), "[workspace]\n").unwrap();
    std::fs::write(dir.join("flake.nix"), "{ }\n").unwrap();
    std::fs::write(dir.join("README.md"), "base\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout).unwrap();
    git(&dir, &["checkout", "-q", "-b", "side"]);
    std::fs::write(dir.join("README.md"), "base\nside\n").unwrap();
    let (on_side, on_merge) = if on_merge { ("", trailer) } else { (trailer, "") };
    git(&dir, &["commit", "-q", "-a", "-m", &format!("docs: side\n\n{on_side}")]);
    git(&dir, &["checkout", "-q", "main"]);
    git(
        &dir,
        &["merge", "-q", "--no-ff", "side", "-m", &format!("Merge side\n\n{on_merge}")],
    );

    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let verdict = super::super::run(&[String::from("--since"), String::from(base.trim())]);
    std::env::set_current_dir(original).unwrap();
    drop(std::fs::remove_dir_all(dir));
    verdict
}

#[test]
fn a_cell_trailer_on_a_merge_commit_is_refused() {
    assert!(std::env::var_os("NEXTEST").is_some(), "run through `just test`");
    for trailer in [
        "Claim-Cell: pinned",
        // Git keeps an indented trailer line, and every reader trims both ends before its prefix.
        "  Claim-Cell: pinned",
        "Weakens-Test: pinned - a reason",
        "Live-Cell: pinned bigquery live-bigquery",
        // Bare, so `relocation` reads no claim and only the refusal can fail this row: any path
        // is either untouched (`Unnamed`) or the fixture's `README.md` (`NotRust`), a Fail itself.
        "Cleanup-Split:",
    ] {
        assert_eq!(
            verdict(trailer, true),
            Verdict::Fail,
            "`{trailer}` on a merge commit must be refused"
        );
    }
    assert_eq!(
        verdict("Claim-Cell: pinned", false),
        Verdict::Pass,
        "the branch's own commit carries it"
    );
}
