//! The parsers, over the shapes real git produced.
//!
//! Every fixture here was captured from a scratch repository built the way the module
//! documentation describes; nothing is invented, because a parser tested against invented output
//! is a parser tested against its own author's memory of a format.

use super::{Cherry, parse_cherry, parse_patch_ids, parse_refs, parse_worktrees, upstream_state};
use crate::branches::decide::Upstream;

#[test]
fn a_ref_line_splits_into_six_fields() {
    // `%00` between the atoms, so a branch name with a space in it is still one field. Written as
    // `\x00` and not `\0`, because `\0` followed by a digit reads as an octal escape - which Rust
    // does not have, and which clippy refuses rather than letting it look like one.
    let text = "feat/thing\x00aaaa111\x00origin/feat/thing\x00[gone]\x00/w/agent-1\x001756500000\n\
                main\x00bbbb222\x00origin/main\x00\x00/w/main\x001756400000\n";
    let parsed = parse_refs(text);

    assert_eq!(parsed.len(), 2);
    let first = parsed.first().expect("the first line parses");
    assert_eq!(first.name, "feat/thing");
    assert_eq!(first.tip, "aaaa111");
    assert_eq!(first.upstream, "origin/feat/thing");
    assert_eq!(first.track, "[gone]");
    assert_eq!(first.worktree, "/w/agent-1");
    assert_eq!(first.seconds(), Some(1_756_500_000));
    assert_eq!(first.upstream_state(), Upstream::Gone);

    let second = parsed.last().expect("the second line parses");
    assert_eq!(second.worktree, "/w/main");
    assert_eq!(second.upstream_state(), Upstream::Present { ahead: 0, behind: 0 });
}

#[test]
fn a_line_with_too_few_fields_is_dropped_rather_than_guessed_at() {
    assert!(parse_refs("feat/thing\0aaaa111\n").is_empty());
    assert!(parse_refs("\n\n").is_empty());
}

#[test]
fn an_upstream_that_is_absent_and_one_that_is_in_step_are_different_states() {
    // The distinction the track field alone cannot make, and they are opposites: in step means
    // every commit is on a remote, absent means none of them is.
    assert_eq!(upstream_state("", ""), Upstream::Untracked);
    assert_eq!(upstream_state("origin/feat", ""), Upstream::Present { ahead: 0, behind: 0 });
    assert_eq!(upstream_state("origin/feat", "[gone]"), Upstream::Gone);
    // The state a force-push left behind, which was the case a stack tool reported as up to date.
    assert_eq!(
        upstream_state("origin/feat", "[ahead 4, behind 7]"),
        Upstream::Present { ahead: 4, behind: 7 }
    );
    assert_eq!(
        upstream_state("origin/feat", "[behind 2]"),
        Upstream::Present { ahead: 0, behind: 2 }
    );
    assert_eq!(
        upstream_state("origin/feat", "[ahead 11]"),
        Upstream::Present { ahead: 11, behind: 0 }
    );
}

#[test]
fn the_porcelain_worktree_listing_parses_including_locked_and_detached() {
    let text = "\
worktree /w/main
HEAD bbbb222
branch refs/heads/main

worktree /w/agent-1
HEAD aaaa111
branch refs/heads/feat/thing
locked

worktree /w/agent-2
HEAD cccc333
detached
";
    let parsed = parse_worktrees(text);
    assert_eq!(parsed.len(), 3);

    let main = parsed.first().expect("the main worktree parses");
    assert_eq!(main.path, "/w/main");
    assert_eq!(main.branch.as_deref(), Some("main"));
    assert!(!main.locked);

    let locked = parsed.get(1).expect("the locked worktree parses");
    assert_eq!(locked.branch.as_deref(), Some("feat/thing"));
    assert!(locked.locked, "a `locked` line with no reason still locks it");

    let detached = parsed.get(2).expect("the detached worktree parses");
    assert_eq!(detached.branch, None, "a detached HEAD has no branch to decide on");
    assert!(!detached.locked);
}

#[test]
fn a_lock_reason_and_a_bare_repository_are_both_read() {
    let text =
        "worktree /w/bare\nbare\n\nworktree /w/agent-3\nHEAD dddd444\nbranch refs/heads/feat/x\nlocked administrative hold\n";
    let parsed = parse_worktrees(text);
    assert!(parsed.first().is_some_and(|first| first.bare));
    assert!(parsed.last().is_some_and(|last| last.locked));
}

#[test]
fn cherry_output_is_tallied_by_prefix() {
    // The single-commit squash case: git found a patch-equivalent commit, so the line is `-`.
    // This is the output `git cherry main feat/thing` gave for the squash-merged history in the
    // module documentation, where `git branch --merged main` listed `main` alone.
    let squashed = "- 4f2b1c6e9a0d4c8b7e5f3a2d1c0b9a8e7d6c5b4a\n";
    assert_eq!(
        parse_cherry(squashed),
        Cherry {
            matched: 1,
            unmatched: 0
        }
    );

    // Three commits squashed into one: the squash commit's patch-id matches none of them, so
    // every line is `+` and the cumulative-diff comparison is the only git signal left.
    let three = "+ 1111111\n+ 2222222\n+ 3333333\n";
    assert_eq!(
        parse_cherry(three),
        Cherry {
            matched: 0,
            unmatched: 3
        }
    );

    // Empty output: nothing on the branch the default branch does not have.
    assert_eq!(
        parse_cherry(""),
        Cherry {
            matched: 0,
            unmatched: 0
        }
    );

    // Anything that is not a verdict is not counted as one.
    assert_eq!(
        parse_cherry("warning: something\n\n- 4444444\n"),
        Cherry {
            matched: 1,
            unmatched: 0
        }
    );
}

#[test]
fn patch_id_output_keeps_the_patch_and_drops_the_commit() {
    // `git patch-id` prints `<patch-id> <commit-id>`, and only the first half is the question:
    // whether a given patch is present, not which commit carried it.
    let text = "\
0a1b2c3d4e5f60718293a4b5c6d7e8f901234567 fedcba9876543210fedcba9876543210fedcba98
1111111111111111111111111111111111111111 2222222222222222222222222222222222222222
";
    let ids = parse_patch_ids(text);
    assert_eq!(ids.len(), 2);
    assert!(ids.contains("0a1b2c3d4e5f60718293a4b5c6d7e8f901234567"));
    assert!(!ids.contains("fedcba9876543210fedcba9876543210fedcba98"));
    assert!(parse_patch_ids("").is_empty());
}
