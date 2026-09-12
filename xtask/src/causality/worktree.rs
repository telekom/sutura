//! Putting a git worktree into the base state - the only part of this gate that is plumbing.
//!
//! Split out of `causality.rs` for the file-length gate, and the seam is real: everything here is
//! `git` and the filesystem, and nothing here decides anything about a test. Two things it carries
//! that a reader would not guess: "revert to base" means two different operations depending on
//! whether the file existed at base ([`base_has`]), and every subprocess has to have the CALLER's
//! git environment stripped or it operates on another repository - which is why [`git`] is the only
//! way this module spawns one.
//!
//! THE QUESTIONS IT ASKS OF A TREE IT DOES NOT CHANGE - [`merge_base`], [`touched`], [`search`],
//! [`head_branch`], [`head_commit`], [`branch_metadata`] and [`at_base`] - are here for that
//! second reason and no other. Each returns raw output and decides nothing; `super::provenance`, `super::stack` and
//! `super::features` parse them, so every classification they feed is testable without a
//! repository.

use std::path::Path;
use std::process::Command;

use super::provenance::Commit;
use super::stack::BranchRef;

/// A `git` invocation in `dir`, with the caller's git environment stripped.
///
/// A CONSTRUCTOR rather than a rule at four call sites, and it is not tidiness: two of the four
/// had lost the stripping. A gate that shells out to git is often invoked BY git - from a hook, or
/// from a command that set up its own index - and `GIT_DIR`/`GIT_INDEX_FILE` outlive the process
/// that set them, so an unstripped subprocess reads another repository. `repo::strip_git_env` holds
/// the list; this holds that nothing here can forget to apply it.
fn git(dir: &Path) -> Command {
    let mut command = Command::new("git");
    crate::repo::strip_git_env(&mut command);
    command.current_dir(dir);
    command
}

/// The commit `earlier` and `later` diverged at, as git printed it.
///
/// **THE REF IS NOT THE COMMIT, and passing the ref through is what made a verdict a function of
/// the last fetch.** `git diff origin/main` compares the tree at whatever `origin/main` points to
/// now, so once the base branch moves the diff carries commits this branch never made - the gate
/// reverts them and measures a tree nobody proposed. `just ship-check` and `ci.yml` each resolve a
/// merge base before invoking the gate; `just causality` handed the ref straight through, which is
/// the venue a person runs and cites. Resolving it HERE means no invocation site can get it wrong,
/// and it is idempotent for the base a person means: the merge base of a commit already behind HEAD
/// is that commit, so `just causality <a commit>` still scopes the gate to it.
///
/// TWO REVISIONS RATHER THAN ONE AGAINST `HEAD`, because `super::stack` asks this three times and
/// only two of those are against HEAD: the third compares the two candidate BASES, which is the
/// guard that stops a stale recorded parent from moving the base off this branch's history.
///
/// Raw text out, [`Commit`] parses it: an unrelated history, an unknown ref or an ambiguous answer
/// all reach `super::provenance::Commit::parse` as text that is not one object name, and the gate
/// refuses rather than splicing it into `git checkout`.
pub(super) fn merge_base(root: &Path, earlier: &str, later: &str) -> String {
    git(root)
        .args(["merge-base", earlier, later])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// The branch HEAD is on, or `None` when HEAD is detached.
///
/// Detached is the ordinary state in two venues that matter - the reconstruction worktree this gate
/// creates, and a CI checkout of a merge commit - and neither has branch metadata to read, so no
/// branch is the same answer as no recorded parent.
pub(super) fn head_branch(root: &Path) -> Option<BranchRef> {
    let out = git(root).args(["symbolic-ref", "--short", "HEAD"]).output().ok()?;
    out.status
        .success()
        .then(|| BranchRef::parse(String::from_utf8_lossy(&out.stdout).trim()))
        .flatten()
}

/// HEAD's own commit, as git printed it.
///
/// Read for ONE guard: a recorded stack parent that already contains this branch forks at HEAD, and
/// a base equal to HEAD makes `git diff <base> --` the uncommitted working tree alone - an exit-0
/// verdict over every file the branch changed. `super::stack::Origin::Contains` is that refusal and
/// it needs the commit rather than a name, because any parent reaching HEAD produces it.
pub(super) fn head_commit(root: &Path) -> String {
    git(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// The commit messages in `base..HEAD`, as git printed them.
///
/// THE RANGE AND NOT HEAD ALONE, because a branch is reviewed whole and
/// `super::relocation::decide` checks a trailer against everything the range did - so a sibling
/// commit that changed behaviour breaks the claim instead of riding on it. `%B` is the raw body,
/// which keeps a trailer on the line it was written on.
///
/// **WHAT IT CANNOT SEE: an UNCOMMITTED split.** `git diff <base>` compares base against the
/// WORKING TREE, so a hand run over uncommitted work has a diff and no message to declare it with.
/// That is the fail-closed direction and it is the point of the carrier: a claim that is not in a
/// commit is not in review either.
///
/// Raw text out, deciding nothing - `super::relocation::Claim` parses it - and the caller's git
/// environment is stripped like every other read here.
pub(super) fn messages(root: &Path, base: &Commit) -> String {
    git(root)
        .args(["log", "--format=%B", &format!("{}..HEAD", base.as_str())])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// The branch tool's metadata blob for `branch`, as git printed it.
///
/// One blob per branch under `refs/branch-metadata/`, which is where the tool this repository pins
/// records a branch's parent. A branch with no metadata - never tracked, or tracked in another
/// worktree - is a non-zero status and reaches the caller as empty text, which
/// `super::stack::recorded_parent` reads as *no recorded parent*: the fallback, and the behaviour
/// that shipped before the derivation existed.
pub(super) fn branch_metadata(root: &Path, branch: &BranchRef) -> String {
    git(root)
        .args(["cat-file", "-p", &format!("refs/branch-metadata/{}", branch.as_str())])
        .stderr(std::process::Stdio::null())
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Every path the diff touched, one per line - **deletions included**.
///
/// `super::diff` builds its `ChangedFile` list from the post-image, so a file this branch DELETED
/// has no entry there at all: its `+++` is `/dev/null`. That is the one shape a whole-file move
/// takes, and [`search`] has to be able to look in it. `--name-only` answers with no post-image
/// involved.
pub(super) fn touched(root: &Path, base: &Commit) -> Vec<String> {
    git(root)
        .args(["diff", "--name-only", base.as_str(), "--"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Lines matching any of `needles` in `base`, under `paths`.
///
/// The pathspec is what keeps this narrow: the question is whether a test the diff NAMES was
/// already in a file the diff also touched, because that is what a move looks like from the base
/// side, and asking the whole tree instead would let one of this tree's duplicated test names
/// answer yes. `-F` because a needle is a fixed string; a non-zero status is "no match" and reaches
/// the caller as empty output, which reads as *nothing moved* - the answer the gate gave before
/// this existed.
pub(super) fn search(root: &Path, base: &Commit, needles: &[String], paths: &[String]) -> String {
    if needles.is_empty() || paths.is_empty() {
        return String::new();
    }
    let mut command = git(root);
    command.args(["grep", "-F"]);
    for needle in needles {
        command.arg("-e").arg(needle);
    }
    command.arg(base.as_str()).arg("--");
    for path in paths {
        command.arg(path);
    }
    command
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Does `path` exist at `base`?
///
/// "Revert to base" means two different things depending on the answer. For a file that
/// existed, it means check out the old content. For a file this branch ADDED, it means the file
/// is not there - and `git checkout base -- <new file>` fails with "did not match any file(s)
/// known to git", which is how this gate first broke in CI.
pub(super) fn base_has(root: &Path, base: &Commit, path: &str) -> bool {
    git(root)
        .arg("cat-file")
        .arg("-e")
        .arg(format!("{}:{path}", base.as_str()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `path`'s content at `base`, or `None` when git did not hand any back.
///
/// SEPARATE FROM [`base_has`] ON PURPOSE, because `super::features` needs the two answers apart:
/// a manifest the base does not have is a NEW package and its base feature table is legitimately
/// empty, while a manifest the base HAS and whose content did not come back leaves that scan with
/// nothing to compare - and answering *nothing was enabled* there would be a claim about a table
/// it never read. One reader, two callers, and the pairing is decided at the call site.
pub(super) fn at_base(root: &Path, base: &Commit, path: &str) -> Option<String> {
    let out = git(root)
        .args(["show", &format!("{}:{path}", base.as_str())])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Set up a detached worktree at HEAD under the given path.
pub(super) fn add_worktree(root: &Path, dir: &Path) -> Result<(), String> {
    let out = git(root)
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
    let outcome = git(root).args(["worktree", "remove", "--force"]).arg(dir).output();
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
pub(super) fn base_state<'a>(root: &Path, base: &Commit, files: &'a [String]) -> BaseState<'a> {
    let (restore, remove) = files.iter().partition(|f| base_has(root, base, f));
    BaseState { restore, remove }
}

/// Check out the base version of the files that had one, and delete the ones this branch added.
pub(super) fn apply(wt: &Path, base: &Commit, state: &BaseState<'_>) -> Result<(), String> {
    if !state.restore.is_empty() {
        let mut checkout = git(wt);
        checkout.args(["checkout", base.as_str(), "--"]);
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
    use super::{BaseState, Commit, apply};

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

        let at = Commit::parse("7a65f1e1a1b2").expect("an object name");
        let applied = apply(&wt, &at, &state);

        let _swept = std::fs::remove_dir_all(&wt);
        assert!(applied.is_ok(), "an absent file is the state asked for, got {applied:?}");
    }
}
