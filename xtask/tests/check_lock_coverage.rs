#![forbid(unsafe_code)]
//! `check-lock-coverage` through the real, built `xtask` binary over a fixture tree. A separate
//! target so causality can revert the gate and see these red: the base binary has no such task.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fixture with the two markers `repo::root` looks for, a root lock, and `extra` files. In a
/// checkout, `CARGO_TARGET_TMPDIR` sits inside the worktree's gitignored `target/`. git answers
/// with an empty listing, and `from_git`'s `files.is_empty()` is what sends it to the walk. Same
/// outcome, different reason.
fn tree(case: &str, prefix: &str, directories: &str, extra: &[&str]) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lock-coverage-{case}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clearing a stale fixture");
    }
    std::fs::create_dir_all(root.join(".github")).expect("the fixture root");
    std::fs::write(root.join("flake.nix"), "{ }\n").expect("the first root marker");
    std::fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("the second root marker");
    std::fs::write(root.join("Cargo.lock"), "# locked\n").expect("the root lock");
    let config = format!("version: 2\nupdates:\n{prefix}  - package-ecosystem: cargo\n    directories: {directories}\n");
    std::fs::write(root.join(".github/dependabot.yml"), config).expect("the config");
    for rel in extra {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the lock's directory");
        std::fs::write(path, "# locked\n").expect("an extra lock");
    }
    root
}

fn run(root: &Path) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-lock-coverage")
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("execute the real xtask binary");
    std::fs::remove_dir_all(root).expect("remove the owned fixture");
    output
}

#[test]
fn every_lock_listed_passes() {
    let output = run(&tree("covered", "", r#"["/", "/fuzz"]"#, &["fuzz/Cargo.lock"]));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ok - 2 lock(s)"),
        "{output:?}"
    );
}

#[test]
fn a_lock_outside_the_listed_directories_fails_naming_it() {
    let prefix = "  - package-ecosystem: github-actions\n    directory: \"/fuzz\"\n";
    let output = run(&tree("uncovered", prefix, r#"["/"]"#, &["fuzz/Cargo.lock"]));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("1 uncovered") && stderr.contains("  /fuzz"), "{output:?}");
}

#[test]
fn a_nested_worktree_lock_is_not_this_repos() {
    let output = run(&tree("worktree", "", r#"["/"]"#, &[".claude/worktrees/wt/Cargo.lock"]));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ok - 1 lock(s)"),
        "{output:?}"
    );
}
