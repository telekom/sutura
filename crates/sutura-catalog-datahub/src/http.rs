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
//! **The `dataset` and `semanticModel` entities are NOT measured against a live instance.** Their
//! field lists come from `docs/adr/0016`'s "Field by field" table, which was read from the platform's
//! own `.pdl` schema sources rather than from a served response. This is stated once here and
//! repeated at each mapping function, because the two facts have different consequences: a wrong
//! guess about `metric`'s envelope would be a regression against a proven round trip, and a wrong
//! guess about the other two would be the FIRST claim this crate has made about them. So both
//! mapping functions refuse an unexpected shape as a typed [`HttpReaderError::UnexpectedShape`]
//! naming the entity and the field, rather than reading past a missing or mistyped key with a
//! default - a guess that happened to be wrong would otherwise certify a bundle silently missing a
//! model or a relationship. **Do not cite this reader as proof the structural half works against a
//! real `DataHub` until an acceptance leg like `tests/provisioned.rs`'s measures it.**
//!
//! # What every read is bounded by
//!
//! [`ReadBounds`] carries a request timeout and a response-size cap, both **settings with defaults,
//! not constants** - [`DEFAULT_TIMEOUT_SECONDS`] and [`DEFAULT_MAX_RESPONSE_BYTES`] are the values a
//! composition root's settings default to, following `sutura-config`'s own convention of a default
//! function per optional key, not a value baked into this type. [`read`](AspectReader::read) makes
//! up to three requests and shares ONE deadline across them - opened once, and what is left after
//! the first two requests is what the third gets - the same shape `sutura-exec-bigquery`'s
//! `CallDeadline` holds for a job's token exchange and its query, and for the same reason: a budget
//! opened per request lets three independent timeouts sum to three times what a deployment declared.
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
//! `scrollId`, or a returned count equal to a reported `total` - is refused
//! ([`HttpReaderError::MorePages`]) rather than silently read as complete: the same "one page or a
//! refusal" shape `sutura-exec-bigquery`'s wire holds for `jobs.query`, because a caller must not
//! certify a bundle built from a `Snapshot` that silently dropped a model, a relationship or a
//! metric.
//!
//! # TLS
//!
//! `ureq`'s compiled-in default root set when the deployment's endpoint IS `https`, `max_redirects(0)`
//! and the proxy left on (`Proxy::try_from_env()`) - three of the four pins
//! `sutura_exec_bigquery::wire::WireAgent::pinned` states for `BigQuery`. **The fourth,
//! `https_only(true)`, is deliberately NOT one of them, and the reason is the difference between the
//! two adapters' `HOST`.** `BigQuery`'s is a compile-time `https://` constant no deployment can
//! change, so refusing a plaintext scheme is a second lock on a destination that was already fixed.
//! This reader's endpoint is the DEPLOYMENT's own declared URL - `docs/adr/0016`'s own measurement
//! tier reaches its `DataHub` over loopback plaintext "by construction", and an internal metadata
//! platform reached over plain HTTP inside a private network is a real deployment shape, not a
//! mistake to refuse. So the scheme is the endpoint's own: a deployment that writes `https://` gets
//! TLS to the root set below; one that writes `http://` gets what it asked for. **Follow-up, not
//! built here:** issue #125 PR2's `security.outbound.transport_anchors` is the future seam for a
//! deployment's own CA, for the endpoints that do use TLS.

use std::time::{Duration, Instant};

use serde_json::Value;
use sutura_domain::identity::Secret;

use crate::document::{DatasetAspect, MetricAspect, RelationshipAspect, Snapshot};
use crate::{AspectReader, DataHubError};

/// How long a socket may stay open past what is left of the shared deadline: connection setup and
/// the last bytes of the answer. Same value and same argument as
/// `sutura_exec_bigquery::wire::bounds`'s `CONNECT_MARGIN`.
const CONNECT_MARGIN: Duration = Duration::from_secs(5);

/// A cap on response HEADERS, read before any body - the same reason
/// `sutura_exec_bigquery::wire::MAX_HEADER_BYTES` exists.
const MAX_HEADER_BYTES: usize = 64 * 1024;

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
/// One quarter of `sutura_exec_bigquery::wire::MAX_ANSWER_BYTES`: a metadata page is descriptions,
/// column names and one metric document, not query rows, and what this defends against is the same
/// case that constant does - something that is not the endpoint answering - rather than a
/// realistic upper bound on a legitimate page.
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// Why a declared bound is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidReadBounds {
    /// Zero would refuse every read rather than bounding one.
    #[error("a {what} of zero would refuse every read rather than bounding one")]
    Zero { what: &'static str },
}

/// What one [`HttpAspectReader::read`] call may spend: a request timeout and a response-size cap.
///
/// A newtype rather than two loose arguments, so a reader cannot be built with an unchecked pair -
/// `sutura_exec_bigquery::wire::JobBounds`'s own shape, minus the money bound this read has no use
/// for (a metadata read is not billed).
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
/// `sutura_exec_bigquery::wire::bounds::CallDeadline`'s shape, held privately here because nothing
/// outside this module needs to open or share one.
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
    /// for the reason `CallDeadline::remaining` gives: a zero timeout means *no timeout* to the
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
    /// `DataHub` refused the request. `Display` renders the status and never `detail` - the same
    /// rule `sutura_exec_bigquery::wire::WireError::Refused` holds, and for the same reason: a
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

/// A `DataHub` GMS, reached over HTTP.
///
/// Not generic over its credential the way `sutura-exec-bigquery`'s transport is: there is exactly
/// one credential shape here, a personal access token, so a type parameter would buy nothing a
/// second constructor would not.
#[derive(Debug, Clone)]
pub struct HttpAspectReader {
    /// `scheme://host[:port]`, no trailing slash.
    endpoint: String,
    /// The qualified name of the structured property THIS DEPLOYMENT registered for the certified
    /// metric document - `docs/adr/0016` decision 7's *not ours to say*, so there is no default.
    property: String,
    token: Secret,
    bounds: ReadBounds,
    agent: ureq::Agent,
}

impl HttpAspectReader {
    /// Opens a reader. `endpoint` and `property` are the deployment's; `token` is read from a
    /// settings-declared file at boot by the composition root, never inline; `bounds` is
    /// [`ReadBounds::parse`]'s output, so a reader cannot be built with an unchecked pair.
    #[must_use]
    pub fn new(endpoint: String, property: String, token: Secret, bounds: ReadBounds) -> Self {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .timeout_global(Some(Budget::socket(bounds.timeout())))
                .max_response_header_size(MAX_HEADER_BYTES)
                .proxy(ureq::Proxy::try_from_env())
                .build(),
        );
        Self {
            endpoint,
            property,
            token,
            bounds,
            agent,
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a bearer has to reach the wire as text; the exposure here builds the one header \
                  value the client parses, which is the whole reason the token exists - the same \
                  shape sutura_exec_bigquery::wire::BigQueryWire::source_bearer holds"
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
        let url = format!("{}/openapi/v3/entity/{entity}?{query}&count=1000", self.endpoint);
        let mut response = self
            .agent
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

    /// The `semanticModel` entity type's `semanticModelRelationship` aspect.
    ///
    /// **Unmeasured against a live instance** - see the module header. `fromColumns`/`toColumns`
    /// arrive as arrays; this crate's own [`RelationshipAspect`] carries one column per side, so a
    /// relationship declaring more than one is refused by name
    /// ([`HttpReaderError::UnexpectedShape`]) rather than reduced to a guess - `docs/adr/0016`'s
    /// "Field by field" table already names this as a place `DataHub` is WIDER than this adapter.
    fn read_relationships(&self, budget: Budget) -> Result<Vec<RelationshipAspect>, HttpReaderError> {
        const ENTITY: &str = "semanticModel";
        let page = self.fetch(budget, ENTITY, &["semanticModelRelationship"])?;
        let entities = Self::entities(&page, ENTITY)?;
        if Self::page_signals_more(&page, entities.len()) {
            return Err(HttpReaderError::MorePages { entity: ENTITY });
        }
        entities.iter().map(harvest_relationship).collect()
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

/// One `semanticModel` entity into this crate's own [`RelationshipAspect`] shape. See
/// [`HttpAspectReader::read_relationships`] for what is and is not measured here.
fn harvest_relationship(entity: &Value) -> Result<RelationshipAspect, HttpReaderError> {
    const ENTITY: &str = "semanticModel";
    let value = entity
        .get("semanticModelRelationship")
        .and_then(|aspect| aspect.get("value"))
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "semanticModelRelationship.value",
        })?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "semanticModelRelationship.value.name",
        })?;
    let from_model = one_endpoint(value, "from")?;
    let from_column = one_column(value, "fromColumns")?;
    let to_model = one_endpoint(value, "to")?;
    let to_column = one_column(value, "toColumns")?;
    let cardinality = match value.get("cardinality").and_then(Value::as_str) {
        None => None,
        Some(raw) => Some(map_cardinality(raw).ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "semanticModelRelationship.value.cardinality",
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
