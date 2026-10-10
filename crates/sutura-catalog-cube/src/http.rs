//! The real [`crate::MetaReader`]: one `GET /cubejs-api/v1/meta` against a running Cube.
//!
//! Behind the crate's default-off `http` feature, so a build that does not ask for it links no
//! outbound TLS stack.
//!
//! # Auth
//!
//! The deployment's Cube token as a [`Secret`], sent as `Authorization: Bearer <token>`. Cube
//! verifies it as a JSON Web Token and answers `403` without one or with one it refuses. The
//! catalog is read as the deployment, never as a caller: the definitions are the same for every
//! caller, and per-caller rules belong to the execution path.
//!
//! # Bounds, TLS and the endpoint
//!
//! [`ReadBounds`] gives the request its timeout and the answer its size cap. [`Endpoint::parse`]
//! accepts `https://` for any host and `http://` only for an IP loopback literal, so a token is never
//! sent in plain text to a host that is not this one. The trust anchors are `ureq`'s compiled-in
//! roots, or exactly the deployment's `security.outbound.transport_anchors`; redirects are not
//! followed.

use sutura_domain::identity::Secret;
pub use sutura_http_client::{
    DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, EndpointMessage, InvalidEndpoint, InvalidReadBounds,
    OutboundAgent, ReadBounds,
};

use crate::MetaReader;
use crate::document::Meta;

/// The metadata API's path under a Cube server's root.
pub const META_PATH: &str = "/cubejs-api/v1/meta";

/// Why the metadata answer was not read.
///
/// Reaches [`crate::CubeCatalog`] boxed inside [`crate::CubeError::Read`]. No variant carries the token,
/// and [`HttpReaderError::Refused`] renders the status and never Cube's own text.
#[derive(Debug, thiserror::Error)]
pub enum HttpReaderError {
    #[error("the Cube metadata API was not reached")]
    Unreachable(#[source] Box<ureq::Error>),
    #[error("the Cube metadata answer could not be read")]
    Unreadable(#[source] Box<ureq::Error>),
    #[error("Cube refused the metadata request with {status}")]
    Refused { status: u16, detail: EndpointMessage },
    #[error("the Cube metadata answer was larger than the {cap}-byte cap this reader will read")]
    TooLarge { cap: u64 },
    #[error("the Cube metadata answer was not the shape this reader expects")]
    Shape(#[source] serde_json::Error),
}

/// A Cube deployment's metadata API, reached over HTTP.
#[derive(Debug, Clone)]
pub struct HttpMetaReader {
    endpoint: Endpoint,
    token: Secret,
    bounds: ReadBounds,
    agent: sutura_tls::Rotating<ureq::Agent>,
}

impl HttpMetaReader {
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
                  parses, the shape sutura-catalog-openmetadata's HttpSnapshotReader::bearer holds"
    )]
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token.expose_secret())
    }

    fn fetch(&self) -> Result<Meta, HttpReaderError> {
        let url = format!("{}{META_PATH}", self.endpoint.as_str());
        let mut response = self
            .agent
            .current()
            .get(&url)
            .config()
            .timeout_global(Some(self.bounds.timeout()))
            .build()
            .header("authorization", self.bearer())
            .call()
            .map_err(|cause| HttpReaderError::Unreachable(Box::new(cause)))?;
        let status = response.status();
        // `ureq`'s `limit` is a backstop above the cap; the length check below is the cap itself.
        let cap = self.bounds.max_response_bytes();
        let text = response
            .body_mut()
            .with_config()
            .limit(cap.saturating_add(cap.max(1024)))
            .read_to_string()
            .map_err(|cause| HttpReaderError::Unreadable(Box::new(cause)))?;
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

impl MetaReader for HttpMetaReader {
    type Error = HttpReaderError;

    fn read(&self) -> Result<Meta, Self::Error> {
        self.fetch()
    }
}
