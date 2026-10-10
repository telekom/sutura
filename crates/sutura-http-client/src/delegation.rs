//! The real [`DelegationExchange`] over the shared outbound client: an RFC 8693 token exchange, or
//! Microsoft Entra ID's on-behalf-of request - see [`Grant`].
//!
//! What is checked on the answer, and what is not:
//!
//! - `token_type` is `Bearer` and, for a token exchange, `issued_token_type` is the access token
//!   this asked for (an on-behalf-of answer carries no `issued_token_type`);
//! - the token is a compact JWT whose payload `aud` carries the requested audience and whose `exp`
//!   lies after the instant of the check.
//!
//! **The payload is decoded, not verified.** It arrived over TLS from the endpoint the deployment
//! declared, and the signature is the data system's to verify, against the issuer's own keys. So
//! the claim checks catch an identity provider configured to issue the wrong audience; they are
//! not a defence against the identity provider itself.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Budget, Endpoint, InvalidEndpoint, ReadBounds, ShownEndpoint};
use base64::Engine as _;
use sutura_domain::identity::{Delegated, DelegationExchange, DelegationFailed, RequestedAudience, Secret};
use ureq::http::Uri;

const TOKEN_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const JWT_BEARER: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";
const ACCESS_TOKEN: &str = "urn:ietf:params:oauth:token-type:access_token";

/// The identity provider's token endpoint: `https://` to any host, `http://` to an IP loopback literal only.
///
/// The origin is held to [`Endpoint::parse`]'s scheme rule; unlike an [`Endpoint`] it keeps its
/// path, and it refuses a query, a fragment and any `@` - in the authority or, where an unencoded
/// `/` in a password ends the parsed authority early, in the path.
///
/// A loopback endpoint is dialled directly, never through a proxy - [`crate::agent()`]'s
/// own pin; `https://` to any other host keeps the agent's proxy, which an identity provider behind
/// an egress proxy needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenEndpoint(String);

impl TokenEndpoint {
    /// # Errors
    ///
    /// [`InvalidEndpoint`], the shared client's own refusal.
    pub fn parse(raw: &str) -> Result<Self, InvalidEndpoint> {
        let beyond = || InvalidEndpoint::PathBeyondRoot {
            given: ShownEndpoint::of(raw),
        };
        if raw.contains('#') {
            return Err(beyond());
        }
        let uri: Uri = raw
            .parse()
            .map_err(|_cause: ureq::http::uri::InvalidUri| InvalidEndpoint::NotAnHttpUrl {
                given: ShownEndpoint::of(raw),
            })?;
        if uri.query().is_some() {
            return Err(beyond());
        }
        if uri.path().contains('@') {
            return Err(InvalidEndpoint::CredentialsInUrl {
                given: ShownEndpoint::of(raw),
            });
        }
        let origin = Endpoint::parse(&format!(
            "{}://{}",
            uri.scheme_str().unwrap_or_default(),
            uri.authority().map_or("", |authority| authority.as_str())
        ))?;
        Ok(Self(format!("{}{}", origin.as_str(), uri.path())))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a declared client identifier is unusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnusableClientId {
    /// Empty, over 255 bytes, or carrying a space or a non-printable byte.
    #[error("the delegation client identifier is empty, over 255 bytes, or not printable ASCII")]
    Unusable,
}

/// This deployment's client at the identity provider and its secret (`client_secret_post`).
///
/// **The most sensitive value in the deployment**: whoever holds it can obtain a pool-audience
/// token for any subject whose inbound token they also hold. `Debug` prints the identifier and `Secret`'s redaction; there is no
/// `Display`.
#[derive(Debug, Clone)]
pub struct ExchangeClient {
    id: String,
    secret: Secret,
}

impl ExchangeClient {
    /// # Errors
    ///
    /// [`UnusableClientId`] for an identifier no form can carry unambiguously.
    pub fn new(id: &str, secret: Secret) -> Result<Self, UnusableClientId> {
        if id.is_empty() || id.len() > 255 || !id.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(UnusableClientId::Unusable);
        }
        Ok(Self {
            id: String::from(id),
            secret,
        })
    }
}

/// Which request an [`OverHttp`] sends its endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    /// RFC 8693: the subject token, asking for `audience`.
    TokenExchange,
    /// Microsoft Entra ID's on-behalf-of: the subject token as the `assertion`, asking for the
    /// scope `<audience>/.default`.
    OnBehalfOf,
}

/// [`DelegationExchange`] over HTTP.
#[derive(Debug)]
pub struct OverHttp {
    endpoint: TokenEndpoint,
    client: ExchangeClient,
    agent: sutura_tls::Rotating<ureq::Agent>,
    bounds: ReadBounds,
    grant: Grant,
}

impl OverHttp {
    /// An RFC 8693 token exchange; [`Self::granting`] asks another way.
    #[must_use]
    pub const fn new(
        endpoint: TokenEndpoint,
        client: ExchangeClient,
        agent: sutura_tls::Rotating<ureq::Agent>,
        bounds: ReadBounds,
    ) -> Self {
        Self {
            endpoint,
            client,
            agent,
            bounds,
            grant: Grant::TokenExchange,
        }
    }

    #[must_use]
    pub const fn granting(mut self, grant: Grant) -> Self {
        self.grant = grant;
        self
    }
}

impl DelegationExchange for OverHttp {
    fn exchange(&self, subject: &Secret, audience: &RequestedAudience) -> Result<Delegated, DelegationFailed> {
        let unreachable = |cause: ureq::Error| DelegationFailed::Unreachable { cause: Box::new(cause) };
        let scope;
        #[expect(
            clippy::disallowed_methods,
            reason = "the subject token and the client secret are the two values this request exists \
                      to send; they go into one form body over the declared endpoint and nowhere \
                      else, and no error built below carries either"
        )]
        let form: &[(&str, &str)] = match self.grant {
            Grant::TokenExchange => &[
                ("grant_type", TOKEN_EXCHANGE),
                ("subject_token", subject.expose_secret()),
                ("subject_token_type", ACCESS_TOKEN),
                ("requested_token_type", ACCESS_TOKEN),
                ("audience", audience.as_str()),
                ("client_id", self.client.id.as_str()),
                ("client_secret", self.client.secret.expose_secret()),
            ],
            Grant::OnBehalfOf => {
                scope = format!("{}/.default", audience.as_str());
                &[
                    ("grant_type", JWT_BEARER),
                    ("assertion", subject.expose_secret()),
                    ("requested_token_use", "on_behalf_of"),
                    ("scope", scope.as_str()),
                    ("client_id", self.client.id.as_str()),
                    ("client_secret", self.client.secret.expose_secret()),
                ]
            }
        };
        let mut response = self
            .agent
            .current()
            .post(self.endpoint.as_str())
            .config()
            .timeout_global(Some(Budget::socket(self.bounds.timeout())))
            .build()
            .send_form(form.iter().copied())
            .map_err(unreachable)?;
        let cap = self.bounds.max_response_bytes();
        let text = response
            .body_mut()
            .with_config()
            .limit(cap.saturating_add(cap.max(1024)))
            .read_to_string()
            .map_err(unreachable)?;
        if text.len() as u64 > cap {
            return Err(DelegationFailed::TooLarge { cap });
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(u64::MAX, |since| since.as_secs());
        answered(self.grant, response.status().as_u16(), &text, audience, now)
    }
}

/// Validates the identity provider's answer at `now`. Pure, so every refusal has a cell without a socket.
pub(crate) fn answered(
    grant: Grant,
    status: u16,
    text: &str,
    audience: &RequestedAudience,
    now: u64,
) -> Result<Delegated, DelegationFailed> {
    let body: Option<serde_json::Value> = serde_json::from_str(text).ok();
    let field = |name: &str| body.as_ref()?.get(name)?.as_str();
    if !(200..300).contains(&status) {
        return Err(DelegationFailed::Refused {
            status,
            error: field("error")
                .filter(|code| code.len() <= 64 && code.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_'))
                .map(String::from),
        });
    }
    if body.is_none() {
        return Err(DelegationFailed::Malformed { what: "not JSON" });
    }
    if (grant == Grant::TokenExchange && field("issued_token_type") != Some(ACCESS_TOKEN))
        || !field("token_type").is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(DelegationFailed::WrongTokenType);
    }
    let token = field("access_token").ok_or(DelegationFailed::Malformed { what: "no access_token" })?;
    let claims = claims_of(token).ok_or(DelegationFailed::Malformed {
        what: "not a compact JWT",
    })?;
    let carries = match claims.get("aud") {
        Some(serde_json::Value::String(single)) => single == audience.as_str(),
        Some(serde_json::Value::Array(many)) => many.iter().any(|one| one.as_str() == Some(audience.as_str())),
        _ => false,
    };
    if !carries {
        return Err(DelegationFailed::WrongAudience);
    }
    let not_after = claims
        .get("exp")
        .and_then(serde_json::Value::as_u64)
        .ok_or(DelegationFailed::Malformed { what: "no numeric exp" })?;
    if not_after <= now {
        return Err(DelegationFailed::AlreadyExpired);
    }
    Ok(Delegated::new(Secret::new(token), not_after))
}

/// The payload of a compact JWS, decoded and NOT verified - see the module header.
fn claims_of(token: &str) -> Option<serde_json::Value> {
    let mut segments = token.split('.');
    let (Some(_header), Some(payload), Some(_signature), None) =
        (segments.next(), segments.next(), segments.next(), segments.next())
    else {
        return None;
    };
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests;
