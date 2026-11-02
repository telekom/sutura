//! [`decide`]'s cells, apart from its implementation in `split.rs`.

use core::num::{NonZeroU64, NonZeroUsize};

use crate::plan::{EstimatedRows, SplitDecision, decide};
use crate::warehouse::estimate::EstimatedBytes;

fn nz_u64(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("a literal fixture is never zero")
}

fn nz_usize(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("a literal fixture is never zero")
}

#[test]
fn an_estimate_at_or_under_the_threshold_reads_whole() {
    let above_bytes = nz_u64(1_000);
    assert_eq!(
        decide(
            EstimatedBytes::parse(1_000),
            EstimatedRows::parse(10_000_000),
            above_bytes,
            nz_usize(100),
            nz_usize(4),
        ),
        SplitDecision::Whole,
        "exactly at the threshold still reads whole"
    );
    assert_eq!(
        decide(
            EstimatedBytes::parse(999),
            EstimatedRows::parse(10_000_000),
            above_bytes,
            nz_usize(100),
            nz_usize(4),
        ),
        SplitDecision::Whole
    );
}

#[test]
fn an_estimate_over_the_threshold_splits_into_the_fewest_partitions_under_the_bound() {
    // 1_000 rows at a 300-row bound need 4 partitions (ceil(1000/300) == 4); 3 would leave one
    // partition at 334 rows, over the bound.
    let decision = decide(
        EstimatedBytes::parse(2_000),
        EstimatedRows::parse(1_000),
        nz_u64(1_000),
        nz_usize(300),
        nz_usize(16),
    );
    assert_eq!(decision, SplitDecision::Split(nz_usize(4)));
}

#[test]
fn the_partition_count_is_capped_at_the_ready_workers_when_that_is_exactly_what_the_bound_needs() {
    // 500 rows at a 125-row bound need exactly 4 partitions (ceil(500/125) == 4), and exactly 4
    // workers are ready: the cap is reached, not exceeded.
    let decision = decide(
        EstimatedBytes::parse(2_000),
        EstimatedRows::parse(500),
        nz_u64(1_000),
        nz_usize(125),
        nz_usize(4),
    );
    assert_eq!(decision, SplitDecision::Split(nz_usize(4)));
}

#[test]
fn one_worker_short_of_what_the_bound_needs_is_refused_not_split_short() {
    // The same 500 rows and 125-row bound still need 4 partitions; only 3 workers are ready, and
    // 500 rows split 3 ways (167 each, rounding up) is still over the bound.
    let decision = decide(
        EstimatedBytes::parse(2_000),
        EstimatedRows::parse(500),
        nz_u64(1_000),
        nz_usize(125),
        nz_usize(3),
    );
    let SplitDecision::Refused(refusal) = decision else {
        panic!("expected a refusal: {decision:?}");
    };
    assert_eq!(refusal.rows(), EstimatedRows::parse(500));
    assert_eq!(refusal.per_stream_bound(), nz_usize(125));
    assert_eq!(refusal.ready_workers(), nz_usize(3));
}

#[test]
fn a_zero_or_unknown_estimate_reads_whole() {
    // A source with no statistics yet (PostgreSQL's `reltuples = -1`, `relpages = 0`) is measured
    // as zero bytes and zero rows by its caller, which is at or under every threshold.
    let decision = decide(
        EstimatedBytes::parse(0),
        EstimatedRows::parse(0),
        nz_u64(1),
        nz_usize(100),
        nz_usize(4),
    );
    assert_eq!(decision, SplitDecision::Whole);
}

#[test]
fn arithmetic_at_the_widest_values_does_not_panic() {
    let decision = decide(
        EstimatedBytes::parse(u64::MAX),
        EstimatedRows::parse(u64::MAX),
        nz_u64(1),
        nz_usize(2),
        nz_usize(1),
    );
    // u64::MAX rows at a bound of two need `2^63` partitions - far more than the one ready
    // worker, so this is refused rather than claiming a partition count nothing could hold.
    assert!(matches!(decision, SplitDecision::Refused(_)), "{decision:?}");

    let decision = decide(
        EstimatedBytes::parse(u64::MAX),
        EstimatedRows::parse(u64::MAX),
        nz_u64(1),
        nz_usize(usize::MAX),
        nz_usize(1),
    );
    assert_eq!(
        decision,
        SplitDecision::Split(nz_usize(1)),
        "one partition of u64::MAX rows sits under a usize::MAX-row bound"
    );
}
