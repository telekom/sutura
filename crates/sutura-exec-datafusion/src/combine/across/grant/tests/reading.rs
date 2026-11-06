//! Cells for the source-reading grant (`super::super::reading`, Decision 5): one per refusal
//! [`verify`] names directly, one per field the signature covers but [`verify`]'s own three
//! equality checks do not test by name, and the partition/expiry boundary cases.
//!
//! # What this does not exercise
//!
//! No reading stage exists yet (the module doc on `reading.rs` states that limit), so no cell here
//! drives a real worker call - that a per-task header hook delivers a distinct `HeaderMap` per task
//! is instead asserted directly against the vendored engine in
//! `vendor/datafusion-distributed/src/passthrough_headers.rs`'s own unit tests (`VENDOR.md`'s local
//! patch (3)). This module proves only [`mint`]/[`verify`]'s own contract.

use sutura_domain::identity::SubjectKey;
use sutura_domain::model::SourceName;

use super::super::super::WorkerRefusal;
use super::super::reading::{
    Expiry, LegDigest, MaskedCaller, Partition, ReadingGrant, ReadingKey, StreamBound, Verification, mint, verify,
};

fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

fn caller() -> MaskedCaller {
    MaskedCaller::of(&SubjectKey::parse("reader@example.com").expect("a test caller is a subject"))
}

fn bound() -> StreamBound {
    StreamBound {
        rows: 1000,
        bytes: 1 << 20,
    }
}

fn grant(leg: &[u8], partition: &[u8], seconds: u64) -> ReadingGrant {
    ReadingGrant::new(
        LegDigest::of(leg),
        source(),
        Partition::of(partition.to_vec()),
        Expiry::at(core::time::Duration::from_secs(seconds)),
        caller(),
        bound(),
    )
}

/// A grant minted under `key`, verified against `wanted_leg`/`wanted_partition` at `now`.
fn verified(
    key: &ReadingKey,
    leg: &[u8],
    partition: &[u8],
    seconds: u64,
    wanted_leg: &[u8],
    wanted_partition: &[u8],
    now: core::time::Duration,
) -> Result<(), WorkerRefusal> {
    let (built, signature) = mint(key, grant(leg, partition, seconds));
    verify(&Verification {
        key,
        grant: &built,
        signature: &signature,
        wanted_leg: LegDigest::of(wanted_leg),
        wanted_source: &source(),
        wanted_partition: &Partition::of(wanted_partition.to_vec()),
        now,
    })
}

#[test]
fn a_grant_naming_the_leg_source_and_partition_a_task_reads_verifies() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let outcome = verified(&key, b"leg-one", b"partition-one", 100, b"leg-one", b"partition-one", now);
    assert_eq!(outcome, Ok(()));
}

#[test]
fn a_grant_naming_a_different_leg_than_the_task_reads_is_refused_as_not_naming() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let outcome = verified(&key, b"leg-one", b"partition-one", 100, b"leg-two", b"partition-one", now);
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}

#[test]
fn a_grant_naming_a_different_source_than_the_task_reads_is_refused_as_not_naming() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let (built, signature) = mint(&key, grant(b"leg-one", b"partition-one", 100));
    let other_source = SourceName::parse("other").expect("a test source is a source");
    let outcome = verify(&Verification {
        key: &key,
        grant: &built,
        signature: &signature,
        wanted_leg: LegDigest::of(b"leg-one"),
        wanted_source: &other_source,
        wanted_partition: &Partition::of(b"partition-one".to_vec()),
        now,
    });
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}

#[test]
fn a_grant_naming_a_different_partition_than_the_task_reads_is_refused_as_not_naming() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let outcome = verified(&key, b"leg-one", b"partition-one", 100, b"leg-one", b"partition-two", now);
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}

#[test]
fn a_grant_past_its_expiry_on_the_worker_s_own_clock_is_refused_as_expired() {
    let key = ReadingKey::drawn().expect("a key draws");
    let past_deadline = core::time::Duration::from_secs(101);
    let outcome = verified(
        &key,
        b"leg-one",
        b"partition-one",
        100,
        b"leg-one",
        b"partition-one",
        past_deadline,
    );
    assert_eq!(outcome, Err(WorkerRefusal::GrantExpired));
}

#[test]
fn a_grant_signed_under_another_task_s_key_is_refused_as_not_naming() {
    let (key, other) = (
        ReadingKey::drawn().expect("a key draws"),
        ReadingKey::drawn().expect("a key draws"),
    );
    let now = core::time::Duration::from_secs(1);
    let (built, signature) = mint(&other, grant(b"leg-one", b"partition-one", 100));
    let outcome = verify(&Verification {
        key: &key,
        grant: &built,
        signature: &signature,
        wanted_leg: LegDigest::of(b"leg-one"),
        wanted_source: &source(),
        wanted_partition: &Partition::of(b"partition-one".to_vec()),
        now,
    });
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}

#[test]
fn a_grant_whose_expiry_is_tampered_after_signing_is_refused_as_not_naming() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let (_, signature) = mint(&key, grant(b"leg-one", b"partition-one", 100));
    let tampered = ReadingGrant::new(
        LegDigest::of(b"leg-one"),
        source(),
        Partition::of(b"partition-one".to_vec()),
        Expiry::at(core::time::Duration::from_secs(999)),
        caller(),
        bound(),
    );
    let outcome = verify(&Verification {
        key: &key,
        grant: &tampered,
        signature: &signature,
        wanted_leg: LegDigest::of(b"leg-one"),
        wanted_source: &source(),
        wanted_partition: &Partition::of(b"partition-one".to_vec()),
        now,
    });
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}

#[test]
fn a_grant_whose_caller_is_tampered_after_signing_is_refused_as_not_naming() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let (_, signature) = mint(&key, grant(b"leg-one", b"partition-one", 100));
    let other_caller = MaskedCaller::of(&SubjectKey::parse("someone-else@example.com").expect("a test caller is a subject"));
    let tampered = ReadingGrant::new(
        LegDigest::of(b"leg-one"),
        source(),
        Partition::of(b"partition-one".to_vec()),
        Expiry::at(core::time::Duration::from_secs(100)),
        other_caller,
        bound(),
    );
    let outcome = verify(&Verification {
        key: &key,
        grant: &tampered,
        signature: &signature,
        wanted_leg: LegDigest::of(b"leg-one"),
        wanted_source: &source(),
        wanted_partition: &Partition::of(b"partition-one".to_vec()),
        now,
    });
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}

#[test]
fn a_grant_whose_bound_is_tampered_after_signing_is_refused_as_not_naming() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let (_, signature) = mint(&key, grant(b"leg-one", b"partition-one", 100));
    let tampered = ReadingGrant::new(
        LegDigest::of(b"leg-one"),
        source(),
        Partition::of(b"partition-one".to_vec()),
        Expiry::at(core::time::Duration::from_secs(100)),
        caller(),
        StreamBound { rows: 1, bytes: 1 },
    );
    let outcome = verify(&Verification {
        key: &key,
        grant: &tampered,
        signature: &signature,
        wanted_leg: LegDigest::of(b"leg-one"),
        wanted_source: &source(),
        wanted_partition: &Partition::of(b"partition-one".to_vec()),
        now,
    });
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}

#[test]
fn a_partition_holding_a_nul_byte_still_verifies() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let partition: &[u8] = b"part\x00ition";
    let outcome = verified(&key, b"leg-one", partition, 100, b"leg-one", partition, now);
    assert_eq!(outcome, Ok(()));
}

#[test]
fn a_grant_at_its_exact_deadline_is_not_yet_expired() {
    let key = ReadingKey::drawn().expect("a key draws");
    let deadline = core::time::Duration::from_secs(100);
    let outcome = verified(
        &key,
        b"leg-one",
        b"partition-one",
        100,
        b"leg-one",
        b"partition-one",
        deadline,
    );
    assert_eq!(outcome, Ok(()));
}

use super::super::super::partition::Forwarded;

fn forwarded(token: &str) -> Forwarded {
    let pool = sutura_domain::identity::WorkloadPool::parse(
        "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/q",
    )
    .expect("a test pool is a pool");
    Forwarded::new(sutura_domain::identity::Secret::new(token), &pool)
}

#[test]
fn a_grant_whose_forwarded_token_is_swapped_after_signing_is_refused_as_not_naming() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let (_, signature) = mint(
        &key,
        grant(b"leg-one", b"partition-one", 100).forwarding(&forwarded("token-a")),
    );
    let tampered = grant(b"leg-one", b"partition-one", 100).forwarding(&forwarded("token-b"));
    let outcome = verify(&Verification {
        key: &key,
        grant: &tampered,
        signature: &signature,
        wanted_leg: LegDigest::of(b"leg-one"),
        wanted_source: &source(),
        wanted_partition: &Partition::of(b"partition-one".to_vec()),
        now,
    });
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}

#[test]
fn a_grant_whose_forwarded_pool_is_swapped_after_signing_is_refused_as_not_naming() {
    let in_project = |project: u8| {
        let pool = sutura_domain::identity::WorkloadPool::parse(&format!(
            "//iam.googleapis.com/projects/{project}/locations/global/workloadIdentityPools/p/providers/q"
        ))
        .expect("a test pool is a pool");
        Forwarded::new(sutura_domain::identity::Secret::new("token-a"), &pool)
    };
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let (_, signature) = mint(&key, grant(b"leg-one", b"partition-one", 100).forwarding(&in_project(1)));
    let tampered = grant(b"leg-one", b"partition-one", 100).forwarding(&in_project(2));
    let outcome = verify(&Verification {
        key: &key,
        grant: &tampered,
        signature: &signature,
        wanted_leg: LegDigest::of(b"leg-one"),
        wanted_source: &source(),
        wanted_partition: &Partition::of(b"partition-one".to_vec()),
        now,
    });
    assert_eq!(outcome, Err(WorkerRefusal::GrantNotNaming));
}
