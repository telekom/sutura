//! The real [`crate::AnswerReader`]: one `GET` of a [`Target`] on the declared endpoint.
//!
//! Behind the crate's default-off `http` feature, so a build that does not ask for it links no
//! outbound TLS stack.
//!
//! # Auth
//!
//! The deployment's token as a [`Secret`], sent as `Authorization: Bearer <token>`, read once when
//! the reader is built. A token that expires is not renewed here: minting one per load (`OAuth2`
//! client credentials) is a token-source seam this reader does not have yet.
//!
//! # Bounds, TLS and the endpoint
//!
//! [`Endpoint::parse`] accepts `https://` for any host and `http://` only for an IP loopback
//! literal, so a token is never sent in plain text to a host that is not this one. The trust anchors
//! are `ureq`'s compiled-in roots, or exactly the deployment's `security.outbound.transport_anchors`.
//! Redirects are never followed: a `3xx` is a refusal. [`ReadBounds`] caps each answer, and its
//! timeout is ONE deadline for the whole load, opened when the load began, so every page and every
//! per-model listing shares it.

use std::time::Instant;

use sutura_domain::identity::Secret;
use sutura_http_client::Budget;
pub use sutura_http_client::{
    DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, EndpointMessage, InvalidEndpoint, InvalidReadBounds,
    OutboundAgent, ReadBounds,
};

use crate::{AnswerReader, Target};

/// Why an answer was not read.
///
/// Reaches [`crate::OpenApiCatalog`] boxed inside [`crate::OpenApiError::Read`]. No variant carries
/// the token, and [`HttpReaderError::Refused`] keeps the endpoint's text bounded.
#[derive(Debug, thiserror::Error)]
pub enum HttpReaderError {
    #[error("the API was not reached")]
    Unreachable(#[source] Box<ureq::Error>),
    #[error("the answer could not be read")]
    Unreadable(#[source] Box<ureq::Error>),
    #[error("the API refused the request with {status}")]
    Refused { status: u16, detail: EndpointMessage },
    #[error("the answer was larger than the {cap}-byte cap this reader will read")]
    TooLarge { cap: u64 },
    #[error("the answer was not JSON")]
    Shape(#[source] serde_json::Error),
    #[error("the {budget_seconds}-second deadline for the whole load was spent")]
    DeadlineSpent { budget_seconds: u64 },
}

/// One API's endpoint, reached over HTTP with the deployment's token.
#[derive(Debug, Clone)]
pub struct HttpAnswerReader {
    endpoint: Endpoint,
    token: Secret,
    bounds: ReadBounds,
    agent: sutura_tls::Rotating<ureq::Agent>,
}

impl HttpAnswerReader {
    /// A reader over a fixed agent: `anchors` `None` keeps `ureq`'s compiled-in roots, `Some`
    /// replaces them with exactly the declared certificates.
    #[must_use]
    pub fn new(endpoint: Endpoint, token: Secret, bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self {
        Self::rotating(endpoint, token, bounds, sutura_http_client::fixed(bounds, anchors))
    }

    /// A reader over the rotating agent a composition root built with [`Self::rotating_agent`].
    #[must_use]
    pub const fn rotating(
        endpoint: Endpoint,
        token: Secret,
        bounds: ReadBounds,
        agent: sutura_tls::Rotating<ureq::Agent>,
    ) -> Self {
        Self {
            endpoint,
            token,
            bounds,
            agent,
        }
    }

    /// The rotating agent for a declared `security.outbound` set, and its poll handle.
    ///
    /// # Errors
    ///
    /// The declared bundle or client identity cannot be loaded.
    pub fn rotating_agent(
        bounds: ReadBounds,
        declared: Option<sutura_tls::Declared>,
    ) -> Result<OutboundAgent, sutura_tls::LoadError> {
        sutura_http_client::rotating_agent(bounds, declared)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a bearer has to reach the wire as text; this builds the one header value the client \
                  parses, the shape sutura-catalog-cube's HttpMetaReader::bearer holds"
    )]
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token.expose_secret())
    }
}

impl AnswerReader for HttpAnswerReader {
    type Error = HttpReaderError;

    fn get(&self, target: &Target, started: Instant) -> Result<serde_json::Value, Self::Error> {
        let left = self
            .bounds
            .timeout()
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero())
            .ok_or(HttpReaderError::DeadlineSpent {
                budget_seconds: self.bounds.timeout().as_secs(),
            })?;
        let url = format!("{}{}", self.endpoint.as_str(), target.as_str());
        let mut response = self
            .agent
            .current()
            .get(&url)
            .config()
            .timeout_global(Some(Budget::socket(left)))
            .build()
            .header("authorization", self.bearer())
            .header("accept", "application/json")
            .call()
            .map_err(|cause| HttpReaderError::Unreachable(Box::new(cause)))?;
        let status = response.status();
        // `ureq`'s `limit` is a backstop above the cap, refused as the cap is; the length check
        // below is the cap itself.
        let cap = self.bounds.max_response_bytes();
        let text = response
            .body_mut()
            .with_config()
            .limit(cap.saturating_add(cap.max(1024)))
            .read_to_string()
            .map_err(|cause| match cause {
                ureq::Error::BodyExceedsLimit(_) => HttpReaderError::TooLarge { cap },
                cause => HttpReaderError::Unreadable(Box::new(cause)),
            })?;
        if started.elapsed() >= self.bounds.timeout() {
            return Err(HttpReaderError::DeadlineSpent {
                budget_seconds: self.bounds.timeout().as_secs(),
            });
        }
        if text.len() as u64 > cap {
            return Err(HttpReaderError::TooLarge { cap });
        }
        if !status.is_success() {
            return Err(HttpReaderError::Refused {
                status: status.as_u16(),
                detail: EndpointMessage::bounded(&text),
            });
        }
        serde_json::from_str(&text).map_err(HttpReaderError::Shape)
    }
}
