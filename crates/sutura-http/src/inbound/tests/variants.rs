//! Refusal variants that were defined, raised in production, and never provoked by a test.
//!
//! Each cell drives one variant through the crate's public entry point - `InboundGate::establish`
//! for the token refusals, `KeySet::parse` for the key-set refusals - and asserts the specific
//! `TokenRejected` or `InvalidKeySet` variant rather than a bare `is_err`.

use std::time::Instant;

use crate::inbound::InvalidGroup;
use crate::inbound::keys::{InvalidKeySet, KeySet};
use crate::inbound::token::TokenRejected;
use crate::testing::{ISSUER, KID, RESOURCE};

use super::fixtures::{bearer, claims, direct, gate_over, jwks, key_pair, signed};

// --------------------------------------------- TokenRejected through establish ----

/// A token that is not a JWT at all - its header cannot be decoded - is refused with
/// [`TokenRejected::UnreadableHeader`].
///
/// Driven through `InboundGate::establish` rather than `TokenValidator::key_id` directly,
/// because the gate is the public entry point the request path uses.
#[tokio::test]
async fn a_token_with_no_readable_header_is_refused_with_unreadable_header() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    // Not a JWT: no dots, no base64 segments.
    let rejected = gate
        .establish(&bearer("not-a-jwt-at-all"), Instant::now())
        .await
        .expect_err("a non-JWT string is not a token");
    assert!(
        matches!(rejected, TokenRejected::UnreadableHeader { .. }),
        "expected UnreadableHeader, got {rejected:?}"
    );
}

/// A token whose `scope` claim carries a character RFC 6749 does not allow is refused with
/// [`TokenRejected::UnusableScope`].
#[tokio::test]
async fn a_token_with_an_unusable_scope_is_refused_with_unusable_scope() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    // `\u{7}` (BEL) is a control character that RFC 6749's `scope-token` production forbids.
    let token = signed(
        &pair,
        KID,
        &claims("someone@example.com", RESOURCE, ISSUER, r#"{"scope":"metrics:\u0007read"}"#),
    );
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("a scope with a control character is not a scope");
    assert!(
        matches!(rejected, TokenRejected::UnusableScope { .. }),
        "expected UnusableScope, got {rejected:?}"
    );
}

/// A token whose `groups` claim carries a non-printable character is refused with
/// [`TokenRejected::UnusableGroups`].
#[tokio::test]
async fn a_token_with_an_unusable_groups_claim_is_refused_with_unusable_groups() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    // A control character in a group value: the same class the domain refuses in a `sub`.
    let token = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            RESOURCE,
            ISSUER,
            r#"{"groups":["finance","\u0007admin"]}"#,
        ),
    );
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("a group with a control character is not a group");
    assert!(
        matches!(rejected, TokenRejected::UnusableGroups { .. }),
        "expected UnusableGroups, got {rejected:?}"
    );
}

/// A `groups` value one character past `MAX_GROUP_LENGTH` (128) is refused as
/// [`InvalidGroup::TooLong`], carrying both counts.
#[tokio::test]
async fn a_group_claim_value_past_the_length_bound_is_refused() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let extra = serde_json::json!({ "groups": ["g".repeat(129)] }).to_string();
    let token = signed(&pair, KID, &claims("someone@example.com", RESOURCE, ISSUER, &extra));
    let result = gate.establish(&bearer(&token), Instant::now()).await;
    assert!(
        matches!(
            result,
            Err(TokenRejected::UnusableGroups {
                cause: InvalidGroup::TooLong { found: 129, limit: 128 }
            })
        ),
        "expected TooLong 129/128, got {result:?}"
    );
}

/// One `groups` value past `MAX_GROUPS` (64) is refused as [`InvalidGroup::TooMany`]. Each value is
/// short, so the token stays far under `MAX_TOKEN_BYTES` and this is the count bound, not the byte one.
#[tokio::test]
async fn too_many_group_claim_values_are_refused() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let groups: Vec<String> = (0..65).map(|n| format!("g{n}")).collect();
    let extra = serde_json::json!({ "groups": groups }).to_string();
    let token = signed(&pair, KID, &claims("someone@example.com", RESOURCE, ISSUER, &extra));
    let result = gate.establish(&bearer(&token), Instant::now()).await;
    assert!(
        matches!(
            result,
            Err(TokenRejected::UnusableGroups {
                cause: InvalidGroup::TooMany { limit: 64 }
            })
        ),
        "expected TooMany 64, got {result:?}"
    );
}

/// A token whose `act` claim carries a `sub` with a control character is refused with
/// [`TokenRejected::UnusableActor`].
#[tokio::test]
async fn a_token_with_an_unusable_actor_is_refused_with_unusable_actor() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let token = signed(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            RESOURCE,
            ISSUER,
            r#"{"act":{"sub":"agent\u0007forge"}}"#,
        ),
    );
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("an actor with a control character is not an actor");
    assert!(
        matches!(rejected, TokenRejected::UnusableActor { .. }),
        "expected UnusableActor, got {rejected:?}"
    );
}

/// A token whose `act` claim nests deeper than `MAX_ACTORS` is refused with
/// [`TokenRejected::TooManyActors`].
#[tokio::test]
async fn a_token_with_too_many_actors_is_refused_with_too_many_actors() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    // Build 9 levels of nested `act` - MAX_ACTORS is 8, and `chain_from`'s check
    // `outer.len() + 2 > MAX_ACTORS` fires on the 8th link (a8), when `outer.len()`
    // is 7: 7 collected + the immediate (a0) + the current (a8) = 9 > 8.
    // The structure is: {"sub":"a0","act":{"sub":"a1","act":...{"sub":"a8"}...}}
    let mut nesting = String::from(r#"{"sub":"a8"}"#);
    for i in (0..8).rev() {
        nesting = format!(r#"{{"sub":"a{i}","act":{nesting}}}"#);
    }
    // `nesting` is now the top-level `act` value: {"sub":"a0","act":{"sub":"a1",...}}
    let extra = format!(r#"{{"act":{nesting}}}"#);
    let token = signed(&pair, KID, &claims("someone@example.com", RESOURCE, ISSUER, &extra));
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("a token with more than eight actors is refused");
    assert!(
        matches!(rejected, TokenRejected::TooManyActors { limit: 8 }),
        "expected TooManyActors with limit 8, got {rejected:?}"
    );
}

/// A token whose header names an empty `kid` is refused with [`TokenRejected::UnusableKeyId`],
/// before any key is looked up.
#[tokio::test]
async fn a_token_naming_an_empty_key_id_is_refused_with_unusable_key_id() {
    let pair = key_pair();
    let gate = gate_over(&direct(), &jwks(KID, &pair));
    let token = signed(&pair, "", &claims("someone@example.com", RESOURCE, ISSUER, ""));
    let rejected = gate
        .establish(&bearer(&token), Instant::now())
        .await
        .expect_err("an empty key id names no key");
    assert!(
        matches!(rejected, TokenRejected::UnusableKeyId { .. }),
        "expected UnusableKeyId, got {rejected:?}"
    );
}

// ------------------------------------------------- InvalidKeySet through parse ----

/// A JWK set carrying a key whose `kty` is neither `oct`, `RSA`, `EC`, nor `OKP` is refused
/// with [`InvalidKeySet::UnsupportedKeyFamily`] at load, before any signature is verified.
#[test]
fn a_key_set_with_an_unsupported_key_family_is_refused_at_load() {
    // A JWK with `kty: "unknown"` - not one of the four the verifier_for match covers,
    // so it falls into the `_` arm that returns `UnsupportedKeyFamily`.
    let document = r#"{"keys":[{"kty":"unknown","kid":"strange","use":"sig","alg":"ES256"}]}"#;
    let refused = KeySet::parse(document).expect_err("an unknown key family is refused at load");
    assert!(
        matches!(refused, InvalidKeySet::UnsupportedKeyFamily),
        "expected UnsupportedKeyFamily, got {refused:?}"
    );
}

/// A JWK whose own `kid` is empty is refused with [`InvalidKeySet::UnusableKeyId`] at load.
#[test]
fn a_key_set_with_an_empty_key_id_is_refused_at_load() {
    let refused = KeySet::parse(&jwks("", &key_pair())).expect_err("an empty key id is refused at load");
    assert!(
        matches!(refused, InvalidKeySet::UnusableKeyId { .. }),
        "expected UnusableKeyId, got {refused:?}"
    );
}

/// A JWK of a supported family whose key material does not decode is refused with
/// [`InvalidKeySet::UnusableKey`] at load.
#[test]
fn a_key_set_with_undecodable_key_material_is_refused_at_load() {
    // `EC` on `P-256` passes the family match; `x` and `y` are not base64url.
    let document = r#"{"keys":[{"kty":"EC","crv":"P-256","kid":"broken","use":"sig","x":"!!!","y":"!!!"}]}"#;
    let refused = KeySet::parse(document).expect_err("undecodable key material is refused at load");
    assert!(
        matches!(refused, InvalidKeySet::UnusableKey { .. }),
        "expected UnusableKey, got {refused:?}"
    );
}
