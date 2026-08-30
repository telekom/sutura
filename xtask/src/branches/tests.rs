//! The flags, and what a run would actually do.
//!
//! The assertion worth having here is that **a dry run produces no actions at all** - not that a
//! code path is not taken, but that the list of things to run is empty, which is a value rather
//! than a promise.

use std::collections::BTreeMap;

use super::decide::{
    Branch, Decision, Equivalence, Forge, Landed, Landing, PullRequest, Standing, Upstream, WorkingTree, Worktree,
    WorktreeBlocker, WorktreeDecision,
};
use super::{Action, Mode, Options, Plan, actions, options};

fn plan() -> Plan {
    let landed = Branch {
        name: String::from("feat/landed"),
        tip: String::from("aaaa111"),
        upstream: Upstream::Gone,
        tip_seconds: Some(1_756_000_000),
        landing: Landing::Landed(Equivalence::Squashed),
        pull_request: PullRequest::Unknown,
        checkout: None,
    };
    let kept = Branch {
        name: String::from("feat/unpushed"),
        upstream: Upstream::Present { ahead: 2, behind: 0 },
        landing: Landing::Unlanded { commits: 2 },
        ..landed.clone()
    };
    let holding = Worktree {
        path: String::from("/w/agent-1"),
        branch: Some(String::from("feat/landed")),
        locked: false,
        present: true,
        state: WorkingTree::Clean,
        standing: Standing::Ordinary,
    };
    let stale = Worktree {
        path: String::from("/w/agent-9"),
        present: false,
        ..holding.clone()
    };
    let main = Worktree {
        path: String::from("/w/main"),
        branch: Some(String::from("main")),
        standing: Standing::Primary,
        ..holding.clone()
    };

    Plan {
        base: super::git::Base {
            rev: String::from("origin/main"),
            name: String::from("main"),
        },
        pulls: super::forge::Pulls {
            forge: Forge::Answered,
            by_branch: BTreeMap::new(),
        },
        branches: vec![
            (
                landed,
                Decision::Delete {
                    because: Landed::Git(Equivalence::Squashed),
                },
            ),
            (
                kept,
                Decision::Keep {
                    because: super::Blocker::UnpushedCommits { ahead: 2 },
                },
            ),
        ],
        worktrees: vec![
            (
                holding,
                WorktreeDecision::Remove {
                    branch: String::from("feat/landed"),
                },
            ),
            (stale, WorktreeDecision::Prune),
            (
                main,
                WorktreeDecision::Keep {
                    because: WorktreeBlocker::Primary,
                },
            ),
        ],
    }
}

#[test]
fn a_dry_run_produces_no_actions_at_all() {
    // Dry run is the default and it is not a code path that happens not to delete: there is
    // nothing in the list to run.
    assert_eq!(actions(&plan(), Mode::DryRun), Vec::<Action>::new());
}

#[test]
fn a_delete_run_removes_the_worktree_prunes_and_then_deletes_the_branch() {
    // The order matters: a worktree still holding a branch makes `git branch -D` fail, and a
    // stale registration has to go before a path can be reused.
    assert_eq!(
        actions(&plan(), Mode::Delete),
        vec![
            Action::RemoveWorktree(String::from("/w/agent-1")),
            Action::PruneWorktrees,
            Action::DeleteBranch(String::from("feat/landed")),
        ]
    );
}

#[test]
fn the_default_mode_is_a_dry_run_and_only_delete_changes_it() {
    let bare = options(&[]).expect("no arguments is valid");
    assert_eq!(
        bare,
        Options {
            mode: Mode::DryRun,
            unused_days: None,
            fetch: false,
            without_forge: false,
            repo: None,
        }
    );
    let asked = options(&[String::from("--delete")]).expect("--delete is valid");
    assert_eq!(asked.mode, Mode::Delete);
}

#[test]
fn the_flags_parse_and_anything_unrecognised_is_a_usage_error() {
    let full = options(&[
        String::from("--delete"),
        String::from("--fetch"),
        String::from("--without-forge"),
        String::from("--unused-days"),
        String::from("14"),
        String::from("--repo"),
        String::from("/w/elsewhere"),
    ])
    .expect("the full form is valid");
    assert_eq!(full.unused_days, Some(14));
    assert!(full.fetch);
    assert!(full.without_forge);
    assert_eq!(full.repo.as_deref(), Some(std::path::Path::new("/w/elsewhere")));

    // A misspelling must not be read as a default. `--force` in particular: there is no flag that
    // overrides a refusal, so accepting it silently would promise something this task never does.
    drop(options(&[String::from("--force")]).unwrap_err());
    drop(options(&[String::from("--unused-days")]).unwrap_err());
    drop(options(&[String::from("--unused-days"), String::from("soon")]).unwrap_err());
    drop(options(&[String::from("--repo")]).unwrap_err());
}
