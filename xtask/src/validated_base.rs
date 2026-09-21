//! The classification-base decision that closes #561, and the proof that an evicted run's
//! commits are still covered.
//!
//! THE MODEL AND ITS LIMIT. The shell this module models lives in `ci.yml`'s base-resolution
//! step, and it is read here as prose, not compared mechanically: nothing in the tree checks
//! that the YAML and these functions agree. That gap is where defect D lived - the marker read
//! took the newest artifact by DESCENDING ID (`head -n 1` over an unsorted page) while id does
//! not track creation time, so the shell could resolve an older run's head than the model's
//! assumption, and the widened range it produced then fed `Test causality`, which had no base of
//! its own on push. Causality now derives its own base per venue (`workflows::step::causality`
//! holds that shell), so the model below is consumed by `Classify the change` only; the
//! merge-group limit is stated at `ci.yml`'s base step, and the push arm's is stated beside it.
//!
//! `ci.yml` has a per-ref concurrency group with `cancel-in-progress: false`. GitHub keeps only ONE
//! pending run per group, so a third arrival EVICTS the second - and an evicted run never starts a
//! job, contributes no validation, and its commits fall out of the NEXT run's range if that range
//! begins at `github.event.before` (the immediately preceding push). Nine `main` commits went
//! validated by nobody that way.
//!
//! # THE DECISION
//!
//! Of the three candidate directions in #561 - a durable marker of the last fully-validated
//! commit, narrowing the group off the downstream tail, or a cumulative (non-adjacent) base - the
//! soundest given this repository's gates is the **durable marker**: classify a main push against
//! the last commit a COMPLETED run validated, not against the previous push. That is the only
//! option that makes "validated" well-defined: a marker only ever ADVANCES on a run that actually
//! completed, so whatever sits between the marker and HEAD is, by construction, not yet covered and
//! must be covered by the next completed run. Narrowing the group fixes the latency of the
//! downstream tail without touching the correctness hole, and a cumulative base still depends on a
//! durable record of what ran - it is strictly more machinery for the same semantics, so it is not
//! taken.
//!
//! # THE PROOF (the pair #561 asks for)
//!
//! Simulate the eviction: push A completes (marker = a1), push B is EVICTED before starting
//! (marker stays a1), push C runs. Resolving C's base with the marker gives `a1`, so C's
//! classification range `a1..c1` CONTAINS B's commit b1 - B's work is covered by a later completed
//! run. Resolving C's base with the old `github.event.before` gives `b1`, so `b1..c1` EXCLUDES b1 -
//! B's commit falls out, which is the exact defect.

/// The classification base for a main push: the durable marker of the last validated commit, or
/// the previous push when no marker exists.
///
/// The marker is never newer than HEAD (a completed run's head is an ancestor of any later push),
/// so using it as the base widens the range to cover everything since the marker - including the
/// commits of runs that were evicted. `before` remains the fallback for the first push or when the
/// marker is unreadable, matching the old behaviour rather than widening to nothing.
fn resolve_push_base(marker: Option<&str>, before: &str) -> String {
    match marker {
        Some(m) if !m.is_empty() => String::from(m),
        _ => String::from(before),
    }
}

/// The classification base for a `merge_group` event: `HEAD`'s own parent.
///
/// The opposite correction from [`resolve_push_base`], for the opposite reason: that widens a
/// push's range to cover what an EVICTED run missed, this NARROWS a `merge_group`'s range to the
/// one entry under test. `HEADGREEN` batches several queue entries into ONE tested tree
/// (`max_entries_to_build`), and each entry previews as ONE commit (this ruleset's own
/// `merge_method: SQUASH`) stacked on the entries ahead of it - so `HEAD` in a `merge_group` job is
/// always single-parent, and its parent is exactly "the state this entry is queued on top of
/// right now", regardless of how many OTHER entries share the batch. Using the batch's own
/// merge-base with the default branch instead walks back through every entry ahead of this one
/// too, which is `github.com/telekom/sutura#876`: a claim-cell declaration or an added test from
/// an EARLIER queued entry then reads as part of the diff under test.
fn resolve_merge_group_base(head_parent: &str) -> String {
    String::from(head_parent)
}

/// Is `candidate` inside the classification range that starts at `range_base`?
///
/// `range_base..head` includes `candidate` exactly when the base is an ancestor-or-equal of the
/// candidate; equivalently the candidate is not a strict descendant of the base. Using an older
/// base widens the range, which is what makes an evicted run's commits covered.
fn range_covers<F>(range_base: &str, candidate: &str, descendants_of: &F) -> bool
where
    F: Fn(&str, &str) -> bool,
{
    !descendants_of(range_base, candidate)
}

#[cfg(test)]
mod tests {
    use super::{range_covers, resolve_merge_group_base, resolve_push_base};

    /// An ancestor-or-equal predicate over the synthetic chain a0 -> a1 -> b1 -> c1.
    fn chain_descendant(x: &str, y: &str) -> bool {
        let rank = |c: &str| match c {
            "a0" => 0,
            "a1" => 1,
            "b1" => 2,
            _ => 3,
        };
        rank(y) <= rank(x)
    }

    /// The issue's required pair, over a synthetic commit graph.
    ///
    /// a0 - A - a1 - B - b1 - C - c1, where A completes, B is EVICTED before any job starts, and C
    /// is the next run that has to cover B's evicted commit b1.
    #[test]
    fn an_evicted_runs_commit_is_covered_by_a_later_completed_run() {
        const BEFORE_C: &str = "b1"; // github.event.before for push C = previous push (B's head)
        let marker_after_a = Some("a1"); // the durable marker after A COMPLETED; B's eviction left it put

        // WITH THE MARKER: C's base is a1, so a1..c1 includes b1 (B's commit) -> covered.
        let base_with_marker = resolve_push_base(marker_after_a, BEFORE_C);
        assert_eq!(base_with_marker, "a1");
        assert!(
            range_covers(&base_with_marker, "b1", &chain_descendant),
            "with the marker, C's range must cover B's evicted commit b1"
        );

        // WITHOUT THE MARKER (old `github.event.before`): C's base is b1, so b1..c1 EXCLUDES the
        // commit b1 itself -> B's commit falls out of every range a completed run considers.
        let base_before = resolve_push_base(None, BEFORE_C);
        assert_eq!(base_before, "b1");
        assert!(
            !range_covers(&base_before, "b1", &chain_descendant),
            "with the old before-base, B's evicted commit b1 falls out of C's range - the defect"
        );
    }

    /// No marker -> the previous push, so a first run on a fresh branch with no completed
    /// predecessor still classifies its own range; an empty marker is treated as absent.
    ///
    /// The name promises only "the base never reaches past the given marker" - the one property
    /// `resolve_push_base` can hold without knowing the graph - so the test stays at that: it
    /// does NOT assert the marker is newer than `before`, because the module has no mechanism
    /// comparing the shell's artifact ordering with this model. The shell's own gate for the
    /// ordering is `workflows::step::causality`'s fixture plus `Classify the change`'s log.
    #[test]
    fn an_empty_marker_falls_back_to_the_previous_push() {
        assert_eq!(resolve_push_base(None, "a0"), "a0");
        assert_eq!(resolve_push_base(Some(""), "a0"), "a0");
    }

    /// An ancestor-or-equal predicate over a synthetic THREE-ENTRY `merge_group` batch: main's tip
    /// `m`, stacked with `e1`, `e2`, `e3` (the entry actually under test, tested WITH the two
    /// entries ahead of it already applied - the shape `#876` measured).
    fn batch_descendant(x: &str, y: &str) -> bool {
        let rank = |c: &str| match c {
            "m" => 0,
            "e1" => 1,
            "e2" => 2,
            _ => 3, // e3
        };
        rank(y) <= rank(x)
    }

    /// `#876`'s required pair, over the same batch: the WRONG base (this branch's merge-base with
    /// the default branch) covers every entry ahead of the one under test; the RIGHT base
    /// (`HEAD`'s own parent) covers only that entry.
    #[test]
    fn the_merge_group_base_covers_only_the_entry_under_test() {
        // THE DEFECT: `git merge-base origin/main HEAD` resolves to `m` here, so `m..e3` covers
        // `e1` and `e2` too - an earlier queued entry's own commit reads as part of the diff
        // under test.
        assert!(
            range_covers("m", "e1", &batch_descendant),
            "the wrong base covers the entry ahead"
        );
        assert!(
            range_covers("m", "e2", &batch_descendant),
            "the wrong base covers the entry ahead"
        );

        // THE FIX: `HEAD`'s own parent is `e2`, so `resolve_merge_group_base` names it, and
        // `e2..e3` covers NEITHER entry ahead - only `e3`, the one this job is testing.
        let base = resolve_merge_group_base("e2");
        assert_eq!(base, "e2");
        assert!(
            !range_covers(&base, "e1", &batch_descendant),
            "the right base must not cover an entry two steps ahead"
        );
        assert!(
            !range_covers(&base, "e2", &batch_descendant),
            "the right base must not cover the entry immediately ahead - only e2..e3 remains"
        );
        assert!(
            range_covers(&base, "e3", &batch_descendant),
            "the entry actually under test must still be covered"
        );
    }
}
