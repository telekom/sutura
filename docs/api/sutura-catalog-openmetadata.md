<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-openmetadata

The public API of `sutura-catalog-openmetadata`, rendered from rustdoc JSON.

A `SemanticCatalog` over an `OpenMetadata` deployment: the richest of the three measured metadata
sources, and still a **declaring** one.

`docs/what-openmetadata-can-carry.md` is the finding that decides this adapter's whole shape.
`OpenMetadata` carries a typed metric entity and declared relationship cardinality - both richer
than `DataHub` or `Frictionless Table Schema` - but keeps **two half-a-definition slots**: a measure's
aggregation-to-column binding lives in `measures[].expression` / `metricExpression.code` as free
text in a dialect set (`SQL`/`Java`/`JavaScript`/`Python`/`External`) that does not intersect this
repository's typed `Term`, and the required filter is a raw SQL `where`. ADR 0016's refusal fires
on both: a measure carried as a raw expression string is half a definition, and taking it would
certify a foreign dialect's free text.

So this adapter is a `CatalogKind::Declaring` source that provides the physical model, the
descriptions, and the declared non-duplicating joins, and treats the kind it cannot faithfully
express exactly as the finding says: a metric whose measure is an expression string is
**reported and not defined** - its `metricType` is decidable but its bound column is not
resolvable from the foreign text, so it is decoded and set aside, never minted into a domain
`Measure`. `capabilities()` declares the deployment-dependent kinds (`Metrics`, `Grains` reached
through those metrics, and `Cardinality` observed only through a dimension reached via a
relationship) as **declared-and-empty may-provide kinds**, the 0011 state
`DefinitionCapabilities::of_may_provide` exists for.

# What is built here, and what is NOT

This crate contains everything `OpenMetadataCatalog` DECIDES about the documents a reader
extracts. It is tested against a fake reader that serves recorded documents - the port gets a fake,
not mocked HTTP. `SnapshotReader` is the seam a real reader over `OpenMetadata`'s `REST` API
(`/api/v1/tables`, `/api/v1/metrics`, …) implements, with a bearer credential; that HTTP reader is
deliberately NOT in this first PR, so the crate stays green (a service has no network in the nix
sandbox). The `http` reader + the live provisioned leg are the recorded follow-up.

# The declaration, and what it means for the bundle

`Structure`, `Descriptions` and `Relationships` are provided unconditionally: a `Table` with its
`columns[]` becomes a model, its `description` the model's description, and a `tableConstraint` /
`foreignKey` whose `relationshipType` is declared `ONE_TO_ONE` / `MANY_TO_ONE` / `ONE_TO_MANY` a
relationship. A relationship whose cardinality is absent **or** `MANY_TO_MANY` is refused naming
it - this is the pleasant surprise the finding records: `OpenMetadata` declares cardinality when it
is there and stays silent when it is not, so an undeclared relationship licenses nothing, and a
row-duplicating one is refused by this adapter rather than defaulted in either direction. A bundle
whose relationships are all unconstrained therefore carries none, lawfully.

`Metrics`, `Grains` and `Cardinality` are declared-and-empty may-provide kinds: whether a bundle
carries any is the deployment's decision (it defined a metric whose binding resolves, or it did
not), so absence is faithful rather than an aspirational claim. The metric entity IS decoded and
`metricType` + `granularity` + `dimensions[].type` are read, but `Measure` is minted only where a
column binding resolves to a domain `Term` without certifying a foreign dialect's free text - which
the recorded fixtures deliberately do not - so today those kinds arrive empty and the expression
strings stay reported-not-defined, exactly as the ADR 0016 refusal demands.

`RequiredFilters`, `AllowedValues` and `Anchors` are not declared at all: the filter is a raw SQL
`where` never parsed into `RequiredFilter`, a dimension carries no allowlist, and the metric/table
entities carry no `Anchor`. `KnowledgeCapabilities::none()` on the knowledge half, because only
prose travels (descriptions/tags) and none of the referent-bearing kinds is read.

## `trait SnapshotReader`

```rust
pub trait SnapshotReader
```

Where the documents a `OpenMetadataCatalog` decides over come from.

**The fake seam.** Everything above this trait is decided and tested against recorded documents;
a real implementor speaks to `OpenMetadata`'s `REST` API, decodes into `document::Snapshot`, and maps
its own failures into `OpenMetadataError::Read`. Every other test reads against the recorded
source. A port rather than a method on the catalog, for the same reason the warehouse port exists:
a catalog that could be swapped for a live source without the conversion changing is the point.

## `enum OpenMetadataError`

```rust
pub enum OpenMetadataError
```

Why a record could not be read as a catalog.

Every variant is a typed contract rather than a message; the variant is what a caller can branch
on. The mapping variants carry the entity name they were refused on, because a catalog is many
entities and "invalid identifier" with no name sends a reader back to all of them.

### Variants

- `Read`
- `Identifier`
- `UnknownSource`
- `MissingDescription`
- `Description`
- `ColumnDescription`
- `CardinalityUnrepresentable`
- `Inconsistent`
- `Knowledge`
- `Digest`

### Implements

`Debug`, `Display`, `Error`

## `struct OpenMetadataCatalog`

```rust
pub struct OpenMetadataCatalog<R>
```

A catalog read from an `OpenMetadata` deployment's documents.

Generic in its reader rather than holding a boxed one, matching the warehouse adapters: there is
one per composition and a generic keeps the chosen reader visible. It carries the deployment's
source mapping - which service/database alias answers to which `SourceName` - and the declared
name and version it is recorded under.

### Methods

```rust
pub const fn new(name: SourceName, version: DefinitionVersion, sources: std::collections::BTreeMap<String, SourceName>, reader: R) -> Self
```

Opens a catalog over an `OpenMetadata` reader.

`sources` is the deployment's mapping from a service/database alias to the `sources.<alias>` a
model on that platform reads from. A model on a platform with no entry is refused at load
(`OpenMetadataError::UnknownSource`) rather than guessed.

### Implements

`Clone`, `Debug`, `SemanticCatalog`

## Module `document`

The ingestion shape a `super::SnapshotReader` returns: what this adapter needs from `OpenMetadata`.

**These are not literally `OpenMetadata`'s wire documents, and saying which is the point.**
`OpenMetadata` serves its entities inside a `REST` envelope with more fields than any one consumer
cares about, and a document that claimed to be that envelope while refusing every field it did not
name would be a false promise the moment a real response arrived. So the shapes here are the
adapter's own canonical statement of the entity CONTENT a reader must extract and decode - the
decision side of `docs/what-openmetadata-can-carry.md`'s transport note - and `deny_unknown_fields`
holds over THIS shape and over the recorded fixtures a reader decodes, rather than over
`OpenMetadata`'s envelope. A real HTTP reader maps the service's document into one of these.

The fields track the entities `docs/what-openmetadata-can-carry.md` measured: a `Table` and its
`columns[]`, a `tableConstraint` / `foreignKey` with a `relationshipType` whose cardinality is
named when present and silent when not, and a `Metric` whose `metricType` is decidable but whose
bound column is not resolvable from its free-text expression. Everything an adapter DECIDES below
this shape is tested against a fake reader that serves recorded documents, which is the port's own
rule.

`RelationshipType` mirrors `OpenMetadata`'s cardinality enumeration: `ONE_TO_ONE`, `MANY_TO_ONE`,
`ONE_TO_MANY` are non-duplicating and license a `JoinType`, while `MANY_TO_MANY` - or a silent
absence - licences nothing and is refused by the conversion, because `JoinType` has no
many-to-many shape and a relationship nobody vouched for licenses no join. The fields are private
with accessors, per the workspace's `check-boundaries` rule that a library crate's types are its
contract; an untyped value (a `name`, a `column`, a `service`) is parsed during the conversion,
not in these carriers.

### `struct Snapshot`

```rust
pub struct Snapshot
```

Everything a reader fetched, before any of it is converted.

`deny_unknown_fields` here too - the three entity groups are the whole of what a reader must
extract, and a snapshot carrying a fourth is a reader this adapter has not been told to expect.

#### Methods

```rust
pub fn metrics(&self) -> &[Metric]
```

The metrics this snapshot carries.

Read so the reported-not-defined cell can prove a metric a snapshot carries never becomes a
domain `Metric`.

```rust
pub const fn new(tables: Vec<Table>, relationships: BTreeMap<String, StructuralRelationship>, metrics: Vec<Metric>) -> Self
```

Builds a snapshot from decoded entity groups - the constructor a real
`super::SnapshotReader` uses to hand the conversion a `Snapshot` it decoded off the wire.
The recorded fixtures and the converter's own tests prefer `Deserialize` (a real reader
reaches for the reader-visible shape), but an HTTP reader assembling a `Snapshot` from
harvested entities needs a constructor the way `sutura-catalog-datahub`'s `Snapshot::new`
provides one.

```rust
pub fn relationship_count(&self) -> usize
```

How many declared relationships the snapshot carries.

```rust
pub fn relationships(&self) -> impl Iterator<Item>
```

The declared relationships, by name.

Iterated rather than indexed, so a caller cannot name a relationship the snapshot does not
carry: the pair comes from one place.

```rust
pub fn tables(&self) -> &[Table]
```

The tables this snapshot carries.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct ColumnMetadata`

```rust
pub struct ColumnMetadata
```

What `OpenMetadata`'s `Column` schema COULD supply a reader beyond a column's name.

Its `dataType`, its own `description`, and (via `Table::primary_key`) whether its
`constraint` is `PRIMARY_KEY`. This adapter's own canonical shape for it - no
`super::SnapshotReader` but the fixture and a test stub exists today, so nothing yet maps a real
`constraint` value into `Table::primary_key`; see the crate header's "What is built here,
and what is NOT".

Identical in shape to `datahub::document::ColumnMetadata` and `rdbms::ColumnMetadata`, and
kept separate for the reason the latter's own doc gives: each is one adapter's reading of a
wire shape none of the others should depend on.

#### Methods

```rust
pub fn data_type(&self) -> Option<&str>
```

```rust
pub fn description(&self) -> Option<&str>
```

```rust
pub const fn new(data_type: Option<String>, description: Option<String>) -> Self
```

#### Implements

`Clone`, `Debug`, `Default`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct Table`

```rust
pub struct Table
```

What a `Table` entity supplies a model: a table, its columns, the service it lives on, and a
description.

`column_metadata` and `primary_key` are both `#[serde(default)]`, so a recorded document that
predates either still deserializes - the same reason every field here has no `pub` constructor:
a document arrives only through `Deserialize`, and a struct literal would let a caller build a
`Table` the load path never checked.

#### Methods

```rust
pub fn column_metadata(&self, column: &str) -> Option<&ColumnMetadata>
```

One column's `dataType`/`description` evidence, by name.

```rust
pub fn columns(&self) -> &[String]
```

The physical column set.

```rust
pub fn description(&self) -> Option<&str>
```

Free-text prose about the table, if any.

```rust
pub fn name(&self) -> &str
```

The table (and model) name.

```rust
pub fn primary_key(&self) -> &[String]
```

Which columns carry a `PRIMARY_KEY` constraint.

```rust
pub fn service(&self) -> &str
```

The service this table lives on.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `enum RelationshipType`

```rust
pub enum RelationshipType
```

`OpenMetadata`'s relationship cardinality, as a closed set.

Named when present and silent when not; `ManyToMany` is refused by the conversion because
`JoinType` has no many-to-many shape.

#### Variants

- `OneToOne`
- `ManyToOne`
- `OneToMany`
- `ManyToMany`

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct StructuralRelationship`

```rust
pub struct StructuralRelationship
```

One declared relationship's structural endpoints and its cardinality.

#### Methods

```rust
pub fn origin_column(&self) -> &str
```

The column of the origin model the join is on.

```rust
pub fn origin_model(&self) -> &str
```

The model the join starts from.

```rust
pub const fn relationship_type(&self) -> Option<RelationshipType>
```

The cardinality, named when `OpenMetadata` declares it and silent when it does not.

```rust
pub fn target_column(&self) -> &str
```

The column of the target model the join is on.

```rust
pub fn target_model(&self) -> &str
```

The model the join reaches.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct Metric`

```rust
pub struct Metric
```

What a `Metric` entity supplies.

**Deliberately minimal and deliberately un-analysed at conversion.** `metricType` is decidable
but the bound column is not resolvable from `metricExpression` / `measures[].expression` - free
text in a foreign dialect - and a measure carried as such is reported and not defined. The metric
is decoded (so the reported-not-defined cell can prove it is carried and ignored) and never
minted into a domain `Metric`.

#### Methods

```rust
pub fn aggregation(&self) -> &str
```

The decidable aggregation-kind word, e.g. `SUM`.

```rust
pub fn expression(&self) -> Option<&str>
```

The raw expression text a deployment wrote to bind the measure to a column.

```rust
pub fn granularity(&self) -> Option<&str>
```

The declared granularity, e.g. `DAY`, if any.

```rust
pub fn name(&self) -> &str
```

The metric's name.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

## Module `fixture`

The recorded fixture corpus and the fake reader that serves it.

This is the only `SnapshotReader` implementor today, and it is the **fake** the port is tested
against - recorded documents, not mocked HTTP. The corpus is a bundle of two models and one
declared non-duplicating join, plus one metric whose measure is an expression string; the metric
is carried and never minted, because its bound column is not resolvable from a foreign-dialect
expression (the reported-not-defined half this crate's declaration promises).

The documents are decoded through `serde_json` at read time, so the same deserialization path a
real reader over `OpenMetadata`'s `REST` API would use is exercised, and `deny_unknown_fields` on
the wire shapes holds over these recorded documents. `over_fixture_source` is what the conformance
registry calls to register the adapter; it is `pub` because an integration suite is a separate
crate and cannot reach a `#[cfg(test)]` item.

### `struct FixtureReader`

```rust
pub struct FixtureReader
```

The fake `SnapshotReader` that serves the recorded corpus.

#### Implements

`Clone`, `Debug`, `SnapshotReader`

### `fn over_fixture_source`

```rust
pub fn over_fixture_source(name: sutura_domain::model::SourceName, version: sutura_domain::pinned::DefinitionVersion) -> crate::OpenMetadataCatalog<FixtureReader>
```

An `OpenMetadataCatalog` over the recorded corpus.

The source mapping answers the one service the corpus names - `warehouse` - with the deployment's
declared source, which is what lets a model on that platform be opened. This is the constructor
the conformance registry uses to register the adapter.

## Module `http`

The real `crate::SnapshotReader`: two paged reads over `OpenMetadata`'s `REST` API, assembled
into one `crate::document::Snapshot`.

Behind the crate's default-off `http` feature - see `Cargo.toml`'s own comment on why - so a
build that does not ask for this reader links no outbound TLS stack.

# What is measured, and what is NOT

**The `Table` and `Metric` wire shapes are read against the published JSON Schema**
(`open-metadata/OpenMetadata`'s `openmetadata-spec`, `table.json`/`metric.json` on `main`) and
the `TableResource`/`MetricResource` Java sources for which fields the list endpoint returns
unconditionally versus only behind `?fields=` - not against a provisioned instance (the nix
sandbox has no network; this is a schema read, not a live one). `docs/what-openmetadata-can-carry.md`
was corrected against the same schema read (its `foreignKeys`/`referencedTable` shape was a
first-draft invention no real deployment serves; `harvest_relationship` below reads
`tableConstraints` instead). So the mapping functions here are a **first claim** this crate has
made about `OpenMetadata`'s served envelope, the same way `sutura-catalog-datahub`'s `dataset`
mapping was before its provisioned tier measured it. Each mapping refuses an unexpected shape as
a typed `HttpReaderError::UnexpectedShape` naming the entity and the field, rather than
reading past a missing or mistyped key with a default - a guess that happened to be wrong would
otherwise certify a bundle silently missing a model, a join or a metric. **Do not cite this
reader as proof the `OpenMetadata` half works against a real instance until an acceptance leg
measures it - the schema read is not that leg.**

# What every read is bounded by

`ReadBounds` carries a request timeout and a response-size cap, both **settings with defaults,
not constants** - `DEFAULT_TIMEOUT_SECONDS` and `DEFAULT_MAX_RESPONSE_BYTES` are the values a
composition root's settings default to, following `sutura-config`'s own convention of a default
function per optional key, not a value baked into this type. `read`
makes up to two requests (tables, then metrics) and shares ONE deadline across them - opened
once, and what is left after the first is what the second gets - the same shape
`sutura_domain::warehouse::deadline::Deadline` and `sutura-catalog-datahub`'s own reader hold.
**The aggregate byte cost of one `read()` is bounded by construction, not by a third check**:
two requests at `cap` each is at most `2×cap` read into memory before either response is
checked, and `fetch`'s own `ureq` backstop (`limit(2×cap)` per request, ahead of the precise
`len > cap` refusal) makes the true per-request ceiling `2×cap` rather than `cap` - so a single
`read()` never holds more than `4×cap` at once across both in-flight bodies. Stated here rather
than measured, because nothing enforces a THIRD, aggregate ceiling; a future third request would
raise this number and this sentence would have to move with it.

# Auth

A bearer token as a `Secret`, sent as `Authorization: Bearer <token>` on every request. The
token is a constructor argument here; a composition root reads it from a settings-declared file
at boot (`token_file`), never inline in a settings document.

# Paging

One page per entity kind, at a generous count. A page that SIGNALS more results exist - an
`after` cursor, or a returned count below a reported `paging.total` - is refused
(`HttpReaderError::MorePages`) rather than silently read as complete: the same "one page or a
refusal" shape `sutura-catalog-datahub`'s reader and `sutura-exec-bigquery`'s wire hold for
`jobs.query`, because a caller must not certify a bundle built from a `Snapshot` that silently
dropped a model or a metric.

# TLS and the endpoint

`Endpoint` and its `Endpoint::parse` now live in `sutura-http-client`, shared with
`sutura-catalog-datahub`'s identical reader since issue #970's review found the two
byte-for-byte the same (`cargo xtask check-jscpd`). `HttpSnapshotReader::new` takes one
rather than a `String` - a caller cannot dial an endpoint this module has not validated. The
grammar and each refusal: `scheme://host[:port]`, scheme `http` or `https` (case-folded) on a
`ureq::http::Uri`, an optional nonzero valid `:port`, an optional trailing `/`, and nothing
else; `https://` for any host, `http://` only for an IP loopback literal
(`sutura_domain::source::host_is_loopback`). This mirrors `DataHub` exactly because the two
readers share the same security posture: a bearer prepared for a plaintext host that is not
loopback is a token handed to whoever answers that name.

`ureq`'s compiled-in default root set (for an `https://` endpoint), `max_redirects(0)` and the
proxy left on are the other pins; a deployment MAY replace the compiled-in roots with its own
CA via `security.outbound.transport_anchors` (`#125`), folded in `sutura_http_client::tls` -
anchors only, no client identity.

### `enum HttpReaderError`

```rust
pub enum HttpReaderError
```

Why one of the two entity reads did not produce the entities it names.

Reaches `crate::OpenMetadataCatalog` boxed inside `OpenMetadataError::Read` - the port's own
coarse variant - so this stays inspectable by a caller that knows to downcast, the `ErasedCause`
shape `.agents/skills/sutura/secure-by-design/SKILL.md` argues for at a boundary.

#### Variants

- `DeadlineSpent` - The shared budget was gone before this entity's page could be requested.
- `Unreachable` - The entity's page was not reached.
- `Unreadable` - The entity's page was reached and its answer could not be read.
- `Refused` - `OpenMetadata` refused the request. `Display` renders the status and never `detail`, because a cause-chain walk that flattens every link with `Display` must not carry endpoint-owned text.
- `TooLarge` - The page was larger than the cap this reader will read.
- `NotADocument` - The page was not a JSON document.
- `UnexpectedShape` - One entity did not carry a field this reader expects, or carried it in a shape it does not recognise.

  **Refused rather than guessed** - see the module header on what is measured and what is not.
  `field` is a dotted path (e.g. `"columns[].name"`) so a refusal names exactly where the
  document stopped matching this reader's expectation.
- `NotTheCanonicalShape` - The page's own field mapped into this crate's canonical aspect shape and that decode failed - a defect in this reader's mapping rather than in the page, since every field reaching `serde_json::from_value` here was already read out of the page by name above.
- `MorePages` - The page stated or implied more results exist than the one page this reader will read.

#### Implements

`Debug`, `Display`, `Error`

### `struct HttpSnapshotReader`

```rust
pub struct HttpSnapshotReader
```

An `OpenMetadata` deployment, reached over HTTP.

Not generic over its credential the way `sutura-exec-bigquery`'s transport is: there is exactly
one credential shape here, a bearer token, so a type parameter would buy nothing a second
constructor would not.

#### Methods

```rust
pub fn new(endpoint: Endpoint, token: Secret, bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self
```

Opens a reader. `endpoint` (only `Endpoint::parse`), `token` (read from a settings-
declared file at boot) and `bounds` (only `ReadBounds::parse`) are all checked first.

**`anchors` is `security.outbound.transport_anchors` (`#125`), resolved once at boot**: `None`
leaves `ureq`'s compiled-in `RootCerts::WebPki`, `Some` replaces it with
`RootCerts::Specific` from exactly the declared certificates - never a union of the two (see
`sutura_http_client::tls`). This constructor never presents a client identity -
`Self::rotating_agent` is the one that does.

```rust
pub const fn rotating(endpoint: Endpoint, token: Secret, bounds: ReadBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self
```

The rotation-lane constructor: holds the rotating agent handle a composition root built (via
`Self::rotating_agent`) and drove to re-read on `sutura_tls::POLL_INTERVAL`. The reader is
per-request, so the agent `current()` resolves to on the next `read` is the latest that loaded.

```rust
pub fn rotating_agent(bounds: ReadBounds, declared: Option<sutura_tls::Declared>) -> Result<OutboundAgent, sutura_tls::LoadError>
```

Builds the reader's rotating agent handle for a declared `security.outbound` set, and (when
one is declared) the `sutura_tls::Rotator` the composition root drives on
`sutura_tls::POLL_INTERVAL`. `None` (no declaration) returns a fixed handle over `ureq`'s
compiled-in roots, presenting no identity, and no poll handle.

# Errors

The declared bundle or client identity cannot be loaded at boot.

#### Implements

`Clone`, `Debug`, `SnapshotReader`

### `use Budget`

### `use DEFAULT_MAX_RESPONSE_BYTES`

### `use DEFAULT_TIMEOUT_SECONDS`

### `use Endpoint`

### `use EndpointMessage`

### `use InvalidEndpoint`

### `use InvalidReadBounds`

### `use OutboundAgent`

### `use ReadBounds`

## Module `test_support`

The happy-path `OpenMetadata` wire pages this crate's own tests need.

Over the loopback fake `sutura-http-client::test_support` now hosts (issue #970's review: this
file's own `FakeServer`/`Scripted`/plumbing was byte-for-byte identical to
`sutura-catalog-datahub`'s copy, `cargo xtask check-jscpd` measured). Re-exported here so
`sutura-cli`'s served-binary suite (`crates/sutura-cli/tests/served/openmetadata.rs`) keeps
building `test_support::FakeServer` off THIS crate's public API - it takes
`sutura-catalog-openmetadata` as a dependency, not `sutura-http-client` directly.

`#[cfg(feature = "http")]`, not `#[cfg(test)]`, for the reason `sutura_http_client::test_support`'s
own header gives: an integration test binary cannot see another crate's `tests/` directory, so
the only way to share a fake across crates is through a library, `pub`, reachable at compile
time from whichever feature both a reader and its composition root's tests turn on.

### `fn tables_page`

```rust
pub fn tables_page() -> serde_json::Value
```

One `tables` page, over the two models the crate's recorded fixture carries (`orders` and
`customers`), each with its columns, data types, and descriptions. Served in the shape a REAL
`OpenMetadata` list endpoint answers: a `data` array with a `paging` block whose `total` matches
what was returned (so the reader does not refuse it as truncated).

### `fn metrics_page`

```rust
pub fn metrics_page() -> serde_json::Value
```

One `metrics` page carrying the recorded fixture's OWN reported-not-defined metric, so the two
transports cannot drift - it is read through the crate's public fixture rather than restated.

### `fn happy_path_answers`

```rust
pub fn happy_path_answers() -> Vec<Scripted>
```

The two pages a `read()` call makes, in order, all answering `200` - what a real `OpenMetadata`
carrying exactly the recorded fixture's content would serve.

### `use CapturedAuthorizations`

### `use FakeServer`

### `use Scripted`

### `constant SERVICE`

The `sources.<alias>` the happy-path tables sit on, matching the recorded fixture corpus's own
 `service: "warehouse"` so the two transports serve one content.

 A fixed test constant, independent of a deployment's own choice, the way `sutura-catalog-datahub`
's `DEPLOYMENT_PROPERTY` is: a fake carrying the adapter's own constant would pass whether the
 service were the deployment's choice or a value this crate required.
