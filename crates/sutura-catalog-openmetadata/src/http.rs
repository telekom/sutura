//! The real [`crate::SnapshotReader`]: two paged reads over `OpenMetadata`'s `REST` API, assembled
//! into one [`crate::document::Snapshot`].
//!
//! Behind the crate's default-off `http` feature - see `Cargo.toml`'s own comment on why - so a
//! build that does not ask for this reader links no outbound TLS stack.
//!
//! # What is measured, and what is NOT
//!
//! **The `Table` and `Metric` wire shapes are read from `docs/what-openmetadata-can-carry.md`'s
//! field-by-field table**, which was itself read out of the published `Table`/`Metric` entity
//! schemas against this repository's `SemanticCatalog` port - not against a provisioned instance
//! (the nix sandbox has no network; the finding states that as an open live check). So the mapping
//! functions here are a **first claim** this crate has made about `OpenMetadata`'s served envelope,
//! the same way `sutura-catalog-datahub`'s `dataset` mapping was before its provisioned tier
//! measured it. Each mapping refuses an unexpected shape as a typed
//! [`HttpReaderError::UnexpectedShape`] naming the entity and the field, rather than reading past a
//! missing or mistyped key with a default - a guess that happened to be wrong would otherwise
//! certify a bundle silently missing a model, a join or a metric. **Do not cite this reader as proof
//! the `OpenMetadata` half works against a real instance until an acceptance leg measures it.**
//!
//! # What every read is bounded by
//!
//! [`ReadBounds`] carries a request timeout and a response-size cap, both **settings with defaults,
//! not constants** - [`DEFAULT_TIMEOUT_SECONDS`] and [`DEFAULT_MAX_RESPONSE_BYTES`] are the values a
//! composition root's settings default to, following `sutura-config`'s own convention of a default
//! function per optional key, not a value baked into this type. [`read`](SnapshotReader::read)
//! makes up to two requests (tables, then metrics) and shares ONE deadline across them - opened
//! once, and what is left after the first is what the second gets - the same shape
//! `sutura_domain::warehouse::deadline::Deadline` and `sutura-catalog-datahub`'s own reader hold.
//!
//! # Auth
//!
//! A bearer token as a [`Secret`], sent as `Authorization: Bearer <token>` on every request. The
//! token is a constructor argument here; a composition root reads it from a settings-declared file
//! at boot (`token_file`), never inline in a settings document.
//!
//! # Paging
//!
//! One page per entity kind, at a generous count. A page that SIGNALS more results exist - an
//! `after` cursor, or a returned count below a reported `paging.total` - is refused
//! ([`HttpReaderError::MorePages`]) rather than silently read as complete: the same "one page or a
//! refusal" shape `sutura-catalog-datahub`'s reader and `sutura-exec-bigquery`'s wire hold for
//! `jobs.query`, because a caller must not certify a bundle built from a `Snapshot` that silently
//! dropped a model or a metric.
//!
//! # TLS and the endpoint
//!
//! [`Endpoint`] and its [`Endpoint::parse`] now live in `sutura-http-client`, shared with
//! `sutura-catalog-datahub`'s identical reader since issue #970's review found the two
//! byte-for-byte the same (`cargo xtask check-jscpd`). [`HttpSnapshotReader::new`] takes one
//! rather than a `String` - a caller cannot dial an endpoint this module has not validated. The
//! grammar and each refusal: `scheme://host[:port]`, scheme `http` or `https` (case-folded) on a
//! `ureq::http::Uri`, an optional nonzero valid `:port`, an optional trailing `/`, and nothing
//! else; `https://` for any host, `http://` only for an IP loopback literal
//! ([`sutura_domain::source::host_is_loopback`]). This mirrors `DataHub` exactly because the two
//! readers share the same security posture: a bearer prepared for a plaintext host that is not
//! loopback is a token handed to whoever answers that name.
//!
//! `ureq`'s compiled-in default root set (for an `https://` endpoint), `max_redirects(0)` and the
//! proxy left on are the other pins; a deployment MAY replace the compiled-in roots with its own
//! CA via `security.outbound.transport_anchors` (`#125`), folded in `sutura_http_client::tls` -
//! anchors only, no client identity.

use serde_json::Value;
use sutura_domain::identity::Secret;
pub use sutura_http_client::{
    Budget, DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, EndpointMessage, InvalidEndpoint, InvalidReadBounds,
    OutboundAgent, ReadBounds,
};

use crate::document::Snapshot;
use crate::{OpenMetadataError, SnapshotReader};

/// Why one of the two entity reads did not produce the entities it names.
///
/// Reaches [`crate::OpenMetadataCatalog`] boxed inside [`OpenMetadataError::Read`] - the port's own
/// coarse variant - so this stays inspectable by a caller that knows to downcast, the `ErasedCause`
/// shape `.agents/skills/sutura/secure-by-design/SKILL.md` argues for at a boundary.
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
    /// `OpenMetadata` refused the request. `Display` renders the status and never `detail`, because
    /// a cause-chain walk that flattens every link with `Display` must not carry endpoint-owned text.
    #[error("OpenMetadata refused the {entity} page with {status}")]
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
    /// One entity did not carry a field this reader expects, or carried it in a shape it does not
    /// recognise.
    ///
    /// **Refused rather than guessed** - see the module header on what is measured and what is not.
    /// `field` is a dotted path (e.g. `"columns[].name"`) so a refusal names exactly where the
    /// document stopped matching this reader's expectation.
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

/// An `OpenMetadata` deployment, reached over HTTP.
///
/// Not generic over its credential the way `sutura-exec-bigquery`'s transport is: there is exactly
/// one credential shape here, a bearer token, so a type parameter would buy nothing a second
/// constructor would not.
#[derive(Debug, Clone)]
pub struct HttpSnapshotReader {
    /// Validated by [`Endpoint::parse`] - `https://` to any host, `http://` only to an IP loopback
    /// literal. No trailing slash.
    endpoint: Endpoint,
    token: Secret,
    bounds: ReadBounds,
    agent: sutura_tls::Rotating<ureq::Agent>,
}

impl HttpSnapshotReader {
    /// Opens a reader. `endpoint` (only [`Endpoint::parse`]), `token` (read from a settings-
    /// declared file at boot) and `bounds` (only [`ReadBounds::parse`]) are all checked first.
    ///
    /// **`anchors` is `security.outbound.transport_anchors` (`#125`), resolved once at boot**: `None`
    /// leaves `ureq`'s compiled-in `RootCerts::WebPki`, `Some` replaces it with
    /// `RootCerts::Specific` from exactly the declared certificates - never a union of the two (see
    /// `sutura_http_client::tls`). This constructor never presents a client identity -
    /// [`Self::rotating_agent`] is the one that does.
    #[must_use]
    pub fn new(endpoint: Endpoint, token: Secret, bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self {
        Self::rotating(endpoint, token, bounds, sutura_http_client::fixed(bounds, anchors))
    }

    /// The rotation-lane constructor: holds the rotating agent handle a composition root built (via
    /// [`Self::rotating_agent`]) and drove to re-read on [`sutura_tls::POLL_INTERVAL`]. The reader is
    /// per-request, so the agent `current()` resolves to on the next `read` is the latest that loaded.
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

    /// Builds the reader's rotating agent handle for a declared `security.outbound` set, and (when
    /// one is declared) the [`sutura_tls::Rotator`] the composition root drives on
    /// [`sutura_tls::POLL_INTERVAL`]. `None` (no declaration) returns a fixed handle over `ureq`'s
    /// compiled-in roots, presenting no identity, and no poll handle.
    ///
    /// # Errors
    ///
    /// The declared bundle or client identity cannot be loaded at boot.
    pub fn rotating_agent(
        bounds: ReadBounds,
        declared: Option<sutura_tls::Declared>,
    ) -> Result<OutboundAgent, sutura_tls::LoadError> {
        sutura_http_client::rotating_agent(bounds, declared)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a bearer has to reach the wire as text; the exposure here builds the one header \
                  value the client parses, which is the whole reason the token exists - the same \
                  shape sutura-catalog-datahub's HttpAspectReader::bearer holds"
    )]
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token.expose_secret())
    }

    /// Requests one entity kind's page, checked as far as *the service answered and it fits the
    /// cap*. Everything past that - the envelope, the entities inside it - is the caller's job.
    fn fetch(&self, budget: Budget, entity: &'static str) -> Result<Value, HttpReaderError> {
        let left = budget.remaining().ok_or(HttpReaderError::DeadlineSpent {
            entity,
            budget_seconds: self.bounds.timeout().as_secs(),
        })?;
        // A generous, fixed count rather than a configured one: raising it does not change the
        // shape of the read, only how large a deployment can be before `MorePages` fires - and a
        // deployment past this needs a different reader (real paging), not a bigger number here.
        let url = format!("{}/api/v1/{entity}?limit=1000", self.endpoint.as_str());
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
        // cap rather than at it - measured by sutura-catalog-datahub rather than guessed. What
        // enforces the deployment's own cap PRECISELY is the explicit length check below.
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
    /// `OpenMetadata`'s paged list envelope reports `paging.total` (how many match in all) and a
    /// `paging.after` cursor when there is another page. Either being present beyond what this page
    /// returned is refused rather than silently read as complete - see the module header's "Paging".
    fn page_signals_more(page: &Value, returned: usize) -> bool {
        let paging = page.get("paging");
        if paging
            .and_then(|paging| paging.get("after"))
            .and_then(Value::as_str)
            .is_some_and(|after| !after.is_empty())
        {
            return true;
        }
        paging
            .and_then(|paging| paging.get("total"))
            .and_then(Value::as_u64)
            .is_some_and(|total| total > returned as u64)
    }

    fn entities<'page>(page: &'page Value, entity: &'static str) -> Result<&'page [Value], HttpReaderError> {
        page.get("data")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .ok_or(HttpReaderError::UnexpectedShape { entity, field: "data" })
    }

    /// The `table` entity kind's page, with the declared relationships already harvested from the
    /// same entities' constraints.
    fn read_tables(&self, budget: Budget) -> TablesRead {
        const ENTITY: &str = "tables";
        let page = self.fetch(budget, ENTITY)?;
        let entities = Self::entities(&page, ENTITY)?;
        if Self::page_signals_more(&page, entities.len()) {
            return Err(HttpReaderError::MorePages { entity: ENTITY });
        }
        let mut tables = Vec::with_capacity(entities.len());
        let mut relationships = Vec::new();
        for entity in entities {
            // The origin model name is read once here so both `harvest_table` (which decodes the
            // model) and `harvest_relationships` (which decodes the joins on it) agree on which
            // table a foreign key sits on.
            let origin_model = entity
                .get("name")
                .and_then(Value::as_str)
                .ok_or(HttpReaderError::UnexpectedShape {
                    entity: ENTITY,
                    field: "name",
                })?
                .to_owned();
            tables.push(harvest_table(entity)?);
            relationships.extend(harvest_relationships(entity, &origin_model, ENTITY)?);
        }
        Ok((tables, relationships))
    }

    /// The `metric` entity kind: `metricType`/`granularity`/`dimensions[]` decide, and the
    /// free-text binding is reported-not-defined by the crate's declaration.
    fn read_metrics(&self, budget: Budget) -> Result<Vec<crate::document::Metric>, HttpReaderError> {
        const ENTITY: &str = "metrics";
        let page = self.fetch(budget, ENTITY)?;
        let entities = Self::entities(&page, ENTITY)?;
        if Self::page_signals_more(&page, entities.len()) {
            return Err(HttpReaderError::MorePages { entity: ENTITY });
        }
        entities.iter().map(harvest_metric).collect()
    }
}

impl SnapshotReader for HttpSnapshotReader {
    type Error = OpenMetadataError;

    fn read(&self) -> Result<Snapshot, Self::Error> {
        let budget = Budget::opened(self.bounds.timeout());
        let snapshot = (|| -> Result<Snapshot, HttpReaderError> {
            let (tables, table_relationships) = self.read_tables(budget)?;
            // Relationships are nested on the table entities themselves (`tableConstraints` /
            // `foreignKeys`), so the table read supplies both the models and the joins - the crate's
            // own `Snapshot` keys them by name, so duplicates collapse and a later constraint
            // overwrites an earlier same-named one the same way deserialization of a JSON map would.
            let mut relationships = std::collections::BTreeMap::new();
            for (name, relationship) in table_relationships {
                drop(relationships.insert(name, relationship));
            }
            let metrics = self.read_metrics(budget)?;
            Ok(Snapshot::new(tables, relationships, metrics))
        })();
        snapshot.map_err(|cause| OpenMetadataError::Read(Box::new(cause)))
    }
}

/// One `table` entity into this crate's own [`crate::document::Table`] shape, refusing an
/// unexpected field by name.
fn harvest_table(entity: &Value) -> Result<crate::document::Table, HttpReaderError> {
    const ENTITY: &str = "tables";
    let name = entity
        .get("name")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "name",
        })?
        .to_owned();
    // The service/database FQN answer. OpenMetadata tables live under a service; this adapter maps
    // that to a `sources.<alias>`. The service lives in the first segment of the fully-qualified
    // name (`service.database.schema.table`) when present, otherwise under `databaseSchema`. We
    // prefer a top-level `service.name` when a real instance nests one, and fall back to the FQN's
    // first segment.
    let service = entity
        .get("service")
        .and_then(|service| service.get("name"))
        .and_then(Value::as_str)
        .or_else(|| {
            let fqn = entity.get("fullyQualifiedName").and_then(Value::as_str)?;
            fqn.split('.').next()
        })
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "service.name / fullyQualifiedName",
        })?
        .to_owned();
    let description = entity.get("description").and_then(Value::as_str).map(str::to_owned);
    let columns = entity
        .get("columns")
        .and_then(Value::as_array)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "columns",
        })?;

    let mut column_names = Vec::with_capacity(columns.len());
    let mut column_metadata = serde_json::Map::new();
    let mut primary_key = Vec::new();
    for column in columns {
        let column_name = column
            .get("name")
            .and_then(Value::as_str)
            .ok_or(HttpReaderError::UnexpectedShape {
                entity: ENTITY,
                field: "columns[].name",
            })?
            .to_owned();
        column_names.push(column_name.clone());
        let data_type = column.get("dataType").and_then(Value::as_str);
        let column_description = column.get("description").and_then(Value::as_str);
        if data_type.is_some() || column_description.is_some() {
            let mut entry = serde_json::Map::new();
            if let Some(data_type) = data_type {
                drop(entry.insert(String::from("data_type"), Value::String(data_type.to_owned())));
            }
            if let Some(column_description) = column_description {
                drop(entry.insert(String::from("description"), Value::String(column_description.to_owned())));
            }
            drop(column_metadata.insert(column_name.clone(), Value::Object(entry)));
        }
        if column.get("constraint").and_then(Value::as_str) == Some("PRIMARY_KEY") {
            primary_key.push(Value::String(column_name));
        }
    }

    let mut document = serde_json::Map::new();
    drop(document.insert(String::from("service"), Value::String(service)));
    drop(document.insert(String::from("name"), Value::String(name)));
    drop(document.insert(String::from("columns"), Value::from(column_names)));
    drop(document.insert(String::from("column_metadata"), Value::Object(column_metadata)));
    drop(document.insert(String::from("primary_key"), Value::Array(primary_key)));
    if let Some(description) = description {
        drop(document.insert(String::from("description"), Value::String(description)));
    }
    serde_json::from_value(Value::Object(document))
        .map_err(|cause| HttpReaderError::NotTheCanonicalShape { entity: ENTITY, cause })
}

/// All declared relationships a `table` entity carries, out of its `foreignKeys` and its
/// `tableConstraints` (a `FOREIGN_KEY` constraint carries the same structural shape).
///
/// One `(name, StructuralRelationship)` join, named because the pair recurs across this module's
/// harvest functions and is over `clippy::type_complexity`'s threshold spelled out in full.
type HarvestedRelationship = (String, crate::document::StructuralRelationship);
/// The relationships one `table` entity's constraints harvest into.
type HarvestedRelationships = Vec<HarvestedRelationship>;
/// What [`HttpSnapshotReader::read_tables`] returns: every table, and every relationship harvested
/// off any of them.
type TablesRead = Result<(Vec<crate::document::Table>, HarvestedRelationships), HttpReaderError>;

/// Each carries `(name, StructuralRelationship)`; the caller keys them by name into the snapshot's
/// map, so a later same-named constraint overwrites an earlier one the way JSON map deserialization
/// would.
///
/// `origin_model` is the enclosing table's model name: `OpenMetadata` nests a foreign key ON the
/// table that owns it, so the join's origin is that table and the target is the table it references.
fn harvest_relationships(
    entity: &Value,
    origin_model: &str,
    entity_kind: &'static str,
) -> Result<HarvestedRelationships, HttpReaderError> {
    let mut out = Vec::new();
    for list_name in ["tableConstraints", "foreignKeys"] {
        let Some(list) = entity.get(list_name).and_then(Value::as_array) else {
            continue;
        };
        for element in list {
            if let Some(relationship) = harvest_relationship(element, origin_model, entity_kind)? {
                out.push(relationship);
            }
        }
    }
    Ok(out)
}

/// One constraint element into a `(name, StructuralRelationship)`, or `None` when it is not a
/// foreign-key relationship a join can be built from.
fn harvest_relationship(
    element: &Value,
    origin_model: &str,
    entity: &'static str,
) -> Result<Option<HarvestedRelationship>, HttpReaderError> {
    // A non-foreign table constraint (a `PRIMARY_KEY`/`UNIQUE` uniqueness constraint) declares no
    // join between two tables, so it contributes nothing. Only a `FOREIGN_KEY` constraint combines
    // two endpoints.
    let constraint = element.get("constraintType").and_then(Value::as_str);
    if constraint.is_some_and(|kind| kind != "FOREIGN_KEY") {
        return Ok(None);
    }
    let name = element
        .get("name")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity,
            field: "foreignKeys[].name",
        })?
        .to_owned();
    // Cardinality, named when present. `MANY_TO_MANY` is carried (the crate's conversion refuses it)
    // and an absent one is carried as silent - both faithful, matching `RelationshipType`'s closed
    // `Deserialize` and the finding's "declares when present, silent when not".
    let relationship_type = element.get("relationshipType").and_then(Value::as_str).map(str::to_owned);
    // The origin is the enclosing table's own column set; the target is the referenced table. The
    // crate's own shape carries one column per side, so a side declaring more than one is refused
    // by name rather than narrowed.
    let origin_column = one_referenced(element, "columns", entity)?;
    let target_model = element
        .get("referencedTable")
        .and_then(|referenced| referenced.get("name"))
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity,
            field: "foreignKeys[].referencedTable.name",
        })?
        .to_owned();
    let target_column = one_referenced(element, "referencedColumns", entity)?;
    let mut document = serde_json::Map::new();
    drop(document.insert(String::from("origin_model"), Value::String(origin_model.to_owned())));
    drop(document.insert(String::from("origin_column"), Value::String(origin_column)));
    drop(document.insert(String::from("target_model"), Value::String(target_model)));
    drop(document.insert(String::from("target_column"), Value::String(target_column)));
    if let Some(relationship_type) = relationship_type {
        drop(document.insert(String::from("relationship_type"), Value::String(relationship_type)));
    }
    let relationship = serde_json::from_value(Value::Object(document))
        .map_err(|cause| HttpReaderError::NotTheCanonicalShape { entity, cause })?;
    Ok(Some((name, relationship)))
}

/// A single-column array field, refused when it holds anything but exactly one column - the same
/// "one column per side" rule `sutura-catalog-datahub`'s `one_column` holds.
fn one_referenced(element: &Value, field: &'static str, entity: &'static str) -> Result<String, HttpReaderError> {
    element
        .get(field)
        .and_then(Value::as_array)
        .filter(|columns| columns.len() == 1)
        .and_then(|columns| columns.first())
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(HttpReaderError::UnexpectedShape { entity, field })
}

/// One `metric` entity into this crate's own [`crate::document::Metric`] shape.
fn harvest_metric(entity: &Value) -> Result<crate::document::Metric, HttpReaderError> {
    const ENTITY: &str = "metrics";
    let name = entity
        .get("name")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "name",
        })?
        .to_owned();
    let aggregation = entity
        .get("metricType")
        .and_then(Value::as_str)
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "metricType",
        })?
        .to_owned();
    let granularity = entity.get("granularity").and_then(Value::as_str).map(str::to_owned);
    // The loose half: `metricExpression.code`, or the first measure's `expression`. Either is the
    // free text the finding reports-and-does-not-define.
    let expression = entity
        .get("metricExpression")
        .and_then(|metric_expression| metric_expression.get("code"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            entity
                .get("measures")
                .and_then(Value::as_array)
                .and_then(|measures| measures.first())
                .and_then(|measure| measure.get("expression"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });

    let mut document = serde_json::Map::new();
    drop(document.insert(String::from("name"), Value::String(name)));
    drop(document.insert(String::from("metricType"), Value::String(aggregation)));
    if let Some(granularity) = granularity {
        drop(document.insert(String::from("granularity"), Value::String(granularity)));
    }
    if let Some(expression) = expression {
        drop(document.insert(String::from("expression"), Value::String(expression)));
    }
    serde_json::from_value(Value::Object(document))
        .map_err(|cause| HttpReaderError::NotTheCanonicalShape { entity: ENTITY, cause })
}
