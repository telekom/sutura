//! A `direct` verifier paired with `requires_gateway` - `github.com/telekom/sutura#2089` - and the
//! one explicit exception it carves out of refusing two configured credential locations at once
//! (`github.com/telekom/sutura#1884`).
//!
//! Every cell below builds the gate through [`InboundGate::with_required_gateway`], the same
//! test-only seam [`InboundGate::with_also_accept`] already uses for `also_accept`: production
//! attaches the pairing in [`InboundGate::from_verifiers_with_outbound`], which reads the gateway's
//! key set from the filesystem - these tests hand it a cache built from an in-memory document
//! instead, same as every other cell in this module.

use std::time::Instant;

use sutura_dev::issuer::{MockIssuer, Token};
use sutura_domain::identity::Subject;

use super::fixtures::{gate_over, header_named, paired, paired_direct};
use crate::inbound::{InboundGate, TokenRejected};
use crate::testing::{ISSUER, RESOURCE};

const TOKEN_HEADER: &str = "x-service-access-token";

fn gateway(issuer: &str, audience: &str) -> MockIssuer {
    MockIssuer::generating(issuer, audience, "gateway-key").expect("a fixture issuer generates a key")
}

fn signed_for(issuer: &MockIssuer, subject: &str) -> String {
    issuer
        .mint(&Token::for_subject(subject).living_for(60))
        .expect("a fixture signs")
}

/// A `direct` gate paired with a standalone `behind-gateway` sub-gate, over fresh issuers each call
/// so no cell can depend on another's key.
fn paired_gate() -> (InboundGate, MockIssuer, MockIssuer) {
    let caller_issuer = MockIssuer::generating(ISSUER, RESOURCE, "caller-key").expect("a fixture issuer generates a key");
    let gateway_issuer = gateway("https://gateway.example.com", "https://gateway.example.com/transit");
    let caller_identity = paired_direct("/unread/caller.json", TOKEN_HEADER);
    // The target `behind-gateway` entry must read its assertion from `Authorization` - the one
    // `NotFitToServe::RequiresGatewayNotInAuthorization` enforces at startup - never from its own
    // `transit_header`, which is what every other `behind-gateway` test in this module uses instead.
    let gateway_identity = sutura_config::InboundIdentity::BehindGateway {
        transit: sutura_config::TransitProof::new(
            sutura_config::ProofHeader::parse("authorization").expect("a fixture header is a header"),
            sutura_config::IssuerUrl::parse_transit_issuer(gateway_issuer.issuer()).expect("a fixture issuer is an issuer"),
            sutura_config::ResourceIdentifier::parse_transit_audience(gateway_issuer.audience()).expect("a fixture audience"),
            sutura_config::KeySetLocation::File(sutura_config::KeySetFile::parse("/unread/gateway.json").expect("a test path")),
            sutura_config::PinnedAlgorithms::of(sutura_config::SigningAlgorithm::Es256),
            sutura_config::RequiredTokenType::Any,
            sutura_config::ProofLifetime::parse(3600).expect("a fixture lifetime is a lifetime"),
        ),
    };
    let gate = gate_over(&caller_identity, caller_issuer.key_set().as_str())
        .with_required_gateway(gate_over_gateway(&gateway_identity, &gateway_issuer));
    (gate, caller_issuer, gateway_issuer)
}

fn gate_over_gateway(identity: &sutura_config::InboundIdentity, issuer: &MockIssuer) -> InboundGate {
    gate_over(identity, issuer.key_set().as_str())
}

#[tokio::test]
async fn an_accepted_pairing_establishes_the_caller_from_the_user_token_not_the_gateway() {
    let (gate, caller_issuer, gateway_issuer) = paired_gate();
    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    let gateway_token = signed_for(&gateway_issuer, "the-fronting-component");
    let headers = paired(TOKEN_HEADER, &caller_token, &gateway_token);
    let caller = gate
        .establish(&headers, Instant::now())
        .await
        .expect("a valid caller token beside a valid gateway assertion is admitted");
    assert_eq!(
        caller.chain().subject(),
        &Subject::verified_by(caller.verifier().id().clone(), "a-user@example.com").expect("a fixture subject")
    );
}

#[tokio::test]
async fn an_absent_gateway_assertion_refuses_the_whole_request() {
    let (gate, caller_issuer, _gateway_issuer) = paired_gate();
    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    let headers = header_named(TOKEN_HEADER, &caller_token);
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::GatewayAssertionRequired(_))
    ));
}

#[tokio::test]
async fn an_invalid_gateway_assertion_refuses_the_whole_request() {
    let (gate, caller_issuer, _gateway_issuer) = paired_gate();
    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    // Signed by an issuer the gateway sub-gate does not trust at all.
    let forged_issuer = gateway(
        "https://not-the-real-gateway.example.com",
        "https://gateway.example.com/transit",
    );
    let forged = signed_for(&forged_issuer, "the-fronting-component");
    let headers = paired(TOKEN_HEADER, &caller_token, &forged);
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::GatewayAssertionRequired(_))
    ));
}

#[tokio::test]
async fn a_user_token_in_authorization_does_not_satisfy_the_gateway_check() {
    let (gate, caller_issuer, _gateway_issuer) = paired_gate();
    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    // The caller's own token, signed by the caller issuer, placed where the gateway assertion
    // belongs - it must fail the gateway sub-gate's own issuer check, never be accepted as it.
    let headers = paired(TOKEN_HEADER, &caller_token, &caller_token);
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::GatewayAssertionRequired(_))
    ));
}

#[tokio::test]
async fn an_anonymous_request_is_still_refused() {
    let (gate, _caller_issuer, _gateway_issuer) = paired_gate();
    let headers = axum::http::HeaderMap::new();
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::Absent { .. })
    ));
}
