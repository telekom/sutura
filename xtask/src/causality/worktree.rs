//! Putting a git worktree into the base state - the only part of this gate that is plumbing.
//!
//! Split out of `causality.rs` for the file-length gate, and the seam is real: everything here is
//! `git` and the filesystem, and nothing here decides anything about a test. Two things it carries
//! that a reader would not guess: "revert to base" means two different operations depending on
//! whether the file existed at base ([`base_has`]), and a subprocess has to have the CALLER's git
//! environment stripped or it operates on another repository.

use std::path::Path;
use std::process::Command;

/// Changed files split by whether they exist at the base commit.
type Partitioned<'a> = (Vec<&'a String>, Vec<&'a String>);

/// Does `path` exist at `base`?
///
/// "Revert to base" means two different things depending on the answer. For a file that
/// existed, it means check out the old content. For a file this branch ADDED, it means the file
/// is not there - and `git checkout base -- <new file>` fails with "did not match any file(s)
/// known to git", which is how this gate first broke in CI.
pub(super) fn base_has(root: &Path, base: &str, path: &str) -> bool {
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

/// Set up a detached worktree at HEAD under the given path.
pub(super) fn add_worktree(root: &Path, dir: &Path) -> Result<(), String> {
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
pub(super) fn remove_worktree(root: &Path, dir: &Path) {
    let outcome = Command::new("git")
        .current_dir(root)
        .args(["worktree", "remove", "--force"])
        .arg(dir)
        .output();
    if let Err(e) = outcome {
        eprintln!("xtask test-causality: could not remove the worktree: {e}");
    }
}

/// The base state to put a worktree into: files to check out at `base`, files to delete.
///
/// Two of these exist per proof. The first is the implementation change; the second is the files
/// held back for carrying their own tests, applied only if the first tree does not build.
pub(super) struct BaseState<'a> {
    pub(super) restore: Vec<&'a String>,
    pub(super) remove: Vec<&'a String>,
}

impl BaseState<'_> {
    /// Nothing to apply, so there is no second attempt to make.
    pub(super) const fn is_empty(&self) -> bool {
        self.restore.is_empty() && self.remove.is_empty()
    }
}

/// Split files into "existed at base, so check it out" and "added here, so delete it".
///
/// "Revert to base" means two different things depending on the answer, and getting it wrong is
/// how this gate first broke in CI - see [`base_has`].
pub(super) fn base_state<'a>(root: &Path, base: &str, files: &'a [String]) -> BaseState<'a> {
    let (restore, remove): Partitioned<'a> = files.iter().partition(|f| base_has(root, base, f));
    BaseState { restore, remove }
}

/// Check out the base version of the files that had one, and delete the ones this branch added.
pub(super) fn apply(wt: &Path, base: &str, state: &BaseState<'_>) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::{BaseState, apply};

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
