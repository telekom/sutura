//! [`decide`]'s, [`Granted::new`]'s and [`Grantee::grant`](crate::plan::Grantee::grant)'s cells,
//! apart from their implementation in `split.rs`.

use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use core::time::Duration;
use std::sync::Arc;
use std::time::SystemTime;

use arrow_schema::Schema;

use crate::identity::SubjectKey;
use crate::nonempty::NonEmpty;
use crate::plan::{EstimatedRows, GrantExpired, Granted, SplitDecision, decide};
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

/// A grant ending at the moment it is made, or before, is refused; one ending a second later is not.
#[test]
fn a_grant_ending_at_or_before_now_is_refused_as_expired() {
    let now = SystemTime::now();
    let grant = |until| {
        Granted::new(
            Vec::new(),
            Arc::new(Schema::empty()),
            NonEmpty::one(vec![0]),
            SubjectKey::parse("user@example.com").expect("a test caller is a subject"),
            until,
            now,
            NonZeroU32::MIN,
            EstimatedBytes::parse(0),
            (),
        )
    };

    for until in [now, now - Duration::from_secs(1)] {
        let refused = grant(until);
        assert!(
            matches!(refused, Err(GrantExpired { .. })),
            "a grant ending at or before now was made: {refused:?}"
        );
    }
    assert!(
        grant(now + Duration::from_secs(1)).is_ok(),
        "a grant ending after now was refused"
    );
}

/// A lookup leg of `k` on `paged`, as a split reads it.
fn lookup_leg() -> crate::plan::LegPlan {
    use crate::model::{ColumnName, DimensionName, MetricName, SourceName, TableName};
    use crate::plan::{PlanBindings, PlanColumn, PlanKey, ResultLabel};

    let table = TableName::parse("paged").expect("a test name is a name");
    crate::plan::LegPlan::Lookup {
        source: SourceName::parse("geo").expect("a test source is a source"),
        metric: crate::pinned::MetricDigest::new(
            MetricName::parse("amount_total").expect("a test name is a name"),
            crate::definitions::DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: table.clone().into(),
        keys: vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("k").expect("a test name is a name")),
            PlanColumn::new(table, ColumnName::parse("k").expect("a test name is a name")),
        )],
        bindings: PlanBindings::none(),
    }
}

fn asker() -> SubjectKey {
    SubjectKey::parse("user@example.com").expect("a test caller is a subject")
}

#[test]
fn a_grant_carries_the_asking_caller_the_leg_its_readers_re_certify_and_each_partition_in_order() {
    use crate::plan::remote::{PartitionLeg, to_wire};

    let now = SystemTime::now();
    let until = now + Duration::from_secs(30);
    let leg = lookup_leg();
    let granted = crate::plan::Grantee::new(&leg, Some(asker()), until, now)
        .expect("a lookup leg on its own columns encodes")
        .grant(
            Arc::new(Schema::empty()),
            [Some(vec![1]), Some(vec![2])],
            NonZeroU32::MIN,
            EstimatedBytes::parse(7),
            (),
        )
        .expect("a verified caller's portable partitions are granted");

    assert_eq!(granted.caller(), &asker());
    assert_eq!(granted.until(), until);
    assert_eq!(
        PartitionLeg::parse(granted.wire()),
        Some(PartitionLeg::new(
            to_wire(&leg).expect("the leg sends"),
            leg.params().to_vec()
        )),
        "the wire is the leg a reading worker re-certifies, with the values it binds"
    );
    assert_eq!(
        granted.partitions().iter().cloned().collect::<Vec<_>>(),
        vec![vec![1], vec![2]]
    );
}

#[test]
fn a_grant_for_a_question_with_no_verified_caller_is_refused() {
    let now = SystemTime::now();
    let refused = crate::plan::Grantee::new(&lookup_leg(), None, now + Duration::from_secs(30), now)
        .expect("a lookup leg on its own columns encodes")
        .grant(
            Arc::new(Schema::empty()),
            [Some(vec![1])],
            NonZeroU32::MIN,
            EstimatedBytes::parse(0),
            (),
        );
    assert!(matches!(refused, Err(crate::plan::NotGranted::NoCaller)), "{refused:?}");
}

#[test]
fn a_grant_of_a_partition_only_this_node_can_read_is_refused() {
    let now = SystemTime::now();
    let grantee = || {
        crate::plan::Grantee::new(&lookup_leg(), Some(asker()), now + Duration::from_secs(30), now)
            .expect("a lookup leg on its own columns encodes")
    };
    for partitions in [vec![Some(vec![1]), None], Vec::new()] {
        let refused = grantee().grant(
            Arc::new(Schema::empty()),
            partitions,
            NonZeroU32::MIN,
            EstimatedBytes::parse(0),
            (),
        );
        assert!(matches!(refused, Err(crate::plan::NotGranted::NotPortable)), "{refused:?}");
    }
}

#[test]
fn a_grant_for_a_question_already_over_is_refused_as_expired() {
    let now = SystemTime::now();
    let refused = crate::plan::Grantee::new(&lookup_leg(), Some(asker()), now, now)
        .expect("a lookup leg on its own columns encodes")
        .grant(
            Arc::new(Schema::empty()),
            [Some(vec![1])],
            NonZeroU32::MIN,
            EstimatedBytes::parse(0),
            (),
        );
    assert!(matches!(refused, Err(crate::plan::NotGranted::Expired(_))), "{refused:?}");
}
