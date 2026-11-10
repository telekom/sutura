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

/// A lookup leg of `k` on `paged` where `k` is the one value it binds, as a split reads it.
fn lookup_leg() -> crate::plan::LegPlan {
    use crate::model::{ColumnName, DimensionName, MetricName, TableName};
    use crate::plan::{PlanBindings, PlanColumn, PlanFilter, PlanKey, PlanPredicate, PredicateOrigin, ResultLabel};
    use crate::warehouse::ParamValue;

    let table = TableName::parse("paged").expect("a test name is a name");
    let k = PlanColumn::new(table.clone(), ColumnName::parse("k").expect("a test name is a name"));
    crate::plan::LegPlan::Lookup {
        source: geo(),
        metric: crate::pinned::MetricDigest::new(
            MetricName::parse("amount_total").expect("a test name is a name"),
            crate::definitions::DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: table.into(),
        keys: vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("k").expect("a test name is a name")),
            k.clone(),
        )],
        bindings: PlanBindings::parse(
            vec![PlanFilter::new(
                PredicateOrigin::Requested,
                PlanPredicate::Equals { column: k, param: 0 },
            )],
            vec![ParamValue::Text(String::from("north"))],
        )
        .expect("one filter binding one value is coherent"),
    }
}

fn geo() -> crate::model::SourceName {
    crate::model::SourceName::parse("geo").expect("a test source is a source")
}

/// The request's own grant, asked by `subject` and usable until `not_after`.
fn asked_by(subject: &crate::identity::Subject, not_after: crate::identity::Expiry) -> crate::identity::BoundToTheRequest {
    use crate::identity::{Agreed, LegCredentials, Minted, Presented, Secret, SourceSet};

    let sources = SourceSet::of(geo());
    let presented = std::collections::BTreeMap::from([(
        geo(),
        Presented::SubjectToken {
            material: Secret::new("a-test-token"),
        },
    )]);
    let credentials =
        LegCredentials::minted(subject.clone(), not_after, &sources, presented).expect("one leg covers its one source");
    let Ok(Agreed::Granted { credentials }) = (Minted::Granted { credentials }).agreeing_with(subject, &sources, 0) else {
        panic!("a fixture grant agrees with its own request");
    };
    credentials
}

fn someone() -> crate::identity::Subject {
    crate::identity::Subject::verified("user@example.com").expect("a test caller is a subject")
}

fn grantee(question_ends: SystemTime) -> crate::plan::Grantee {
    crate::plan::Grantee::new(
        &lookup_leg(),
        &asked_by(&someone(), crate::identity::Expiry::NothingExpires),
        question_ends,
    )
    .expect("a verified caller's lookup leg on its own columns is offered")
}

#[test]
fn a_grant_carries_the_asking_caller_the_leg_its_readers_re_certify_and_each_partition_in_order() {
    use crate::plan::remote::{PartitionLeg, to_wire};

    let now = SystemTime::now();
    let until = now + Duration::from_secs(30);
    let leg = lookup_leg();
    let granted = grantee(until)
        .grant(
            Arc::new(Schema::empty()),
            [Some(vec![1]), Some(vec![2])],
            NonZeroU32::MIN,
            EstimatedBytes::parse(7),
            (),
            now,
        )
        .expect("a verified caller's portable partitions are granted");

    assert_eq!(
        granted.caller(),
        &SubjectKey::parse("user@example.com").expect("a test caller is a subject")
    );
    assert_eq!(granted.until(), until);
    assert_eq!(leg.params().len(), 1, "the leg binds one value");
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
fn a_grant_ends_with_the_request_s_own_grant_when_that_ends_before_the_question() {
    let now = SystemTime::now();
    let expires = now + Duration::from_secs(5);
    let not_after = crate::identity::Expiry::At {
        unix_seconds: expires
            .duration_since(std::time::UNIX_EPOCH)
            .expect("now is after the epoch")
            .as_secs(),
    };
    let offered = crate::plan::Grantee::new(&lookup_leg(), &asked_by(&someone(), not_after), now + Duration::from_secs(30))
        .expect("a verified caller's lookup leg is offered");
    assert!(
        offered.until() <= expires,
        "{:?} outlives the request's grant",
        offered.until()
    );
    assert!(offered.until() > now, "the request's grant has not ended yet");
}

#[test]
fn a_question_with_no_verified_caller_is_offered_no_grant() {
    let deployment = crate::identity::Subject::TheDeploymentItself;
    let refused = crate::plan::Grantee::new(
        &lookup_leg(),
        &asked_by(&deployment, crate::identity::Expiry::NothingExpires),
        SystemTime::now() + Duration::from_secs(30),
    );
    assert!(matches!(refused, Err(crate::plan::NotOffered::NoCaller)), "{refused:?}");
}

#[test]
fn a_grant_of_a_partition_only_this_node_can_read_is_refused() {
    let now = SystemTime::now();
    for partitions in [vec![Some(vec![1]), None], Vec::new()] {
        let refused = grantee(now + Duration::from_secs(30)).grant(
            Arc::new(Schema::empty()),
            partitions,
            NonZeroU32::MIN,
            EstimatedBytes::parse(0),
            (),
            now,
        );
        assert!(matches!(refused, Err(crate::plan::NotGranted::NotPortable)), "{refused:?}");
    }
}

/// The instant the partitions came back is the one checked, not the one the grantee was offered at.
#[test]
fn a_grant_ending_before_its_partitions_came_back_is_refused_as_expired() {
    let offered_at = SystemTime::now();
    let until = offered_at + Duration::from_secs(1);
    let refused = grantee(until).grant(
        Arc::new(Schema::empty()),
        [Some(vec![1])],
        NonZeroU32::MIN,
        EstimatedBytes::parse(0),
        (),
        until,
    );
    assert!(matches!(refused, Err(crate::plan::NotGranted::Expired(_))), "{refused:?}");
}

#[test]
fn a_grant_both_unportable_and_expired_is_refused_as_unportable() {
    let now = SystemTime::now();
    let refused = grantee(now).grant(
        Arc::new(Schema::empty()),
        [None],
        NonZeroU32::MIN,
        EstimatedBytes::parse(0),
        (),
        now,
    );
    assert!(matches!(refused, Err(crate::plan::NotGranted::NotPortable)), "{refused:?}");
}

/// `at`, truncated to the whole second a token service states an expiry in.
fn whole_second(at: SystemTime) -> SystemTime {
    let seconds = at
        .duration_since(std::time::UNIX_EPOCH)
        .expect("now is after the epoch")
        .as_secs();
    std::time::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// A token the pool issued until `not_after`, in exchange for the subject token `from`.
fn exchanged(from: &str, not_after: SystemTime) -> crate::identity::SourceToken {
    use crate::identity::{Delegated, Secret, SourceToken, WorkloadPool};

    let pool = WorkloadPool::parse("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/q")
        .expect("a test pool is a pool");
    let seconds = not_after
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    SourceToken::exchanged(
        &Secret::new(from),
        &pool,
        Delegated::new(Secret::new("a-forwarded-token"), seconds),
    )
}

#[test]
fn a_grant_forwards_no_token_exchanged_from_a_credential_its_caller_did_not_present() {
    let now = SystemTime::now();
    let refused =
        grantee(now + Duration::from_secs(30)).forwarding(exchanged("another-callers-token", now + Duration::from_secs(60)));
    assert!(matches!(refused, Err(crate::plan::NotTheCallersToken)), "{refused:?}");
}

#[test]
fn a_grant_read_with_its_callers_token_ends_when_the_token_does() {
    let now = SystemTime::now();
    let token_ends = whole_second(now + Duration::from_secs(20));
    let granted = grantee(now + Duration::from_secs(60))
        .forwarding(exchanged("a-test-token", token_ends))
        .expect("a token exchanged from the caller's own credential is forwarded")
        .grant(
            Arc::new(Schema::empty()),
            [Some(vec![1])],
            NonZeroU32::MIN,
            EstimatedBytes::parse(0),
            (),
            now,
        )
        .expect("a grant ending with a live token is granted");

    assert_eq!(granted.until(), token_ends, "the grant outlives the token it is read with");
    assert!(
        matches!(granted.reads(), crate::plan::ReadAs::CallerToken(token) if token.not_after() == token_ends),
        "{:?}",
        granted.reads()
    );
}

#[test]
fn a_grant_whose_forwarded_token_has_ended_is_refused_as_expired() {
    let now = SystemTime::now();
    let refused = grantee(now + Duration::from_secs(60))
        .forwarding(exchanged("a-test-token", whole_second(now)))
        .expect("a token exchanged from the caller's own credential is forwarded")
        .grant(
            Arc::new(Schema::empty()),
            [Some(vec![1])],
            NonZeroU32::MIN,
            EstimatedBytes::parse(0),
            (),
            now,
        );
    assert!(matches!(refused, Err(crate::plan::NotGranted::Expired(_))), "{refused:?}");
}

fn principal(name: &str) -> crate::identity::PrincipalName {
    crate::identity::PrincipalName::parse(name).expect("a test principal is a principal")
}

#[test]
fn a_grant_switches_to_no_principal_but_its_own_callers() {
    let refused = grantee(SystemTime::now() + Duration::from_secs(30)).switching(principal("another@example.com"));
    assert!(matches!(refused, Err(crate::plan::NotTheCallersPrincipal)), "{refused:?}");
}

#[test]
fn a_grant_switched_to_its_callers_principal_reads_as_that_principal() {
    let now = SystemTime::now();
    let granted = grantee(now + Duration::from_secs(30))
        .switching(principal("user@example.com"))
        .expect("the caller's own principal is switched to")
        .grant(
            Arc::new(Schema::empty()),
            [Some(vec![1])],
            NonZeroU32::MIN,
            EstimatedBytes::parse(0),
            (),
            now,
        )
        .expect("a live grant is granted");

    assert!(
        matches!(granted.reads(), crate::plan::ReadAs::CallerPrincipal(name) if name.as_str() == "user@example.com"),
        "{:?}",
        granted.reads()
    );
}
