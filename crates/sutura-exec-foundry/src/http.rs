//! The real [`FoundryTransport`]: synchronous Ontology SQL over the Foundry v2 API.
//!
//! The endpoint is a single `POST /api/v2/sqlQueries/executeOntology?preview=true`. The request
//! body carries Spark SQL and native positional parameter values separately. Redirects are disabled
//! by the shared HTTP client, and one [`Deadline`] covers request setup and body reading.

use std::time::Instant;

use sutura_domain::identity::Secret;
use sutura_domain::warehouse::deadline::Deadline;
pub use sutura_http_client::{
    DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, InvalidEndpoint, InvalidReadBounds, ReadBounds,
};

use crate::{FoundryTransport, SqlRequest};

/// The Foundry Ontology SQL endpoint's fixed path.
pub const SQL_PATH: &str = "/api/v2/sqlQueries/executeOntology";

/// Why Foundry did not produce an Arrow answer.
#[derive(Debug, thiserror::Error)]
pub enum HttpFoundryError {
    #[error("the Foundry SQL endpoint was not reached")]
    Unreachable(#[source] Box<ureq::Error>),
    #[error("the Foundry SQL answer could not be read")]
    Unreadable(#[source] Box<ureq::Error>),
    #[error("Foundry refused the SQL request with {status}")]
    Refused { status: u16 },
    #[error("the Foundry SQL answer was larger than the {cap}-byte cap this transport reads")]
    TooLarge { cap: u64 },
    #[error("the deadline ran out before Foundry answered")]
    DeadlineSpent,
    #[error("the Foundry SQL request could not be encoded as JSON")]
    Unencodable(#[source] serde_json::Error),
}

/// A bounded Foundry Ontology SQL client.
#[derive(Debug, Clone)]
pub struct HttpFoundry {
    endpoint: Endpoint,
    bounds: ReadBounds,
    agent: sutura_tls::Rotating<ureq::Agent>,
}

impl HttpFoundry {
    /// Builds a client over a fixed outbound trust declaration.
    #[must_use]
    pub fn new(endpoint: Endpoint, bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self {
        Self::rotating(endpoint, bounds, sutura_http_client::fixed(bounds, anchors))
    }

    /// Builds a client over the composition root's rotating outbound agent.
    #[must_use]
    pub const fn rotating(endpoint: Endpoint, bounds: ReadBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self {
        Self { endpoint, bounds, agent }
    }

    fn remaining(deadline: Deadline) -> Result<core::time::Duration, HttpFoundryError> {
        deadline
            .remaining_at(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or(HttpFoundryError::DeadlineSpent)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a bearer has to reach the wire as text; this is the one sanctioned header construction"
    )]
    fn bearer(token: &Secret) -> String {
        format!("Bearer {}", token.expose_secret())
    }

    fn answer(&self, mut response: ureq::http::Response<ureq::Body>, deadline: Deadline) -> Result<Vec<u8>, HttpFoundryError> {
        let status = response.status();
        let cap = self.bounds.max_response_bytes();
        let body = response
            .body_mut()
            .with_config()
            .limit(cap.saturating_add(1))
            .read_to_vec()
            .map_err(|cause| match cause {
                ureq::Error::BodyExceedsLimit(_) => HttpFoundryError::TooLarge { cap },
                cause => HttpFoundryError::Unreadable(Box::new(cause)),
            })?;
        if deadline.remaining_at(Instant::now()).is_none() {
            return Err(HttpFoundryError::DeadlineSpent);
        }
        if body.len() as u64 > cap {
            return Err(HttpFoundryError::TooLarge { cap });
        }
        if !status.is_success() {
            return Err(HttpFoundryError::Refused { status: status.as_u16() });
        }
        Ok(body)
    }
}

impl FoundryTransport for HttpFoundry {
    type Error = HttpFoundryError;

    fn execute(&self, request: &SqlRequest, token: &Secret, deadline: Deadline) -> Result<Vec<u8>, Self::Error> {
        let body = serde_json::to_vec(request).map_err(HttpFoundryError::Unencodable)?;
        let url = format!("{}{SQL_PATH}", self.endpoint.as_str());
        let response = self
            .agent
            .current()
            .post(&url)
            .query("preview", "true")
            .config()
            .timeout_global(Some(Self::remaining(deadline)?))
            .build()
            .header("authorization", Self::bearer(token))
            .header("content-type", "application/json")
            .header("accept", "application/octet-stream")
            .send(&body)
            .map_err(|cause| HttpFoundryError::Unreachable(Box::new(cause)))?;
        self.answer(response, deadline)
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, HttpFoundryError::Refused { status: 401 | 403 })
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        match error {
            HttpFoundryError::DeadlineSpent => true,
            HttpFoundryError::Unreachable(cause) | HttpFoundryError::Unreadable(cause) => {
                matches!(**cause, ureq::Error::Timeout(_))
            }
            HttpFoundryError::Refused { .. } | HttpFoundryError::TooLarge { .. } | HttpFoundryError::Unencodable(_) => false,
        }
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(error, HttpFoundryError::TooLarge { .. })
    }
}
