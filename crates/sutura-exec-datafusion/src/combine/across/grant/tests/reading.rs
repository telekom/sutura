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

type SignedGrant = ([u8; 32], Vec<u8>);

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
        (sutura_domain::identity::VerifierId::primary(), [0; 32]),
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
        (sutura_domain::identity::VerifierId::primary(), [0; 32]),
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
        (sutura_domain::identity::VerifierId::primary(), [0; 32]),
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
        (sutura_domain::identity::VerifierId::primary(), [0; 32]),
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

use super::super::super::partition::SwitchedPrincipal;

fn switched(name: &str) -> SwitchedPrincipal {
    SwitchedPrincipal::new(sutura_domain::identity::PrincipalName::parse(name).expect("a test principal is a principal"))
}

#[test]
fn a_grant_whose_principal_is_swapped_after_signing_is_refused_as_not_naming() {
    let key = ReadingKey::drawn().expect("a key draws");
    let now = core::time::Duration::from_secs(1);
    let (_, signature) = mint(
        &key,
        grant(b"leg-one", b"partition-one", 100).switching(&switched("reader@example.com")),
    );
    let tampered = grant(b"leg-one", b"partition-one", 100).switching(&switched("other@example.com"));
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
fn a_grant_naming_both_a_token_and_a_principal_does_not_decode() {
    let key = ReadingKey::drawn().expect("a key draws");
    let (built, signature) = mint(
        &key,
        grant(b"leg-one", b"partition-one", 100)
            .forwarding(&forwarded("token-a"))
            .switching(&switched("reader@example.com")),
    );
    let decoded = super::super::reading::decode(&super::super::reading::encode(&built, &signature));
    assert!(decoded.is_none(), "a grant naming a token and a principal decoded");
}

fn admitted_read(
    keys: &super::super::ReadingKeys,
    (key, grant): &SignedGrant,
) -> Result<super::super::AdmittedRead, WorkerRefusal> {
    super::super::verify_reading(
        keys,
        &super::super::ReadingCall {
            wire: b"leg-one",
            source: &source(),
            partition: b"partition-one",
            key: *key,
            grant: Some(grant),
            forwarded: None,
            switched: None,
        },
        core::time::Duration::from_secs(1),
    )
}

fn granted_to(verifier: &str, signed_sub: &str) -> SignedGrant {
    let caller = sutura_domain::identity::Subject::verified_by(
        sutura_domain::identity::VerifierId::parse(verifier).expect("a verifier id"),
        signed_sub,
    )
    .expect("a subject");
    super::super::granted_read(
        b"leg-one",
        &source(),
        b"partition-one",
        (&caller, core::time::Duration::from_secs(100)),
        (1000, 1 << 20),
        &super::super::GrantReads::ServiceUser,
    )
    .expect("a qualified caller's grant mints")
}

#[test]
fn the_same_reading_partition_has_distinct_kept_key_slots_for_each_verifier_and_exact_subject() {
    let keys = super::super::ReadingKeys::default();
    for (verifier, subject) in [
        ("staff", "user@example.com"),
        ("partner", "user@example.com"),
        ("staff", " user@example.com "),
    ] {
        let grant = granted_to(verifier, subject);
        let admitted = admitted_read(&keys, &grant).expect("each qualified caller owns its own slot");
        assert_eq!(admitted.verifier.as_str(), verifier);
        admitted_read(&keys, &grant).expect("a retry retains its key");
    }
}

#[test]
fn a_reading_caller_cannot_replace_its_first_key_for_the_same_partition() {
    let keys = super::super::ReadingKeys::default();
    let first = granted_to("staff", "user@example.com");
    admitted_read(&keys, &first).expect("the first key admits");
    let replacement = granted_to("staff", "user@example.com");
    assert_ne!(first.0, replacement.0, "the task keys were drawn separately");
    assert!(matches!(
        admitted_read(&keys, &replacement),
        Err(WorkerRefusal::GrantNotNaming)
    ));
    admitted_read(&keys, &first).expect("a refused replacement did not evict the first key");
}

#[test]
fn a_reading_grants_verifier_and_caller_context_are_authenticated_before_being_returned() {
    use super::super::reading::{decode, encode};
    use sutura_domain::identity::VerifierId;

    let original = granted_to("staff", "user@example.com");
    let (built, signature) = decode(&original.1).expect("the real mint encodes a grant");
    let staff = VerifierId::parse("staff").expect("a verifier id");
    for (verifier, context) in [
        (VerifierId::parse("partner").expect("a verifier id"), *built.caller_context()),
        (staff, [9; 32]),
    ] {
        let tampered = ReadingGrant::new(
            LegDigest::of(b"leg-one"),
            source(),
            Partition::of(b"partition-one".to_vec()),
            Expiry::at(core::time::Duration::from_secs(100)),
            MaskedCaller::of(&SubjectKey::parse("user@example.com").expect("a subject")),
            (verifier, context),
            bound(),
        );
        let grant = (original.0, encode(&tampered, &signature));
        assert!(matches!(
            admitted_read(&super::super::ReadingKeys::default(), &grant),
            Err(WorkerRefusal::GrantNotNaming)
        ));
    }
    admitted_read(&super::super::ReadingKeys::default(), &original).expect("the untampered grant admits");
}

fn replacing_field(encoded: &[u8], ordinal: usize, replacement: &[u8]) -> Vec<u8> {
    let mut rest = encoded;
    let mut rebuilt = Vec::new();
    for index in 0..14 {
        let colon = rest.iter().position(|&byte| byte == b':').expect("an encoded field");
        let (digits, tail) = rest.split_at(colon);
        let length: usize = core::str::from_utf8(digits)
            .expect("a decimal length")
            .parse()
            .expect("a length");
        let (field, tail) = tail.get(1..).expect("past the colon").split_at(length);
        let field = if index == ordinal { replacement } else { field };
        rebuilt.extend_from_slice(field.len().to_string().as_bytes());
        rebuilt.push(b':');
        rebuilt.extend_from_slice(field);
        rest = tail;
    }
    assert_eq!(rest, b"");
    rebuilt
}

#[test]
fn malformed_reading_caller_fields_are_refused_before_a_key_slot_is_occupied() {
    let original = granted_to("staff", "user@example.com");
    for (ordinal, replacement) in [
        (6, Vec::new()),
        (6, vec![b'a'; 64]),
        (6, b"not/an/id".to_vec()),
        (7, vec![0; 31]),
        (7, vec![0; 33]),
        (4, b"1000000000".to_vec()),
    ] {
        let keys = super::super::ReadingKeys::default();
        let malformed = ([255; 32], replacing_field(&original.1, ordinal, &replacement));
        assert_ne!(malformed.0, original.0);
        assert!(matches!(admitted_read(&keys, &malformed), Err(WorkerRefusal::GrantNotNaming)));
        admitted_read(&keys, &original).expect("malformed fields never reserved the valid caller's slot");
    }
}
