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
    /// Ten minutes, the same window `Credential::assertion` uses and for the same reason: an
    /// assertion is a bearer credential in flight, and its window is its replay window.
    const LIVES_FOR: u64 = 600;

    let url = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = serde_json::json!({ "alg": "RS256", "typ": "JWT", "kid": private_key_id });
    let claims = serde_json::json!({
        "iss": client_email,
        "sub": client_email,
        "aud": TOKEN_ENDPOINT,
        "target_audience": target_audience,
        "iat": now_unix_seconds,
        "exp": now_unix_seconds.saturating_add(LIVES_FOR),
    });
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
    if !status.is_success() {
        let refusal: TokenRefusal = serde_json::from_str(&body).unwrap_or(TokenRefusal { error: None });
        return Err(TokenUnavailable::Refused {
            status: status.as_u16(),
            named: crate::wire::bounded(refusal.error),
        });
    }
    let response: IdTokenResponse = serde_json::from_str(&body).map_err(|cause| TokenUnavailable::NotADocument { cause })?;
    let token = response
        .id_token
        .filter(|t| !t.trim().is_empty())
        .ok_or(TokenUnavailable::NoToken)?;
    Ok(Secret::new(token))
}
