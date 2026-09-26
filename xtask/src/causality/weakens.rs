//! `Weakens-Test: <test-fn-name> - <reason>` - the escape hatch for a deletion the author judges
//! safe, filed against `github.com/telekom/sutura#1031`'s review.
//!
//! `Plan::DeletedTests` is a hard `Verdict::Fail` with no route to a pass: a routine breaking
//! change that removes a field together with its `assert_eq!` line stays red forever, and "state
//! the evidence in the handoff" names no mechanism that clears it. This mirrors `Claim-Cell:`'s
//! shape (`super::claim::Claim::of`) rather than inventing one: a trailer is a CLAIM this module
//! CHECKS against `super::edited::removed_in`'s own answer, never a permission - an unrecognised
//! name waives nothing, and the record lives in `git log` rather than in a silent override.
//!
//! **Named, not blanket.** `Weakens-Test: t - reason` waives ONLY the test named `t`; a deletion
//! this diff also made to a DIFFERENT test, `u`, still refuses even when the same commit carries a
//! trailer for `t`. A trailer naming a test this diff never deleted waives nothing (and is not an
//! error either - the same asymmetry `Claim-Cell:`'s reverse direction already accepts).
//!
//! **Root's default, owner may veto.** This is the ESCAPE HATCH the review asked for; if the
//! project owner decides a legitimate removal should go through some other route (a maintainer
//! override, a separate revert-tracking issue), this mechanism is the one to remove or replace -
//! it is additive and nothing else depends on it existing.

use std::collections::BTreeMap;

const TRAILER: &str = "Weakens-Test:";

/// What the range's commit messages declared: a deleted test's name to the stated reason.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Waived(BTreeMap<String, String>);

impl Waived {
    /// Read every `Weakens-Test:` trailer out of `log` (`base..HEAD`'s NUL-delimited messages,
    /// [`super::worktree::messages`]'s own format - `<hash>\0<body>\0..`). A malformed trailer -
    /// no `-`, an empty name or an empty reason - is not a claim: requiring both is what stops a
    /// bare `Weakens-Test: t` riding through with no stated reason at all.
    ///
    /// SPLIT PER COMMIT FIRST, the same way [`super::claim::Claim::of`] does: a naive
    /// `log.lines()` over the whole stream reads a commit's HASH glued to the first line of its
    /// own body (`worktree::commit_logs`'s own doc states the format), and a trailer that happened
    /// to be that first line would never match with the hash prefixing it.
    pub(super) fn of(log: &str) -> Self {
        let mut map = BTreeMap::new();
        for (_, message) in crate::causality::worktree::commit_logs(log) {
            for line in message.lines() {
                let Some(rest) = line.trim().strip_prefix(TRAILER) else {
                    continue;
                };
                let Some((name, reason)) = rest.trim().split_once('-') else {
                    continue;
                };
                let (name, reason) = (name.trim(), reason.trim());
                if name.is_empty() || reason.is_empty() {
                    continue;
                }
                map.insert(String::from(name), String::from(reason));
            }
        }
        Self(map)
    }

    /// The stated reason, if `name` was waived.
    pub(super) fn reason(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::Waived;

    /// One commit's entry in `worktree::messages`'s own NUL-delimited format, `<hash>\0<body>\0`.
    fn commit(hash: &str, body: &str) -> String {
        format!("{hash}\u{0}{body}\u{0}")
    }

    #[test]
    fn a_well_formed_trailer_waives_the_test_it_names() {
        let log = commit(
            "abc123",
            "fix: drop a stale check\n\nWeakens-Test: an_old_pin - the field it asserted was removed\n",
        );
        let waived = Waived::of(&log);
        assert_eq!(waived.reason("an_old_pin"), Some("the field it asserted was removed"));
        assert_eq!(waived.reason("a_different_test"), None);
    }

    #[test]
    fn a_trailer_with_no_reason_waives_nothing() {
        let waived = Waived::of(&commit("abc123", "Weakens-Test: an_old_pin\n"));
        assert_eq!(waived.reason("an_old_pin"), None);
    }

    #[test]
    fn a_trailer_with_no_name_waives_nothing() {
        let waived = Waived::of(&commit("abc123", "Weakens-Test:  - no name before the dash\n"));
        assert_eq!(waived.reason(""), None);
    }

    #[test]
    fn two_trailers_across_the_range_both_apply() {
        let mut log = commit("abc123", "Weakens-Test: first - reason one\n");
        log.push_str(&commit("def456", "Weakens-Test: second - reason two\n"));
        let waived = Waived::of(&log);
        assert_eq!(waived.reason("first"), Some("reason one"));
        assert_eq!(waived.reason("second"), Some("reason two"));
    }

    #[test]
    fn a_trailer_glued_to_the_commit_hash_by_the_nul_separator_is_not_read_as_the_hash() {
        // The bug a naive `log.lines()` over the WHOLE stream has: the hash and the body's first
        // line share one physical line, `<hash>\0Weakens-Test: t - reason`, so a `.lines()` scan
        // that never splits per commit first sees `abc123\u{0}Weakens-Test: t - reason` and
        // matches nothing. Splitting through `worktree::commit_logs` first is what fixes it.
        let log = commit("abc123", "Weakens-Test: t - the trailer is the body's first line");
        let waived = Waived::of(&log);
        assert_eq!(waived.reason("t"), Some("the trailer is the body's first line"));
    }
}
