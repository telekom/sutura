//! Reading git, and nothing else. The decision lives in [`super::decide`] and never shells out.
//!
//! The split is what makes the interesting case assertable: every function here turns command
//! output into a value, and the parsers are pure so the shapes real git produces can be pinned as
//! text. Nothing here decides anything.
//!
//! # The history the squash-merge fixtures come from
//!
//! Reproducible in a scratch repository, and worth doing by hand once because the naive answer is
//! so convincing:
//!
//! ```text
//! git init -q -b main scratch && cd scratch
//! git commit -q --allow-empty -m root
//! git switch -qc feat/thing && echo one > a.txt && git add a.txt && git commit -qm 'add a'
//! git switch -q main
//! git merge -q --squash feat/thing && git commit -qm 'add a (#1)'
//!
//! git branch --merged main     # prints `main` alone - feat/thing is INVISIBLE to it
//! git cherry main feat/thing   # prints `- <sha>` - a patch-equivalent commit was found
//! ```
//!
//! The branch's commit is not an ancestor of anything on `main`, so `--merged` is right and
//! useless. `git cherry` compares patch-ids, which is why it sees through the squash - for a
//! single commit. Squash three commits and no individual patch-id matches, which is what
//! [`Landings::of`] falls through to the cumulative-diff comparison for.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::decide::{Equivalence, Landing, Upstream, WorkingTree};
use crate::repo;

/// How far back along the default branch to look for a squash commit.
///
/// Bounded because the scan reads one diff per commit. A branch whose squash commit is older than
/// this simply has no cumulative-diff signal, and no signal keeps the branch - which is the safe
/// direction and the reason a bound is acceptable here at all.
const MAX_BASE_COMMITS: &str = "500";

/// The fields one branch contributes, NUL-separated so no value can contain the separator.
const REF_FORMAT: &str = "%(refname:short)%00%(objectname)%00%(upstream:short)%00\
     %(upstream:track)%00%(worktreepath)%00%(committerdate:unix)";

/// One line of `git for-each-ref`, still as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefLine {
    /// Short branch name.
    pub(crate) name: String,
    /// The commit it points at.
    pub(crate) tip: String,
    /// Short upstream name, empty when there is none.
    pub(crate) upstream: String,
    /// `%(upstream:track)`: `[gone]`, `[ahead 2, behind 7]`, or empty.
    pub(crate) track: String,
    /// The worktree holding it, empty when none does.
    pub(crate) worktree: String,
    /// Committer date of the tip, epoch seconds as text.
    pub(crate) committer_seconds: String,
}

impl RefLine {
    /// Where its upstream went.
    pub(crate) fn upstream_state(&self) -> Upstream {
        upstream_state(&self.upstream, &self.track)
    }

    /// Committer date as a number, or `None` when git reported something unparseable.
    pub(crate) fn seconds(&self) -> Option<i64> {
        self.committer_seconds.trim().parse().ok()
    }
}

/// One block of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeLine {
    /// Absolute path git reported.
    pub(crate) path: String,
    /// The branch checked out there, short name, or `None` for a detached HEAD.
    pub(crate) branch: Option<String>,
    /// A locked worktree is left alone.
    pub(crate) locked: bool,
    /// The repository's own bare or main worktree.
    pub(crate) bare: bool,
}

/// What `git cherry` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cherry {
    /// Commits with a patch-equivalent commit on the default branch (`-`).
    pub(crate) matched: u32,
    /// Commits with none (`+`).
    pub(crate) unmatched: u32,
}

/// The repository root above `dir`, as git sees it.
pub(crate) fn toplevel(dir: &Path) -> Option<PathBuf> {
    let out = git(dir).args(["rev-parse", "--show-toplevel"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// `git fetch --prune`, so `[gone]` means gone rather than "gone since whenever you last fetched".
///
/// Opt-in, because it is the one thing here that touches the network. Skipping it is safe in the
/// direction that matters: stale remote-tracking refs make an upstream look present when it is
/// gone, which keeps a branch rather than deleting one.
pub(crate) fn fetch_and_prune(root: &Path) -> Result<(), String> {
    let out = git(root)
        .args(["fetch", "--prune"])
        .output()
        .map_err(|error| format!("git fetch did not run: {error}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(first_line(&String::from_utf8_lossy(&out.stderr)))
}

/// The branch everything is compared against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Base {
    /// What to compare against - `origin/main` where there is a remote, so a stale local trunk
    /// cannot make a landed branch look unlanded.
    pub(crate) rev: String,
    /// The local branch name that is protected from deletion.
    pub(crate) name: String,
}

/// Resolve the default branch, preferring what the remote says its HEAD is.
pub(crate) fn base(root: &Path) -> Result<Base, String> {
    if let Some(rev) = symbolic_remote_head(root) {
        let name = rev
            .split_once('/')
            .map_or_else(|| rev.clone(), |(_, tail)| String::from(tail));
        return Ok(Base { rev, name });
    }
    // This fallback is only reached when `origin/HEAD` is unset. Prefer the sole trunk, then the
    // conventional legacy name for repositories where this helper is reused.
    for candidate in ["origin/main", "origin/master", "main", "master"] {
        if exists(root, candidate) {
            let name = candidate.split_once('/').map_or(candidate, |(_, tail)| tail);
            return Ok(Base {
                rev: String::from(candidate),
                name: String::from(name),
            });
        }
    }
    Err(String::from(
        "could not resolve a default branch: none of origin/main, origin/master, main or master exists",
    ))
}

/// The branch checked out where this invocation is running.
pub(crate) fn current_branch(dir: &Path) -> Option<String> {
    let out = git(dir).args(["branch", "--show-current"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Every local branch, with the state the decision reads.
pub(crate) fn refs(root: &Path) -> Result<Vec<RefLine>, String> {
    let out = git(root)
        .args(["for-each-ref", "--format", REF_FORMAT, "refs/heads"])
        .output()
        .map_err(|error| format!("git for-each-ref did not run: {error}"))?;
    if !out.status.success() {
        return Err(first_line(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(parse_refs(&String::from_utf8_lossy(&out.stdout)))
}

/// Split `git for-each-ref` output into fields. A line with the wrong field count is dropped
/// rather than guessed at.
pub(crate) fn parse_refs(text: &str) -> Vec<RefLine> {
    text.lines().filter_map(parse_ref_line).collect()
}

fn parse_ref_line(line: &str) -> Option<RefLine> {
    let mut fields = line.split('\0');
    let parsed = RefLine {
        name: String::from(fields.next()?),
        tip: String::from(fields.next()?),
        upstream: String::from(fields.next()?),
        track: String::from(fields.next()?),
        worktree: String::from(fields.next()?),
        committer_seconds: String::from(fields.next()?),
    };
    (!parsed.name.is_empty()).then_some(parsed)
}

/// Read `%(upstream:short)` and `%(upstream:track)` together.
///
/// Both are needed: the track field alone cannot tell "in step with its upstream" from "has no
/// upstream at all", and those are opposite situations - the first means every commit is on a
/// remote somewhere, the second means none of them is.
pub(crate) fn upstream_state(upstream: &str, track: &str) -> Upstream {
    if upstream.trim().is_empty() {
        return Upstream::Untracked;
    }
    if track.contains("gone") {
        return Upstream::Gone;
    }
    Upstream::Present {
        ahead: counted(track, "ahead"),
        behind: counted(track, "behind"),
    }
}

/// The number after `word` in a track string like `[ahead 2, behind 7]`.
fn counted(track: &str, word: &str) -> u32 {
    track
        .split_once(word)
        .and_then(|(_, tail)| {
            let digits: String = tail.trim_start().chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .unwrap_or(0)
}

/// Every registered worktree.
pub(crate) fn worktrees(root: &Path) -> Result<Vec<WorktreeLine>, String> {
    let out = git(root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|error| format!("git worktree list did not run: {error}"))?;
    if !out.status.success() {
        return Err(first_line(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(parse_worktrees(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse `git worktree list --porcelain`: blank-line separated blocks of `key value` lines.
pub(crate) fn parse_worktrees(text: &str) -> Vec<WorktreeLine> {
    let mut out: Vec<WorktreeLine> = Vec::new();
    for line in text.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => out.push(WorktreeLine {
                path: String::from(value),
                branch: None,
                locked: false,
                bare: false,
            }),
            "branch" => {
                if let Some(current) = out.last_mut() {
                    current.branch = value.strip_prefix("refs/heads/").map(String::from);
                }
            }
            // `locked` and `bare` arrive with no value, so the split above leaves them as the key.
            "locked" => {
                if let Some(current) = out.last_mut() {
                    current.locked = true;
                }
            }
            "bare" => {
                if let Some(current) = out.last_mut() {
                    current.bare = true;
                }
            }
            _ => {}
        }
    }
    out
}

/// What one worktree's working tree holds.
///
/// A directory that is gone is not read as clean: that is the stale registration case, and the
/// caller decides it from the directory's absence rather than from an empty `git status`.
pub(crate) fn working_tree(path: &Path) -> WorkingTree {
    if !path.is_dir() {
        return WorkingTree::Unreadable {
            because: String::from("the directory is not there"),
        };
    }
    let out = git(path).args(["status", "--porcelain"]).output();
    match out {
        Err(error) => WorkingTree::Unreadable {
            because: format!("git status did not run: {error}"),
        },
        Ok(out) if !out.status.success() => WorkingTree::Unreadable {
            because: first_line(&String::from_utf8_lossy(&out.stderr)),
        },
        Ok(out) => {
            if String::from_utf8_lossy(&out.stdout).trim().is_empty() {
                WorkingTree::Clean
            } else {
                WorkingTree::Dirty
            }
        }
    }
}

/// The landing question, with the expensive half cached.
///
/// The cumulative-diff comparison needs every patch-id on the default branch since a merge base,
/// and branches cut from the same point share one. Reading it once per merge base rather than once
/// per branch is the difference between one scan and thirty.
pub(crate) struct Landings<'a> {
    root: &'a Path,
    base_rev: &'a str,
    /// Merge base commit to the patch-ids on the default branch since it.
    scanned: BTreeMap<String, BTreeSet<String>>,
}

impl<'a> Landings<'a> {
    pub(crate) const fn new(root: &'a Path, base_rev: &'a str) -> Self {
        Self {
            root,
            base_rev,
            scanned: BTreeMap::new(),
        }
    }

    /// Where this branch's commits are, relative to the default branch.
    pub(crate) fn of(&mut self, branch: &str) -> Landing {
        let Some(cherry) = cherry(self.root, self.base_rev, branch) else {
            return Landing::Undetermined {
                because: "git cherry did not answer",
            };
        };
        // No output at all means the branch has nothing the default branch lacks: an ancestor,
        // which is the one case `git branch --merged` also gets right.
        if cherry.matched == 0 && cherry.unmatched == 0 {
            return Landing::Landed(Equivalence::Ancestor);
        }
        if cherry.unmatched == 0 {
            return Landing::Landed(Equivalence::PerCommit);
        }
        match self.squashed(branch) {
            Ok(true) => Landing::Landed(Equivalence::Squashed),
            Ok(false) => Landing::Unlanded {
                commits: cherry.unmatched,
            },
            Err(because) => Landing::Undetermined { because },
        }
    }

    /// Is the branch's cumulative diff one commit on the default branch?
    ///
    /// This is the squash-of-several-commits case: the squash commit's diff is the branch's whole
    /// diff, so it matches nothing per commit and everything at once.
    fn squashed(&mut self, branch: &str) -> Result<bool, &'static str> {
        let merge_base = merge_base(self.root, self.base_rev, branch).ok_or("git merge-base did not answer")?;
        let cumulative = patch_ids(self.root, &["diff", merge_base.as_str(), branch])?;
        let Some(id) = cumulative.iter().next() else {
            return Err("the branch's cumulative diff produced no patch-id");
        };
        if !self.scanned.contains_key(&merge_base) {
            let range = format!("{merge_base}..{}", self.base_rev);
            let ids = patch_ids(
                self.root,
                &["log", "-p", "--no-merges", "--max-count", MAX_BASE_COMMITS, range.as_str()],
            )?;
            self.scanned.insert(merge_base.clone(), ids);
        }
        Ok(self.scanned.get(&merge_base).is_some_and(|on_base| on_base.contains(id)))
    }
}

/// `git cherry <base> <branch>`: one line per commit, `-` for a patch already represented.
fn cherry(root: &Path, base_rev: &str, branch: &str) -> Option<Cherry> {
    let out = git(root).args(["cherry", base_rev, branch]).output().ok()?;
    out.status
        .success()
        .then(|| parse_cherry(&String::from_utf8_lossy(&out.stdout)))
}

/// Tally `git cherry` output. Anything that is neither `+` nor `-` is not a verdict and is skipped.
pub(crate) fn parse_cherry(text: &str) -> Cherry {
    let mut tally = Cherry {
        matched: 0,
        unmatched: 0,
    };
    for line in text.lines() {
        match line.trim_start().chars().next() {
            Some('-') => tally.matched = tally.matched.saturating_add(1),
            Some('+') => tally.unmatched = tally.unmatched.saturating_add(1),
            _ => {}
        }
    }
    tally
}

fn merge_base(root: &Path, base_rev: &str, branch: &str) -> Option<String> {
    let out = git(root).args(["merge-base", base_rev, branch]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Run `git <args>` and hand its output to `git patch-id --stable`.
///
/// Two processes rather than a shell pipeline, and the write happens on its own thread: a diff
/// large enough to fill the pipe buffer while `patch-id` is filling its own output buffer would
/// otherwise deadlock, and that is a hang rather than an error.
fn patch_ids(root: &Path, args: &[&str]) -> Result<BTreeSet<String>, &'static str> {
    // The cause is deliberately dropped rather than wired: what this returns becomes
    // `Landing::Undetermined`, which is printed in a report for a person deciding whether to run
    // `--delete`. Which of the two commands could not answer is what they can act on; an `io::Error`
    // from a spawn is not.
    let diff = git(root).args(args).output().map_err(|_io| "git could not produce a diff")?;
    if !diff.status.success() {
        return Err("git could not produce a diff");
    }
    let mut child = git(root)
        .args(["patch-id", "--stable"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_io| "git patch-id did not start")?;
    let mut sink = child.stdin.take().ok_or("git patch-id has no stdin")?;
    let bytes = diff.stdout;
    let writer = std::thread::spawn(move || {
        sink.write_all(&bytes)?;
        sink.flush()
    });
    let out = child.wait_with_output().map_err(|_io| "git patch-id did not finish")?;
    // A broken pipe here is not a failure: `patch-id` may stop reading before the whole diff is
    // written. What decides the outcome is whether it produced ids.
    drop(writer.join());
    if !out.status.success() {
        return Err("git patch-id failed");
    }
    Ok(parse_patch_ids(&String::from_utf8_lossy(&out.stdout)))
}

/// Read `git patch-id` output: `<patch-id> <commit-id>` per patch, and the commit half is not
/// wanted - the question is only whether a given patch is present.
pub(crate) fn parse_patch_ids(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect()
}

/// Does this revision resolve?
fn exists(root: &Path, rev: &str) -> bool {
    git(root)
        .args(["rev-parse", "--verify", "--quiet", rev])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// What the remote says its HEAD is, as `origin/<branch>`.
fn symbolic_remote_head(root: &Path) -> Option<String> {
    let out = git(root)
        .args(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Run a git command against one directory, with the ambient git environment stripped.
fn git(dir: &Path) -> Command {
    let mut command = Command::new("git");
    repo::strip_git_env(&mut command);
    command.current_dir(dir);
    command
}

/// The first line of a message, bounded. A git error can be several lines and the report wants one.
fn first_line(text: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    line.chars().take(200).collect()
}

/// Delete one branch, or say why git would not.
///
/// `-D` and not `-d`, deliberately: `-d` refuses a branch whose commits are not ancestors of the
/// default branch, which is EVERY squash-merged branch - the exact case this task exists for. The
/// safety is the decision, not the flag.
pub(crate) fn delete_branch(root: &Path, branch: &str) -> Result<String, String> {
    let out = git(root)
        .args(["branch", "-D", branch])
        .output()
        .map_err(|error| format!("git branch -D did not run: {error}"))?;
    if out.status.success() {
        return Ok(first_line(&String::from_utf8_lossy(&out.stdout)));
    }
    Err(first_line(&String::from_utf8_lossy(&out.stderr)))
}

/// Remove one worktree.
///
/// No `--force`: it would discard uncommitted work, and a dirty worktree has already been refused
/// by the decision. A removal git declines is reported rather than retried harder.
pub(crate) fn remove_worktree(root: &Path, path: &str) -> Result<(), String> {
    let out = git(root)
        .args(["worktree", "remove", path])
        .output()
        .map_err(|error| format!("git worktree remove did not run: {error}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(first_line(&String::from_utf8_lossy(&out.stderr)))
}

/// Drop registrations whose directories are gone. Locked ones are skipped by git itself.
pub(crate) fn prune_worktrees(root: &Path) -> Result<String, String> {
    let out = git(root)
        .args(["worktree", "prune", "--verbose"])
        .output()
        .map_err(|error| format!("git worktree prune did not run: {error}"))?;
    if out.status.success() {
        return Ok(String::from(String::from_utf8_lossy(&out.stdout).trim()));
    }
    Err(first_line(&String::from_utf8_lossy(&out.stderr)))
}

#[cfg(test)]
mod tests;
