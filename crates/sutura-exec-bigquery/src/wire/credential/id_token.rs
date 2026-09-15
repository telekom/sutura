//! [`mint_id_token`]: a service account's own key traded for a Google-issued OIDC ID token, rather
//! than the access token [`AccessTokens::bearer`](crate::wire::AccessTokens::bearer) mints.
//!
//! **CI-only, and a submodule of `credential` rather than a sibling of it for a visibility
//! reason.** [`super::Kind`], `super::TOKEN_ENDPOINT` and `super::MAX_ANSWER_BYTES` are private to
//! that module; a descendant module sees them under ordinary Rust privacy, a sibling would not.
//!
//! **Free functions, not a second `impl Credential` block.** `clippy::multiple_inherent_impl` is
//! denied workspace-wide, so [`Credential::mint_id_token`](super::Credential::mint_id_token) - the
//! one public line this module adds to that type's surface - lives in `credential.rs`'s existing
//! `impl` block and delegates here in one line. Everything else stays a plain function taking
//! `&Credential`, which reaches the same private fields a method would.
//!
//! Nothing in `AccessTokens` calls anything here - the port this crate ships needs an access
//! token, never an identity token - so this exists for `examples/mint_subject_assertion.rs`, which
//! a workflow step runs to mint the two subject assertions
//! `crates/sutura-exec-bigquery/tests/exchanged_identity.rs` needs, from the same per-principal
//! keys the environment already holds rather than from a new long-lived secret - telekom/sutura#376.

use base64::Engine as _;
use sutura_domain::identity::Secret;

use super::{Credential, Kind, MAX_ANSWER_BYTES, TOKEN_ENDPOINT, TokenRefusal, TokenUnavailable};
use crate::wire::CallDeadline;

/// The token endpoint's answer to an assertion carrying `target_audience` rather than `scope`.
///
/// **A separate document from `TokenResponse`, not an added field on it**: the endpoint returns
/// EITHER `access_token` OR `id_token` depending on which claim the assertion carried, never both,
/// so one struct with two `Option` fields would be able to represent an answer that cannot occur.
/// No `Debug`, for `TokenResponse`'s reason: an ID token is a bearer credential in the clear until
/// [`mint_id_token`] wraps it in a [`Secret`].
#[derive(serde::Deserialize)]
struct IdTokenResponse {
    #[serde(default)]
    id_token: Option<String>,
}

/// The signed assertion a service account trades for a Google-issued ID TOKEN rather than an
/// access token.
///
/// **A sibling of `Credential::assertion`, not a generalisation of it** - the two grants ask the
/// same endpoint for two different documents, and the field that decides which one comes back is
/// `target_audience` in place of `scope` (this is the mechanism `google-auth`'s
/// `service_account.IDTokenCredentials` implements, and it is why this is a second function rather
/// than a parameter on the first - a caller asking for one or the other is a choice at the call
/// site, not a runtime branch inside one signature). Everything about HOW the assertion is built
/// and signed is identical to `Credential::assertion`, which is why this is the only other place
/// that reads `client_email`/`private_key_id`/`private_key` directly.
/// Ten minutes, the same window `Credential::assertion` uses and for the same reason: an
/// assertion is a bearer credential in flight, and its window is its replay window.
const LIVES_FOR: u64 = 600;

/// The unsigned claims of the minted assertion, as a pure function of its inputs.
///
/// Split out of [`id_token_assertion`] so the claim set and replay window can be pinned offline:
/// the fixture cannot reach the signing half, which needs a real 2048-bit key, but the claims
/// document is the decision and it needs none of them.
fn id_token_claims(client_email: &str, target_audience: &str, now_unix_seconds: u64) -> serde_json::Value {
    serde_json::json!({
        "iss": client_email,
        "sub": client_email,
        "aud": TOKEN_ENDPOINT,
        "target_audience": target_audience,
        "iat": now_unix_seconds,
        "exp": now_unix_seconds.saturating_add(LIVES_FOR),
    })
}

#[expect(
    clippy::disallowed_methods,
    reason = "the private key has to be decoded to sign with it, exactly as `Credential::assertion` argues"
)]
fn id_token_assertion(
    client_email: &str,
    private_key_id: &str,
    private_key: &Secret,
    target_audience: &str,
    now_unix_seconds: u64,
) -> Result<String, TokenUnavailable> {
    let url = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = serde_json::json!({ "alg": "RS256", "typ": "JWT", "kid": private_key_id });
    let claims = id_token_claims(client_email, target_audience, now_unix_seconds);
    let mut signing_input = url.encode(header.to_string().as_bytes());
    signing_input.push('.');
    signing_input.push_str(&url.encode(claims.to_string().as_bytes()));

    let der = base64::engine::general_purpose::STANDARD
        .decode(private_key.expose_secret().as_bytes())
        .map_err(|_ignored| TokenUnavailable::NotSigned)?;
    let key = ring::signature::RsaKeyPair::from_pkcs8(&der).map_err(|cause| TokenUnavailable::Unsigned { cause })?;
    let mut signature = vec![0_u8; key.public().modulus_len()];
    key.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|_ignored| TokenUnavailable::NotSigned)?;

    let mut token = signing_input;
    token.push('.');
    token.push_str(&url.encode(&signature));
    Ok(token)
}

/// Trades a service account's own key for a Google-issued OIDC ID token audienced to
/// `target_audience`, with no `iamcredentials` call and no new IAM grant.
///
/// **What this is for:** `target_audience` is the value the workload-identity pool provider's
/// `allowed_audiences` accepts (`test-infra/pulumi/google/__main__.py`'s `workload_provider`) - the
/// resulting `id_token` IS the `subject_token` `crate::sts::StsExchange` presents to Google's STS.
/// It is a FEDERATED identity once exchanged (the token's `sub` is this account's numeric unique
/// id, never its email), which is why the exchange still needs the
/// `iamcredentials.generateAccessToken` hop this crate does not implement, tracked on
/// telekom/sutura#376.
///
/// Only a `service_account` credential can do this - trading a refresh token for an ID token is a
/// different grant this function does not build, so `Kind::AuthorizedUser` is refused by name
/// rather than attempted.
///
/// `pub(super)`, not `pub`: [`Credential::mint_id_token`](super::Credential::mint_id_token) is the
/// one public entry point, in `credential.rs`'s own `impl` block, so this crate has exactly one
/// `impl Credential` (`clippy::multiple_inherent_impl` is denied workspace-wide).
pub(super) fn mint_id_token(
    credential: &Credential,
    target_audience: &str,
    now_unix_seconds: u64,
    within: CallDeadline,
) -> Result<Secret, TokenUnavailable> {
    /// The grant a signed assertion is presented under - the same grant `Credential::bearer` uses
    /// for a service account; only the claims inside the assertion differ.
    const ASSERTION_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

    let Kind::ServiceAccount {
        ref client_email,
        ref private_key_id,
        ref private_key,
        ..
    } = credential.kind
    else {
        return Err(TokenUnavailable::NotAServiceAccount);
    };
    let assertion = id_token_assertion(client_email, private_key_id, private_key, target_audience, now_unix_seconds)?;
    exchange_id_token(
        credential,
        [("grant_type", ASSERTION_GRANT), ("assertion", assertion.as_str())],
        within,
    )
}

/// Posts a form to the token endpoint and reads an ID token out of the answer.
///
/// **`Credential::exchange`'s sibling, not its generalisation** - that function parses
/// `access_token` into a `Bearer` with a deadline this crate's transport consumes; this one parses
/// `id_token` into a bare [`Secret`], because the caller here immediately hands it to a different
/// exchange (Google's STS) rather than presenting it at this crate's own transport. A shared parse
/// would have to make one of the two document shapes optional on the other's behalf.
///
/// No `#[expect(clippy::disallowed_methods)]` here, matching `Credential::exchange`: the form is
/// already assembled by the caller, so nothing in this function calls `Secret::expose_secret` -
/// that exposure is [`id_token_assertion`]'s, which carries the expectation.
fn exchange_id_token<'form, F>(credential: &Credential, form: F, within: CallDeadline) -> Result<Secret, TokenUnavailable>
where
    F: IntoIterator<Item = (&'form str, &'form str)>,
{
    let left = within.remaining().ok_or(TokenUnavailable::DeadlineSpent)?;
    let mut answer = credential
        .agent
        .agent()
        .post(TOKEN_ENDPOINT)
        .config()
        .timeout_global(Some(CallDeadline::socket(left)))
        .build()
        .send_form(form)
        .map_err(|cause| TokenUnavailable::Unreachable { cause: Box::new(cause) })?;
    let status = answer.status();
    let body = answer
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_string()
        .map_err(|cause| TokenUnavailable::Unreadable { cause: Box::new(cause) })?;
    decide_id_token_answer(status.as_u16(), &body)
}

/// Decides a token-endpoint answer given its status and body, with no transport involved.
///
/// Split out of [`exchange_id_token`] so a non-2xx refusal can be pinned offline - the wire half
/// still lives where it lives, but the decision "a non-2xx status is a typed [`TokenUnavailable::Refused`],
/// a 2xx body is the `id_token` document or nothing" needs no network and no key.
fn decide_id_token_answer(status_code: u16, body: &str) -> Result<Secret, TokenUnavailable> {
    if !(200..300).contains(&status_code) {
        // A refusal document is best-effort: what is guaranteed is the status, and the code is
        // read out of the body when the body is the document the provider documents.
        let refusal: TokenRefusal = serde_json::from_str(body).unwrap_or(TokenRefusal { error: None });
        return Err(TokenUnavailable::Refused {
            status: status_code,
            named: crate::wire::bounded(refusal.error),
        });
    }
    let response: IdTokenResponse = serde_json::from_str(body).map_err(|cause| TokenUnavailable::NotADocument { cause })?;
    let token = response
        .id_token
        .filter(|t| !t.trim().is_empty())
        .ok_or(TokenUnavailable::NoToken)?;
    Ok(Secret::new(token))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::super::Document;
    use super::{
        Credential, LIVES_FOR, TOKEN_ENDPOINT, TokenUnavailable, decide_id_token_answer, id_token_claims, mint_id_token,
    };
    use crate::wire::{BytesBilledCeiling, CallDeadline, JobBounds, QueryDeadline, WireAgent};

    /// The pinned client, the only kind this module accepts.
    fn pinned() -> WireAgent {
        WireAgent::pinned(JobBounds::of(
            QueryDeadline::parse(30).expect("30 seconds is a deadline"),
            BytesBilledCeiling::parse(1024 * 1024).expect("a mebibyte is a ceiling"),
        ))
    }

    /// Where a refusal says the file was. A path that does not exist, never read.
    fn at() -> std::path::PathBuf {
        std::path::PathBuf::from("/nonexistent/credentials.json")
    }

    /// A complete `authorized_user`. None of these values is a credential; they are the shape.
    fn authorized_user() -> Credential {
        let document = Document {
            kind: String::from("authorized_user"),
            universe_domain: None,
            client_id: Some(String::from("an-installed-app.apps.example")),
            client_secret: Some(String::from("not-a-secret")),
            refresh_token: Some(String::from("not-a-token")),
            client_email: None,
            private_key: None,
            private_key_id: None,
            project_id: None,
        };
        Credential::read_document(document, &at(), pinned()).expect("a complete user credential reads")
    }

    #[test]
    fn id_token_claims_are_exactly_the_minted_set_with_no_scope() {
        let claims = id_token_claims("a-robot@example.invalid", "https://aud.invalid", 1_700_000_000);
        let map = claims.as_object().expect("claims are a JSON object");
        let keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(keys, BTreeSet::from(["iss", "sub", "aud", "target_audience", "iat", "exp"]));
        assert!(!map.contains_key("scope"), "the ID-token grant carries no `scope`");
        assert_eq!(map["iss"], "a-robot@example.invalid");
        assert_eq!(map["sub"], "a-robot@example.invalid");
        assert_eq!(map["aud"], TOKEN_ENDPOINT);
        assert_eq!(map["target_audience"], "https://aud.invalid");
        assert_eq!(map["iat"].as_u64(), Some(1_700_000_000));
        // The replay window: a ten-minute bearer in flight.
        assert_eq!(map["exp"].as_u64().unwrap() - map["iat"].as_u64().unwrap(), LIVES_FOR);
        assert_eq!(LIVES_FOR, 600);
    }

    #[test]
    fn an_authorized_user_credential_is_refused_by_name_with_no_socket() {
        // `mint_id_token` returns before any exchange for a non-service-account, so no socket is
        // opened and no deadline is spent: the refusal is by name, never attempted.
        let user = authorized_user();
        let within = CallDeadline::opened(QueryDeadline::parse(30).expect("30 seconds is a deadline"));
        match mint_id_token(&user, "https://aud.invalid", 1_700_000_000, within) {
            Err(TokenUnavailable::NotAServiceAccount) => {}
            other => panic!("expected NotAServiceAccount, got {other:?}"),
        }
    }

    #[test]
    fn a_non_2xx_token_endpoint_answer_is_a_typed_refusal() {
        // The decision carries the status and the provider's bounded code, never free text - the
        // refusal vocabulary `TokenUnavailable::Refused` exists to hold.
        let body = r#"{"error":"invalid_client","error_description":"the client is not valid"}"#;
        match decide_id_token_answer(400, body) {
            Err(TokenUnavailable::Refused { status, named }) => {
                assert_eq!(status, 400);
                assert_eq!(named, "invalid_client");
            }
            other => panic!("expected a typed refusal, got {other:?}"),
        }
    }
}
