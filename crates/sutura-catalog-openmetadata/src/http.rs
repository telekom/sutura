//! The real [`crate::SnapshotReader`]: two paged reads over `OpenMetadata`'s `REST` API, assembled
//! into one [`crate::document::Snapshot`].
//!
//! Behind the crate's default-off `http` feature - see `Cargo.toml`'s own comment on why - so a
//! build that does not ask for this reader links no outbound TLS stack.
//!
//! # What is measured, and what is NOT
//!
//! **The `Table` and `Metric` wire shapes are read against the published JSON Schema**
//! (`open-metadata/OpenMetadata`'s `openmetadata-spec`, `table.json`/`metric.json` on `main`) and
//! the `TableResource`/`MetricResource` Java sources for which fields the list endpoint returns
//! unconditionally versus only behind `?fields=` - not against a provisioned instance (the nix
//! sandbox has no network; this is a schema read, not a live one). `docs/what-openmetadata-can-carry.md`
//! was corrected against the same schema read (its `foreignKeys`/`referencedTable` shape was a
//! first-draft invention no real deployment serves; `harvest_relationship` below reads
//! `tableConstraints` instead). So the mapping functions here are a **first claim** this crate has
//! made about `OpenMetadata`'s served envelope, the same way `sutura-catalog-datahub`'s `dataset`
//! mapping was before its provisioned tier measured it. Each mapping refuses an unexpected shape as
//! a typed [`HttpReaderError::UnexpectedShape`] naming the entity and the field, rather than
//! reading past a missing or mistyped key with a default - a guess that happened to be wrong would
//! otherwise certify a bundle silently missing a model, a join or a metric. **Do not cite this
//! reader as proof the `OpenMetadata` half works against a real instance until an acceptance leg
//! measures it - the schema read is not that leg.**
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
//! **The aggregate byte cost of one `read()` is bounded by construction, not by a third check**:
//! two requests at `cap` each is at most `2×cap` read into memory before either response is
//! checked, and `fetch`'s own `ureq` backstop (`limit(2×cap)` per request, ahead of the precise
//! `len > cap` refusal) makes the true per-request ceiling `2×cap` rather than `cap` - so a single
//! `read()` never holds more than `4×cap` at once across both in-flight bodies. Stated here rather
//! than measured, because nothing enforces a THIRD, aggregate ceiling; a future third request would
//! raise this number and this sentence would have to move with it.
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
    ///
    /// `fields` is the list endpoint's own `?fields=` query parameter - `TableResource.FIELDS`
    /// (`openmetadata-service`'s Java source, measured against `main`) shows `columns` and
    /// `tableConstraints` are relationship-backed and populated only when named there; an empty
    /// slice omits the parameter entirely, for entity kinds (`metrics`) whose fields this reader
    /// needs are always returned.
    fn fetch(&self, budget: Budget, entity: &'static str, fields: &[&str]) -> Result<Value, HttpReaderError> {
        let left = budget.remaining().ok_or(HttpReaderError::DeadlineSpent {
            entity,
            budget_seconds: self.bounds.timeout().as_secs(),
        })?;
        // A generous, fixed count rather than a configured one: raising it does not change the
        // shape of the read, only how large a deployment can be before `MorePages` fires - and a
        // deployment past this needs a different reader (real paging), not a bigger number here.
        let fields_param = if fields.is_empty() {
            String::new()
        } else {
            format!("&fields={}", fields.join(","))
        };
        let url = format!("{}/api/v1/{entity}?limit=1000{fields_param}", self.endpoint.as_str());
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
        let page = self.fetch(budget, ENTITY, &["columns", "tableConstraints"])?;
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
        // `MetricResource.FIELDS` (measured against `main`) lists neither `metricType`,
        // `granularity`, `metricExpression` nor `measures` - these are core fields the resource
        // always returns, so unlike `tables` this list needs no `?fields=`.
        let page = self.fetch(budget, ENTITY, &[])?;
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
            // Relationships are nested on the table entities themselves (`tableConstraints`), so
            // the table read supplies both the models and the joins - the crate's
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
    // that to a `sources.<alias>`. We prefer a top-level `service.name` when a real instance
    // nests one, and fall back to the table's own fully-qualified name's first segment
    // (`service.database.schema.table`) - `split_fqn`, not a naive `.split('.')`, because the
    // first segment can itself be quoted if the service name holds a `.` or a `"`.
    let service = entity
        .get("service")
        .and_then(|service| service.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            let fqn = entity.get("fullyQualifiedName").and_then(Value::as_str)?;
            split_fqn(fqn)?.into_iter().next()
        })
        .ok_or(HttpReaderError::UnexpectedShape {
            entity: ENTITY,
            field: "service.name / fullyQualifiedName",
        })?;
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

/// All declared relationships a `table` entity carries, out of its `tableConstraints`' own
/// `FOREIGN_KEY` entries.
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
/// **The published wire shape has no `foreignKeys` array and no per-constraint `name`.**
/// `TableConstraint` (`table.json`, measured against `open-metadata/OpenMetadata@main`) is exactly
/// `{constraintType, columns, referredColumns, relationshipType}` with `additionalProperties:
/// false` - this crate's first draft invented a `foreignKeys` array carrying `name` and a nested
/// `referencedTable.name`, which no real deployment ever serves; fixed here to read
/// `tableConstraints` only.
///
/// `origin_model` is the enclosing table's model name: a foreign key constraint is nested ON the
/// table that owns it, so the join's origin is that table and the target is the table
/// `referredColumns` names.
fn harvest_relationships(
    entity: &Value,
    origin_model: &str,
    entity_kind: &'static str,
) -> Result<HarvestedRelationships, HttpReaderError> {
    let Some(list) = entity.get("tableConstraints").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for element in list {
        if let Some(relationship) = harvest_relationship(element, origin_model, entity_kind)? {
            out.push(relationship);
        }
    }
    Ok(out)
}

/// One constraint element into a `(name, StructuralRelationship)`, or `None` when it is not a
/// foreign-key relationship a join can be built from.
///
/// **Stated limit: both endpoints are matched by BARE table name, not by fully qualified name.**
/// `origin_model` is the enclosing entity's own `name`; `target_model` is `split_fqn_tail`'s fourth
/// segment, also bare. Two tables of the same name under different services or schemas are
/// therefore the same model to this crate, and the map `read()` collects relationships into keys
/// by NAME, so a same-named constraint from a second such table overwrites rather than adds one -
/// the same collision `docs/what-openmetadata-can-carry.md` should read this alongside. Matching
/// by the qualifying prefix instead (`referredColumns`' first three segments against the origin's
/// own) is possible but not done: `crate::document::Table`'s own model name is already the bare
/// `name` this crate reads everywhere else, so a qualified join target would need a second identity
/// this adapter does not otherwise carry.
fn harvest_relationship(
    element: &Value,
    origin_model: &str,
    entity: &'static str,
) -> Result<Option<HarvestedRelationship>, HttpReaderError> {
    // A non-foreign table constraint (a `PRIMARY_KEY`/`UNIQUE` uniqueness constraint) declares no
    // join between two tables, so it contributes nothing. Only a `FOREIGN_KEY` constraint combines
    // two endpoints; a constraint carrying no `constraintType` at all names none either.
    if element.get("constraintType").and_then(Value::as_str) != Some("FOREIGN_KEY") {
        return Ok(None);
    }
    // Cardinality, named when present. `MANY_TO_MANY` is carried (the crate's conversion refuses it)
    // and an absent one is carried as silent - both faithful, matching `RelationshipType`'s closed
    // `Deserialize` and the finding's "declares when present, silent when not".
    let relationship_type = element.get("relationshipType").and_then(Value::as_str).map(str::to_owned);
    // The origin is the enclosing table's own column set. The crate's own shape carries one column
    // per side, so a side declaring more than one is refused by name rather than narrowed.
    let origin_column = one_referenced(element, "columns", entity)?;
    // `referredColumns` carries a fully qualified column name
    // (`service.database.schema.table.column`, exactly five segments, quoted per
    // `FullyQualifiedName`'s own grammar where a segment holds its own `.` or `"`), never a bare
    // column name and never a separate `referencedTable` field - there is no such field on the
    // wire. `split_fqn_tail` is the strict parser; see its own header for what it refuses and why.
    let referred = one_referenced(element, "referredColumns", entity)?;
    let (target_model, target_column) = split_fqn_tail(&referred).ok_or(HttpReaderError::UnexpectedShape {
        entity,
        field: "tableConstraints[].referredColumns[0]",
    })?;
    // No per-constraint `name` exists on the wire, so this crate synthesises a stable one from the
    // join it describes.
    let name = format!("{origin_model}_{origin_column}_fk");
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

/// A single-element array field, refused when it holds anything but exactly one entry - the same
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

/// Splits a fully qualified name into its dot-separated segments, decoded per
/// `FullyQualifiedName`'s own grammar (`FullyQualifiedName.java`, measured against
/// `open-metadata/OpenMetadata@main`): a segment containing a literal `.` or `"` is wrapped in
/// `"..."`, with an embedded `"` doubled (`""`) rather than escaped any other way - `quoteName`'s
/// own doc comment states exactly that pair, and `isQuotedName`/`decodeQuotedName` are the two
/// halves this walk mirrors. `None` on a segment that never closes its quote, on a bare `"`
/// outside one, or on an empty segment (two `.` in a row, or a leading or trailing one) - the
/// shape a hand-rolled `char == '"' => toggle` parser (the round-2 review's own probe) accepts
/// silently and gets wrong on an escaped `""` or an unterminated quote.
fn split_fqn(fqn: &str) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut chars = fqn.chars().peekable();
    loop {
        let mut segment = String::new();
        match chars.peek() {
            None => return None,
            Some('"') => {
                let _ignored = chars.next();
                loop {
                    match chars.next() {
                        None => return None,
                        Some('"') => {
                            if chars.peek() == Some(&'"') {
                                let _ignored = chars.next();
                                segment.push('"');
                            } else {
                                break;
                            }
                        }
                        Some(other) => segment.push(other),
                    }
                }
            }
            Some(_) => {
                while let Some(&c) = chars.peek() {
                    if c == '.' {
                        break;
                    }
                    if c == '"' {
                        // A bare quote outside a quoted segment is not this grammar: `quoteName`
                        // wraps any name containing one, so an unquoted run can never carry one.
                        return None;
                    }
                    segment.push(c);
                    let _ignored = chars.next();
                }
            }
        }
        if segment.is_empty() {
            return None;
        }
        segments.push(segment);
        match chars.next() {
            None => break,
            Some('.') => {}
            Some(_) => return None,
        }
    }
    Some(segments)
}

/// Splits a column's fully qualified name into its table and column names - the last two of
/// exactly `service.database.schema.table.column`'s five segments. `FullyQualifiedName.
/// getColumnName`/`getTableFQN` (measured against `main`) accept FIVE OR MORE - a nested
/// struct column's own children extend the name past the fifth segment
/// (`service.database.schema.table.column.child1.child2`) - but a `FOREIGN_KEY` constraint never
/// targets a struct member, only a column, so this crate narrows to exactly five and refuses
/// anything else by name rather than silently taking the fifth segment of a longer path and
/// dropping the rest, the way the upstream helper does. **Stated limit, not a bug**: a real
/// instance whose referred column is itself a struct member (unusual for a primary/foreign key)
/// is refused here rather than read as the wrong column.
fn split_fqn_tail(fqn: &str) -> Option<(String, String)> {
    let mut segments = split_fqn(fqn)?;
    if segments.len() != 5 {
        return None;
    }
    let column = segments.pop()?;
    let table = segments.pop()?;
    Some((table, column))
}

/// One `metric` entity into this crate's own [`crate::document::Metric`] shape.
///
/// **Stated limit:** `metric.json` requires only `id` and `name` - `metricType` is optional on the
/// wire. This crate's own [`crate::document::Metric`] requires it (a reported-not-defined metric
/// with no aggregation kind is not a shape any test here has needed), so an untyped metric refuses
/// the WHOLE catalog read with [`HttpReaderError::UnexpectedShape`] rather than being skipped and
/// silently dropped from the bundle - a deliberate refusal, not an oversight.
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

#[cfg(test)]
mod fqn_tests {
    use super::{split_fqn, split_fqn_tail};

    #[test]
    fn a_five_segment_fqn_gives_the_table_and_the_column() {
        assert_eq!(
            split_fqn_tail("warehouse.default.sales.customers.customer_id"),
            Some((String::from("customers"), String::from("customer_id")))
        );
    }

    #[test]
    fn a_two_segment_fqn_is_refused() {
        assert_eq!(split_fqn_tail("table.column"), None);
    }

    #[test]
    fn a_four_segment_fqn_is_refused() {
        assert_eq!(split_fqn_tail("warehouse.sales.customers.customer_id"), None);
    }

    #[test]
    fn a_six_segment_fqn_is_refused() {
        // A nested struct column's own child - a real, but out-of-scope, OpenMetadata shape:
        // `getColumnName`/`getTableFQN` accept it upstream, and this crate refuses it by name
        // instead, per `split_fqn_tail`'s own header.
        assert_eq!(split_fqn_tail("warehouse.default.sales.customers.address.city"), None);
    }

    #[test]
    fn an_unterminated_quote_is_refused() {
        assert_eq!(split_fqn(r#"warehouse.default.sales."customers.customer_id"#), None);
    }

    #[test]
    fn a_bare_quote_outside_a_quoted_segment_is_refused() {
        assert_eq!(split_fqn(r#"ware"house.default.sales.customers.customer_id"#), None);
    }

    #[test]
    fn an_empty_segment_from_two_consecutive_dots_is_refused() {
        assert_eq!(split_fqn("warehouse..sales.customers.customer_id"), None);
    }

    #[test]
    fn a_leading_or_trailing_dot_is_refused() {
        assert_eq!(split_fqn(".warehouse.default.sales.customers"), None);
        assert_eq!(split_fqn("warehouse.default.sales.customers."), None);
    }

    /// **The escaped-quote happy cell** - a segment holding a literal `"` is doubled per
    /// `FullyQualifiedName.quoteName`'s own doc comment, and `split_fqn` must decode it back to
    /// the one literal quote rather than leaving the escape in the segment or refusing it.
    #[test]
    fn a_segment_with_an_escaped_quote_decodes_to_one_literal_quote() {
        assert_eq!(
            split_fqn_tail(r#"warehouse.default."my ""weird"" schema".customers.customer_id"#),
            Some((String::from("customers"), String::from("customer_id")))
        );
        assert_eq!(
            split_fqn(r#"warehouse.default."my ""weird"" schema".customers.customer_id"#).map(|s| s[2].clone()),
            Some(String::from(r#"my "weird" schema"#))
        );
    }

    /// A segment holding a literal `.` (quoted, per the same grammar) must not be split on that
    /// internal dot - `warehouse.default."sales.eu".customers.customer_id` is still five segments,
    /// the third being the literal text `sales.eu`.
    #[test]
    fn a_quoted_segment_holding_a_literal_dot_is_not_split_on_it() {
        assert_eq!(
            split_fqn(r#"warehouse.default."sales.eu".customers.customer_id"#),
            Some(vec![
                String::from("warehouse"),
                String::from("default"),
                String::from("sales.eu"),
                String::from("customers"),
                String::from("customer_id"),
            ])
        );
    }
}
