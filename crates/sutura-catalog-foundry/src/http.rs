//! The real [`crate::OntologyReader`]: Foundry's ontology v2 metadata API, over HTTP.
//!
//! Behind the crate's default-off `http` feature, so a build that does not ask for it links no
//! outbound TLS stack.
//!
//! # What is dialled
//!
//! Only the declared [`Endpoint`], at two fixed paths under [`API_ROOT`]: the object types, then
//! each object type's outgoing link types. The ontology segment is an [`OntologyIdentifier`] and
//! the object-type segment a [`ModelName`] parsed from the answer before it reaches a path; neither
//! grammar can hold `/`, `?`, `#` or `%`, so an answer cannot steer a request off those paths. The
//! one other piece of an answer a request carries is the `nextPageToken`, sent back as the
//! `pageToken` query parameter of the same path, query-encoded and at most
//! [`MAX_PAGE_TOKEN_BYTES`]. No URL in an answer is followed, and redirects are not followed.
//!
//! # Bounds
//!
//! One [`Budget`] covers the whole read - every page, every link list and any token mint -
//! because the link read is one request per object type. [`ReadBounds`] caps each answer, and
//! [`PageLimits`] caps the entities of each list. A repeated page token, or an empty page that
//! reports more, is refused rather than followed. The socket timeout includes the shared five-second
//! connection margin; a spent budget is checked after token acquisition and body reading. JSON
//! decoding and ontology assembly are not interrupted by that deadline.
//!
//! # Auth
//!
//! A [`TokenSource`]: a fixed token, or a client-credentials grant on the same origin. Either is
//! the deployment's own identity.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use sutura_domain::model::{InvalidIdentifier, ModelName};
pub use sutura_http_client::token::{TokenSource, TokenUnavailable};
use sutura_http_client::{Budget, PageReport, Pager};
pub use sutura_http_client::{
    DEFAULT_MAX_ENTITIES, DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, EndpointMessage, InvalidEndpoint,
    InvalidReadBounds, OutboundAgent, PageLimits, PagingRefusal, ReadBounds, RotatingAgent,
};

use crate::OntologyReader;
use crate::document::{LinkTypeSide, ObjectType, Ontology, Page};

/// The ontology API's path under a Foundry host's root.
pub const API_ROOT: &str = "/api/v2/ontologies";

/// The page size each list asks for: Foundry's documented default.
pub const PAGE_SIZE: usize = 500;

/// The longest `nextPageToken` sent back.
pub const MAX_PAGE_TOKEN_BYTES: usize = 4096;

/// The page limits a reader starts with: [`PAGE_SIZE`] and the shared entity bound.
pub const PAGE_LIMITS: PageLimits = match PageLimits::parse(PAGE_SIZE, DEFAULT_MAX_ENTITIES) {
    Ok(limits) => limits,
    Err(_) => PageLimits::DEFAULT,
};

/// Why a declared ontology identifier is unusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidOntologyIdentifier {
    /// Empty, over 255 bytes, not starting with a letter or digit, or carrying a byte other than an
    /// ASCII letter, digit, `.`, `-` or `_`.
    #[error(
        "the ontology is not an API name or resource identifier: letters, digits, `.`, `-` and `_`, starting with a letter or digit"
    )]
    Unusable,
}

/// The ontology a reader reads: its API name or its resource identifier, one path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyIdentifier(String);

impl OntologyIdentifier {
    /// # Errors
    ///
    /// [`InvalidOntologyIdentifier`] for text that is not one path segment of that alphabet.
    pub fn parse(raw: &str) -> Result<Self, InvalidOntologyIdentifier> {
        let usable = raw.len() <= 255
            && raw.bytes().next().is_some_and(|first| first.is_ascii_alphanumeric())
            && raw
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
        if !usable {
            return Err(InvalidOntologyIdentifier::Unusable);
        }
        Ok(Self(String::from(raw)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why the ontology was not read.
///
/// Reaches [`crate::FoundryCatalog`] boxed inside [`crate::FoundryError::Read`]. No variant carries
/// the token, a page token or Foundry's own text outside [`EndpointMessage`].
#[derive(Debug, thiserror::Error)]
pub enum HttpReaderError {
    #[error("the {budget_seconds}-second read deadline was spent before the {what} were read")]
    DeadlineSpent { what: &'static str, budget_seconds: u64 },
    #[error("no bearer for the {what} request")]
    Token {
        what: &'static str,
        #[source]
        cause: TokenUnavailable,
    },
    #[error("the Foundry {what} endpoint was not reached")]
    Unreachable {
        what: &'static str,
        #[source]
        cause: Box<ureq::Error>,
    },
    #[error("the Foundry {what} answer could not be read")]
    Unreadable {
        what: &'static str,
        #[source]
        cause: Box<ureq::Error>,
    },
    #[error("Foundry refused the {what} request with {status}")]
    Refused {
        what: &'static str,
        status: u16,
        detail: EndpointMessage,
    },
    #[error("the Foundry {what} answer was larger than the {cap}-byte cap this reader will read")]
    TooLarge { what: &'static str, cap: u64 },
    #[error("the Foundry {what} answer was not the shape this reader expects")]
    Shape {
        what: &'static str,
        #[source]
        cause: serde_json::Error,
    },
    #[error("the Foundry {what} list could not be read whole")]
    Paging {
        what: &'static str,
        #[source]
        cause: PagingRefusal,
    },
    #[error("the Foundry {what} answer handed back a page token over {max} bytes")]
    PageTokenTooLong { what: &'static str, max: usize },
    /// An object type whose API name is not a name, refused before it reaches a path.
    #[error("an object type's API name is not a name, so its link types were not asked for")]
    ObjectTypeName(#[source] InvalidIdentifier),
}

const OBJECT_TYPES: &str = "object types";
const LINK_TYPES: &str = "outgoing link types";

/// A Foundry ontology's metadata API, reached over HTTP.
#[derive(Debug)]
pub struct HttpOntologyReader {
    endpoint: Endpoint,
    ontology: OntologyIdentifier,
    token: TokenSource,
    bounds: ReadBounds,
    limits: PageLimits,
    agent: RotatingAgent,
}

impl HttpOntologyReader {
    /// A reader over the rotating agent a composition root built with [`Self::rotating_agent`], or
    /// a fixed one from [`sutura_http_client::fixed`].
    #[must_use]
    pub const fn new(
        endpoint: Endpoint,
        ontology: OntologyIdentifier,
        token: TokenSource,
        bounds: ReadBounds,
        agent: RotatingAgent,
    ) -> Self {
        Self {
            endpoint,
            ontology,
            token,
            bounds,
            limits: PAGE_LIMITS,
            agent,
        }
    }

    #[must_use]
    pub const fn with_page_limits(mut self, limits: PageLimits) -> Self {
        self.limits = limits;
        self
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

    /// Every entry of one list, page after page.
    fn list<T>(&self, budget: Budget, what: &'static str, path: &str) -> Result<Vec<T>, HttpReaderError>
    where
        T: DeserializeOwned,
    {
        let mut pager = Pager::new(self.limits);
        let mut entries = Vec::new();
        loop {
            let page: Page<T> = self.fetch(budget, what, path, &pager)?;
            let next = page.next_page_token();
            if next.is_some_and(|token| token.len() > MAX_PAGE_TOKEN_BYTES) {
                return Err(HttpReaderError::PageTokenTooLong {
                    what,
                    max: MAX_PAGE_TOKEN_BYTES,
                });
            }
            let more = pager
                .advance(PageReport::new(page.len(), next, None))
                .map_err(|cause| HttpReaderError::Paging { what, cause })?;
            entries.extend(page.into_data());
            if !more {
                return Ok(entries);
            }
        }
    }

    fn fetch<T>(&self, budget: Budget, what: &'static str, path: &str, pager: &Pager) -> Result<Page<T>, HttpReaderError>
    where
        T: DeserializeOwned,
    {
        let bearer = self
            .token
            .bearer(budget)
            .map_err(|cause| HttpReaderError::Token { what, cause })?;
        let left = budget.remaining().ok_or(HttpReaderError::DeadlineSpent {
            what,
            budget_seconds: self.bounds.timeout().as_secs(),
        })?;
        let url = format!("{}{path}", self.endpoint.as_str());
        let mut request = self
            .agent
            .current()
            .get(&url)
            .query("pageSize", pager.page_size().to_string());
        if let Some(token) = pager.cursor() {
            request = request.query("pageToken", token);
        }
        let mut response = request
            .config()
            .timeout_global(Some(Budget::socket(left)))
            .build()
            .header("authorization", bearer.header())
            .call()
            .map_err(|cause| HttpReaderError::Unreachable {
                what,
                cause: Box::new(cause),
            })?;
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
                ureq::Error::BodyExceedsLimit(_) => HttpReaderError::TooLarge { what, cap },
                cause => HttpReaderError::Unreadable {
                    what,
                    cause: Box::new(cause),
                },
            })?;
        if budget.remaining().is_none() {
            return Err(HttpReaderError::DeadlineSpent {
                what,
                budget_seconds: self.bounds.timeout().as_secs(),
            });
        }
        if text.len() as u64 > cap {
            return Err(HttpReaderError::TooLarge { what, cap });
        }
        if !status.is_success() {
            return Err(HttpReaderError::Refused {
                what,
                status: status.as_u16(),
                detail: EndpointMessage::bounded(&text),
            });
        }
        serde_json::from_str(&text).map_err(|cause| HttpReaderError::Shape { what, cause })
    }
}

impl OntologyReader for HttpOntologyReader {
    type Error = HttpReaderError;

    fn read(&self) -> Result<Ontology, Self::Error> {
        let budget = Budget::opened(self.bounds.timeout());
        let root = format!("{API_ROOT}/{}/objectTypes", self.ontology.as_str());
        let object_types: Vec<ObjectType> = self.list(budget, OBJECT_TYPES, &root)?;
        let mut outgoing = BTreeMap::new();
        for object_type in &object_types {
            let name = ModelName::parse(object_type.api_name()).map_err(HttpReaderError::ObjectTypeName)?;
            let sides: Vec<LinkTypeSide> =
                self.list(budget, LINK_TYPES, &format!("{root}/{}/outgoingLinkTypes", name.as_str()))?;
            outgoing.insert(String::from(object_type.api_name()), sides);
        }
        Ok(Ontology::new(object_types, outgoing))
    }
}
