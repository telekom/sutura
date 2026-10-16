//! Refusal variants that were defined, raised in production, and never provoked by a test.
//!
//! Each cell drives one variant through the crate's public entry point - `InboundGate::establish`
//! for the token refusals, `KeySet::parse` for the key-set refusals - and asserts the specific
//! `TokenRejected` or `InvalidKeySet` variant rather than a bare `is_err`.

use std::time::Instant;

use base64::Engine as _;

use crate::inbound::InvalidGroup;
use crate::inbound::keys::{InvalidKeySet, KeySet};
use crate::inbound::token::TokenRejected;
use crate::testing::{ISSUER, KID, RESOURCE};

use super::audience::{CLIENT_ID, entra_declared};
use super::fixtures::{bearer, claims, direct, gate_over, jwk, jwks, key_pair, signed, signed_as};

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

/// A `typ: JWT` token for [`CLIENT_ID`] with `extra` claims, through a deployment that reads `roles`.
async fn established_reading_roles(extra: &str) -> Result<crate::inbound::VerifiedCaller, TokenRejected> {
    let pair = key_pair();
    let settings = entra_declared(&format!(
        "    audience: \"{CLIENT_ID}\"\n    extra_scope_claims: [\"roles\"]\n"
    ));
    let gate = gate_over(settings.security().inbound().expect("an inbound identity"), &jwks(KID, &pair));
    let token = signed_as(
        &pair,
        KID,
        &claims("someone@example.com", CLIENT_ID, ISSUER, extra),
        Some("JWT"),
    );
    gate.establish(&bearer(&token), Instant::now()).await
}

/// With `extra_scope_claims` set, a token that names no client in `azp` is refused with
/// [`TokenRejected::ClientNotNamed`], asserted through its message.
#[tokio::test]
async fn a_token_naming_no_client_is_refused_where_extra_scope_claims_are_read() {
    let rejected = established_reading_roles(
        r#"{"ver":"2.0","preferred_username":"user@example.com","roles":["sutura:catalog.read","sutura:metrics.ask","sutura:sql.run"]}"#,
    )
    .await
    .expect_err("a token naming no client grants nothing");
    assert_eq!(rejected.to_string(), "the presented token names no client in `azp`");
}

/// With `extra_scope_claims` set, a token whose client is the API itself is refused with
/// [`TokenRejected::ClientIsTheApi`], asserted through its message.
#[tokio::test]
async fn a_token_whose_client_is_the_api_is_refused_where_extra_scope_claims_are_read() {
    let rejected = established_reading_roles(&format!(
        r#"{{"azp":"{CLIENT_ID}","ver":"2.0","roles":["sutura:catalog.read"]}}"#
    ))
    .await
    .expect_err("a token whose client is the API grants nothing");
    assert_eq!(
        rejected.to_string(),
        "the presented token names this deployment's own audience as its client"
    );
}

/// With `extra_scope_claims` set, an empty `azp` names no client.
#[tokio::test]
async fn an_empty_client_is_refused_where_extra_scope_claims_are_read() {
    let rejected = established_reading_roles(r#"{"azp":"","ver":"2.0","roles":["sutura:catalog.read"]}"#)
        .await
        .expect_err("an empty client grants nothing");
    assert_eq!(rejected.to_string(), "the presented token names no client in `azp`");
}

/// With `extra_scope_claims` set, an `azp` with whitespace around it is not a client id.
#[tokio::test]
async fn the_api_id_with_whitespace_around_it_is_refused_where_extra_scope_claims_are_read() {
    let rejected = established_reading_roles(&format!(
        r#"{{"azp":" {CLIENT_ID}\t","ver":"2.0","roles":["sutura:catalog.read"]}}"#
    ))
    .await
    .expect_err("a padded client grants nothing");
    assert_eq!(rejected.to_string(), "the presented token names no client in `azp`");
}

/// With `extra_scope_claims` set, the API's own id in another case is still the API.
#[tokio::test]
async fn the_api_id_in_another_case_is_refused_where_extra_scope_claims_are_read() {
    let rejected = established_reading_roles(&format!(
        r#"{{"azp":"{}","ver":"2.0","roles":["sutura:catalog.read"]}}"#,
        CLIENT_ID.to_uppercase()
    ))
    .await
    .expect_err("the API in another case grants nothing");
    assert_eq!(
        rejected.to_string(),
        "the presented token names this deployment's own audience as its client"
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

// ------------------------------------------------------- InvalidKeySet from a PEM ----
//
// Each refusal is asserted by its message, which names one variant.

/// The key pair's self-signed certificate as PEM, the shape cert-manager writes to `tls.crt`.
pub(super) fn certificate_pem(pair: &rcgen::KeyPair) -> String {
    rcgen::CertificateParams::new(vec![String::from("sutura.example.com")])
        .expect("a test name is a subject alternative name")
        .self_signed(pair)
        .expect("a self-signed test certificate signs")
        .pem()
}

/// The key pair's RFC 7638 SHA-256 thumbprint, taken from its JWK and not from any PEM.
pub(super) fn thumbprint_of(pair: &rcgen::KeyPair) -> String {
    serde_json::from_str::<jsonwebtoken::jwk::Jwk>(&jwk("unnamed", pair))
        .expect("a test JWK is a JWK")
        .thumbprint(jsonwebtoken::jwk::ThumbprintHash::SHA256)
        .expect("a test JWK has a thumbprint")
}

/// `der` armoured under `label`, for a PEM block no generator writes.
fn pem_block(label: &str, der: &[u8]) -> String {
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    format!("-----BEGIN {label}-----\n{body}\n-----END {label}-----\n")
}

/// RFC 7638 section 3.1's example RSA key, as the `PUBLIC KEY` block an operator would hold.
const RFC_7638_EXAMPLE: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA0vx7agoebGcQSuuPiLJX
ZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tS
oc/BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ/2W+5JsGY4Hc5n9yBXArwl93lqt
7/RN5w6Cf0h4QyQ5v+65YGjQR0/FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0
zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt+bFTWhAI4vMQFh6WeZu0f
M4lFd2NcRwr3XPksINHaQ+G/xBniIqbw0Ls1jF44+csFCur+kEgU8awapJzKnqDK
gwIDAQAB
-----END PUBLIC KEY-----
";

/// RFC 7517 appendix A.1's example P-256 key, as a `PUBLIC KEY` block.
const RFC_7517_P256_EXAMPLE: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEMKBCTNIcKUSDii11ySs3526iDZ8A
iTo7Tu6KPAqv7D7gS2XpJFbZiItSs3m9+9Ue6GnvHw/GW2ZZaVtszggXIw==
-----END PUBLIC KEY-----
";

/// RFC 8037 appendix A.2's example Ed25519 key, as a `PUBLIC KEY` block.
const RFC_8037_ED25519_EXAMPLE: &str = "-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=
-----END PUBLIC KEY-----
";

/// The ids a document's key set holds, as strings.
fn pem_ids(document: &str) -> Vec<String> {
    KeySet::parse(document)
        .expect("a PEM with one key is a key set")
        .ids()
        .into_iter()
        .map(ToString::to_string)
        .collect()
}

/// The thumbprint is the RFC's own, read off the RFC's own example rather than off this library.
#[test]
fn a_pem_key_id_is_the_rfc_7638_thumbprint_of_its_key() {
    assert_eq!(pem_ids(RFC_7638_EXAMPLE), ["NzbLsXh8uDCcd-6MNwXF4W_7noWXFZAfHkxZsRGC9Xs"]);
}

/// The EC and Ed25519 thumbprints are fixed vectors, not this library's: Ed25519's is RFC 8037
/// appendix A.3's, and P-256's was computed by an RFC 7638 implementation outside this library over
/// the RFC 7517 key's `crv`, `kty`, `x` and `y`.
#[test]
fn a_pem_key_id_on_p_256_and_ed25519_is_an_independent_rfc_7638_thumbprint() {
    assert_eq!(
        pem_ids(RFC_7517_P256_EXAMPLE),
        ["cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s"]
    );
    assert_eq!(
        pem_ids(RFC_8037_ED25519_EXAMPLE),
        ["kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"]
    );
}

/// Whitespace before the armour, as a hand-edited file or a heredoc leaves it, still reads as PEM.
#[test]
fn a_pem_opened_by_whitespace_is_read_as_pem() {
    let ids = KeySet::parse(&format!("\n  {RFC_7638_EXAMPLE}"))
        .map(|set| set.ids().into_iter().map(ToString::to_string).collect::<Vec<_>>())
        .map_err(|refused| refused.to_string());
    assert_eq!(ids, Ok(vec![String::from("NzbLsXh8uDCcd-6MNwXF4W_7noWXFZAfHkxZsRGC9Xs")]));
}

/// A `PUBLIC KEY` on each curve the issuer fixtures generate is named by the thumbprint of its JWK.
#[test]
fn a_pem_public_key_on_every_supported_curve_is_named_by_its_thumbprint() {
    for algorithm in [
        &rcgen::PKCS_ECDSA_P256_SHA256,
        &rcgen::PKCS_ECDSA_P384_SHA384,
        &rcgen::PKCS_ED25519,
    ] {
        let pair = rcgen::KeyPair::generate_for(algorithm).expect("a test key pair generates");
        assert_eq!(pem_ids(&pair.public_key_pem()), [thumbprint_of(&pair)], "{algorithm:?}");
    }
}

/// The certificates after the leaf are its chain: skipped, never a key.
#[test]
fn a_certificate_chain_yields_only_the_leaf_key() {
    let (leaf, issuer) = (key_pair(), key_pair());
    let chain = format!("{}{}", certificate_pem(&leaf), certificate_pem(&issuer));
    assert_eq!(pem_ids(&chain), [thumbprint_of(&leaf)]);
}

/// A private key block is refused wherever it sits, including after a usable leaf.
#[test]
fn a_pem_holding_a_private_key_is_refused_as_private_key_material() {
    let pair = key_pair();
    for document in [
        pair.serialize_pem(),
        format!("{}{}", certificate_pem(&pair), pair.serialize_pem()),
        pem_block("EC PRIVATE KEY", b"never decoded"),
    ] {
        let refused = KeySet::parse(&document).expect_err("a private key is not a key set");
        assert_eq!(
            refused.to_string(),
            "the key set holds a private key. A verifier holds public keys only: point security.inbound.key_set_file at the certificate or the public key"
        );
    }
}

/// A private key block is refused before anything else is read: inside a chain certificate, after
/// text that is not armour, and after a block that would itself be refused.
#[test]
fn a_private_key_block_anywhere_in_the_document_is_refused() {
    let pair = key_pair();
    for document in [
        format!(
            "{}-----BEGIN CERTIFICATE-----\n{}-----END CERTIFICATE-----\n",
            certificate_pem(&pair),
            pair.serialize_pem()
        ),
        format!("Bag Attributes\n{}", pair.serialize_pem()),
        format!("{}{}", pem_block("X509 CRL", b"a revocation list"), pair.serialize_pem()),
    ] {
        let refused = KeySet::parse(&document).expect_err("a private key is not a key set");
        assert_eq!(
            refused.to_string(),
            "the key set holds a private key. A verifier holds public keys only: point security.inbound.key_set_file at the certificate or the public key",
            "{document}"
        );
    }
}

/// One signing key per file: a second `PUBLIC KEY`, or a `PUBLIC KEY` beside a certificate in either
/// order, is refused.
#[test]
fn a_second_key_in_a_pem_is_refused() {
    let (first, second) = (key_pair(), key_pair());
    for document in [
        format!("{}{}", first.public_key_pem(), second.public_key_pem()),
        format!("{}{}", first.public_key_pem(), certificate_pem(&second)),
        format!("{}{}", certificate_pem(&first), second.public_key_pem()),
    ] {
        let refused = KeySet::parse(&document).expect_err("two keys in one PEM are not a key set");
        assert_eq!(
            refused.to_string(),
            "the PEM key set holds more than one key: one PUBLIC KEY, or one certificate and its chain"
        );
    }
}

#[test]
fn a_pem_block_that_is_neither_a_certificate_nor_a_public_key_is_refused() {
    let refused = KeySet::parse(&pem_block("X509 CRL", b"a revocation list")).expect_err("a CRL carries no key");
    assert_eq!(
        refused.to_string(),
        "the PEM key set holds a block that is neither a CERTIFICATE nor a PUBLIC KEY"
    );
}

#[test]
fn malformed_pem_armour_is_refused_as_not_pem() {
    let key = key_pair().public_key_pem();
    for document in [
        key.replace("-----END PUBLIC KEY-----\n", ""),
        format!("{key}a line outside any block\n"),
        key.replace("-----END PUBLIC KEY-----", "-----BEGIN PUBLIC KEY-----"),
        String::from("-----BEGIN PUBLIC KEY-----\n!!!!\n-----END PUBLIC KEY-----\n"),
    ] {
        let refused = KeySet::parse(&document).expect_err("malformed armour is not a key set");
        assert_eq!(
            refused.to_string(),
            "the key set opens as PEM and is not well-formed PEM",
            "{document}"
        );
    }
}

#[test]
fn a_certificate_that_does_not_parse_is_refused() {
    let refused = KeySet::parse(&pem_block("CERTIFICATE", b"not DER")).expect_err("noise is not a certificate");
    assert_eq!(refused.to_string(), "the PEM key set's certificate could not be read");
}

/// A public key that does not parse, and a P-256 point under the P-384 curve name - which the library
/// alone would read as a P-256 key by its length.
#[test]
fn a_public_key_that_does_not_parse_or_disagrees_with_its_curve_is_refused() {
    let mut mislabelled = vec![
        0x30, 0x56, 0x30, 0x10, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x22,
        0x03, 0x42, 0x00,
    ];
    mislabelled.extend_from_slice(key_pair().public_key_raw());
    for document in [pem_block("PUBLIC KEY", b"not DER"), pem_block("PUBLIC KEY", &mislabelled)] {
        let refused = KeySet::parse(&document).expect_err("an unreadable public key is not a key set");
        assert_eq!(refused.to_string(), "the PEM key set's public key could not be read");
    }
}
