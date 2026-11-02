//! The RFC 6749 client credentials grant: this deployment's own access token, for a system that
//! authorizes the deployment rather than the caller.
//!
//! What is checked on the answer: a success status, JSON, a `token_type` of `Bearer`, a non-empty
//! `access_token` and a positive integer `expires_in`. **The token is opaque and never decoded**: the
//! system it is presented to verifies it, so a token for the wrong audience is refused there, not
//! here.
//!
//! **The token is the deployment's, not the caller's.** What it may read, every caller of a reader
//! built on it reads; nothing here knows who is asking.

use base64::Engine as _;
use parking_lot::Mutex;
use sutura_domain::identity::Secret;

use super::{ExchangeClient, TokenEndpoint, Unsent};
use crate::ReadBounds;

/// Seconds before its expiry a held token stops being presented, so one does not lapse in flight.
const MARGIN: u64 = 60;

const GRANT: (&str, &str) = ("grant_type", "client_credentials");

/// How the client authenticates at the token endpoint (RFC 6749, section 2.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    /// `client_secret_basic`: an `Authorization: Basic` header over the identifier and the secret,
    /// each form-encoded first as the RFC asks. A token endpoint that does not decode them refuses an
    /// identifier or secret outside `[A-Za-z0-9*-._]`.
    Basic,
    /// `client_secret_post`: `client_id` and `client_secret` in the form body.
    Post,
}

/// The `scope` a request asks for (RFC 6749, section 3.3): tokens of printable ASCII other than `"`
/// and `\`, separated by single spaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope(String);

impl Scope {
    /// # Errors
    ///
    /// [`UnusableScope`] for a value the RFC's `scope` syntax refuses.
    pub fn parse(raw: &str) -> Result<Self, UnusableScope> {
        let token =
            |token: &str| !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_graphic() && !b"\"\\".contains(&byte));
        if raw.split(' ').all(token) {
            Ok(Self(String::from(raw)))
        } else {
            Err(UnusableScope)
        }
    }
}

/// A scope RFC 6749 cannot carry: empty, a space at either edge or twice, or a byte outside the
/// token set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a scope is tokens of printable ASCII other than `\"` and `\\`, separated by single spaces")]
pub struct UnusableScope;

/// Why the token endpoint produced no usable token.
#[derive(Debug, thiserror::Error)]
pub enum ClientCredentialsFailed {
    /// The token endpoint could not be reached or its answer not read.
    #[error("the token endpoint could not be reached")]
    Unreachable {
        #[source]
        cause: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The token endpoint answered with a non-success status.
    #[error("the token endpoint refused the client credentials ({status}, {})", error.as_deref().unwrap_or("no error code"))]
    Refused {
        /// The HTTP status.
        status: u16,
        /// The RFC 6749 `error` code, when it had the registered shape.
        error: Option<String>,
    },
    /// The answer was larger than the deployment's cap.
    #[error("the token endpoint's answer is over the {cap}-byte cap")]
    TooLarge {
        /// The cap.
        cap: u64,
    },
    /// The answer lacked a field this grant needs, or carried one in an unusable shape.
    #[error("the token endpoint's answer is malformed: {what}")]
    Malformed {
        /// Which part, named here rather than quoted from the answer.
        what: &'static str,
    },
    /// The token endpoint issued something other than a bearer access token.
    #[error("the token endpoint issued something other than a bearer access token")]
    WrongTokenType,
}

/// The client credentials grant at one token endpoint, holding the last token it was issued.
#[derive(Debug)]
pub struct ClientCredentials {
    endpoint: TokenEndpoint,
    client: ExchangeClient,
    auth: ClientAuth,
    scope: Option<Scope>,
    agent: sutura_tls::Rotating<ureq::Agent>,
    bounds: ReadBounds,
    held: Mutex<Option<Held>>,
}

/// An issued token and the second since the epoch after which it is no longer presented.
#[derive(Debug)]
struct Held {
    token: Secret,
    not_after: u64,
}

impl ClientCredentials {
    #[must_use]
    pub const fn new(
        endpoint: TokenEndpoint,
        client: ExchangeClient,
        auth: ClientAuth,
        agent: sutura_tls::Rotating<ureq::Agent>,
        bounds: ReadBounds,
    ) -> Self {
        Self {
            endpoint,
            client,
            auth,
            scope: None,
            agent,
            bounds,
            held: Mutex::new(None),
        }
    }

    /// Asks for `scope` in every request. Without it no `scope` is sent, and the token endpoint
    /// grants its default.
    #[must_use]
    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = Some(scope);
        self
    }

    /// The held token while more than a minute of it remains, otherwise a new one.
    ///
    /// Its lifetime counts from before the request was sent, so it is never held longer than the
    /// token endpoint granted. One request at a time: a caller arriving during one waits for it.
    ///
    /// # Errors
    ///
    /// [`ClientCredentialsFailed`]: nothing usable came back.
    pub fn bearer(&self) -> Result<Secret, ClientCredentialsFailed> {
        let mut held = self.held.lock();
        let now = super::now();
        if let Some(current) = held.as_ref()
            && now.saturating_add(MARGIN) < current.not_after
        {
            return Ok(current.token.clone());
        }
        let issued = self.requested(now)?;
        let token = issued.token.clone();
        *held = Some(issued);
        drop(held);
        Ok(token)
    }

    fn requested(&self, now: u64) -> Result<Held, ClientCredentialsFailed> {
        #[expect(
            clippy::disallowed_methods,
            reason = "the client secret is the value this request exists to send; it goes into one \
                      header or one form body over the declared endpoint and nowhere else, and no \
                      error built below carries it"
        )]
        let secret = self.client.secret.expose_secret();
        let authorization = match self.auth {
            ClientAuth::Basic => Some(basic(&self.client.id, secret)),
            ClientAuth::Post => None,
        };
        let mut form = vec![GRANT];
        if self.auth == ClientAuth::Post {
            form.extend([("client_id", self.client.id.as_str()), ("client_secret", secret)]);
        }
        if let Some(ref scope) = self.scope {
            form.push(("scope", scope.0.as_str()));
        }
        let (status, text) = super::post(&self.agent, &self.endpoint, self.bounds, authorization.as_ref(), &form).map_err(
            |unsent| match unsent {
                Unsent::Unreachable(cause) => ClientCredentialsFailed::Unreachable { cause: Box::new(cause) },
                Unsent::TooLarge(cap) => ClientCredentialsFailed::TooLarge { cap },
            },
        )?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the answer is read here and nowhere else; no error built from it carries the token"
        )]
        answered(status, text.expose_secret(), now)
    }
}

/// The `Authorization` value over the form-encoded identifier and secret.
#[expect(
    clippy::disallowed_methods,
    reason = "the pair is encoded straight into the header value, itself a Secret"
)]
fn basic(id: &str, secret: &str) -> Secret {
    let pair = Secret::new(format!("{}:{}", form_encoded(id), form_encoded(secret)));
    Secret::new(format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(pair.expose_secret())
    ))
}

/// `application/x-www-form-urlencoded`, which RFC 6749 asks of both halves of a Basic credential.
fn form_encoded(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || b"*-._".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            for nibble in [byte >> 4, byte & 0xF] {
                encoded.extend(char::from_digit(u32::from(nibble), 16).map(|digit| digit.to_ascii_uppercase()));
            }
        }
    }
    encoded
}

/// Validates the token endpoint's answer at `now`. Pure, so every refusal has a cell without a socket.
fn answered(status: u16, text: &str, now: u64) -> Result<Held, ClientCredentialsFailed> {
    let body: Option<serde_json::Value> = serde_json::from_str(text).ok();
    let field = |name: &str| body.as_ref()?.get(name);
    let text_of = |name: &str| field(name)?.as_str();
    if !(200..300).contains(&status) {
        return Err(ClientCredentialsFailed::Refused {
            status,
            error: super::registered(text_of("error")),
        });
    }
    if body.is_none() {
        return Err(ClientCredentialsFailed::Malformed { what: "not JSON" });
    }
    if !text_of("token_type").is_some_and(|kind| kind.eq_ignore_ascii_case("bearer")) {
        return Err(ClientCredentialsFailed::WrongTokenType);
    }
    let token = text_of("access_token")
        .filter(|token| !token.is_empty())
        .ok_or(ClientCredentialsFailed::Malformed { what: "no access_token" })?;
    let lifetime = field("expires_in")
        .and_then(serde_json::Value::as_u64)
        .filter(|&seconds| seconds > 0)
        .ok_or(ClientCredentialsFailed::Malformed {
            what: "no positive integer expires_in",
        })?;
    Ok(Held {
        token: Secret::new(token),
        not_after: now.saturating_add(lifetime),
    })
}

#[cfg(test)]
mod tests;
