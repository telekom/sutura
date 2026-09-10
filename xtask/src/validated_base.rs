//! The classification-base decision that closes #561, and the proof that an evicted run's
//! commits are still covered.
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
    use super::{range_covers, resolve_push_base};

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

    #[test]
    fn a_marker_never_advances_past_head() {
        // No marker -> the previous push, so a first run on a fresh branch with no completed
        // predecessor still classifies its own range; an empty marker is treated as absent.
        assert_eq!(resolve_push_base(None, "a0"), "a0");
        assert_eq!(resolve_push_base(Some(""), "a0"), "a0");
    }
}
