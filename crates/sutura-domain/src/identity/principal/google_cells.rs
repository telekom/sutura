//! The audit label cannot change the verifier-scoped Google subject key.
use super::Subject;
use crate::identity::{VerifiedAddress, VerifierId};
#[test]
fn a_google_email_change_changes_the_masked_record_but_not_subject_identity_or_hash() {
    let verifier = VerifierId::parse("google").expect("verifier");
    let first = Subject::verified_by_address(
        verifier.clone(),
        "subject-123",
        &VerifiedAddress::parse("first@example.com").expect("address"),
    )
    .expect("subject");
    let renamed = Subject::verified_by_address(
        verifier,
        "subject-123",
        &VerifiedAddress::parse("renamed@example.com").expect("address"),
    )
    .expect("subject");
    assert_eq!(first, renamed);
    let mut keys = std::collections::HashSet::new();
    let _new = keys.insert(first.clone());
    assert!(
        keys.contains(&renamed),
        "email cannot split a caller's budget or cache identity"
    );
    assert_ne!(first.id(), renamed.id());
    assert_eq!(first.signed_sub(), Some("subject-123"));
    assert!(!format!("{first:?}").contains("first@example.com"));
    let other = Subject::verified_by_address(
        VerifierId::parse("other").expect("verifier"),
        "subject-123",
        &VerifiedAddress::parse("first@example.com").expect("address"),
    )
    .expect("subject");
    assert_ne!(first, other, "the verifier remains part of caller identity");
}
