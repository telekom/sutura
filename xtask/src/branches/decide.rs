//! The decision: may this branch be deleted, and on what evidence?
//!
//! A pure function over parsed git state, and that shape is the point. Every signal this reads -
//! where the upstream went, what `git cherry` said, what the forge reported, what the worktree
//! holds - arrives as a value, so the case that matters can be asserted without a repository:
//! **a branch whose work is in the default branch by way of a SQUASH merge, which
//! `git branch --merged` cannot see at all.** The commits of a squash-merged branch are not
//! ancestors of anything on the default branch, so the built-in answer is "unmerged" for a branch
//! that is entirely landed. That single fact is why this task exists.
//!
//! # It fails SAFE, and that is the opposite of the direction the rest of this tool points
//!
//! `classify` and its siblings **fail open**: an unmapped path or a bad base ref makes them run
//! everything, because the expensive failure there is a check nobody ran. Here the expensive
//! failure is a **deleted branch**, so every undetermined signal keeps the branch and the report
//! names the signal that was missing. There is no flag that overrides a refusal: `--delete` moves
//! the plan from print to execute, and nothing widens the plan itself.
//!
//! # What licenses a delete
//!
//! Exactly one of three, all three meaning "the work is already somewhere else":
//!
//! * `git cherry` finds a patch-equivalent commit on the default branch for every commit on the
//!   branch - a rebase-merge, or a squash of a single commit;
//! * the branch's CUMULATIVE diff is one commit on the default branch - a squash of several
//!   commits, where no individual commit matches anything;
//! * the forge reports a MERGED pull request whose recorded head is exactly this branch's tip.
//!
//! The third covers what patch-id cannot see and the first two cover what a forge cannot be asked
//! about, which is why both mechanisms are here rather than whichever one was easier.
//!
//! An upstream that is gone is **corroboration and never a licence.** A forge deletes the remote
//! branch on merge - and so does a person abandoning unmerged work, which is the same observation
//! with the opposite meaning. The tip-equality check on a merged pull request is the other half of
//! that caution: a pull request merged this morning says nothing about a commit written since.

use std::collections::BTreeSet;

/// Branch names that are never deleted, whatever any signal says.
///
/// The default branch is added to this at the call site, because it is read from the repository
/// rather than assumed. These are the conventional long-lived names: a repository that renamed
/// its trunk still has an old name that must not be swept up.
const PROTECTED: &[&str] = &[
    "HEAD",
    "main",
    "master",
    "trunk",
    "develop",
    "dev",
    "staging",
    "production",
    "release",
];

/// How a branch relates to the remote branch it tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Upstream {
    /// Nothing ever pushed it. Every commit on it exists here and nowhere else.
    Untracked,
    /// It tracked a remote branch, and that branch is gone.
    Gone,
    /// Present, with how far the two have drifted.
    Present {
        /// Commits here that the upstream does not have.
        ahead: u32,
        /// Commits the upstream has and this branch does not.
        behind: u32,
    },
}

/// How the branch's work reached the default branch, when it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Equivalence {
    /// The branch's commits are already on the default branch - the one case
    /// `git branch --merged` also gets right.
    Ancestor,
    /// Every commit has a patch-equivalent commit on the default branch (`git cherry`).
    PerCommit,
    /// The branch's cumulative diff is one commit on the default branch: a squash merge.
    Squashed,
}

/// Where the branch's commits are, relative to the default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Landing {
    /// The work is on the default branch, by this mechanism.
    Landed(Equivalence),
    /// This many commits on the branch have no equivalent on the default branch.
    Unlanded {
        /// How many, for the report - a reader wants to know whether it is one or forty.
        commits: u32,
    },
    /// git could not answer. **Not landed**: an undetermined signal never licenses a delete.
    Undetermined {
        /// Which command could not answer. Printed, so the reader knows what to fix.
        because: &'static str,
    },
}

/// What the forge says about the branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PullRequest {
    /// It is the head of an open pull request.
    Open {
        /// The pull request number.
        number: u64,
    },
    /// It is the head of a merged pull request, with the commit the forge recorded as its head.
    Merged {
        /// The pull request number.
        number: u64,
        /// The head commit at merge time. Compared against the branch tip: work committed after
        /// the merge is work the merge did not carry.
        head: String,
    },
    /// The forge knows no pull request with this head. Not evidence of anything.
    Unknown,
}

/// Could the forge be asked at all?
///
/// The open-pull-request guard has no git-only substitute - a branch with an open pull request
/// looks exactly like a branch without one from inside a checkout - so a silent forge is not
/// degraded to the weaker signal. It stops the delete and says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Forge {
    /// It answered.
    Answered,
    /// The caller stated there is no forge to ask - a checkout with no remote, or one whose remote
    /// hosts no pull requests. The guard is **waived, explicitly**, and the report says so on its
    /// own line: the difference between a waiver and a silent fallback is that somebody asked for
    /// this one.
    Waived,
    /// It did not answer, and this is why.
    Silent {
        /// What went wrong, in one line.
        because: String,
    },
}

/// The state of a checkout's working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkingTree {
    /// `git status --porcelain` was empty.
    Clean,
    /// It reported something. Losing that is the failure this task must not have.
    Dirty,
    /// It could not be read - a permission, a missing directory, a git that would not run.
    Unreadable {
        /// What went wrong.
        because: String,
    },
}

/// The worktree a branch is checked out in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Checkout {
    /// Where it is. Reported, because "checked out somewhere" is not an answer a reader can act on.
    pub(crate) path: String,
    /// A locked worktree is left alone entirely.
    pub(crate) locked: bool,
    /// What its working tree looks like.
    pub(crate) state: WorkingTree,
}

/// Everything the decision reads about one branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Branch {
    /// The short name.
    pub(crate) name: String,
    /// The commit it points at.
    pub(crate) tip: String,
    /// Where its upstream went.
    pub(crate) upstream: Upstream,
    /// Committer date of the tip, in seconds since the epoch. `None` when git reported none.
    pub(crate) tip_seconds: Option<i64>,
    /// What git says about the work having landed.
    pub(crate) landing: Landing,
    /// What the forge says.
    pub(crate) pull_request: PullRequest,
    /// The worktree holding it, if any.
    pub(crate) checkout: Option<Checkout>,
}

/// The rules that are the same for every branch in one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Policy {
    /// The branch every comparison was made against, and one more name that is never deleted.
    pub(crate) default_branch: String,
    /// The branch checked out where this invocation is running.
    pub(crate) current_branch: Option<String>,
    /// Whether the forge could be asked.
    pub(crate) forge: Forge,
    /// Now, in seconds since the epoch.
    pub(crate) now_seconds: i64,
    /// Leave a branch alone whose tip is newer than this many days. `None` applies no bound.
    pub(crate) unused_days: Option<u32>,
}

/// The evidence that licensed a delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Landed {
    /// git itself: patch equivalence against the default branch.
    Git(Equivalence),
    /// The forge: a merged pull request whose recorded head is this branch's tip.
    MergedPullRequest {
        /// The pull request number.
        number: u64,
    },
}

/// Why a branch was left alone. Every variant is printed: a skip with no stated reason is
/// indistinguishable from a branch the tool never considered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Blocker {
    /// The default branch, or a conventional long-lived name.
    Protected,
    /// Checked out where this invocation is running.
    CurrentBranch,
    /// The forge could not be asked, so the open-pull-request guard could not run.
    ForgeSilent {
        /// What went wrong.
        because: String,
    },
    /// The head of an open pull request.
    OpenPullRequest {
        /// The pull request number.
        number: u64,
    },
    /// Its worktree has uncommitted work.
    UncommittedWork {
        /// Where.
        path: String,
    },
    /// Its worktree's state could not be read, so "clean" is not established.
    WorkingTreeUnreadable {
        /// Where.
        path: String,
        /// What went wrong.
        because: String,
    },
    /// Checked out in a worktree. Reported with the path, which is the thing a reader has to act
    /// on and the thing a stack tool that tracks only its own branches will not tell them.
    CheckedOut {
        /// Where.
        path: String,
        /// Whether that worktree is locked.
        locked: bool,
    },
    /// Commits its upstream does not have, and no landed signal to say they are elsewhere.
    UnpushedCommits {
        /// How many.
        ahead: u32,
    },
    /// Commits with no equivalent on the default branch.
    NotOnDefaultBranch {
        /// How many.
        commits: u32,
    },
    /// A signal could not be read. Fail-safe: undetermined is not landed.
    LandingUndetermined {
        /// Which command could not answer.
        because: &'static str,
    },
    /// A merged pull request, but the branch has moved since it merged.
    MergedPullRequestMovedOn {
        /// The pull request number.
        number: u64,
        /// What the forge recorded as merged.
        head: String,
        /// Where the branch is now.
        tip: String,
    },
    /// Landed, but touched more recently than the age bound allows.
    TouchedRecently {
        /// How old the tip is, in days.
        age_days: i64,
        /// The bound that was asked for.
        bound: u32,
    },
    /// An age bound was asked for and git reported no date to measure against.
    AgeUnknown {
        /// The bound that was asked for.
        bound: u32,
    },
}

/// What to do with one branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Delete it, on this evidence.
    Delete {
        /// The evidence.
        because: Landed,
    },
    /// Leave it alone, for this reason.
    Keep {
        /// The reason.
        because: Blocker,
    },
}

/// May this branch be deleted?
///
/// Guards first, then evidence, then the age bound. The order decides which reason a reader is
/// shown, and the sharper reason wins: a dirty worktree is reported as uncommitted work rather
/// than as "checked out somewhere", because those call for different actions.
pub(crate) fn decide(branch: &Branch, policy: &Policy) -> Decision {
    if let Some(because) = guard(branch, policy) {
        return Decision::Keep { because };
    }
    match landed(branch) {
        Err(because) => Decision::Keep { because },
        Ok(evidence) => {
            too_recent(branch, policy).map_or(Decision::Delete { because: evidence }, |because| Decision::Keep { because })
        }
    }
}

/// The same question with the checkout guard lifted: would this branch be deleted if it were not
/// checked out anywhere?
///
/// The worktree side needs this and must not answer it with a second copy of the rules, so it is
/// the same function asked with the checkout removed. A branch is never deleted in the run that
/// removes its worktree - the checkout guard stands - but the worktree it holds is removable once
/// the work has landed, and a re-run then deletes the branch.
pub(crate) fn landed_though_checked_out(branch: &Branch, policy: &Policy) -> Option<Landed> {
    let bare = Branch {
        checkout: None,
        ..branch.clone()
    };
    match decide(&bare, policy) {
        Decision::Delete { because } => Some(because),
        Decision::Keep { .. } => None,
    }
}

/// Everything that stops a delete before any evidence is weighed.
fn guard(branch: &Branch, policy: &Policy) -> Option<Blocker> {
    if branch.name == policy.default_branch || PROTECTED.contains(&branch.name.as_str()) {
        return Some(Blocker::Protected);
    }
    if policy.current_branch.as_deref() == Some(branch.name.as_str()) {
        return Some(Blocker::CurrentBranch);
    }
    // Before the pull-request state is read, because a state nobody could read is not a state.
    // An exhaustive match rather than an `if let`, so a third way of not knowing has to be
    // answered here rather than defaulting to "carry on".
    match &policy.forge {
        Forge::Answered | Forge::Waived => {}
        Forge::Silent { because } => {
            return Some(Blocker::ForgeSilent {
                because: because.clone(),
            });
        }
    }
    if let PullRequest::Open { number } = &branch.pull_request {
        return Some(Blocker::OpenPullRequest { number: *number });
    }
    branch.checkout.as_ref().map(blocked_by_checkout)
}

/// A checkout always blocks. Which reason is reported depends on what the worktree holds.
fn blocked_by_checkout(checkout: &Checkout) -> Blocker {
    match &checkout.state {
        WorkingTree::Dirty => Blocker::UncommittedWork {
            path: checkout.path.clone(),
        },
        WorkingTree::Unreadable { because } => Blocker::WorkingTreeUnreadable {
            path: checkout.path.clone(),
            because: because.clone(),
        },
        WorkingTree::Clean => Blocker::CheckedOut {
            path: checkout.path.clone(),
            locked: checkout.locked,
        },
    }
}

/// The evidence, or the reason there is none.
fn landed(branch: &Branch) -> Result<Landed, Blocker> {
    match &branch.landing {
        Landing::Landed(how) => Ok(Landed::Git(*how)),
        Landing::Unlanded { commits } => forge_or(branch, unlanded(branch, *commits)),
        Landing::Undetermined { because } => forge_or(branch, Blocker::LandingUndetermined { because }),
    }
}

/// The forge's answer, falling back to the reason git gave for not licensing a delete.
///
/// A merged pull request whose head has MOVED is not evidence, and it replaces the git reason
/// rather than deferring to it: "merged, and you have written something since" is the more useful
/// sentence, and it is the case where a careless tool loses a commit.
fn forge_or(branch: &Branch, otherwise: Blocker) -> Result<Landed, Blocker> {
    match &branch.pull_request {
        PullRequest::Merged { number, head } if *head == branch.tip => Ok(Landed::MergedPullRequest { number: *number }),
        PullRequest::Merged { number, head } => Err(Blocker::MergedPullRequestMovedOn {
            number: *number,
            head: head.clone(),
            tip: branch.tip.clone(),
        }),
        PullRequest::Open { .. } | PullRequest::Unknown => Err(otherwise),
    }
}

/// Commits that are nowhere else, named by the sharper of the two available words.
const fn unlanded(branch: &Branch, commits: u32) -> Blocker {
    match branch.upstream {
        Upstream::Present { ahead, .. } if ahead > 0 => Blocker::UnpushedCommits { ahead },
        // Untracked means every commit on it is unpushed by definition, which is the same
        // sentence with a different count.
        Upstream::Untracked => Blocker::UnpushedCommits { ahead: commits },
        Upstream::Gone | Upstream::Present { .. } => Blocker::NotOnDefaultBranch { commits },
    }
}

/// The optional age bound. A missing date is undetermined, so it keeps the branch.
fn too_recent(branch: &Branch, policy: &Policy) -> Option<Blocker> {
    let bound = policy.unused_days?;
    let Some(seconds) = branch.tip_seconds else {
        return Some(Blocker::AgeUnknown { bound });
    };
    let age_days = age_in_days(policy.now_seconds, seconds);
    (age_days < i64::from(bound)).then_some(Blocker::TouchedRecently { age_days, bound })
}

/// Whole days between two epoch seconds, never negative.
#[expect(
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "whole days is what the bound is expressed in, so truncation IS the semantics: a \
              branch touched 23 hours ago is nought days old and the bound reads the same way a \
              person saying `older than two days` reads it"
)]
const fn age_in_days(now_seconds: i64, then_seconds: i64) -> i64 {
    // A tip dated in the FUTURE - a wrong clock somewhere, or a rewritten commit date - has an age
    // of zero rather than a negative one, which is the direction that keeps the branch.
    let elapsed = now_seconds.saturating_sub(then_seconds);
    if elapsed <= 0 {
        return 0;
    }
    elapsed / 86_400
}

/// What a worktree is to this invocation.
///
/// One field rather than two flags, because the two cases it names are not independent - the main
/// worktree is often also the one a person is standing in - and because both mean the same thing to
/// the decision: never remove this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    /// The repository's main worktree.
    Primary,
    /// Where this invocation is running. Removing it would remove the tool's own footing.
    RunningHere,
    /// Neither: an ordinary worktree, and the only kind that can be removed.
    Ordinary,
}

/// A registered worktree, as the decision reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Worktree {
    /// Where it is.
    pub(crate) path: String,
    /// The branch checked out there, or `None` for a detached HEAD.
    pub(crate) branch: Option<String>,
    /// Locked worktrees are left alone, and reported as locked.
    pub(crate) locked: bool,
    /// Is the directory still there? A registration whose directory is gone is the stale case.
    pub(crate) present: bool,
    /// What its working tree holds.
    pub(crate) state: WorkingTree,
    /// What it is to this invocation.
    pub(crate) standing: Standing,
}

/// Why a worktree was left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorktreeBlocker {
    /// The repository's main worktree.
    Primary,
    /// The one this invocation is running in - removing it would remove the tool's own footing.
    RunningHere,
    /// Locked. Somebody said so on purpose.
    Locked,
    /// Uncommitted work.
    UncommittedWork,
    /// Its state could not be read.
    WorkingTreeUnreadable {
        /// What went wrong.
        because: String,
    },
    /// No branch to decide on.
    DetachedHead,
    /// Its branch is not deletable, so the checkout is still wanted.
    BranchKept {
        /// The branch.
        branch: String,
    },
}

/// What to do with one worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorktreeDecision {
    /// `git worktree prune`: the directory is gone and only the registration is left.
    Prune,
    /// `git worktree remove`: its branch has landed and the tree is clean.
    Remove {
        /// The branch it holds.
        branch: String,
    },
    /// Leave it alone.
    Keep {
        /// Why.
        because: WorktreeBlocker,
    },
}

/// May this worktree be removed?
///
/// `landed` is the set of branches whose work is already on the default branch - the branches
/// [`landed_though_checked_out`] answered for. The worktree goes first and the branch follows on a
/// re-run, which keeps "never delete a branch checked out in a worktree" literally true.
pub(crate) fn decide_worktree(worktree: &Worktree, landed: &BTreeSet<String>) -> WorktreeDecision {
    match worktree.standing {
        Standing::Primary => {
            return WorktreeDecision::Keep {
                because: WorktreeBlocker::Primary,
            };
        }
        Standing::RunningHere => {
            return WorktreeDecision::Keep {
                because: WorktreeBlocker::RunningHere,
            };
        }
        Standing::Ordinary => {}
    }
    // Before the missing-directory case: `git worktree prune` skips a locked registration, so
    // reporting it as prunable would promise something git will not do.
    if worktree.locked {
        return WorktreeDecision::Keep {
            because: WorktreeBlocker::Locked,
        };
    }
    if !worktree.present {
        return WorktreeDecision::Prune;
    }
    match &worktree.state {
        WorkingTree::Dirty => {
            return WorktreeDecision::Keep {
                because: WorktreeBlocker::UncommittedWork,
            };
        }
        WorkingTree::Unreadable { because } => {
            return WorktreeDecision::Keep {
                because: WorktreeBlocker::WorkingTreeUnreadable {
                    because: because.clone(),
                },
            };
        }
        WorkingTree::Clean => {}
    }
    match &worktree.branch {
        None => WorktreeDecision::Keep {
            because: WorktreeBlocker::DetachedHead,
        },
        Some(branch) if landed.contains(branch) => WorktreeDecision::Remove { branch: branch.clone() },
        Some(branch) => WorktreeDecision::Keep {
            because: WorktreeBlocker::BranchKept { branch: branch.clone() },
        },
    }
}

#[cfg(test)]
mod tests;
