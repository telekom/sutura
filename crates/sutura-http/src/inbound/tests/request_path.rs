//! The alternative to `transit_audience`: a gateway that binds the provider by request path.

use std::time::Instant;

use sutura_config::{ProofLifetime, RequestPathPrefix, RequiredTokenType};

use super::fixtures::{
    claims, claims_no_audience, expiry_in, gate_over, gateway_bound_by_request_path, jwks, key_pair, signed, transit,
};
use crate::testing::{ISSUER, KID};

fn prefix(raw: &str) -> RequestPathPrefix {
    RequestPathPrefix::parse(raw).expect("a well-formed prefix parses")
}

fn gate_bound_to(prefix_raw: &str) -> (rcgen::KeyPair, crate::inbound::InboundGate) {
    let pair = key_pair();
    let declaration = gateway_bound_by_request_path(
        prefix(prefix_raw),
        RequiredTokenType::access_token(),
        ProofLifetime::default_lifetime(),
    );
    let gate = gate_over(&declaration, &jwks(KID, &pair));
    (pair, gate)
}

// `cause_chain` walks only `#[source]` links and these refusals carry none, so the top-level
// `Display` - the refusal's own `#[error(...)]` message - is what a caller and a log actually see.
fn refusal_names_requestpath(refused: &crate::inbound::TokenRejected) -> bool {
    refused.to_string().to_lowercase().contains("requestpath")
}

#[tokio::test]
async fn a_requestpath_starting_with_the_configured_prefix_establishes() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let token = signed(
        &pair,
        KID,
        &claims_no_audience(
            "someone@example.com",
            ISSUER,
            &format!(
                r#"{{"requestPath":"/hub/app/v1/mcp","iat":{},"exp":{}}}"#,
                expiry_in(-60),
                expiry_in(60)
            ),
        ),
    );
    let caller = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect("a requestPath starting with the configured prefix establishes");
    assert_eq!(caller.chain().subject().established(), "verified");
}

#[tokio::test]
async fn a_token_with_no_requestpath_is_refused() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let token = signed(&pair, KID, &claims_no_audience("someone@example.com", ISSUER, ""));
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a token with no requestPath is refused");
    assert!(refusal_names_requestpath(&refused), "{refused:?}");
}

#[tokio::test]
async fn a_requestpath_not_starting_with_the_configured_prefix_is_refused() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let token = signed(
        &pair,
        KID,
        &claims_no_audience("someone@example.com", ISSUER, r#"{"requestPath":"/other/route"}"#),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a requestPath outside the prefix is refused");
    assert!(refusal_names_requestpath(&refused), "{refused:?}");
}

/// A sibling route whose text merely extends the prefix, with no `/` boundary, must not match -
/// the trick `RequestPathPrefix::matches` is built to resist.
#[tokio::test]
async fn a_requestpath_extending_the_prefix_as_sibling_text_is_refused() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let token = signed(
        &pair,
        KID,
        &claims_no_audience("someone@example.com", ISSUER, r#"{"requestPath":"/hub/app/v1evil/mcp"}"#),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a sibling route extending the prefix as text is refused");
    assert!(refusal_names_requestpath(&refused), "{refused:?}");
}

/// Fail-closed: this mode never compares an `aud`, so a token carrying one anyway is refused
/// outright rather than admitted with an unchecked claim.
#[tokio::test]
async fn a_token_carrying_an_audience_is_refused_even_with_a_matching_requestpath() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let token = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            "https://sutura.example.com",
            ISSUER,
            r#"{"requestPath":"/hub/app/v1/mcp"}"#,
        ),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("an aud claim in request-path mode is refused");
    assert!(refused.to_string().to_lowercase().contains("aud"), "{refused:?}");
}

/// A `requestPath` with a dot segment is refused outright, never compared - even where it would
/// otherwise start with the configured prefix after a browser-style normalisation.
#[tokio::test]
async fn a_requestpath_with_a_dot_dot_segment_is_refused() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let token = signed(
        &pair,
        KID,
        &claims_no_audience("someone@example.com", ISSUER, r#"{"requestPath":"/hub/app/v1/../v1/mcp"}"#),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a requestPath with a dot segment is refused");
    assert!(refusal_names_requestpath(&refused), "{refused:?}");
}

/// A `requestPath` with a doubled `/` is refused outright, for the same reason as a dot segment.
#[tokio::test]
async fn a_requestpath_with_a_doubled_slash_is_refused() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let token = signed(
        &pair,
        KID,
        &claims_no_audience("someone@example.com", ISSUER, r#"{"requestPath":"/hub//app/v1/mcp"}"#),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a requestPath with a doubled slash is refused");
    assert!(refusal_names_requestpath(&refused), "{refused:?}");
}
