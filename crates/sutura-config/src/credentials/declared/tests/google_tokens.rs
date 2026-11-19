//! A Google-token source forwards only a token established by tokeninfo.
use super::*;
#[test]
fn a_caller_google_token_source_refuses_jwt_provenance_and_forwards_each_google_callers_own_token() {
    let broker = broker().caller_google_token(source("warehouse"));
    let at = source("warehouse");
    let sources = SourceSet::of(at.clone());
    let jwt = broker.mint(&caller("subject-123"), &sources).expect("refusal is an outcome");
    assert!(
        matches!(jwt, Minted::Refused { .. }),
        "a direct JWT cannot pass as a Google access token"
    );
    for subject in ["subject-123", "subject-456"] {
        let context = caller(subject).admitted_by(AdmittingVerifier::google_tokeninfo(VerifierId::primary()));
        let minted = broker.mint(&context, &sources).expect("Google subject is mintable");
        assert_eq!(
            presented_at(minted, subject, &sources, &at),
            format!("assertion.for.{subject}")
        );
    }
}
