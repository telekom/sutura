//! The unauthenticated token boundary: arbitrary bytes presented as a bearer token.
//!
//! **The boundary.** This target exercises the textual token parser before a signature has been
//! accepted. `TokenValidator::key_id` decodes the untrusted header and turns its `kid` into a map
//! key; only a key selected by that path reaches signature verification.
//!
//! **What is asserted beyond "did not abort".** A generated input that reaches verification must
//! not verify against the fixed public key. The fixture contains no private signing material or
//! signing oracle, so a successful verification would be a signature-verification bypass.
//!
//! **Limits.** Inputs larger than [`sutura_http::inbound::MAX_TOKEN_BYTES`] are discarded before
//! lossy decoding; the production unit test covers the oversized refusal. The HTTP boundary rejects
//! non-UTF-8 header values before this target's `String::from_utf8_lossy` conversion, so invalid
//! wire bytes are not represented here. Signed claim deserialization, token type, actor nesting,
//! scopes and principal parsing are intentionally out of scope: generated input cannot cross the
//! signature check, and this harness has no private key. This target therefore does not claim
//! coverage of the post-signature claims path or caller impersonation.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sutura_config::{
    InboundIdentity, IssuerUrl, KeySetFile, PinnedAlgorithms, RequiredTokenType, ResourceIdentifier, SigningAlgorithm,
};
use sutura_http::inbound::keys::KeySet;
use sutura_http::inbound::token::{MAX_TOKEN_BYTES, TokenValidator};

/// A verifying key in the shape the deployment reads.
///
/// Only the public EC point is committed. No private signing material is present or available to
/// this harness, which is what makes an `Ok` result from `verify` a security finding.
const PUBLIC_KEY_SET: &str = r#"{"keys":[{"kty":"EC","crv":"P-256","kid":"fuzz-seed-key","use":"sig","alg":"ES256","x":"ge0gp9sWfrleV4-8_B6u_hVpGbmJnOpKBEHy07sJEVk","y":"0b7NwSMh9mtMtfTwEV47263Ti8PbhwCV2y_6mU5_2h4"}]}"#;

/// Build declaration-shaped fixtures once; the declaration is not fuzz input.
fn validator() -> &'static (TokenValidator, KeySet) {
    static BUILT: std::sync::OnceLock<(TokenValidator, KeySet)> = std::sync::OnceLock::new();
    BUILT.get_or_init(|| {
        let identity = InboundIdentity::Direct {
            resource: ResourceIdentifier::parse("https://sutura.example.com").expect("fixture audience is valid"),
            authorization_server: IssuerUrl::parse("https://issuer.example.com").expect("fixture issuer is valid"),
            key_set: KeySetFile::parse("/nonexistent/jwks.json").expect("fixture path is valid"),
            algorithms: PinnedAlgorithms::of(SigningAlgorithm::Es256),
            token_type: RequiredTokenType::access_token(),
        };
        let keys = KeySet::parse(PUBLIC_KEY_SET).expect("fixture key set is valid");
        (TokenValidator::new(&identity.requirement()), keys)
    })
}

fuzz_target!(|data: &[u8]| {
    // Keep the harness's own work bounded before converting arbitrary bytes into text. The
    // production `MAX_TOKEN_BYTES` refusal is covered by the HTTP unit test; this target focuses
    // on inputs that can reach the parser and verifier after that refusal.
    if data.len() > MAX_TOKEN_BYTES {
        return;
    }
    let presented = String::from_utf8_lossy(data);
    if presented.len() > MAX_TOKEN_BYTES {
        return;
    }
    let (validator, keys) = validator();

    let Ok(key_id) = TokenValidator::key_id(&presented) else {
        return;
    };
    let Some(key) = keys.get(&key_id) else {
        return;
    };

    // Reaching this call mirrors the production order: header parsing and key selection happen
    // before verification. The public-only fixture means success is the finding, not the panic.
    assert!(
        validator.verify(&presented, &key).is_err(),
        "a generated token verified without private signing material"
    );
});
