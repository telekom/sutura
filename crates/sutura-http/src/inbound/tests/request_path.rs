//! The alternative to `transit_audience`: a gateway that binds the provider by request path.

use std::time::Instant;

use sutura_config::{ProofLifetime, RequestPathPrefix, RequiredTokenType};

use super::fixtures::{claims, gate_over, gateway_bound_by_request_path, jwks, key_pair, signed, transit};
use crate::testing::{ISSUER, KID};

fn prefix(raw: &str) -> RequestPathPrefix {
    RequestPathPrefix::parse(raw).expect("a well-formed prefix parses")
}

#[tokio::test]
async fn a_requestpath_starting_with_the_configured_prefix_establishes() {
    let pair = key_pair();
    let declaration = gateway_bound_by_request_path(
        prefix("/hub/app/v1/"),
        RequiredTokenType::access_token(),
        ProofLifetime::default_lifetime(),
    );
    let gate = gate_over(&declaration, &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            "ignored-without-transit_audience",
            ISSUER,
            r#"{"requestPath":"/hub/app/v1/mcp"}"#,
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
    let pair = key_pair();
    let declaration = gateway_bound_by_request_path(
        prefix("/hub/app/v1/"),
        RequiredTokenType::access_token(),
        ProofLifetime::default_lifetime(),
    );
    let gate = gate_over(&declaration, &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &claims("someone@example.com", "ignored-without-transit_audience", ISSUER, ""),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a token with no requestPath is refused");
    let chain = crate::surface::cause_chain(&refused).join(" ").to_lowercase();
    assert!(chain.contains("requestpath"), "{refused:?}");
}

#[tokio::test]
async fn a_requestpath_not_starting_with_the_configured_prefix_is_refused() {
    let pair = key_pair();
    let declaration = gateway_bound_by_request_path(
        prefix("/hub/app/v1/"),
        RequiredTokenType::access_token(),
        ProofLifetime::default_lifetime(),
    );
    let gate = gate_over(&declaration, &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            "ignored-without-transit_audience",
            ISSUER,
            r#"{"requestPath":"/other/route"}"#,
        ),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a requestPath outside the prefix is refused");
    let chain = crate::surface::cause_chain(&refused).join(" ").to_lowercase();
    assert!(chain.contains("requestpath"), "{refused:?}");
}

/// A sibling route whose text merely extends the prefix, with no `/` boundary, must not match -
/// the trick `RequestPathPrefix::matches` is built to resist.
#[tokio::test]
async fn a_requestpath_extending_the_prefix_as_sibling_text_is_refused() {
    let pair = key_pair();
    let declaration = gateway_bound_by_request_path(
        prefix("/hub/app/v1/"),
        RequiredTokenType::access_token(),
        ProofLifetime::default_lifetime(),
    );
    let gate = gate_over(&declaration, &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            "ignored-without-transit_audience",
            ISSUER,
            r#"{"requestPath":"/hub/app/v1evil/mcp"}"#,
        ),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a sibling route extending the prefix as text is refused");
    let chain = crate::surface::cause_chain(&refused).join(" ").to_lowercase();
    assert!(chain.contains("requestpath"), "{refused:?}");
}
