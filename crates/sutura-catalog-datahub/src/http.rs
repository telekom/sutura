//! The real [`crate::AspectReader`]: three paged reads over `DataHub`'s versioned `OpenAPI` v3
//! entity surface, assembled into one [`Snapshot`].
//!
//! Behind the crate's default-off `http` feature - see `Cargo.toml`'s own comment on why - so a
//! build that does not ask for this reader links no outbound TLS stack.
//!
//! # What is measured, and what is not - stated here because it decides how this module is written
//!
//! **The `metric` entity's wire shape is confirmed against a live instance.**
//! `tests/provisioned.rs`'s *Revision, 2026-09-04* round-tripped the recorded fixture's own document
//! through a real `DataHub` 1.7.0 and compared what came back against the fixture byte-for-byte, so
//! [`harvest_metric`] maps exactly the envelope that suite measured: `entities[]`, each carrying
//! `metricInfo.value` and `structuredProperties.value.properties[]`.
//!
//! **The `semanticModel` entity's relationship shape IS measured against a live instance, the
//! `dataset` entity's is NOT.** Real `DataHub` 1.7.0 carries a joined relationship NOT as a
//! top-level `semanticModelRelationship` aspect (which GMS drops on write) but nested inside the
//! `semanticModel` entity's own `semanticModelInfo.value.relationships[]` array - measured on this
//! repository's docker tier (2026-09-16) by upserting the recorded corpus's `orders_to_customer`
//! relationship under `semanticModelInfo` and reading it back. So
//! [`HttpAspectReader::read_relationships`] maps the aspect it fetches to the array
//! [`harvest_relationship`] walks, and the fake `happy_path_answers` page serves the same nested
//! shape - one content over two transports. The `dataset` entity's field
//! list is still from `docs/adr/0016`'s "Field by field" table, read from the platform's own `.pdl`
//! schema rather than from a served response. The two facts have different consequences: a wrong
//! guess about `metric`'s envelope would be a regression against a proven round trip, and a wrong
//! guess about `dataset` would be a FIRST claim this crate has made about it. Both mapping functions
//! refuse an unexpected shape as a typed [`HttpReaderError::UnexpectedShape`] naming the entity and
//! the field, rather than reading past a missing or mistyped key with a default - a guess that
//! happened to be wrong would otherwise certify a bundle silently missing a model or a relationship.
//! **Do not cite this reader as proof the `dataset` half works against a real `DataHub` until an
//! acceptance leg like `tests/provisioned.rs`'s measures it.**
//!
//! # What every read is bounded by
//!
//! [`ReadBounds`] carries a request timeout and a response-size cap, both **settings with defaults,
//! not constants** - [`DEFAULT_TIMEOUT_SECONDS`] and [`DEFAULT_MAX_RESPONSE_BYTES`] are the values a
//! composition root's settings default to, following `sutura-config`'s own convention of a default
//! function per optional key, not a value baked into this type. [`read`](AspectReader::read) makes
//! up to three requests and shares ONE deadline across them - opened once, and what is left after
//! the first two requests is what the third gets - the same shape `sutura_domain::warehouse::deadline::Deadline`
//! holds for a job's execution, and for the same reason: a budget opened per request lets three
//! independent timeouts sum to three times what a deployment declared.
//!
//! # Auth
//!
//! A personal access token as a [`Secret`], sent as `Authorization: Bearer <token>` on every
//! request. The token is a constructor argument here; a composition root reads it from a settings-
//! declared file at boot (`token_file`, the naming convention `credential_file`/`password_file`
//! already hold), never inline in a settings document.
//!
//! # Paging
//!
//! One page per entity type, at a generous count. A page that SIGNALS more results exist - a
//! `scrollId`, or a returned count below a reported `total` - is refused
//! ([`HttpReaderError::MorePages`]) rather than silently read as complete: the same "one page or a
//! refusal" shape `sutura-exec-bigquery`'s wire holds for `jobs.query`, because a caller must not
//! certify a bundle built from a `Snapshot` that silently dropped a model, a relationship or a
//! metric. **Unmeasured: whether a real v3 last page ever carries a `scrollId` of its own.** If it
//! does, every read of a real instance is a refusal, and the follow-up acceptance leg (shaped like
//! `tests/provisioned.rs`) has to measure this before PR2 wires the composition - the `scrollId` arm
//! is a defensible guess against the platform's own "there is more" convention, not something this
//! crate has watched a real GMS answer.
//!
//! # TLS and the endpoint
//!
//! [`Endpoint::parse`] is the ONLY way to obtain an [`Endpoint`], and [`HttpAspectReader::new`] takes one rather than a
//! `String` - a caller cannot dial an endpoint this module has not validated. What [`Endpoint::parse`] accepts, exactly:
//! `scheme://host[:port]`, scheme `http` or `https` (case-folded), on a [`Uri`] (`ureq`'s own re-export of the `http` crate's
//! parser, the SAME type `ureq` itself parses a request URL into before dialling), an OPTIONAL nonzero valid `:port`, an
//! OPTIONAL trailing `/`, and NOTHING else: a path, query or fragment is [`InvalidEndpoint::PathBeyondRoot`] (fragment
//! checked on the RAW text, because `http::Uri` silently discards a `#`), a bad port is [`InvalidEndpoint::NotAnHttpUrl`],
//! and a `user[:pass]@` prefix is [`InvalidEndpoint::CredentialsInUrl`]. `https://` is accepted for any host; `http://` only
//! for an IP loopback LITERAL - [`sutura_domain::source::host_is_loopback`], the ONE predicate
//! `sutura_config::sources::transport` also calls for Postgres's `transport_mode: plaintext` (issue 124's fail-closed rule,
//! `github.com/telekom/sutura#653`): a hostname is not an address, so `localhost` does not count either, and only something
//! that parses as `IpAddr` and answers `is_loopback()` does - the same function both crates call, so a divergence between
//! them is a compile error, not a review's job to notice.
//!
//! **This section has been wrong twice, and both corrections are worth keeping visible rather than
//! silently fixed - the second because the first one's OWN reasoning had a gap in it.**
//!
//! The first draft removed `https_only(true)` (`BigQuery`'s own pin) entirely, arguing from this
//! record's own measurement tier reaching its `DataHub` over loopback plaintext "by construction" -
//! true, but an argument for LOOPBACK plaintext, not for plaintext to any host a deployment might
//! type. With no parse at all, a bearer was dialled in clear text to
//! `http://datahub.example.internal` exactly as readily as to `http://127.0.0.1`, refused only by a
//! connection timeout. The first fix was [`Endpoint`], parsed by splitting the string by hand.
//!
//! **The hand-split parse was itself the second gap, and a second review measured it.** For
//! `http://[::1]:1@localhost:<port>`, the hand-rolled host extraction took the text before the
//! LAST `:` in the authority - `::1` for the bracketed case - so the endpoint parsed as loopback
//! while the REAL host, `localhost` (everything after the userinfo's `@`), is exactly the name
//! [`Endpoint::parse`] is supposed to refuse in plaintext. A reader built from that string dialled
//! `localhost` with the bearer prepared. Parsing with [`Uri`] - the SAME parser `ureq` itself uses -
//! closes this the way it should have been closed the first time: `Authority::host` already
//! resolves past userinfo correctly, and `Endpoint::parse` additionally refuses any `user[:pass]@`
//! prefix outright rather than trusting that resolution to stay correct.
//!
//! `ureq`'s compiled-in default root set (for an `https://` endpoint), `max_redirects(0)` and the
//! proxy left on (`Proxy::try_from_env()`) are the other pins; a deployment MAY replace the
//! compiled-in roots with its own CA via `security.outbound.transport_anchors` (`#125`), folded in
//! [`super::tls_roots`] - anchors only, no client identity.

use std::time::{Duration, Instant};
use ureq::http::Uri;

use serde_json::Value;
use sutura_domain::identity::Secret;

use crate::document::{DatasetAspect, MetricAspect, RelationshipAspect, Snapshot};
use crate::{AspectReader, DataHubError};

/// How long a socket may stay open past what is left of the shared deadline: connection setup and
/// the last bytes of the answer. A deadline of zero would mean *no timeout* to the client
/// underneath, and this margin is what keeps the socket's own bound from reading that way.
const CONNECT_MARGIN: Duration = Duration::from_secs(5);

/// A cap on response HEADERS, read before any body - a foreign endpoint's header block is
/// untrusted input like anything else read off the wire, and has to be bounded before this reads
/// any of it.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Builds a fresh `ureq::Agent` with this reader's pins over the given TLS configuration - the one
/// place the pins are written, so the fixed and rotating constructors use the same client. There
/// is no `https_only(true)` here: this crate pursues a validated [`Endpoint`] (which already
/// refuses a non-loopback plaintext host) rather than a compile-time `https://` constant, so the
/// scheme pin lives in the parse, not in the agent.
fn agent_from_tls(socket: Duration, tls: ureq::tls::TlsConfig) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(socket))
            .max_response_header_size(MAX_HEADER_BYTES)
            .proxy(ureq::Proxy::try_from_env())
            .tls_config(tls)
            .build(),
    )
}

/// The recommended default request timeout, in seconds, for a composition root's settings default.
///
/// Matches `server.request_timeout_seconds`'s own shipped default: a metadata read that outlives the
/// request timeout in front of it cannot answer inside the budget the caller was promised anyway.
/// **Not read by anything in this module** - a caller passes the number it resolved, through
/// [`ReadBounds::parse`], the same single-owner shape `BytesBilledCeiling::parse` holds for
/// `BigQuery`'s ceiling: this crate owns the range, a settings tree owns that the key was written.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// The recommended default response-size cap, in bytes, for a composition root's settings default.
///
/// A metadata page is descriptions, column names and one metric document, not query rows, so what
/// this defends against is something that is not the endpoint answering at all - a redirect loop,
/// a proxy gone wrong - rather than a realistic upper bound on a legitimate page.
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// Why a declared bound is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidReadBounds {
    /// Zero would refuse every read rather than bounding one.
    #[error("a {what} of zero would refuse every read rather than bounding one")]
    Zero { what: &'static str },
}

/// A reader's rotating agent handle and (when a declaration exists) the poll handle that keeps it
/// current - named because the spelled-out pair is over this workspace's `type_complexity`
/// threshold.
type OutboundAgent = (sutura_tls::Rotating<ureq::Agent>, Option<sutura_tls::Rotator<ureq::Agent>>);

/// What one [`HttpAspectReader::read`] call may spend: a request timeout and a response-size cap.
///
/// A newtype rather than two loose arguments, so a reader cannot be built with an unchecked pair -
/// a request timeout paired with a response-size cap, and no money bound: a metadata read is not
/// billed.
#[derive(Debug, Clone, Copy)]
pub struct ReadBounds {
    timeout: Duration,
    max_response_bytes: u64,
}

impl ReadBounds {
    /// Parses a declared timeout and cap, refusing either at zero.
    pub const fn parse(timeout_seconds: u64, max_response_bytes: u64) -> Result<Self, InvalidReadBounds> {
        if timeout_seconds == 0 {
            return Err(InvalidReadBounds::Zero { what: "request timeout" });
        }
        if max_response_bytes == 0 {
            return Err(InvalidReadBounds::Zero {
                what: "response size cap",
            });
        }
        Ok(Self {
            timeout: Duration::from_secs(timeout_seconds),
            max_response_bytes,
        })
    }

    #[inline]
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    #[inline]
    #[must_use]
    pub const fn max_response_bytes(&self) -> u64 {
        self.max_response_bytes
    }
}

/// One shared budget across a `read()` call's (up to) three requests.
///
/// The same shape `sutura_domain::warehouse::deadline::Deadline` holds - an instant opened once,
/// read as what is left rather than re-derived - kept as its own type and held privately here
/// because nothing outside this module needs to open or share one.
#[derive(Debug, Clone, Copy)]
struct Budget {
    started: Instant,
    total: Duration,
}

impl Budget {
    fn opened(total: Duration) -> Self {
        Self {
            started: Instant::now(),
            total,
        }
    }

    /// What is left of the budget, or `None` when it is spent. `None` rather than a zero duration,
    /// for the reason `Deadline::remaining_at` gives: a zero timeout means *no timeout* to the
    /// client underneath.
    fn remaining(self) -> Option<Duration> {
        self.total.checked_sub(self.started.elapsed()).filter(|left| !left.is_zero())
    }

    const fn socket(left: Duration) -> Duration {
        left.saturating_add(CONNECT_MARGIN)
    }
}

/// `DataHub`'s own message on a refusal.
///
/// Redacted the way `sutura_exec_bigquery::wire::EndpointMessage` is: bounded, filtered, and
/// reachable only through [`Self::as_str`] - never through `Debug`, which is the rendering a
/// cause-chain walk uses.
#[derive(Clone, PartialEq, Eq)]
pub struct EndpointMessage(String);

impl EndpointMessage {
    fn bounded(raw: &str) -> Self {
        /// Long enough for the endpoint's own sentences, short enough that a log line stays a line.
        const MAX_DETAIL_CHARS: usize = 400;
        Self(
            raw.chars()
                .filter(|c| c.is_ascii_graphic() || *c == ' ')
                .take(MAX_DETAIL_CHARS)
                .collect(),
        )
    }

    /// The message itself, for a caller that has decided it may render it.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for EndpointMessage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "<DataHub's own message, {} char(s), redacted>", self.0.len())
    }
}

/// Why one of the three entity reads did not produce the aspects it names.
///
/// Reaches [`crate::DataHubCatalog`] boxed inside [`DataHubError::Read`] - the port's own coarse
/// variant - so this stays inspectable by a caller that knows to downcast, the `ErasedCause` shape
/// `.agents/skills/sutura/secure-by-design/SKILL.md` argues for at a boundary.
#[derive(Debug, thiserror::Error)]
pub enum HttpReaderError {
    /// The shared budget was gone before this entity's page could be requested.
    #[error("this read's {budget_seconds}-second budget was spent before the {entity} page could be requested")]
    DeadlineSpent { entity: &'static str, budget_seconds: u64 },
    /// The entity's page was not reached.
    #[error("the {entity} page was not reached")]
    Unreachable {
        entity: &'static str,
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The entity's page was reached and its answer could not be read.
    #[error("the {entity} page's answer could not be read")]
    Unreadable {
        entity: &'static str,
        #[source]
        cause: Box<ureq::Error>,
    },
    /// `DataHub` refused the request. `Display` renders the status and never `detail`, because a
    /// cause-chain walk that flattens every link with `Display` must not carry endpoint-owned text.
    #[error("DataHub refused the {entity} page with {status}")]
    Refused {
        entity: &'static str,
        status: u16,
        detail: EndpointMessage,
    },
    /// The page was larger than the cap this reader will read.
    #[error("the {entity} page was larger than the {cap}-byte cap this reader will read")]
    TooLarge { entity: &'static str, cap: u64 },
    /// The page was not a JSON document.
    #[error("the {entity} page was not a JSON document")]
    NotADocument {
        entity: &'static str,
        #[source]
        cause: serde_json::Error,
    },
    /// One entity's aspect did not carry a field this reader expects, or carried it in a shape it
    /// does not recognise.
    ///
    /// **Refused rather than guessed** - see the module header on which of the three entity shapes
    /// this applies to. `field` is a dotted path (`"schemaMetadata.value.fields[].fieldPath"`) so a
    /// refusal names exactly where the document stopped matching this reader's expectation.
    #[error("the {entity} entity's {field} was not present, or not the shape this reader expects")]
    UnexpectedShape { entity: &'static str, field: &'static str },
    /// The page's own field mapped into this crate's canonical aspect shape and that decode failed -
    /// a defect in this reader's mapping rather than in the page, since every field reaching
    /// `serde_json::from_value` here was already read out of the page by name above.
    #[error("the {entity} entity mapped to a document this crate's own shape refused")]
    NotTheCanonicalShape {
        entity: &'static str,
        #[source]
        cause: serde_json::Error,
    },
    /// The page stated or implied more results exist than the one page this reader will read.
    #[error("the {entity} page indicated more results than the one page this reader will read")]
    MorePages { entity: &'static str },
}

/// Why a declared endpoint is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidEndpoint {
    /// Not a parseable URL, or a parseable URL naming neither `http` nor `https`, or one naming no
    /// authority at all.
    #[error("{given} is not an http:// or https:// URL")]
    NotAnHttpUrl { given: String },
    /// The authority carries `user[:pass]@` - refused outright. **This is not merely defence in
    /// depth against a spoofed host**: the round-2 review measured a reader built from
    /// `http://[::1]:1@localhost:<port>` dialling `localhost` in clear text with the bearer
    /// prepared, because a hand-rolled host extraction split on the wrong delimiter. Parsing with
    /// [`Uri`] closes that specific bypass on its own - `Authority::host` resolves to the text
    /// AFTER the last `@`, which is `localhost` here, so the loopback check below already sees the
    /// real target - but a declared endpoint has no legitimate use for embedded credentials, so
    /// this refuses the shape by name rather than relying on that resolution being correct forever.
    #[error("{given} carries credentials in the URL (a user[:pass]@ prefix), which is refused")]
    CredentialsInUrl { given: String },
    /// A path, a query or a fragment beyond the bare root - a reverse-proxy path prefix is a real
    /// shape, not yet supported, a stated limit. The fragment is checked on RAW text in
    /// [`Endpoint::parse`]: `http::Uri` silently discards a `#`.
    #[error("{given} carries a path, query or fragment beyond the root, which this reader does not support")]
    PathBeyondRoot { given: String },
    /// `http://` to a host that is not an IP loopback literal - see
    /// [`sutura_domain::source::host_is_loopback`].
    #[error(
        "http:// is refused for {host} - only an IP loopback literal (127.0.0.1, ::1) may carry a \
         bearer in clear text; write https:// or a loopback address"
    )]
    PlaintextBeyondLoopback { host: String },
}

/// A validated `DataHub` endpoint, obtainable only through [`Self::parse`] - see the module
/// header's "TLS and the endpoint" section for the accepted grammar and each refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    /// Parses and validates against [`Uri`] - the SAME parser `ureq` itself dials with, rather
    /// than a hand-rolled split, which is what let the round-2 review's userinfo form
    /// (`http://[::1]:1@localhost`) reach `host_is_loopback` with the wrong string. The stored
    /// form is rebuilt from the parsed `scheme`/`authority`, so any root spelling normalises alike.
    pub fn parse(raw: &str) -> Result<Self, InvalidEndpoint> {
        let not_an_http_url = || InvalidEndpoint::NotAnHttpUrl { given: raw.to_owned() };
        // `http::Uri` silently discards a fragment at parse, so a `#` is refused HERE, on the raw text.
        if raw.contains('#') {
            return Err(InvalidEndpoint::PathBeyondRoot { given: raw.to_owned() });
        }
        let uri: Uri = raw
            .trim()
            .parse()
            .map_err(|_cause: ureq::http::uri::InvalidUri| not_an_http_url())?;
        let scheme = uri.scheme_str().unwrap_or_default();
        if scheme != "http" && scheme != "https" {
            return Err(not_an_http_url());
        }
        let authority = uri.authority().ok_or_else(not_an_http_url)?;
        if authority.as_str().contains('@') {
            return Err(InvalidEndpoint::CredentialsInUrl { given: raw.to_owned() });
        }
        // A declared port must be a valid nonzero `u16`; bytes (not `&str`) so a bracketed IPv6
        // host's own colons are never mistaken for the port's.
        let after_host = authority
            .as_str()
            .as_bytes()
            .get(authority.host().len()..)
            .unwrap_or_default();
        if after_host.starts_with(b":") && !matches!(authority.port_u16(), Some(port) if port > 0) {
            return Err(not_an_http_url());
        }
        let root_only = uri
            .path_and_query()
            .is_none_or(|path_and_query| matches!(path_and_query.as_str(), "" | "/"));
        if !root_only {
            return Err(InvalidEndpoint::PathBeyondRoot { given: raw.to_owned() });
        }
        if scheme == "http" {
            // `Authority::host` already resolves past any userinfo (to the text after the LAST
            // `@`), and keeps IPv6 brackets - stripped here because `host_is_loopback` parses an
            // `IpAddr`, which does not accept them.
            let host = authority.host().trim_start_matches('[').trim_end_matches(']');
            if !sutura_domain::source::host_is_loopback(host) {
                return Err(InvalidEndpoint::PlaintextBeyondLoopback { host: host.to_owned() });
            }
        }
        Ok(Self(format!("{scheme}://{}", authority.as_str())))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A `DataHub` GMS, reached over HTTP.
///
/// Not generic over its credential the way `sutura-exec-bigquery`'s transport is: there is exactly
/// one credential shape here, a personal access token, so a type parameter would buy nothing a
/// second constructor would not.
#[derive(Debug, Clone)]
pub struct HttpAspectReader {
    /// Validated by [`Endpoint::parse`] - `https://` to any host, `http://` only to an IP loopback
    /// literal. No trailing slash.
    endpoint: Endpoint,
    /// The qualified name of the structured property THIS DEPLOYMENT registered for the certified
    /// metric document - `docs/adr/0016` decision 7's *not ours to say*, so there is no default.
    property: String,
    token: Secret,
    bounds: ReadBounds,
    agent: sutura_tls::Rotating<ureq::Agent>,
}

impl HttpAspectReader {
    /// Opens a reader. `endpoint` (only [`Endpoint::parse`]), `property`, `token` (read from a
    /// settings-declared file at boot) and `bounds` (only [`ReadBounds::parse`]) are all checked first.
    ///
    /// **`anchors` is `security.outbound.transport_anchors` (`#125`), resolved once at boot**: `None`
    /// leaves `ureq`'s compiled-in `RootCerts::WebPki` (every deployment before `security.outbound`),
    /// `Some` replaces it with `RootCerts::Specific` from exactly the declared certificates - never a
    /// union of the two (see [`super::tls_roots`]). This constructor never presents a client
    /// identity - [`Self::rotating_agent`] is the one that does, over the same declaration
    /// (`security.outbound.client_certificate`/`client_key`, `github.com/telekom/sutura#911`).
    #[must_use]
    pub fn new(
        endpoint: Endpoint,
        property: String,
        token: Secret,
        bounds: ReadBounds,
        anchors: Option<sutura_tls::LoadedAnchors>,
    ) -> Self {
        Self::rotating(
            endpoint,
            property,
            token,
            bounds,
            sutura_tls::Rotating::fixed(agent_from_tls(
                Budget::socket(bounds.timeout()),
                super::tls_roots::config(anchors, None),
            )),
        )
    }

    /// The rotation-lane constructor: holds the rotating agent handle a composition root built (via
    /// [`Self::rotating_agent`]) and drove to re-read on [`sutura_tls::POLL_INTERVAL`]. The reader is
    /// per-request, so the agent `current()` resolves to on the next `read` is the latest that loaded -
    /// a replaced bundle (`security.outbound.transport_anchors`, `github.com/telekom/sutura#125`) is
    /// adopted by the next read, no drain (per `docs/adr/0010`).
    #[must_use]
    pub const fn rotating(
        endpoint: Endpoint,
        property: String,
        token: Secret,
        bounds: ReadBounds,
        agent: sutura_tls::Rotating<ureq::Agent>,
    ) -> Self {
        Self {
            endpoint,
            property,
            token,
            bounds,
            agent,
        }
    }

    /// Builds the reader's rotating agent handle for a declared `security.outbound` set, and (when
    /// one is declared) the [`sutura_tls::Rotator`] the composition root drives on
    /// [`sutura_tls::POLL_INTERVAL`]. `None` (no declaration) returns a fixed handle over `ureq`'s
    /// compiled-in roots, presenting no identity, and no poll handle. Rebuilt over
    /// `RootCerts::Specific` from each freshly loaded bundle and, when
    /// [`sutura_tls::Declared::identity`] is declared, the freshly loaded identity too - never a
    /// union, never a second external read.
    ///
    /// # Errors
    ///
    /// The declared bundle or client identity cannot be loaded at boot.
    pub fn rotating_agent(
        bounds: ReadBounds,
        declared: Option<sutura_tls::Declared>,
    ) -> Result<OutboundAgent, sutura_tls::LoadError> {
        let Some(declared) = declared else {
            return Ok((
                sutura_tls::Rotating::fixed(agent_from_tls(
                    Budget::socket(bounds.timeout()),
                    super::tls_roots::config(None, None),
                )),
                None,
            ));
        };
        let (anchors, identity) = declared.into_parts();
        let socket = Budget::socket(bounds.timeout());
        let rebuild = move |loaded, identity: Option<sutura_tls::LoadedIdentity>| {
            Ok::<_, sutura_tls::LoadError>(agent_from_tls(socket, super::tls_roots::config(Some(loaded), identity)))
        };
        let initial_anchors = sutura_tls::load_anchors(&anchors)?;
        let initial_identity = identity.as_ref().map(sutura_tls::load_identity).transpose()?;
        let initial = agent_from_tls(socket, super::tls_roots::config(Some(initial_anchors), initial_identity));
        let rotator = sutura_tls::Rotator::new(anchors, identity, rebuild, initial);
        Ok((rotator.rotating(), Some(rotator)))
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a bearer has to reach the wire as text; the exposure here builds the one header \
                  value the client parses, which is the whole reason the token exists - the same \
                  shape sutura_exec_bigquery::adbc::identity::credential_options holds for its own \
                  once-only exposure at a boundary"
    )]
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token.expose_secret())
    }

    /// Requests one entity type's page, checked as far as *the service answered and it fits the
    /// cap*. Everything past that - the envelope, the aspects inside it - is the caller's job.
    fn fetch(&self, budget: Budget, entity: &'static str, aspects: &[&str]) -> Result<Value, HttpReaderError> {
        let left = budget.remaining().ok_or(HttpReaderError::DeadlineSpent {
            entity,
            budget_seconds: self.bounds.timeout().as_secs(),
        })?;
        let query = aspects
            .iter()
            .map(|aspect| format!("aspects={aspect}"))
            .collect::<Vec<_>>()
            .join("&");
        // A generous, fixed count rather than a configured one: raising it does not change the
        // shape of the read, only how large a deployment can be before `MorePages` fires - and a
        // deployment past this needs a different reader (real paging), not a bigger number here.
        let url = format!("{}/openapi/v3/entity/{entity}?{query}&count=1000", self.endpoint.as_str());
        let mut response = self
            .agent
            .current()
            .get(&url)
            .config()
            .timeout_global(Some(Budget::socket(left)))
            .build()
            .header("authorization", self.bearer())
            .call()
            .map_err(|cause| HttpReaderError::Unreachable {
                entity,
                cause: Box::new(cause),
            })?;
        let status = response.status();
        // `ureq`'s own `limit` is a generous BACKSTOP, set well above the deployment's declared
        // cap rather than at it - measured rather than assumed: a body landing exactly on a limit
        // set to `cap + 1` still failed the read on this version of `ureq`, so this module does
        // not rely on that library's own boundary behaviour for a precise refusal. What enforces
        // the deployment's own cap PRECISELY is the explicit length check below; `ureq`'s limit
        // exists only so a truly unbounded reply cannot be read into memory at all.
        let cap = self.bounds.max_response_bytes();
        let hard_ceiling = cap.saturating_add(cap.max(1024));
        let text = response
            .body_mut()
            .with_config()
            .limit(hard_ceiling)
            .read_to_string()
            .map_err(|cause| HttpReaderError::Unreadable {
                entity,
                cause: Box::new(cause),
            })?;
        if text.len() as u64 > cap {
            return Err(HttpReaderError::TooLarge { entity, cap });
        }
        if !status.is_success() {
            return Err(HttpReaderError::Refused {
                entity,
                status: status.as_u16(),
                detail: EndpointMessage::bounded(&text),
            });
        }
        serde_json::from_str(&text).map_err(|cause| HttpReaderError::NotADocument { entity, cause })
    }

    /// Whether a page's own fields say more results exist than the page this reader read.
    ///
    /// A `scrollId` is the surface's own "there is more" token; a returned count equal to the
    /// requested one with a stated `total` above it is the same fact stated the other way, for a
    /// surface that answers `total` without a scroll token on a short page. Both are checked because
    /// neither is measured to be the surface's only tell - see the module header's limit on the
    /// dataset/semanticModel shapes.
    fn page_signals_more(page: &Value, returned: usize) -> bool {
        if page.get("scrollId").and_then(Value::as_str).is_some() {
            return true;
        }
        page.get("total")
            .and_then(Value::as_u64)
            .is_some_and(|total| total > returned as u64)
    }

    fn entities<'page>(page: &'page Value, entity: &'static str) -> Result<&'page [Value], HttpReaderError> {
        page.get("entities")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .ok_or(HttpReaderError::UnexpectedShape {
                entity,
                field: "entities",
            })
    }

    /// The `dataset` entity type: `schemaMetadata` for columns, `datasetProperties` for prose.
    ///
    /// **Unmeasured against a live instance** - see the module header. `Model.name` and
    /// `Model.table` are both filled from the entity's own URN name segment
    /// ([`split_dataset_urn`]), because this reader has no other source for a *second*, adapter-only
    /// identifier - a deployment needing the two to differ is not representable by this reader
    /// today, which is a real limit and not a placeholder.
    fn read_datasets(&self, budget: Budget) -> Result<Vec<DatasetAspect>, HttpReaderError> {
        const ENTITY: &str = "dataset";
        let page = self.fetch(budget, ENTITY, &["schemaMetadata", "datasetProperties"])?;
        let entities = Self::entities(&page, ENTITY)?;
        if Self::page_signals_more(&page, entities.len()) {
            return Err(HttpReaderError::MorePages { entity: ENTITY });
        }
        entities.iter().map(harvest_dataset).collect()
    }

    /// The `semanticModel` entity type's `semanticModelInfo` aspect, whose `relationships[]` array
    /// this reader walks. Real `DataHub` carries a joined relationship INSIDE
    /// `semanticModelInfo.value.relationships[]`, not as a top-level aspect - measured against the
    /// docker tier (2026-09-16); a top-level `semanticModelRelationship` write is accepted and then
    /// dropped by GMS, so the nested shape is the only one the two transports share.
    ///
    /// `fromColumns`/`toColumns` arrive as arrays; this crate's own [`RelationshipAspect`] carries
    /// one column per side, so a relationship declaring more than one is refused by name
    /// ([`HttpReaderError::UnexpectedShape`]) rather than reduced to a guess - `docs/adr/0016`'s
    /// "Field by field" table already names this as a place `DataHub` is WIDER than this adapter.
    /// A `semanticModel` entity with `semanticModelInfo` but no `relationships` contributes none,
    /// the same way a dataset with no annotation contributes an empty description.
    fn read_relationships(&self, budget: Budget) -> Result<Vec<RelationshipAspect>, HttpReaderError> {
        const ENTITY: &str = "semanticModel";
        let page = self.fetch(budget, ENTITY, &["semanticModelInfo"])?;
        let entities = Self::entities(&page, ENTITY)?;
        if Self::page_signals_more(&page, entities.len()) {
            return Err(HttpReaderError::MorePages { entity: ENTITY });
        }
        let mut relationships = Vec::new();
        for entity in entities {
            let info = entity.get("semanticModelInfo").and_then(|aspect| aspect.get("value")).ok_or(
                HttpReaderError::UnexpectedShape {
                    entity: ENTITY,
                    field: "semanticModelInfo.value",
                },
            )?;
            let list = info
                .get("relationships")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            for relationship in list {
                relationships.push(harvest_relationship(relationship)?);
            }
        }
        Ok(relationships)
    }

    /// The `metric` entity type: `metricInfo` for the promotion candidate's raw half,
    /// `structuredProperties` for the deployment-defined certified half.
    ///
    /// **Measured against a live instance** - see the module header; this maps exactly the envelope
    /// `tests/provisioned.rs`'s `harvest` compared byte-for-byte against the recorded fixture.
    fn read_metrics(&self, budget: Budget) -> Result<Vec<MetricAspect>, HttpReaderError> {
        const ENTITY: &str = "metric";
        let page = self.fetch(budget, ENTITY, &["metricInfo", "structuredProperties"])?;
        let entities = Self::entities(&page, ENTITY)?;
        if Self::page_signals_more(&page, entities.len()) {
            return Err(HttpReaderError::MorePages { entity: ENTITY });
        }
        entities.iter().map(|entity| harvest_metric(entity, &self.property)).collect()
    }
}

impl AspectReader for HttpAspectReader {
    fn read(&self) -> Result<Snapshot, DataHubError> {
        let budget = Budget::opened(self.bounds.timeout());
        let snapshot = (|| -> Result<Snapshot, HttpReaderError> {
            let datasets = self.read_datasets(budget)?;
            let relationships = self.read_relationships(budget)?;
            let metrics = self.read_metrics(budget)?;
            Ok(Snapshot::new(datasets, relationships, metrics))
        })();
        snapshot.map_err(|cause| DataHubError::Read { cause: Box::new(cause) })
    }
}

/// Splits a `dataset` URN (`urn:li:dataset:(urn:li:dataPlatform:<platform>,<name>,<ENV>)`) into its
/// platform and name segments.
fn split_dataset_urn(urn: &str) -> Option<(String, String)> {
    let inner = urn.strip_prefix("urn:li:dataset:(")?.strip_suffix(')')?;
    let mut parts = inner.splitn(3, ',');
    let platform_urn = parts.next()?;
    let name = parts.next()?;
    let platform = platform_urn.strip_prefix("urn:li:dataPlatform:")?;
    Some((platform.to_owned(), name.to_owned()))
}

/// One `dataset` entity into this crate's own [`DatasetAspect`] shape, refusing an unexpected field
/// by name. See [`HttpAspectReader::read_datasets`] for what is and is not measured here.
fn harvest_dataset(entity: &Value) -> Result<DatasetAspect, HttpReaderError> {
    const ENTITY: &str = "dataset";
    let urn = entity
        .get("urn")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "urn",
        })?;
    let (platform, name) = split_dataset_urn(urn).ok_or(HttpReaderError::UnexpectedShape {
        entity: ENTITY,
        field: "urn",
    })?;
    let fields = entity
        .get("schemaMetadata")
        .and_then(|aspect| aspect.get("value"))
        .and_then(|value| value.get("fields"))
        .and_then(Value::as_array)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "schemaMetadata.value.fields",
        })?;
    let columns = fields
        .iter()
        .map(|field| {
            field
                .get("fieldPath")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(HttpReaderError::UnexpectedShape {
                    entity: ENTITY,
                    field: "schemaMetadata.value.fields[].fieldPath",
                })
        })
        .collect::<Result<Vec<String>, _>>()?;
    // `nativeDataType`/`description` are both per-field optional - a field carrying neither
    // contributes no entry, which is what makes `column_metadata` empty rather than aspirational
    // for a schema nobody has annotated at the column level.
    let mut column_metadata = serde_json::Map::new();
    let mut primary_key = Vec::new();
    for field in fields {
        let Some(path) = field.get("fieldPath").and_then(Value::as_str) else {
            continue;
        };
        let data_type = field.get("nativeDataType").and_then(Value::as_str);
        let field_description = field.get("description").and_then(Value::as_str);
        if data_type.is_some() || field_description.is_some() {
            let mut entry = serde_json::Map::new();
            if let Some(data_type) = data_type {
                drop(entry.insert(String::from("data_type"), Value::String(data_type.to_owned())));
            }
            if let Some(field_description) = field_description {
                drop(entry.insert(String::from("description"), Value::String(field_description.to_owned())));
            }
            drop(column_metadata.insert(path.to_owned(), Value::Object(entry)));
        }
        if field.get("isPartOfKey").and_then(Value::as_bool) == Some(true) {
            primary_key.push(Value::String(path.to_owned()));
        }
    }
    // Absent prose is an empty description rather than a refusal: `datasetProperties` may be absent
    // on a dataset nobody has annotated, and this adapter's own `Description::parse` accepts empty.
    let description = entity
        .get("datasetProperties")
        .and_then(|aspect| aspect.get("value"))
        .and_then(|value| value.get("description"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut document = serde_json::Map::new();
    drop(document.insert(String::from("name"), Value::String(name.clone())));
    drop(document.insert(String::from("table"), Value::String(name)));
    drop(document.insert(String::from("platform"), Value::String(platform)));
    drop(document.insert(String::from("columns"), Value::from(columns)));
    drop(document.insert(String::from("description"), Value::String(description)));
    drop(document.insert(String::from("column_metadata"), Value::Object(column_metadata)));
    drop(document.insert(String::from("primary_key"), Value::Array(primary_key)));
    serde_json::from_value(Value::Object(document))
        .map_err(|cause| HttpReaderError::NotTheCanonicalShape { entity: ENTITY, cause })
}

/// One array field expected to hold exactly one string. `DataHub`'s `fromColumns`/`toColumns` are
/// arrays; this crate represents one column per relationship side, so more than one is refused.
fn one_column(value: &Value, field: &'static str) -> Result<String, HttpReaderError> {
    value
        .get(field)
        .and_then(Value::as_array)
        .filter(|columns| columns.len() == 1)
        .and_then(|columns| columns.first())
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: "semanticModel",
            field,
        })
}

/// A dataset URN field (`from`/`to`) reduced to the model name this crate's own [`RelationshipAspect`]
/// names its endpoints by.
fn one_endpoint(value: &Value, field: &'static str) -> Result<String, HttpReaderError> {
    let urn = value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: "semanticModel",
            field,
        })?;
    split_dataset_urn(urn)
        .map(|(_, name)| name)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: "semanticModel",
            field,
        })
}

/// `ERModelRelationshipCardinality`'s screaming-case wire spelling into the lowercase spelling
/// `crate::document::Cardinality`'s `Deserialize` accepts - `document.rs`'s own header is explicit
/// that its shapes are this adapter's canonical statement and not `DataHub`'s literal wire spelling.
fn map_cardinality(raw: &str) -> Option<&'static str> {
    match raw {
        "ONE_ONE" => Some("one_one"),
        "ONE_N" => Some("one_n"),
        "N_ONE" => Some("n_one"),
        "N_N" => Some("n_n"),
        _ => None,
    }
}

/// One relationship element out of a `semanticModel` entity's
/// `semanticModelInfo.value.relationships[]` array, into this crate's own [`RelationshipAspect`]
/// shape. See [`HttpAspectReader::read_relationships`] for what is and is not measured here.
fn harvest_relationship(relationship: &Value) -> Result<RelationshipAspect, HttpReaderError> {
    const ENTITY: &str = "semanticModel";
    let name = relationship
        .get("name")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "semanticModelInfo.value.relationships[].name",
        })?;
    let from_model = one_endpoint(relationship, "from")?;
    let from_column = one_column(relationship, "fromColumns")?;
    let to_model = one_endpoint(relationship, "to")?;
    let to_column = one_column(relationship, "toColumns")?;
    let cardinality = match relationship.get("cardinality").and_then(Value::as_str) {
        None => None,
        Some(raw) => Some(map_cardinality(raw).ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "semanticModelInfo.value.relationships[].cardinality",
        })?),
    };
    let mut document = serde_json::Map::new();
    drop(document.insert(String::from("name"), Value::String(name.to_owned())));
    drop(document.insert(String::from("from_model"), Value::String(from_model)));
    drop(document.insert(String::from("from_column"), Value::String(from_column)));
    drop(document.insert(String::from("to_model"), Value::String(to_model)));
    drop(document.insert(String::from("to_column"), Value::String(to_column)));
    if let Some(cardinality) = cardinality {
        drop(document.insert(String::from("cardinality"), Value::String(String::from(cardinality))));
    }
    serde_json::from_value(Value::Object(document))
        .map_err(|cause| HttpReaderError::NotTheCanonicalShape { entity: ENTITY, cause })
}

/// One `metric` entity into this crate's own [`MetricAspect`] shape - the mapping
/// `tests/provisioned.rs`'s `harvest` wrote out first, moved into the library it was always meant to
/// belong to. `property` is the deployment-declared qualified name a structured property is
/// registered under; a property whose urn does not end with it is not this deployment's metric
/// document and is left unread, the same way a metric with none stays a promotion candidate.
fn harvest_metric(entity: &Value, property: &str) -> Result<MetricAspect, HttpReaderError> {
    const ENTITY: &str = "metric";
    let info = entity
        .get("metricInfo")
        .and_then(|aspect| aspect.get("value"))
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "metricInfo.value",
        })?;
    let name = info
        .get("name")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "metricInfo.value.name",
        })?;
    let dialect_entry = info
        .get("expression")
        .and_then(|expression| expression.get("dialects"))
        .and_then(Value::as_array)
        .and_then(|dialects| dialects.first())
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "metricInfo.value.expression.dialects[0]",
        })?;
    let dialect = dialect_entry
        .get("dialect")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "dialects[0].dialect",
        })?;
    let expression = dialect_entry
        .get("expression")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "dialects[0].expression",
        })?;
    let sutura_scalar = entity
        .get("structuredProperties")
        .and_then(|aspect| aspect.get("value"))
        .and_then(|value| value.get("properties"))
        .and_then(Value::as_array)
        .and_then(|properties| {
            properties.iter().find(|candidate| {
                candidate
                    .get("propertyUrn")
                    .and_then(Value::as_str)
                    .is_some_and(|urn| urn.ends_with(property))
            })
        })
        .and_then(|matched| matched.get("values"))
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|first| first.get("string"))
        .and_then(Value::as_str);
    let mut document = serde_json::Map::new();
    drop(document.insert(String::from("name"), Value::String(name.to_owned())));
    drop(document.insert(String::from("dialect"), Value::String(dialect.to_owned())));
    drop(document.insert(String::from("expression"), Value::String(expression.to_owned())));
    if let Some(scalar) = sutura_scalar {
        drop(document.insert(String::from("sutura"), serde_json::json!({ "string_value": scalar })));
    }
    serde_json::from_value(Value::Object(document))
        .map_err(|cause| HttpReaderError::NotTheCanonicalShape { entity: ENTITY, cause })
}

#[cfg(test)]
mod tests;
