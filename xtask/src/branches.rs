//! `clean-branches`: delete local branches, and the worktrees holding them, once the work has
//! landed - and print what it would do rather than doing it.
//!
//! # Why this is a task rather than a `git branch --merged` one-liner
//!
//! **`git branch --merged` cannot see a squash-merged branch.** The commits on such a branch are
//! not ancestors of the commit that carries their content, so git reports the branch as unmerged
//! and is right to. Where every pull request is squash-merged, that makes the built-in answer
//! useless: on the day this task was written, twenty-one squash merges landed and the cleanup was
//! done three times by hand - reading pull-request state, then searching the default branch for
//! each branch's final artefact. That procedure is what lives here.
//!
//! Two things went wrong during those manual passes, and both are now cases with a reason printed
//! next to them: a worktree held a branch that was four ahead and seven behind after a force-push,
//! and a stack tool reported *"all branches up to date"* while three branches sat in worktrees it
//! did not track. **"Nothing to do" was wrong and silent**, which is why this prints every branch
//! it considered, kept ones included.
//!
//! # Dry run by default, and fail SAFE
//!
//! Printing is the default and `--delete` is the only thing that acts, because a cleanup task that
//! deletes on a bare invocation is one nobody runs twice. **There is no flag that overrides a
//! refusal**: `--delete` moves the plan from print to execute and widens nothing.
//!
//! The direction is the opposite of `classify` and its siblings, which **fail open** and run
//! everything when they cannot tell - there the expensive failure is a check nobody ran. Here the
//! expensive failure is a deleted branch, so anything undetermined keeps the branch and the report
//! names the signal that was missing. [`decide`] carries that reasoning next to the decision.
//!
//! # Shape
//!
//! `git` reads state, [`decide`] decides, this file prints and - only under `--delete` - acts. The
//! decision is a pure function over parsed state, which is what lets the squash-merge case be
//! asserted without a repository.

mod decide;
mod forge;
mod git;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use decide::{Blocker, Decision, Equivalence, Forge, Landed, WorkingTree, WorktreeBlocker, WorktreeDecision};

use crate::Verdict;
use crate::repo;

/// Whether this invocation prints or acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Print the plan. The default.
    DryRun,
    /// Execute it.
    Delete,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Options {
    mode: Mode,
    /// Leave a branch alone whose tip is newer than this many days.
    unused_days: Option<u32>,
    /// `git fetch --prune` first, so `[gone]` is current rather than as of the last fetch.
    fetch: bool,
    /// State that there is no forge to ask - a checkout with no remote. The open-pull-request
    /// guard is then WAIVED rather than skipped silently, and the report says which.
    without_forge: bool,
    /// Act on another checkout - how the decision gets exercised against a constructed history.
    repo: Option<PathBuf>,
}

/// One thing a `--delete` run would do. Ordered: worktrees first, then the prune, then branches.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    /// `git worktree remove <path>`.
    RemoveWorktree(String),
    /// `git worktree prune`.
    PruneWorktrees,
    /// `git branch -D <branch>`.
    DeleteBranch(String),
}

/// Everything the report prints and everything an execution would do.
struct Plan {
    /// What the comparisons were made against.
    base: git::Base,
    /// Whether the forge answered, and what it said.
    pulls: forge::Pulls,
    /// Every local branch, with its decision.
    branches: Vec<(decide::Branch, Decision)>,
    /// Every registered worktree, with its decision.
    worktrees: Vec<(decide::Worktree, WorktreeDecision)>,
}

/// Read the flags. Anything unrecognised is a usage error rather than a default.
fn options(args: &[String]) -> Result<Options, String> {
    let mut parsed = Options {
        mode: Mode::DryRun,
        unused_days: None,
        fetch: false,
        without_forge: false,
        repo: None,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--delete" => parsed.mode = Mode::Delete,
            "--fetch" => parsed.fetch = true,
            "--without-forge" => parsed.without_forge = true,
            "--unused-days" => {
                let value = rest.next().ok_or_else(|| String::from("--unused-days needs a number"))?;
                parsed.unused_days = Some(
                    value
                        .parse()
                        .map_err(|_not_a_number| format!("--unused-days wants a number, not `{value}`"))?,
                );
            }
            "--repo" => {
                let value = rest.next().ok_or_else(|| String::from("--repo needs a path"))?;
                parsed.repo = Some(PathBuf::from(value));
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(parsed)
}

pub(crate) fn run(args: &[String]) -> Verdict {
    let options = match options(args) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!("xtask clean-branches: {problem}");
            usage();
            return Verdict::Usage;
        }
    };
    let Some(root) = root_of(&options) else {
        eprintln!("xtask clean-branches: could not determine the repository root");
        return Verdict::Fail;
    };
    if options.fetch
        && let Err(problem) = git::fetch_and_prune(&root)
    {
        // Not fatal: a fetch that failed leaves the remote-tracking refs as they were, and stale
        // refs make an upstream look present when it is gone - which keeps branches.
        eprintln!("  warning: git fetch --prune failed: {problem}");
        eprintln!("  upstream state is as of the last successful fetch");
    }
    let plan = match collect(&root, &options) {
        Ok(plan) => plan,
        Err(problem) => {
            eprintln!("xtask clean-branches: {problem}");
            return Verdict::Fail;
        }
    };
    report(&root, &plan, &options);

    let actions = actions(&plan, options.mode);
    if actions.is_empty() {
        if options.mode == Mode::DryRun {
            println!();
            println!("Nothing was deleted. Re-run with `--delete` to apply the plan above.");
        }
        return Verdict::Pass;
    }
    execute(&root, &actions)
}

fn usage() {
    eprintln!("usage: cargo xtask clean-branches [--delete] [--unused-days <n>] [--fetch] [--without-forge] [--repo <path>]");
    eprintln!("  dry run by default; --delete is the only thing that removes anything");
}

/// Which checkout to act on.
fn root_of(options: &Options) -> Option<PathBuf> {
    options.repo.as_ref().map_or_else(
        || repo::root().and_then(|root| git::toplevel(&root)).or_else(repo::root),
        |path| git::toplevel(path),
    )
}

/// Read the repository into a plan.
fn collect(root: &Path, options: &Options) -> Result<Plan, String> {
    let base = git::base(root)?;
    let refs = git::refs(root)?;
    let listed = git::worktrees(root)?;
    let pulls = if options.without_forge {
        forge::waived()
    } else {
        forge::ask(root)
    };
    let here = std::env::current_dir().ok();

    let worktrees: Vec<decide::Worktree> = listed
        .iter()
        .enumerate()
        .map(|(index, line)| worktree_facts(index, line, here.as_deref()))
        .collect();

    let policy = decide::Policy {
        default_branch: base.name.clone(),
        current_branch: git::current_branch(root),
        forge: pulls.forge.clone(),
        now_seconds: now_seconds(),
        unused_days: options.unused_days,
    };

    let mut landings = git::Landings::new(root, &base.rev);
    let mut branches: Vec<(decide::Branch, Decision)> = Vec::new();
    let mut landed: BTreeSet<String> = BTreeSet::new();
    for line in &refs {
        let branch = decide::Branch {
            name: line.name.clone(),
            tip: line.tip.clone(),
            upstream: line.upstream_state(),
            tip_seconds: line.seconds(),
            landing: landings.of(&line.name),
            pull_request: pulls.of(&line.name),
            checkout: checkout_of(line, &worktrees),
        };
        if decide::landed_though_checked_out(&branch, &policy).is_some() {
            landed.insert(branch.name.clone());
        }
        let decision = decide::decide(&branch, &policy);
        branches.push((branch, decision));
    }

    let worktrees = worktrees
        .into_iter()
        .map(|worktree| {
            let decision = decide::decide_worktree(&worktree, &landed);
            (worktree, decision)
        })
        .collect();

    Ok(Plan {
        base,
        pulls,
        branches,
        worktrees,
    })
}

/// One registered worktree, as the decision reads it.
///
/// The FIRST entry `git worktree list` prints is the repository's main worktree, which is why the
/// index is a parameter: it is the only thing in the porcelain output that says so.
fn worktree_facts(index: usize, line: &git::WorktreeLine, here: Option<&Path>) -> decide::Worktree {
    let path = PathBuf::from(&line.path);
    let present = path.is_dir();
    decide::Worktree {
        path: line.path.clone(),
        branch: line.branch.clone(),
        locked: line.locked,
        present,
        state: if present {
            git::working_tree(&path)
        } else {
            WorkingTree::Unreadable {
                because: String::from("the directory is not there"),
            }
        },
        standing: if index == 0 || line.bare {
            decide::Standing::Primary
        } else if here.is_some_and(|here| here.starts_with(&path)) {
            decide::Standing::RunningHere
        } else {
            decide::Standing::Ordinary
        },
    }
}

/// The worktree holding this branch, if any.
fn checkout_of(line: &git::RefLine, worktrees: &[decide::Worktree]) -> Option<decide::Checkout> {
    if line.worktree.trim().is_empty() {
        return None;
    }
    let known = worktrees.iter().find(|worktree| worktree.path == line.worktree);
    Some(decide::Checkout {
        path: line.worktree.clone(),
        locked: known.is_some_and(|worktree| worktree.locked),
        state: known.map_or_else(
            || WorkingTree::Unreadable {
                because: String::from("git did not list that worktree"),
            },
            |worktree| worktree.state.clone(),
        ),
    })
}

/// Seconds since the epoch, or 0 when the clock is unreadable - which makes every age zero and so
/// keeps every branch an age bound was asked about.
fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0))
}

/// What a run would do, in order. **Empty for a dry run** - which is the dry-run-by-default claim
/// as a value rather than as a promise about a code path.
fn actions(plan: &Plan, mode: Mode) -> Vec<Action> {
    if mode == Mode::DryRun {
        return Vec::new();
    }
    let mut out: Vec<Action> = Vec::new();
    for (worktree, decision) in &plan.worktrees {
        if matches!(*decision, WorktreeDecision::Remove { .. }) {
            out.push(Action::RemoveWorktree(worktree.path.clone()));
        }
    }
    if plan
        .worktrees
        .iter()
        .any(|(_, decision)| *decision == WorktreeDecision::Prune)
    {
        out.push(Action::PruneWorktrees);
    }
    for (branch, decision) in &plan.branches {
        if matches!(*decision, Decision::Delete { .. }) {
            out.push(Action::DeleteBranch(branch.name.clone()));
        }
    }
    out
}

/// Run the plan. Every failure is reported and the rest still runs: a worktree git will not remove
/// is not a reason to leave the next one behind.
fn execute(root: &Path, actions: &[Action]) -> Verdict {
    println!();
    let mut failures = 0_usize;
    for action in actions {
        match action {
            Action::RemoveWorktree(path) => match git::remove_worktree(root, path) {
                Ok(()) => println!("  removed worktree  {path}"),
                Err(problem) => {
                    failures = failures.saturating_add(1);
                    eprintln!("  FAILED to remove worktree {path}: {problem}");
                }
            },
            Action::PruneWorktrees => match git::prune_worktrees(root) {
                Ok(said) if said.is_empty() => println!("  pruned stale worktree registrations"),
                Ok(said) => println!("  pruned: {said}"),
                Err(problem) => {
                    failures = failures.saturating_add(1);
                    eprintln!("  FAILED to prune worktrees: {problem}");
                }
            },
            Action::DeleteBranch(branch) => match git::delete_branch(root, branch) {
                Ok(said) if said.is_empty() => println!("  deleted branch    {branch}"),
                Ok(said) => println!("  {said}"),
                Err(problem) => {
                    failures = failures.saturating_add(1);
                    eprintln!("  FAILED to delete {branch}: {problem}");
                }
            },
        }
    }
    if failures == 0 {
        return Verdict::Pass;
    }
    eprintln!();
    eprintln!("xtask clean-branches: {failures} action(s) failed - nothing else was changed");
    Verdict::Fail
}

/// Print the plan. Identical in both modes, so what a reader is shown before a delete is what a
/// dry run would have shown them.
fn report(root: &Path, plan: &Plan, options: &Options) {
    let mode = match options.mode {
        Mode::DryRun => "DRY RUN - nothing will be deleted",
        Mode::Delete => "DELETE - the plan below will be executed",
    };
    println!("xtask clean-branches: {mode}");
    println!("  repository  {}", root.display());
    println!(
        "  comparing   {} (the branch `{}` is protected)",
        plan.base.rev, plan.base.name
    );
    match &plan.pulls.forge {
        Forge::Answered => {
            let (open, merged) = plan.pulls.tally();
            println!("  forge       answered: {open} open, {merged} merged pull request(s)");
        }
        Forge::Waived => {
            println!("  forge       NOT ASKED - --without-forge was passed");
            println!("              the open-pull-request guard is waived, deliberately, by whoever ran this");
        }
        Forge::Silent { because } => {
            println!("  forge       UNAVAILABLE: {because}");
            println!("              the open-pull-request guard could not run, so nothing is deletable");
        }
    }
    match options.unused_days {
        None => println!("  age bound   none asked for"),
        Some(days) => println!("  age bound   a branch touched in the last {days} day(s) is left alone"),
    }

    print_branches(plan);
    print_worktrees(plan);
}

fn print_branches(plan: &Plan) {
    let delete: Vec<(&str, String)> = plan
        .branches
        .iter()
        .filter_map(|(branch, decision)| match decision {
            Decision::Delete { because } => Some((branch.name.as_str(), evidence(because))),
            Decision::Keep { .. } => None,
        })
        .collect();
    let keep: Vec<(&str, String)> = plan
        .branches
        .iter()
        .filter_map(|(branch, decision)| match decision {
            Decision::Keep { because } => Some((branch.name.as_str(), skipped(because))),
            Decision::Delete { .. } => None,
        })
        .collect();

    println!();
    println!("branches to delete ({}):", delete.len());
    print_rows(&delete);
    println!();
    println!("branches kept ({}):", keep.len());
    print_rows(&keep);
}

fn print_worktrees(plan: &Plan) {
    let mut remove: Vec<(&str, String)> = Vec::new();
    let mut prune: Vec<(&str, String)> = Vec::new();
    let mut keep: Vec<(&str, String)> = Vec::new();
    for (worktree, decision) in &plan.worktrees {
        let path = worktree.path.as_str();
        match decision {
            WorktreeDecision::Remove { branch } => {
                remove.push((path, format!("holds `{branch}`, whose work has landed; clean and unlocked")));
            }
            WorktreeDecision::Prune => {
                prune.push((path, String::from("registered, but its directory is gone")));
            }
            WorktreeDecision::Keep { because } => keep.push((path, worktree_skipped(because))),
        }
    }
    println!();
    println!("worktrees to remove ({}):", remove.len());
    print_rows(&remove);
    println!();
    println!("worktrees to prune ({}):", prune.len());
    print_rows(&prune);
    println!();
    println!("worktrees kept ({}):", keep.len());
    print_rows(&keep);
}

/// One `name  reason` block, or an explicit `none`. Silence would read as "nothing was considered".
fn print_rows(rows: &[(&str, String)]) {
    if rows.is_empty() {
        println!("  none");
        return;
    }
    let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0).min(48);
    for (name, reason) in rows {
        println!("  {name:width$}  {reason}");
    }
}

/// What licensed a delete, in a sentence.
fn evidence(landed: &Landed) -> String {
    match landed {
        Landed::Git(Equivalence::Ancestor) => String::from("its commits are already on the default branch"),
        Landed::Git(Equivalence::PerCommit) => {
            String::from("every commit has a patch-equivalent commit on the default branch (git cherry)")
        }
        Landed::Git(Equivalence::Squashed) => {
            String::from("its cumulative diff is one commit on the default branch - a squash merge")
        }
        Landed::MergedPullRequest { number } => {
            format!("pull request #{number} is merged, and its recorded head is this branch's tip")
        }
    }
}

/// Why a branch was left alone, in a sentence.
fn skipped(blocker: &Blocker) -> String {
    match blocker {
        Blocker::Protected => String::from("the default branch, or a long-lived name"),
        Blocker::CurrentBranch => String::from("checked out where this ran"),
        Blocker::ForgeSilent { because } => {
            format!("the forge could not be asked ({because}), so nothing is deletable")
        }
        Blocker::OpenPullRequest { number } => format!("pull request #{number} is open"),
        Blocker::UncommittedWork { path } => format!("uncommitted work in {path}"),
        Blocker::WorkingTreeUnreadable { path, because } => {
            format!("could not read the working tree in {path}: {because}")
        }
        Blocker::CheckedOut { path, locked } => {
            let lock = if *locked { ", locked" } else { "" };
            format!("checked out in {path}{lock}")
        }
        Blocker::UnpushedCommits { ahead } => {
            format!("{ahead} commit(s) exist here and nowhere else")
        }
        Blocker::NotOnDefaultBranch { commits } => {
            format!("{commits} commit(s) with no equivalent on the default branch")
        }
        Blocker::LandingUndetermined { because } => {
            format!("undetermined: {because} - so it is not treated as landed")
        }
        Blocker::MergedPullRequestMovedOn { number, head, tip } => format!(
            "pull request #{number} merged {}, and the branch is at {} - written since",
            short(head),
            short(tip)
        ),
        Blocker::TouchedRecently { age_days, bound } => {
            format!("landed, but touched {age_days} day(s) ago and the bound is {bound}")
        }
        Blocker::AgeUnknown { bound } => {
            format!("an age bound of {bound} day(s) was asked for and git reported no date")
        }
    }
}

/// Why a worktree was left alone, in a sentence.
fn worktree_skipped(blocker: &WorktreeBlocker) -> String {
    match blocker {
        WorktreeBlocker::Primary => String::from("the repository's main worktree"),
        WorktreeBlocker::RunningHere => String::from("this invocation is running in it"),
        WorktreeBlocker::Locked => String::from("locked - somebody said so on purpose"),
        WorktreeBlocker::UncommittedWork => String::from("uncommitted work"),
        WorktreeBlocker::WorkingTreeUnreadable { because } => {
            format!("could not read its working tree: {because}")
        }
        WorktreeBlocker::DetachedHead => String::from("detached HEAD - no branch to decide on"),
        WorktreeBlocker::BranchKept { branch } => {
            format!("holds `{branch}`, which is not deletable - see above")
        }
    }
}

/// A commit, short enough to read.
fn short(commit: &str) -> String {
    commit.chars().take(8).collect()
}

#[cfg(test)]
mod tests;
