//! Putting a git worktree into the base state - the only part of this gate that is plumbing.
//!
//! Split out of `causality.rs` for the file-length gate, and the seam is real: everything here is
//! `git` and the filesystem, and nothing here decides what a test proves. Three things it carries
//! that a reader would not guess: "revert to base" means two different operations depending on
//! whether the file existed at base ([`base_has`]), and every subprocess has to have the CALLER's
//! git environment stripped or it operates on another repository - which is why [`git`] is the only
//! way this module spawns one - and a git call that could not start or exited non-zero is
//! [`GitFailed`], never an empty answer.
//!
//! THE QUESTIONS IT ASKS OF A TREE IT DOES NOT CHANGE - [`merge_base`], [`touched`], [`deleted`],
//! [`search`], [`head_branch`], [`head_commit`], [`branch_metadata`], [`base_has`] and [`naming`]
//! return raw output or [`GitFailed`] and decide nothing; `super::provenance`, `super::stack` and
//! `super::features` parse them, so every classification they feed is testable without a
//! repository. [`at_base`] is the exception: it returns `None` for a failed call as well as an
//! absent one, as before. `feature_activation` pairs it with [`base_has`] and refuses `None` as
//! unreadable. The other readers of the base image are not changed here and do not all refuse it:
//! `deletion_in` does, while the rest read `None` as no text.
//! Two compositions are glue over those reads and still decide nothing: [`parent`] binds
//! `super::stack::parent_of` to this repository, and [`already_there`] asks
//! `super::provenance::Moved` about the base.

use std::path::Path;
use std::process::Command;

use sutura_dev::discovery::path_for;
use sutura_dev::scope::Scope;

use super::place::AddedTest;
use super::provenance::{Commit, Moved, Reach, needles};
use super::stack::{self, BranchRef, Parent};

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

/// A `git` call that did not answer: it could not start, or it exited non-zero.
///
/// **A FAILED CALL IS NOT AN EMPTY ANSWER.** Every question here used to read one as empty text, and
/// where empty means *nothing to check* - no merge commits, no touched path, no base file - that let
/// a check pass over a tree git never read. Each caller now refuses or maps it to the fallback it
/// states.
#[derive(Debug)]
pub(super) struct GitFailed {
    call: String,
    why: String,
}

impl GitFailed {
    /// A failure to put in the way of a caller that is under test; no git ran.
    #[cfg(test)]
    pub(super) fn unreadable() -> Self {
        Self {
            call: String::from("ls-tree"),
            why: String::from("exit status: 128"),
        }
    }

    /// Print the refusal; for `map_err` where the caller returns [`Verdict::Fail`](crate::Verdict::Fail).
    pub(super) fn refuse(self) {
        eprintln!("xtask test-causality: FAILED - {self}");
    }
}

impl std::fmt::Display for GitFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`git {}` did not answer: {}", self.call, self.why)
    }
}

/// `command`'s stdout, or why git did not give it.
///
/// `no_match` is for the one status git uses as an answer: exit 1 from a question whose reply may be
/// *none* (`merge-base` with no common ancestor, `grep` with no match, `rev-parse --verify --quiet`
/// of an absent ref). It reads as empty text; every other non-zero exit, and a signal, is a failure.
fn stdout(mut command: Command, no_match: bool) -> Result<String, GitFailed> {
    let call = command
        .get_args()
        .next()
        .map_or_default(|arg| arg.to_string_lossy().into_owned());
    let failed = |why: String| GitFailed { call: call.clone(), why };
    let out = command.output().map_err(|e| failed(format!("could not start: {e}")))?;
    if out.status.success() || (no_match && out.status.code() == Some(1)) {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(failed(format!(
            "{}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
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
/// Raw text out, [`Commit`] parses it. An unrelated history exits 1 and reaches
/// `super::provenance::Commit::parse` as empty text; an unknown ref exits 128 and is [`GitFailed`].
/// The gate refuses both rather than splicing either into `git checkout`.
pub(super) fn merge_base(root: &Path, earlier: &str, later: &str) -> Result<String, GitFailed> {
    let mut command = git(root);
    command.args(["merge-base", earlier, later]);
    stdout(command, true)
}

/// The branch HEAD is on, or `None` when HEAD is detached.
///
/// Detached is the ordinary state in two venues that matter - the reconstruction worktree this gate
/// creates, and a CI checkout of a merge commit - and neither has branch metadata to read, so no
/// branch is the same answer as no recorded parent. `symbolic-ref -q` exits 1 for detached and 128
/// for a call that failed, so only the first is `None`.
pub(super) fn head_branch(root: &Path) -> Result<Option<BranchRef>, GitFailed> {
    let mut command = git(root);
    command.args(["symbolic-ref", "-q", "--short", "HEAD"]);
    Ok(BranchRef::parse(stdout(command, true)?.trim()))
}

/// HEAD's own commit, as git printed it.
///
/// Read for ONE guard: a recorded stack parent that already contains this branch forks at HEAD, and
/// a base equal to HEAD makes `git diff <base> --` the uncommitted working tree alone - an exit-0
/// verdict over every file the branch changed. `super::stack::Origin::Contains` is that refusal and
/// it needs the commit rather than a name, because any parent reaching HEAD produces it.
pub(super) fn head_commit(root: &Path) -> Result<String, GitFailed> {
    let mut command = git(root);
    command.args(["rev-parse", "HEAD"]);
    stdout(command, false)
}

/// The commit messages in `base..HEAD`, as git printed them: `<hash>\0<body>\0` per commit.
///
/// THE RANGE AND NOT HEAD ALONE, because a branch is reviewed whole and
/// `super::relocation::decide` checks a trailer against everything the range did - so a sibling
/// commit that changed behaviour breaks the claim instead of riding on it. `%B` is the raw body,
/// which keeps a trailer on the line it was written on. The hash is framed in (`-z`) for
/// `super::claim::Claim::of`, which ties each `Claim-Cell:` to its own commit
/// (`github.com/telekom/sutura#954`); `super::relocation::Claim::of` reads lines and gets the
/// stream with each NUL turned into a newline, so a trailer on a body's first line stays its own.
///
/// **WHAT IT CANNOT SEE: an UNCOMMITTED split.** `git diff <base>` compares base against the
/// WORKING TREE, so a hand run over uncommitted work has a diff and no message to declare it with.
/// That is the fail-closed direction and it is the point of the carrier: a claim that is not in a
/// commit is not in review either.
fn range_messages(root: &Path, base: &Commit) -> Result<String, GitFailed> {
    range_log(root, base, &[])
}

/// [`range_messages`] for a fixture repository, where a failed git call is a failed test.
#[cfg(test)]
pub(super) fn messages(root: &Path, base: &Commit) -> String {
    range_messages(root, base).expect("a scratch repository answers git")
}

/// The MERGE commits in `base..HEAD` - more than one parent - in [`range_messages`]' own framing, so
/// [`commit_logs`] splits them unchanged. Read for one refusal, `super::refusals::report_merged_trailers`.
fn merges(root: &Path, base: &Commit) -> Result<String, GitFailed> {
    range_log(root, base, &["--merges"])
}

/// What `run` reads of `base..HEAD` up front, together: one failed read is one refusal, not a value
/// that differs per call site. `already_there` asks for the touched paths again on the proof path.
pub(super) struct Range {
    pub(super) merges: String,
    pub(super) messages: String,
    pub(super) touched: Vec<String>,
}

pub(super) fn range(root: &Path, base: &Commit) -> Result<Range, GitFailed> {
    Ok(Range {
        merges: merges(root, base)?,
        messages: range_messages(root, base)?,
        touched: touched(root, base)?,
    })
}

fn range_log(root: &Path, base: &Commit, filter: &[&str]) -> Result<String, GitFailed> {
    let mut command = git(root);
    command
        .args(["log", "-z", "--format=%H%x00%B"])
        .args(filter)
        .arg(format!("{}..HEAD", base.as_str()));
    stdout(command, false)
}

/// Splits [`range_messages`]' NUL-delimited per-commit stream into `(hash, body)` pairs.
///
/// `git log -z --format=%H%x00%B` prints, for each commit, `<hash>\0<body>\0`; a trailing NUL
/// leaves an empty final field that is dropped. A commit with no message body still yields its
/// hash and an empty body. Lives beside the stream it parses, so the two cannot drift.
pub(super) fn commit_logs(log: &str) -> Vec<(&str, &str)> {
    let mut fields: Vec<&str> = log.split('\u{0}').collect();
    if fields.last().is_some_and(|f| f.is_empty()) {
        fields.pop();
    }
    fields.as_chunks::<2>().0.iter().map(|[hash, body]| (*hash, *body)).collect()
}

/// The branch tool's metadata blob for `branch`, as git printed it.
///
/// One blob per branch under `refs/branch-metadata/`, which is where the tool this repository pins
/// records a branch's parent. A branch with no metadata - never tracked, or tracked in another
/// worktree - is an absent ref and reaches the caller as empty text, which
/// `super::stack::recorded_parent` reads as *no recorded parent*: the fallback, and the behaviour
/// that shipped before the derivation existed. ABSENT IS ASKED FIRST (`rev-parse --verify --quiet`
/// exits 1 for it), so a `cat-file` that then fails is a [`GitFailed`] and not that fallback.
pub(super) fn branch_metadata(root: &Path, branch: &BranchRef) -> Result<String, GitFailed> {
    let reference = format!("refs/branch-metadata/{}", branch.as_str());
    let mut find = git(root);
    find.args(["rev-parse", "--verify", "--quiet", &reference]);
    if stdout(find, true)?.trim().is_empty() {
        return Ok(String::new());
    }
    let mut read = git(root);
    read.args(["cat-file", "-p", &reference]);
    stdout(read, false)
}

/// The branch below HEAD's in the stack, resolved against `asked_for`; `Ok(None)` is the fallback.
///
/// GLUE AND NO DECISION: [`stack::parent_of`] composes the reads and [`stack::Base::of`] chooses.
/// The metadata read can fail and that is [`GitFailed`], not *no recorded parent*. The merge-base
/// reads inside `parent_of` fall back on failure, deliberately: a parent branch deleted after
/// merging is an ordinary state there, and the fallback measures the named ref - the wider base.
pub(super) fn parent(root: &Path, asked_for: &Commit) -> Result<Option<Parent>, GitFailed> {
    let branch = head_branch(root)?;
    let blob = std::cell::Cell::new(
        branch
            .as_ref()
            .map(|b| branch_metadata(root, b))
            .transpose()?
            .unwrap_or_default(),
    );
    let metadata = |_: &BranchRef| blob.take();
    let fork = |earlier: &str, later: &str| merge_base(root, earlier, later).unwrap_or_default();
    Ok(stack::parent_of(
        asked_for,
        &stack::Reads {
            branch,
            metadata: &metadata,
            merge_base: &fork,
        },
    ))
}

/// Every tracked `.rs` file in the working tree that contains `word` as a whole word.
///
pub(super) fn naming(root: &Path, word: &str) -> Result<Vec<String>, GitFailed> {
    let mut command = git(root);
    command.args(["grep", "-l", "-z", "-w", "-F", "-e", word, "--", "*.rs"]);
    Ok(stdout(command, true)?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(String::from)
        .collect())
}

/// Every path the diff touched, one per line - **deletions included**.
///
/// `super::diff` builds its `ChangedFile` list from the post-image, so a file this branch DELETED
/// has no entry there at all: its `+++` is `/dev/null`. That is the one shape a whole-file move
/// takes, and [`search`] has to be able to look in it. `--name-only` answers with no post-image
/// involved.
pub(super) fn touched(root: &Path, base: &Commit) -> Result<Vec<String>, GitFailed> {
    let mut command = git(root);
    command.args(["diff", "--name-only", base.as_str(), "--"]);
    Ok(stdout(command, false)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(String::from)
        .collect())
}

/// Every path the base has and the working tree deleted or moved, one per line, as git printed it.
///
/// `--no-renames`, because a moved file is otherwise reported as `R` and never as `D`, so the old
/// path - the one a restored `mod` line names - would not be listed.
pub(super) fn deleted(root: &Path, base: &Commit) -> Result<Vec<String>, GitFailed> {
    let mut command = git(root);
    command.args(["diff", "--name-only", "--no-renames", "--diff-filter=D", base.as_str(), "--"]);
    Ok(stdout(command, false)?.lines().map(String::from).collect())
}

/// Lines matching any of `needles` in `base`, under `paths`.
///
/// The pathspec is what keeps this narrow: the question is whether a test the diff NAMES was
/// already in a file the diff also touched, because that is what a move looks like from the base
/// side, and asking the whole tree instead would let one of this tree's duplicated test names
/// answer yes. `-F` because a needle is a fixed string; exit 1 is "no match" and reaches the caller
/// as empty output, which reads as *nothing moved* - the answer the gate gave before this existed.
pub(super) fn search(root: &Path, base: &Commit, needles: &[String], paths: &[String]) -> Result<String, GitFailed> {
    if needles.is_empty() || paths.is_empty() {
        return Ok(String::new());
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
    stdout(command, true)
}

/// Which of the tests `named` the base commit already had, asked of git.
///
/// TWO CALLS AND NO DECISION HERE. The search is restricted to the `.rs` files THIS DIFF TOUCHED -
/// deletions included, which is why the path list comes from git rather than from the parsed diff -
/// because that is what a move looks like from the base side: the file the test came from lost
/// those lines, so it is in the diff. Asking the whole tree instead would let one of this tree's
/// duplicated test names answer yes about a genuinely new test. `provenance` owns both the patterns
/// and the classification, so the direction it fails in is stated where it is decided.
pub(super) fn already_there(root: &Path, base: &Commit, named: &[&str], scoped: &[AddedTest]) -> Result<Moved, GitFailed> {
    let paths: Vec<String> = touched(root, base)?
        .into_iter()
        .filter(|path| Reach::of(path) == Reach::Compiled)
        .collect();
    let found = search(root, base, &needles(named), &paths)?;
    let edited: Vec<&str> = scoped.iter().filter(|test| test.is_edited()).map(AddedTest::name).collect();
    Ok(Moved::of(named, &edited, &found))
}

/// Does `path` exist at `base`?
///
/// "Revert to base" means two different things depending on the answer. For a file that
/// existed, it means check out the old content. For a file this branch ADDED, it means the file
/// is not there - and `git checkout base -- <new file>` fails with "did not match any file(s)
/// known to git", which is how this gate first broke in CI.
///
/// `ls-tree` and not `cat-file -e`, because the latter exits 128 for an absent path AND for a base
/// git cannot read, so the two could not be told apart. `ls-tree` exits 0 with no entry for an absent
/// path and 128 only for a failure, which is [`GitFailed`] here and never "absent".
pub(super) fn base_has(root: &Path, base: &Commit, path: &str) -> Result<bool, GitFailed> {
    let mut command = git(root);
    command.args(["ls-tree", "-z", "--name-only", base.as_str(), "--", path]);
    Ok(!stdout(command, false)?.is_empty())
}

/// `path`'s content at `base`, or `None` when git did not hand any back.
///
/// SEPARATE FROM [`base_has`] ON PURPOSE, because `super::features` needs the two answers apart:
/// a manifest the base does not have is a NEW package and its base feature table is legitimately
/// empty, while a manifest the base HAS and whose content did not come back leaves that scan with
/// nothing to compare - and answering *nothing was enabled* there would be a claim about a table
/// it never read. One reader, and the pairing is decided at the call site.
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

/// [`add_worktree`] for the base run, with a COPY of the root's discovery file, so a tier-backed
/// cell there reaches the tier the head run did - `runner::nextest` says why.
///
/// A copy rather than a symlink: a `forget` in the base tree must not withdraw the root's entry.
/// [`remove_worktree`] deletes it with the worktree. The claim arm's kill worktree does not call
/// this - it starts its own tier.
pub(super) fn add_base_worktree(root: &Path, dir: &Path) -> Result<(), String> {
    add_worktree(root, dir)?;
    let scope = |at: &Path| Scope::from_root(at).map_err(|e| e.to_string());
    let (from, to) = (path_for(&scope(root)?), path_for(&scope(dir)?));
    if !from.exists() {
        return Ok(());
    }
    let failed = |e: std::io::Error| format!("could not copy `{}` into the base worktree: {e}", from.display());
    if let Some(state) = to.parent() {
        std::fs::create_dir_all(state).map_err(failed)?;
    }
    std::fs::copy(&from, &to).map(drop).map_err(failed)
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
pub(super) fn base_state<'a>(root: &Path, base: &Commit, files: &'a [String]) -> Result<BaseState<'a>, GitFailed> {
    let (mut restore, mut remove) = (Vec::new(), Vec::new());
    for file in files {
        if base_has(root, base, file)? {
            &mut restore
        } else {
            &mut remove
        }
        .push(file);
    }
    Ok(BaseState { restore, remove })
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
    use std::path::{Path, PathBuf};

    use super::{
        BaseState, BranchRef, Commit, apply, base_has, base_state, branch_metadata, deleted, head_branch, head_commit,
        merge_base, merges, naming, range, range_messages, search, touched,
    };

    /// A scratch repository holding `src/a.rs` in one commit, and that commit.
    fn one_commit_in_a_scratch_repo(label: &str) -> (PathBuf, Commit) {
        let dir = std::env::temp_dir().join(format!("sutura-causality-{label}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(dir.join("src")).expect("a scratch tree");
        std::fs::write(dir.join("src/a.rs"), "pub fn one() -> u8 {\n    1\n}\n").expect("a module");
        let git = |args: &[&str]| {
            let mut command = std::process::Command::new("git");
            crate::repo::strip_git_env(&mut command);
            let out = command.current_dir(&dir).args(args).output().expect("git runs");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=user@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-m",
            "base",
        ]);
        let base = Commit::parse(&git(&["rev-parse", "HEAD"])).expect("an object name");
        (dir, base)
    }

    #[test]
    fn a_moved_file_is_listed_by_its_old_path() {
        let (dir, base) = one_commit_in_a_scratch_repo("moved");
        let mut mv = std::process::Command::new("git");
        crate::repo::strip_git_env(&mut mv);
        assert!(
            mv.current_dir(&dir)
                .args(["mv", "src/a.rs", "src/b.rs"])
                .status()
                .expect("git runs")
                .success()
        );
        let listed = deleted(&dir, &base).unwrap();
        drop(std::fs::remove_dir_all(&dir));
        assert_eq!(listed, vec![String::from("src/a.rs")]);
    }

    /// Git exits 128 for a base it cannot read and cannot start in a directory that is gone; neither
    /// is an answer, and every helper used to read both as empty text.
    #[test]
    fn a_failed_git_call_is_an_error_and_never_an_empty_answer() {
        let (dir, _) = one_commit_in_a_scratch_repo("failed");
        let unknown = Commit::parse("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").expect("an object name");
        let gone: &Path = &dir.join("absent");
        let branch = BranchRef::parse("a-branch").expect("a branch name");
        let files = [String::from("src/a.rs")];
        let needles = [String::from("one")];
        let answered: Vec<&str> = [
            ("merge_base", merge_base(&dir, "no-such-ref", "HEAD").is_ok()),
            ("range_messages", range_messages(&dir, &unknown).is_ok()),
            ("merges", merges(&dir, &unknown).is_ok()),
            ("range", range(&dir, &unknown).is_ok()),
            ("touched", touched(&dir, &unknown).is_ok()),
            ("deleted", deleted(&dir, &unknown).is_ok()),
            ("search", search(&dir, &unknown, &needles, &files).is_ok()),
            ("base_has", base_has(&dir, &unknown, "src/a.rs").is_ok()),
            ("base_state", base_state(&dir, &unknown, &files).is_ok()),
            ("head_commit", head_commit(gone).is_ok()),
            ("head_branch", head_branch(gone).is_ok()),
            ("naming", naming(gone, "one").is_ok()),
            ("branch_metadata", branch_metadata(gone, &branch).is_ok()),
        ]
        .into_iter()
        .filter_map(|(call, ok)| ok.then_some(call))
        .collect();
        let why = range_messages(&dir, &unknown).unwrap_err().to_string();
        drop(std::fs::remove_dir_all(&dir));
        assert!(answered.is_empty(), "a failed git call read as an answer: {answered:?}");
        assert!(why.contains("git log"), "the refusal names the call: {why}");
    }

    /// The other half: git's own *nothing here* answers stay answers, so refusing a failure does not
    /// turn every absent path or unmatched search into a refusal.
    #[test]
    fn an_absent_answer_is_still_an_answer() {
        let (dir, base) = one_commit_in_a_scratch_repo("absent");
        let branch = BranchRef::parse("a-branch").expect("a branch name");
        let files = [String::from("src/a.rs")];
        let present = base_has(&dir, &base, "src/a.rs");
        let absent = base_has(&dir, &base, "src/none.rs");
        let unmatched = (naming(&dir, "zzz"), search(&dir, &base, &[String::from("zzz")], &files));
        let named = naming(&dir, "one");
        let untracked = branch_metadata(&dir, &branch);
        let forked_at = merge_base(&dir, base.as_str(), "HEAD");
        let on_a_branch = head_branch(&dir);
        let mut detach = std::process::Command::new("git");
        crate::repo::strip_git_env(&mut detach);
        assert!(
            detach
                .current_dir(&dir)
                .args(["checkout", "-q", "--detach"])
                .status()
                .expect("git runs")
                .success()
        );
        let detached = head_branch(&dir);
        drop(std::fs::remove_dir_all(&dir));
        assert!(present.unwrap(), "a file at base is present");
        assert!(!absent.unwrap(), "a path the base lacks is absent, not a failure");
        assert_eq!(unmatched.0.unwrap(), Vec::<String>::new());
        assert_eq!(unmatched.1.unwrap(), "");
        assert_eq!(named.unwrap(), vec![String::from("src/a.rs")]);
        assert_eq!(untracked.unwrap(), "", "no metadata blob is no recorded parent");
        assert_eq!(forked_at.unwrap().trim(), base.as_str());
        assert_eq!(on_a_branch.unwrap().as_ref().map(BranchRef::as_str), Some("main"));
        assert_eq!(detached.unwrap(), None, "a detached HEAD is no branch, not a failure");
    }

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
