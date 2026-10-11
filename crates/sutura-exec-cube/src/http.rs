//! The real [`CubeTransport`]: `GET /cubejs-api/v1/meta` and `POST /cubejs-api/v1/load` against a
//! running Cube.
//!
//! Each request has `Authorization: Bearer <token>`. [`Endpoint::parse`] accepts `https://` for any
//! host and `http://` only for an IP loopback literal, so a token never travels in plain text to a
//! host that is not this one. Redirects are not followed. [`ReadBounds`] caps each answer's size;
//! the plan's deadline bounds each request's time.
//!
//! Cube answers a query it is still computing with `{"error":"Continue wait"}`, and the client asks
//! again. This transport asks again until the deadline runs out.

use std::time::Instant;

use sutura_domain::identity::Secret;
use sutura_domain::warehouse::deadline::Deadline;
pub use sutura_http_client::{Endpoint, InvalidEndpoint, InvalidReadBounds, ReadBounds};

use crate::{Answer, CubeTransport};

/// The metadata API's path under a Cube server's root.
pub const META_PATH: &str = "/cubejs-api/v1/meta";
/// The query API's path under a Cube server's root.
pub const LOAD_PATH: &str = "/cubejs-api/v1/load";

/// What Cube answers while a query is still running.
const CONTINUE_WAIT: &str = "Continue wait";

/// Why a request to Cube did not produce an answer.
///
/// No variant carries the token or Cube's own text: [`HttpCubeError::Refused`] keeps only the
/// status, because Cube's error text can repeat what the request sent.
#[derive(Debug, thiserror::Error)]
pub enum HttpCubeError {
    #[error("Cube was not reached")]
    Unreachable(#[source] Box<ureq::Error>),
    #[error("the Cube answer could not be read")]
    Unreadable(#[source] Box<ureq::Error>),
    #[error("Cube refused the request with {status}")]
    Refused { status: u16 },
    #[error("the Cube answer was larger than the {cap}-byte cap this transport reads")]
    TooLarge { cap: u64 },
    #[error("the deadline ran out before Cube answered")]
    DeadlineSpent,
    #[error("the query could not be encoded as JSON")]
    Unencodable(#[source] serde_json::Error),
}

/// A Cube deployment's REST API, reached over HTTP.
#[derive(Debug, Clone)]
pub struct HttpCube {
    endpoint: Endpoint,
    bounds: ReadBounds,
    agent: sutura_tls::Rotating<ureq::Agent>,
}

impl HttpCube {
    /// A transport over a fixed agent: `anchors` `None` keeps `ureq`'s compiled-in roots, `Some`
    /// replaces them with exactly the declared certificates.
    #[must_use]
    pub fn new(endpoint: Endpoint, bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self {
        Self::rotating(endpoint, bounds, sutura_http_client::fixed(bounds, anchors))
    }

    /// A transport over the rotating agent a composition root built.
    #[must_use]
    pub const fn rotating(endpoint: Endpoint, bounds: ReadBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self {
        Self { endpoint, bounds, agent }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a bearer has to reach the wire as text; this builds the one header value the client \
                  parses, the shape sutura-catalog-cube's HttpMetaReader::bearer holds"
    )]
    fn bearer(token: &Secret) -> String {
        format!("Bearer {}", token.expose_secret())
    }

    fn remaining(deadline: Deadline) -> Result<core::time::Duration, HttpCubeError> {
        deadline
            .remaining_at(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or(HttpCubeError::DeadlineSpent)
    }

    fn answer(&self, mut response: ureq::http::Response<ureq::Body>) -> Answer<HttpCubeError> {
        let status = response.status();
        // `ureq`'s `limit` is a backstop above the cap; the length check below is the cap itself.
        let cap = self.bounds.max_response_bytes();
        let body = response
            .body_mut()
            .with_config()
            .limit(cap.saturating_add(cap.max(1024)))
            .read_to_vec()
            .map_err(|cause| HttpCubeError::Unreadable(Box::new(cause)))?;
        if body.len() as u64 > cap {
            return Err(HttpCubeError::TooLarge { cap });
        }
        if !status.is_success() {
            return Err(HttpCubeError::Refused { status: status.as_u16() });
        }
        Ok(body)
    }
}

/// Whether `body` is Cube's "still computing" answer rather than a result.
fn still_computing(body: &[u8]) -> bool {
    #[derive(serde::Deserialize)]
    struct Waiting {
        error: String,
    }
    serde_json::from_slice::<Waiting>(body).is_ok_and(|waiting| waiting.error == CONTINUE_WAIT)
}

impl CubeTransport for HttpCube {
    type Error = HttpCubeError;

    fn meta(&self, token: &Secret, deadline: Deadline) -> Answer<Self::Error> {
        let url = format!("{}{META_PATH}", self.endpoint.as_str());
        let response = self
            .agent
            .current()
            .get(&url)
            .config()
            .timeout_global(Some(Self::remaining(deadline)?))
            .build()
            .header("authorization", Self::bearer(token))
            .call()
            .map_err(|cause| HttpCubeError::Unreachable(Box::new(cause)))?;
        self.answer(response)
    }

    fn load(&self, query: &serde_json::Value, token: &Secret, deadline: Deadline) -> Answer<Self::Error> {
        let url = format!("{}{LOAD_PATH}", self.endpoint.as_str());
        let body = serde_json::to_vec(&serde_json::json!({ "query": query })).map_err(HttpCubeError::Unencodable)?;
        loop {
            let response = self
                .agent
                .current()
                .post(&url)
                .config()
                .timeout_global(Some(Self::remaining(deadline)?))
                .build()
                .header("authorization", Self::bearer(token))
                .header("content-type", "application/json")
                .send(&body)
                .map_err(|cause| HttpCubeError::Unreachable(Box::new(cause)))?;
            let answer = self.answer(response)?;
            if !still_computing(&answer) {
                return Ok(answer);
            }
        }
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(*error, HttpCubeError::Refused { status: 401 | 403 })
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        match *error {
            HttpCubeError::DeadlineSpent => true,
            HttpCubeError::Unreachable(ref cause) | HttpCubeError::Unreadable(ref cause) => {
                matches!(**cause, ureq::Error::Timeout(_))
            }
            HttpCubeError::Refused { .. } | HttpCubeError::TooLarge { .. } | HttpCubeError::Unencodable(_) => false,
        }
    }
}
