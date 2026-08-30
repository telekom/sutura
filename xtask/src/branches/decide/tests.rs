//! The decision, asserted without a repository.
//!
//! The first test is the one the whole task exists for: a squash-merged branch, which
//! `git branch --merged` reports as unmerged because none of its commits is an ancestor of
//! anything on the default branch. The fixtures here are the output real git gave for exactly
//! that history - the construction is in the module documentation of `super::super::git`, and
//! `parse_cherry` in that module is what turns it into the `Landing` these tests take as given.

use std::collections::BTreeSet;

use super::{
    Blocker, Branch, Checkout, Decision, Equivalence, Forge, Landed, Landing, Policy, PullRequest, Standing, Upstream,
    WorkingTree, Worktree, WorktreeBlocker, WorktreeDecision, decide, decide_worktree, landed_though_checked_out,
};

/// Seconds in a day, for readable ages.
const DAY: i64 = 86_400;

/// A fixed "now", so an age assertion is not a function of when the suite runs.
const NOW: i64 = 1_800_000_000;

/// The ordinary case: a forge that answered, no age bound, running on the default branch.
fn policy() -> Policy {
    Policy {
        default_branch: String::from("main"),
        current_branch: Some(String::from("main")),
        forge: Forge::Answered,
        now_seconds: NOW,
        unused_days: None,
    }
}

/// A branch with nothing remarkable about it: pushed, in step, not checked out, not landed.
fn branch(name: &str) -> Branch {
    Branch {
        name: String::from(name),
        tip: String::from("aaaa111"),
        upstream: Upstream::Present { ahead: 0, behind: 0 },
        tip_seconds: Some(NOW - (30 * DAY)),
        landing: Landing::Unlanded { commits: 1 },
        pull_request: PullRequest::Unknown,
        checkout: None,
    }
}

fn clean_checkout(path: &str) -> Checkout {
    Checkout {
        path: String::from(path),
        locked: false,
        state: WorkingTree::Clean,
    }
}

#[test]
fn a_squash_merged_branch_is_deleted_although_git_branch_merged_cannot_see_it() {
    // THE test. The history is: one commit on `feat/thing`, squash-merged into `main`, the
    // branch's own commit still not an ancestor of anything. `git branch --merged main` prints
    // `main` alone - the branch is invisible to it - while `git cherry main feat/thing` prints
    // `- <sha>`, a patch-equivalent commit found. That "-" is this `Landing`.
    let mut landed = branch("feat/thing");
    landed.landing = Landing::Landed(Equivalence::PerCommit);
    landed.upstream = Upstream::Gone;

    assert_eq!(
        decide(&landed, &policy()),
        Decision::Delete {
            because: Landed::Git(Equivalence::PerCommit)
        }
    );
}

#[test]
fn a_squash_of_several_commits_needs_the_cumulative_diff_and_is_still_deleted() {
    // The gap in the per-commit signal, and the reason there are two git mechanisms rather than
    // one: three commits squashed into one produce a commit whose patch-id matches none of them,
    // so `git cherry` reports three "+" lines. The cumulative diff of the branch is what matches.
    let mut squashed = branch("feat/three-commits");
    squashed.landing = Landing::Landed(Equivalence::Squashed);

    assert_eq!(
        decide(&squashed, &policy()),
        Decision::Delete {
            because: Landed::Git(Equivalence::Squashed)
        }
    );

    // And with neither git signal, the forge still carries it - as long as the tip is what the
    // forge merged.
    let mut only_the_forge = branch("feat/three-commits");
    only_the_forge.landing = Landing::Unlanded { commits: 3 };
    only_the_forge.pull_request = PullRequest::Merged {
        number: 52,
        head: only_the_forge.tip.clone(),
    };
    assert_eq!(
        decide(&only_the_forge, &policy()),
        Decision::Delete {
            because: Landed::MergedPullRequest { number: 52 }
        }
    );
}

#[test]
fn an_unpushed_commit_is_never_deleted() {
    // The failure this task must not have. Ahead of its upstream, nothing equivalent on the
    // default branch: the commit exists in one place on earth.
    let mut ahead = branch("feat/unpushed");
    ahead.upstream = Upstream::Present { ahead: 2, behind: 0 };
    ahead.landing = Landing::Unlanded { commits: 2 };

    assert_eq!(
        decide(&ahead, &policy()),
        Decision::Keep {
            because: Blocker::UnpushedCommits { ahead: 2 }
        }
    );

    // An upstream that never existed is the same sentence with a different count.
    let mut untracked = branch("feat/never-pushed");
    untracked.upstream = Upstream::Untracked;
    untracked.landing = Landing::Unlanded { commits: 4 };
    assert_eq!(
        decide(&untracked, &policy()),
        Decision::Keep {
            because: Blocker::UnpushedCommits { ahead: 4 }
        }
    );
}

#[test]
fn an_upstream_that_is_gone_is_corroboration_and_never_a_licence() {
    // A forge deletes the remote branch on merge - and so does a person abandoning unmerged
    // work. Alone, it deletes nothing.
    let mut abandoned = branch("feat/abandoned");
    abandoned.upstream = Upstream::Gone;
    abandoned.landing = Landing::Unlanded { commits: 3 };

    assert_eq!(
        decide(&abandoned, &policy()),
        Decision::Keep {
            because: Blocker::NotOnDefaultBranch { commits: 3 }
        }
    );
}

#[test]
fn a_branch_checked_out_in_a_worktree_is_never_deleted_and_the_path_is_reported() {
    // The case a stack tool that tracks only its own branches reports as "nothing to do".
    let mut landed = branch("feat/landed");
    landed.landing = Landing::Landed(Equivalence::PerCommit);
    landed.checkout = Some(clean_checkout("/w/agent-1"));

    assert_eq!(
        decide(&landed, &policy()),
        Decision::Keep {
            because: Blocker::CheckedOut {
                path: String::from("/w/agent-1"),
                locked: false,
            }
        }
    );

    // And the worktree side still knows the work landed, which is what makes the two-step work.
    assert_eq!(
        landed_though_checked_out(&landed, &policy()),
        Some(Landed::Git(Equivalence::PerCommit))
    );
}

#[test]
fn uncommitted_work_is_reported_as_uncommitted_work_rather_than_as_a_checkout() {
    // Two different actions for the reader, so two different reasons.
    let mut dirty = branch("feat/landed");
    dirty.landing = Landing::Landed(Equivalence::PerCommit);
    dirty.checkout = Some(Checkout {
        path: String::from("/w/agent-2"),
        locked: false,
        state: WorkingTree::Dirty,
    });

    assert_eq!(
        decide(&dirty, &policy()),
        Decision::Keep {
            because: Blocker::UncommittedWork {
                path: String::from("/w/agent-2")
            }
        }
    );
    // Fail-safe: a working tree nobody could read is not a clean one, and the branch is kept
    // even though every landing signal says the work is elsewhere.
    let mut unreadable = dirty;
    unreadable.checkout = Some(Checkout {
        path: String::from("/w/agent-2"),
        locked: false,
        state: WorkingTree::Unreadable {
            because: String::from("git status did not run"),
        },
    });
    assert_eq!(
        decide(&unreadable, &policy()),
        Decision::Keep {
            because: Blocker::WorkingTreeUnreadable {
                path: String::from("/w/agent-2"),
                because: String::from("git status did not run"),
            }
        }
    );
    // Lifting the checkout says the WORK landed, and it does not make the worktree removable:
    // that is `decide_worktree`'s question, and it refuses a tree it could not read. The two
    // guards are separate on purpose - a branch is never deleted in the run that removes its
    // worktree, so each has to hold on its own.
    assert_eq!(
        landed_though_checked_out(&unreadable, &policy()),
        Some(Landed::Git(Equivalence::PerCommit))
    );
}

#[test]
fn an_open_pull_request_is_never_deleted() {
    // Even with every git signal saying the work is on the default branch: an open pull request
    // means somebody is still reading it.
    let mut open = branch("feat/in-review");
    open.landing = Landing::Landed(Equivalence::PerCommit);
    open.pull_request = PullRequest::Open { number: 61 };

    assert_eq!(
        decide(&open, &policy()),
        Decision::Keep {
            because: Blocker::OpenPullRequest { number: 61 }
        }
    );
}

#[test]
fn a_silent_forge_deletes_nothing_and_says_which_signal_was_missing() {
    // FAIL SAFE, and the inverse of `classify`, which runs everything when it cannot tell. The
    // open-pull-request guard has no git-only substitute, so this is not degraded to the weaker
    // signal - it is refused, with the reason.
    let mut landed = branch("feat/landed");
    landed.landing = Landing::Landed(Equivalence::PerCommit);
    let silent = Policy {
        forge: Forge::Silent {
            because: String::from("gh is not on PATH"),
        },
        ..policy()
    };

    assert_eq!(
        decide(&landed, &silent),
        Decision::Keep {
            because: Blocker::ForgeSilent {
                because: String::from("gh is not on PATH")
            }
        }
    );

    // A WAIVER is a different thing from a silence, and it is the flag that makes the difference:
    // somebody stated there is no forge to ask, so the guard has nothing to protect.
    let waived = Policy {
        forge: Forge::Waived,
        ..policy()
    };
    assert_eq!(
        decide(&landed, &waived),
        Decision::Delete {
            because: Landed::Git(Equivalence::PerCommit)
        }
    );
}

#[test]
fn a_merged_pull_request_whose_head_moved_on_is_kept() {
    // A pull request merged this morning says nothing about a commit written since. Without the
    // tip comparison this is the shape that loses one.
    let mut moved = branch("feat/kept-working");
    moved.landing = Landing::Unlanded { commits: 1 };
    moved.pull_request = PullRequest::Merged {
        number: 42,
        head: String::from("bbbb222"),
    };

    assert_eq!(
        decide(&moved, &policy()),
        Decision::Keep {
            because: Blocker::MergedPullRequestMovedOn {
                number: 42,
                head: String::from("bbbb222"),
                tip: String::from("aaaa111"),
            }
        }
    );
}

#[test]
fn an_undetermined_landing_signal_is_not_a_landed_one() {
    let mut unknown = branch("feat/unknown");
    unknown.landing = Landing::Undetermined {
        because: "git cherry did not run",
    };

    assert_eq!(
        decide(&unknown, &policy()),
        Decision::Keep {
            because: Blocker::LandingUndetermined {
                because: "git cherry did not run"
            }
        }
    );
}

#[test]
fn the_default_branch_the_current_branch_and_the_long_lived_names_are_protected() {
    let mut default = branch("main");
    default.landing = Landing::Landed(Equivalence::PerCommit);
    assert_eq!(
        decide(&default, &policy()),
        Decision::Keep {
            because: Blocker::Protected
        }
    );

    for name in ["master", "develop", "production", "trunk"] {
        let mut long_lived = branch(name);
        long_lived.landing = Landing::Landed(Equivalence::PerCommit);
        assert_eq!(
            decide(&long_lived, &policy()),
            Decision::Keep {
                because: Blocker::Protected
            },
            "{name} must be protected"
        );
    }

    // A repository whose default branch is not called `main` protects that name too, and the
    // branch this invocation is standing on is reported as itself rather than as protected.
    let mut here = branch("feat/here");
    here.landing = Landing::Landed(Equivalence::PerCommit);
    let standing_on_it = Policy {
        current_branch: Some(String::from("feat/here")),
        ..policy()
    };
    assert_eq!(
        decide(&here, &standing_on_it),
        Decision::Keep {
            because: Blocker::CurrentBranch
        }
    );
}

#[test]
fn the_age_bound_leaves_a_branch_touched_this_morning_alone() {
    let mut fresh = branch("feat/landed-today");
    fresh.landing = Landing::Landed(Equivalence::PerCommit);
    fresh.tip_seconds = Some(NOW - (2 * DAY));
    let bounded = Policy {
        unused_days: Some(7),
        ..policy()
    };

    assert_eq!(
        decide(&fresh, &bounded),
        Decision::Keep {
            because: Blocker::TouchedRecently { age_days: 2, bound: 7 }
        }
    );

    // Past the bound it goes.
    let mut old = fresh.clone();
    old.tip_seconds = Some(NOW - (9 * DAY));
    assert_eq!(
        decide(&old, &bounded),
        Decision::Delete {
            because: Landed::Git(Equivalence::PerCommit)
        }
    );

    // No date to measure against is undetermined, so it keeps.
    let mut undated = fresh;
    undated.tip_seconds = None;
    assert_eq!(
        decide(&undated, &bounded),
        Decision::Keep {
            because: Blocker::AgeUnknown { bound: 7 }
        }
    );
    // With no bound asked for, a missing date decides nothing.
    assert_eq!(
        decide(&undated, &policy()),
        Decision::Delete {
            because: Landed::Git(Equivalence::PerCommit)
        }
    );
}

/// A worktree with nothing remarkable about it.
fn worktree(path: &str, branch: Option<&str>) -> Worktree {
    Worktree {
        path: String::from(path),
        branch: branch.map(String::from),
        locked: false,
        present: true,
        state: WorkingTree::Clean,
        standing: Standing::Ordinary,
    }
}

fn landed_set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

#[test]
fn a_worktree_holding_a_landed_branch_is_removed_and_everything_else_is_reported() {
    let holding = worktree("/w/agent-1", Some("feat/landed"));
    assert_eq!(
        decide_worktree(&holding, &landed_set(&["feat/landed"])),
        WorktreeDecision::Remove {
            branch: String::from("feat/landed")
        }
    );

    // Its branch is not deletable: the checkout is still wanted.
    assert_eq!(
        decide_worktree(&holding, &landed_set(&[])),
        WorktreeDecision::Keep {
            because: WorktreeBlocker::BranchKept {
                branch: String::from("feat/landed")
            }
        }
    );
}

#[test]
fn a_locked_worktree_is_skipped_and_reported_as_locked_even_when_its_directory_is_gone() {
    let mut locked = worktree("/w/agent-3", Some("feat/landed"));
    locked.locked = true;
    assert_eq!(
        decide_worktree(&locked, &landed_set(&["feat/landed"])),
        WorktreeDecision::Keep {
            because: WorktreeBlocker::Locked
        }
    );

    // `git worktree prune` skips a locked registration, so reporting it as prunable would
    // promise something git will not do.
    locked.present = false;
    assert_eq!(
        decide_worktree(&locked, &landed_set(&[])),
        WorktreeDecision::Keep {
            because: WorktreeBlocker::Locked
        }
    );
}

#[test]
fn a_registration_whose_directory_is_gone_is_the_stale_case() {
    let mut gone = worktree("/w/agent-4", Some("feat/landed"));
    gone.present = false;
    assert_eq!(decide_worktree(&gone, &landed_set(&[])), WorktreeDecision::Prune);
}

#[test]
fn the_primary_worktree_the_running_one_and_a_dirty_one_are_never_removed() {
    let mut primary = worktree("/w/main", Some("main"));
    primary.standing = Standing::Primary;
    assert_eq!(
        decide_worktree(&primary, &landed_set(&["main"])),
        WorktreeDecision::Keep {
            because: WorktreeBlocker::Primary
        }
    );

    let mut here = worktree("/w/agent-5", Some("feat/landed"));
    here.standing = Standing::RunningHere;
    assert_eq!(
        decide_worktree(&here, &landed_set(&["feat/landed"])),
        WorktreeDecision::Keep {
            because: WorktreeBlocker::RunningHere
        }
    );

    let mut dirty = worktree("/w/agent-6", Some("feat/landed"));
    dirty.state = WorkingTree::Dirty;
    assert_eq!(
        decide_worktree(&dirty, &landed_set(&["feat/landed"])),
        WorktreeDecision::Keep {
            because: WorktreeBlocker::UncommittedWork
        }
    );

    let mut unreadable = worktree("/w/agent-7", Some("feat/landed"));
    unreadable.state = WorkingTree::Unreadable {
        because: String::from("git status did not run"),
    };
    assert_eq!(
        decide_worktree(&unreadable, &landed_set(&["feat/landed"])),
        WorktreeDecision::Keep {
            because: WorktreeBlocker::WorkingTreeUnreadable {
                because: String::from("git status did not run")
            }
        }
    );

    let detached = worktree("/w/agent-8", None);
    assert_eq!(
        decide_worktree(&detached, &landed_set(&[])),
        WorktreeDecision::Keep {
            because: WorktreeBlocker::DetachedHead
        }
    );
}
