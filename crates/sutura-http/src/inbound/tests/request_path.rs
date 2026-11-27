//! The alternative to `transit_audience`: a gateway that binds the provider by request path.

use std::time::Instant;

use sutura_config::{ProofLifetime, RequestPathPrefix, RequiredTokenType};

use super::fixtures::{
    claims, claims_no_audience, expiry_in, gate_over, gateway_bound_by_request_path, jwks, key_pair, signed, transit,
};
use crate::inbound::TokenRejected;
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

/// A `requestPath` this mode never compares at all - a dot segment, a doubled `/`, a `%`, a `\`,
/// or a non-ASCII byte - and the refusal that proves it, with the configured prefix a token
/// presenting it would otherwise satisfy.
async fn refused_as_not_plain(path: &str) {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let claim = serde_json::to_string(path).expect("a test path is a JSON string");
    let token = signed(
        &pair,
        KID,
        &claims_no_audience("someone@example.com", ISSUER, &format!(r#"{{"requestPath":{claim}}}"#)),
    );
    let refused = gate
        .establish(&transit(&token), Instant::now())
        .await
        .expect_err("a requestPath this mode will not compare is refused");
    assert!(matches!(refused, TokenRejected::RequestPathNotPlain), "{refused:?}");
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
    assert!(matches!(refused, TokenRejected::NoRequestPath), "{refused:?}");
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
    assert!(matches!(refused, TokenRejected::RequestPathNotBound), "{refused:?}");
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
    assert!(matches!(refused, TokenRejected::RequestPathNotBound), "{refused:?}");
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
    assert!(
        matches!(refused, TokenRejected::AudiencePresentInRequestPathMode),
        "{refused:?}"
    );
}

/// A `requestPath` with a dot segment is refused outright, never compared - even where it would
/// otherwise start with the configured prefix after a browser-style normalisation.
#[tokio::test]
async fn a_requestpath_with_a_dot_dot_segment_is_refused() {
    refused_as_not_plain("/hub/app/v1/../v1/mcp").await;
}

/// A `requestPath` with a doubled `/` is refused outright, for the same reason as a dot segment.
#[tokio::test]
async fn a_requestpath_with_a_doubled_slash_is_refused() {
    refused_as_not_plain("/hub//app/v1/mcp").await;
}

/// A `requestPath` carrying a `%` is refused outright - a presented path is compared literally,
/// never percent-decoded first.
#[tokio::test]
async fn a_requestpath_with_a_percent_sign_is_refused() {
    refused_as_not_plain("/hub/app/v1/%2e%2e/mcp").await;
}

/// A `requestPath` carrying a backslash is refused outright - some normalisers read it as a path
/// separator, and this mode takes no side in that disagreement.
#[tokio::test]
async fn a_requestpath_with_a_backslash_is_refused() {
    refused_as_not_plain(r"/hub/app/v1\mcp").await;
}

/// A non-ASCII `requestPath` is refused outright, rather than compared under some assumed encoding.
#[tokio::test]
async fn a_non_ascii_requestpath_is_refused() {
    refused_as_not_plain("/hub/app/v1/mcp\u{00e9}").await;
}

fn token_for_path(pair: &rcgen::KeyPair, path: &str) -> String {
    signed(
        pair,
        KID,
        &claims_no_audience(
            "someone@example.com",
            ISSUER,
            &format!(
                r#"{{"requestPath":"{path}","iat":{},"exp":{}}}"#,
                expiry_in(-60),
                expiry_in(60)
            ),
        ),
    )
}

/// The provider's bare base path - the URL a gateway's catalog hands to clients - is the
/// configured prefix without its trailing `/`, and is admitted (`github.com/telekom/sutura#2098`).
#[tokio::test]
async fn the_bare_base_path_of_the_configured_prefix_establishes() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let caller = gate
        .establish(&transit(&token_for_path(&pair, "/hub/app/v1")), Instant::now())
        .await
        .expect("the bare base path establishes");
    assert_eq!(caller.chain().subject().established(), "verified");
}

/// Admitting the bare base path must not admit a sibling route whose name extends it as text.
#[tokio::test]
async fn a_sibling_extending_the_bare_base_path_as_text_is_refused() {
    let (pair, gate) = gate_bound_to("/hub/app/v1/");
    let refused = gate
        .establish(&transit(&token_for_path(&pair, "/hub/app/v10")), Instant::now())
        .await
        .expect_err("a sibling route is refused");
    assert!(matches!(refused, TokenRejected::RequestPathNotBound), "{refused:?}");
}
