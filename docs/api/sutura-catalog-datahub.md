<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-datahub

The public API of `sutura-catalog-datahub`, rendered from rustdoc JSON.

A `SemanticCatalog` over `DataHub`'s entity aspects: the canonical **declaring** source.

`docs/adr/0016-what-datahub-can-carry.md` is the measurement that decides what this adapter is.
`DataHub` 1.7.0 holds a measure as a raw expression string in a dialect set that does not intersect
this repository's, and its physical relationships default cardinality to many-to-many - so it
supplies the physical model, the descriptions and the join columns, and supplies no measure this
adapter will execute **unless the deployment defines the metric itself**, no reliable cardinality,
no definitional filter, no grain, no value allowlist and no anchor. That is the shape of a
**declaring** adapter, and it is measured against its own declaration rather than against the
golden adapters' oracle. **The excluded half is the knowledge kinds, declared empty below** -
every one of the nine `DefinitionKind`s is covered, either provided unconditionally or as a
declared-and-empty may-provide kind, so there is no definition kind this adapter declares itself
out of.

**Issue #202 is the exception the previous paragraph stops at, and it lives in this crate.**
`DataHub`'s `structuredProperty` is scalar-only, so a deployment cannot define a nested metric
object; what it CAN define is one string-valued structured property - under a name of its own,
`sutura` being the field this adapter's canonical shape carries it under rather than a urn this
crate dictates - and the
reader decodes its scalar value into the closed-vocabulary content a certified metric needs -
the flat-to-nested assembly `document::SuturaProperty::assemble` implements and the recorded
fixture is kept in. Where that shape is present, this adapter reads it into a certified
`Metric`, turning `provides no
metrics` into `provides metrics for a metric that carries the custom shape`. Where it is absent,
the metric stays the promotion candidate `docs/adr/0016` describes. The shape is closed - the
measure and filter vocabularies are `sutura_domain`'s own, and `deny_unknown_fields` refuses a
property this adapter does not recognise rather than guessing.

# What is built here, and what is NOT

This crate contains everything `DataHubCatalog` DECIDES about the aspects it reads, and it is
tested against a fake reader that serves recorded documents - the port gets a fake, not mocked
HTTP. `AspectReader` is the seam a real reader over `DataHub`'s versioned `OpenAPI` v3 entity
surface implements, and since issue #202's HTTP reader, one now does: `http::HttpAspectReader`,
behind this crate's default-off `http` feature, with a personal access token as a bearer. Its own
module header states what is measured against a live `DataHub` and what is not - only the
`metric` entity's wire shape is, today. The recorded fixture source in `fixture` and the two
test doubles (`tests::Stub`, the acceptance suite's `Composed`) remain what every other test in
this crate reads against. **And nothing serves the new reader yet:** no composition root links
this crate (its only dependant is `sutura-app`, as a dev-dependency), and `sutura-serve` refuses
`catalog.kind: datahub` by name - that half is a separate, stacked change. Everything here is
decided and tested; what is not is a served composition - the *Built and not wired* register in
`.agents/skills/sutura/query-surface/SKILL.md` records it, and that register is the one place it
may be read from - it is not an invariant.

**What that register no longer says is that the cost is unmeasured.** `docs/adr/0016`'s
*Revision, 2026-09-04* has the numbers, off a provisioned instance: a bundle's metric half is ONE
paged request carrying `structuredProperties` and `metricInfo` inline, not a request per metric -
**and that surface is search-backed, so it is not read-your-writes.** A reader written against it
pages an eventually-consistent view; the by-urn form is the immediate one. The limit belongs
beside the cost rather than after it.

# The declaration, and what it means for the bundle

`SemanticCatalog::KIND` is `CatalogKind::Declaring`. `SemanticCatalog::capabilities`
provides `Structure`, `Descriptions` and `Relationships` unconditionally, and declares
`Metrics`, `Grains`, `RequiredFilters`, `AllowedValues` and `Anchors` as **declared-and-empty
may-provide kinds** - the 0011 state `DefinitionCapabilities::of_may_provide` adds, whose whole
job is exactly this: a `DataHub` metric's measure, grains, filters, dimension allowlists and anchor
all arrive from the deployment-defined `sutura` structured property, so whether a bundle carries
any of them is the deployment's decision and absence is a faithful bundle, not an aspirational
claim. A `DataHub`-only deployment therefore uses the physical model, the prose, the join columns
and whatever metrics and definitions the deployment wrote as structured properties, and a bundle
with none of the deployment-authored kinds still loads and validates, because
`Definitions::assemble` has no minimum-metric refusal.

**The knowledge half is empty on purpose, and why is worth stating rather than glossed.**
`DataHub` keeps its glossary-like synonym content on a separate entity (`AiContext`), and nothing
here reads it - so a standalone bundle carries no `Knowledge` referent for a phrase or a caveat to
attach to, and the declaration says no knowledge rather than advertising a capability this crate
cannot satisfy alone.

# The one nuance that is the point

`Cardinality` arrives in `DataHub` as an unreliable default (relationships default to
many-to-many), so this adapter refuses a relationship it cannot vouch for rather than reading
one: absent or many-to-many cardinality is refused naming the relationship, not defaulted in
either direction. What the adapter WILL carry is a cardinality the deployment DECLARES a
dimension through - a dimension with a `via` is observed as *a dimension reached through a
relationship*, which is the only way `Cardinality` is produced here, and the declaration marks
it declared-and-empty for exactly that reason. A metric whose measure is a raw expression string
is read - `document::MetricAspect` is decoded - and never converted into a `Measure`,
because that is the promotion-candidate half; only a metric carrying the deployment-defined
`document::SuturaProperty` becomes a certified one.

## `trait AspectReader`

```rust
pub trait AspectReader
```

Where a snapshot's aspects come from.

**The fake seam.** Everything above this trait is decided and tested against recorded documents;
a real implementor speaks to `DataHub`'s versioned `OpenAPI` v3 entity surface, decodes into
`document::Snapshot`, and maps its own failures into `DataHubError::Read`. Since issue #202
one does: `http::HttpAspectReader`, behind the `http` feature. Every other test in this crate
still reads against the recorded source in `fixture` - the RESPONSE SHAPE a real implementor has
to map, and the surface's consistency, were measured rather than guessed before it was written -
see the crate header, and `http`'s own module header for which of the three entity shapes that
measurement actually covers.

A port rather than a method on `DataHubCatalog` for the same reason the warehouse port exists:
a catalog that could be swapped for a live source without the conversion changing is the point.

## `enum DataHubError`

```rust
pub enum DataHubError
```

Why a record could not be read as a catalog.

Every variant is a typed contract rather than a message; the message is for a human and the
variant is what a caller can branch on. The mapping variants carry the entity name they were
refused on, because a catalog is many entities and "invalid identifier" with no name sends a
reader back to all of them.

### Variants

- `Read` - The source did not produce a snapshot.

  An owned boxed cause, the `ErasedCause` shape this repository's boundary errors use: the
  concrete failure belongs to whichever reader is installed, and the chain still walks.
- `UnknownPlatform` - A model named a platform this deployment declared no `sources.<alias>` for.

  A `sources.<alias>` entry per platform is what lets a model's data system be opened at all -
  `docs/adr/0016` decision 7 - and the mapping is the deployment's, not this adapter's. A model
  on an unmapped platform is refused rather than guessed.
- `CardinalityUnrepresentable` - A relationship carries no cardinality, or one this adapter cannot represent.

  `docs/adr/0016` decision 5: a relationship reaches a dimension only where cardinality is
  declared and representable, and absent or many-to-many is refused naming the relationship
  rather than defaulted in either direction - `ManyToOne` as a default assumes the fan-out away,
  and `OneToMany` refuses every dimension.
- `Identifier` - A name on a snapshot did not parse as the identifier kind it claims to be.
- `Sutura` - The `sutura` structured property's scalar value is not the metric content it claims to be.
- `Description` - Prose on a snapshot is not a usable description.
- `Inconsistent` - The models, relationships and columns did not hold together.
- `Knowledge` - The bundle's knowledge does not hold together.

  This adapter declares no knowledge capability, so any knowledge content it were handed would
  be refused here (the `UndeclaredContent` guard) rather than dropped or forwarded. Today no
  snapshot produces knowledge - a `Snapshot` has no knowledge aspect to read at all - so
  this stays the wiring for content that cannot occur in the standalone deployment. Since
  issue #202 the reason is no longer that no metric exists for a `Referent` to name: a
  standalone bundle carries a certified metric.
- `Digest`

### Implements

`Debug`, `Display`, `Error`

## `struct DataHubCatalog`

```rust
pub struct DataHubCatalog<R>
```

A catalog read from `DataHub`'s entity aspects.

Generic in its reader rather than holding a boxed one, matching the warehouse adapters: there is
one per composition and a generic keeps the chosen reader visible. It carries the deployment's
source mapping - which `dataPlatform` answers to which `SourceName` - and the declared name and
version it is recorded under, the same way `sutura-catalog-local` carries its own.

### Methods

```rust
pub const fn new(name: SourceName, version: DefinitionVersion, sources: BTreeMap<String, SourceName>, reader: R) -> Self
```

Opens a catalog over a `DataHub` reader.

`sources` is the deployment's mapping from a data platform to the `sources.<alias>` a model
on that platform reads from. A model on a platform with no entry is refused at load (see
`DataHubError::UnknownPlatform`) rather than guessed, which is what makes opening the model's
data system the deployment's decision rather than this adapter's.

### Implements

`Clone`, `Debug`, `SemanticCatalog`

## Module `document`

The ingestion shape a `super::AspectReader` returns: what this adapter needs from `DataHub`.

**These are not literally `DataHub`'s wire documents, and saying which is the point.** `DataHub`
serves aspects inside an `OpenAPI` v3 envelope with more fields than any one consumer cares about,
and a document that claims to be that envelope while refusing every field it does not name would
be a false promise the moment a real response arrived. So the shapes here are the adapter's own
canonical statement of the aspect CONTENT a reader must extract and decode - the decision side of
`docs/adr/0016`'s transport note - and `deny_unknown_fields` holds over THIS shape and over the
recorded fixtures a reader decodes, rather than over `DataHub`'s envelope. A real HTTP reader maps
the service's document into one of these, exactly as `sutura-exec-bigquery`'s transport decodes
into that crate's own `wire::document` shapes.

The fields track the aspects `docs/adr/0016` measured: a `dataset`'s `schemaMetadata` and its
description aspects, a `semanticModel`'s relationships with their cardinality, and a `metric`'s
expression. Everything an adapter DECIDES below this shape is tested against a fake reader that
serves recorded documents, which is the port's own rule.

`Cardinality` mirrors `DataHub`'s `ERModelRelationshipCardinality` (declared-and-empty in this
adapter, produced only by a deployment-defined dimension with a `via`), and `N_1`-to-`N_1` is
refused by the conversion rather than carried, because `JoinType` has no many-to-many shape - a
relationship nobody vouched for licenses nothing.

The fields are private with constructors and accessors, per the workspace's `check-boundaries`
rule that a library crate's types are its contract. The guards are not in the constructors -
these are carriers of already-typed values, and where a value is untyped (a `column`, a
`model`) it is parsed during the conversion, not here - they are in the typed serde fields and
the two closedness claims (an unknown key is refused by `deny_unknown_fields`, a value that is
not a usable identifier is refused by the conversion's parse) that a `pub` field would let a
struct literal walk past.

### `struct Snapshot`

```rust
pub struct Snapshot
```

Everything a reader fetched, before any of it is converted.

`deny_unknown_fields` here too - the three aspect groups are the whole of what a reader must
extract, and a snapshot carrying a fourth is a reader this adapter has not been told to expect,
which is exactly the silent-acceptance the attribute exists to refuse on the nested shapes.

#### Methods

```rust
pub fn datasets(&self) -> &[DatasetAspect]
```

```rust
pub fn metrics(&self) -> &[MetricAspect]
```

```rust
pub const fn new(datasets: Vec<DatasetAspect>, relationships: Vec<RelationshipAspect>, metrics: Vec<MetricAspect>) -> Self
```

A snapshot assembled from the three kinds of aspect a reader fetched.

```rust
pub fn relationships(&self) -> &[RelationshipAspect]
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct DatasetAspect`

```rust
pub struct DatasetAspect
```

What a `dataset` entity supplies a model: a table, its columns, the platform it lives on, and a
description.

#### Methods

```rust
pub fn columns(&self) -> &[String]
```

```rust
pub fn description(&self) -> &str
```

```rust
pub fn name(&self) -> &str
```

```rust
pub const fn new(name: String, table: String, platform: String, columns: Vec<String>, description: String) -> Self
```

A dataset aspect.

```rust
pub fn platform(&self) -> &str
```

```rust
pub fn table(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct RelationshipAspect`

```rust
pub struct RelationshipAspect
```

What one `SemanticModelRelationship` supplies: the two endpoints and their columns, plus a
cardinality this adapter may or may not be able to represent.

#### Methods

```rust
pub const fn cardinality(&self) -> Option<Cardinality>
```

```rust
pub fn name(&self) -> &str
```

```rust
pub const fn new(name: String, from_model: String, from_column: String, to_model: String, to_column: String, cardinality: Option<Cardinality>) -> Self
```

A `SemanticModelRelationship` aspect.

```rust
pub fn origin_column(&self) -> &str
```

```rust
pub fn origin_model(&self) -> &str
```

```rust
pub fn to_column(&self) -> &str
```

```rust
pub fn to_model(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `enum Cardinality`

```rust
pub enum Cardinality
```

`DataHub`'s `ERModelRelationshipCardinality`, as a closed set.

#### Variants

- `OneOne`
- `OneN`
- `NOne`
- `NN` - Many-to-many. Refused by the conversion: there is no `JoinType` for `N_1`-to-`N_1`, and a many-to-many relationship licenses no join.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct MetricAspect`

```rust
pub struct MetricAspect
```

What a `metric` entity's `MetricInfo.expression` carries: a raw string in a dialect `DataHub`
names.

A metric that also carries the deployment-defined `SuturaContent` becomes a **certified**
metric here; one that does not stays the promotion candidate whose measure is `expression` and
is never converted. The two are the same entity and the distinction is an `Option` because which
one a given metric is depends on what the deployment defined, not on anything this adapter
decides.

#### Methods

```rust
pub fn dialect(&self) -> &str
```

The dialect the raw expression string is written in.

Part of the promotion candidate's other half; see `Self::expression` for why nothing reads
it yet.

```rust
pub fn expression(&self) -> &str
```

The promotion candidate's other half - the raw expression string in a dialect nothing here
renders.

No consumer today, and `docs/adr/0016` says so rather than pretending otherwise: the aspect
is decoded and a metric without the `sutura` property is set aside, never converted, and its
expression is not re-read. These accessors and `Self::dialect` are the readable shape a
future reporter would use.

```rust
pub fn name(&self) -> &str
```

```rust
pub const fn new(name: String, dialect: String, expression: String) -> Self
```

A promotion-candidate metric aspect, whose measure is a raw expression string.

```rust
pub const fn sutura(&self) -> Option<&SuturaProperty>
```

The deployment-defined certified content, when the metric carries it.

```rust
pub fn with_sutura(name: String, dialect: String, expression: String, sutura: SuturaContent) -> Self
```

A certified metric aspect: the expression string beside a deployment-defined content.

The two are both carried because `docs/adr/0016`'s *reconcile, never assume* rule still
applies - the raw expression is a promotion candidate's other half and remains readable even
where the structured property is what this adapter certifies. The content is serialized into
the scalar form `SuturaProperty` stores, which is the shape the wire and the recorded
fixtures carry.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct SuturaProperty`

```rust
pub struct SuturaProperty
```

`DataHub`'s view of the deployment-defined metric content: ONE structured property, whose single
scalar value is the JSON document below.

**This is what makes the scalar-only constraint literal rather than prose.** `DataHub`'s
`structuredProperty` has no nested or record value type, so a deployment cannot define a nested
object at all; what it can define is one string-valued property, and `Self::assemble` is the
step that turns that scalar into the nested `SuturaContent` - the issue #202 mechanism,
implemented here and exercised by a fixture recorded in this flat form rather than left to a
sentence.

**The property's NAME is the deployment's and does not appear here.** `sutura` is the field
`MetricAspect` carries this under on the adapter's own canonical shape; which structured
property a reader maps onto it is `docs/adr/0016` decision 7's *not ours to say*, and
`tests/provisioned.rs` registers one whose name shares nothing with this field precisely so the
independence is measured.

The scalar payload is bounded by the value-type limits a deployment's `DataHub` enforces - the
platform names its own as `structuredProperties.keywordMaxLength`, because the value is indexed
as an Elasticsearch keyword, so the bound is an index setting rather than a constant here.
What is measured is that the refusal NAMES that setting; nothing has raised it and retried,
so whether a deployment can move it is `DataHub`'s documentation and not this repository's
measurement. This crate adds no bound of its own.

#### Methods

```rust
pub fn assemble(&self) -> Result<SuturaContent, serde_json::Error>
```

The deployment-defined metric content, decoded from the scalar.

The one place `DataHub`'s scalar store becomes this adapter's nested statement of what the
deployment meant. A malformed value - which cannot arise from `with_sutura`, but can from
any wire - is a refusal naming nothing, which is why the conversion maps it into a typed
`DataHubError` rather than letting it fall through as a serde string.

```rust
pub const fn new(string_value: String) -> Self
```

A structured property whose string value is `json`.

```rust
pub fn string_value(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct SuturaContent`

```rust
pub struct SuturaContent
```

What a metric needs to be certified, in the JSON document a deployment's `sutura` property
carries.

`SuturaProperty` is why the deployment's view is one scalar and this is the reader's decoded
statement of what that scalar means.

**Everything a markdown metric can carry arrives here, over the domain's own closed
vocabularies.** The `measure` is the `sutura-domain` `Measure` type itself - the closed set,
written exactly as this repository writes it - the `grains` and `required_filters` are the
closed `Grain` and `RequiredFilter` enums, and a `dimension` and an `anchor` mirror the markdown
document's shapes. `deny_unknown_fields` sits on this document, on the measure and on the term
inside it, on a filter, on a dimension, on the anchor and on the range inside the anchor, and
refuses a property this adapter does not recognise rather than guessing. An aggregate out of the
closed set, an unknown operator, an unparseable value, an unknown grain, an unknown key at any of
those levels - all fail the decode before the conversion sees them, naming the key.

**The range is where that used to stop**, which is worth recording because the claim read *at
every depth* while it was one depth short: `sutura_domain::calendar::TimeRangeInput` carried no
`deny_unknown_fields`, so a key written INSIDE the range object was discarded in silence rather
than named, and the metric was certified from a document nobody had read in full. The attribute
is on that domain shape now, which closes the same hole on the markdown catalog and question
paths that decode the same type, and
`a_key_inside_an_anchor_range_is_refused_through_the_load_path` is what holds it here.

The three free strings - `model`, `time_column`, and each nested `column` - are the one thing
this shape cannot close, and they are parsed as domain identifier types during the conversion
(through the same `try_from` every catalog identifier uses) rather than at decode, which is what
turns an unparseable name into a typed `DataHubError::Identifier` naming the metric.

The original issue #202 scope carried the measure, the time column and the grains, and this
container of the rest of a metric is the closure of that scope: definitional filters, dimensions
with their allowlists, an anchor and prose all arrive the same way a markdown document carries
them, because `docs/adr/0011` closes the route by which any OTHER source could add them to a
metric this adapter defines. The one absence that stays is `cardinality`, which `DataHub`
carries but this adapter refuses to represent (see the crate header).

#### Methods

```rust
pub const fn anchor(&self) -> Option<&SuturaAnchor>
```

The certified number, when the deployment declared one.

```rust
pub fn description(&self) -> &str
```

```rust
pub fn dimensions(&self) -> &[SuturaDimension]
```

```rust
pub const fn full(model: String, measure: Measure, time_column: String, grains: Vec<Grain>, description: String, required_filters: Vec<RequiredFilter>, dimensions: Vec<SuturaDimension>, anchor: Option<SuturaAnchor>) -> Self
```

The certified content of a metric in full, optional shapes included.

```rust
pub fn grains(&self) -> &[Grain]
```

```rust
pub const fn measure(&self) -> &Measure
```

```rust
pub fn model(&self) -> &str
```

```rust
pub const fn new(model: String, measure: Measure, time_column: String, grains: Vec<Grain>) -> Self
```

The certified content of a metric, with the optional shapes defaulted to absent.

```rust
pub fn required_filters(&self) -> &[RequiredFilter]
```

```rust
pub fn time_column(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `struct SuturaDimension`

```rust
pub struct SuturaDimension
```

One dimension, spelled exactly as a markdown metric's dimension is.

A list entry with its own `name`, so a deployment declaring one name twice survives to
`Definitions::assemble` to be refused rather than silently collapsing.

#### Methods

```rust
pub fn into_domain(self) -> Dimension
```

Into the domain type `Definitions::assemble` holds.

```rust
pub const fn new(name: DimensionName, column: ColumnName, via: Option<RelationshipName>, allowed_values: Option<std::collections::BTreeSet<DimensionValue>>, description: Description) -> Self
```

A dimension, in the order `Definitions::assemble` wants it.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `struct SuturaAnchor`

```rust
pub struct SuturaAnchor
```

The number a metric is expected to produce, spelled exactly as a markdown metric's anchor is.

#### Methods

```rust
pub fn into_domain(self) -> Anchor
```

Into the domain type `Definitions::assemble` holds.

```rust
pub const fn new(range: TimeRange, value: AnchorValue) -> Self
```

An anchor.

```rust
pub const fn range(&self) -> TimeRange
```

```rust
pub fn value(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

## Module `fixture`

The recorded fixture corpus and the fake reader that serves it.

This is the only `AspectReader` implementor today, and it is the **fake** the port is tested
against - recorded aspect documents, not mocked HTTP. Until a real reader exists this is what a
`crate::DataHubCatalog` reads. The corpus is a bundle of models, one relationship and **one
certified metric** - the metric is `revenue`, and it carries the deployment-defined structured
property that the adapter decodes into a domain `Metric`, which is the issue #202 claim:
`DataHub` provides metrics for a metric that carries the custom shape. The raw expression string
beside it stays the promotion-candidate half and is never converted.

**The corpus is not only recorded, it is CONFIRMED against the platform.**
`tests/provisioned.rs` writes this metric's scalar into a provisioned `DataHub`, reads the aspect
back, and asserts the decoded `crate::document::MetricAspect` equals the one recorded here - so
the fixture is faithful to the platform rather than only to itself.

**The metric content is recorded in the FLAT form, and that is the point of keeping it as text.**
`DataHub`'s `structuredProperty` is scalar-only, so a deployment defines metric content as one
string-valued property, under a name of its own; the corpus records the shape a reader hands over
once it has mapped that property - `"sutura": {
"string_value": "..." }` with the closed-vocabulary document as the scalar's text - and the read
path exercises `document::SuturaProperty::assemble`, the scalar-to-nested step issue #202 is
about, on every load. The documents are decoded through `serde_json` at read time, so the same
deserialization path a real reader would use is exercised, and `deny_unknown_fields` on the wire
shapes holds over these recorded documents.

### `struct FixtureReader`

```rust
pub struct FixtureReader
```

The fake `AspectReader` that serves the recorded corpus.

#### Implements

`AspectReader`, `Clone`, `Debug`

### `fn over_fixture_source`

```rust
pub fn over_fixture_source(name: sutura_domain::model::SourceName, version: sutura_domain::pinned::DefinitionVersion) -> crate::DataHubCatalog<FixtureReader>
```

A `crate::DataHubCatalog` over the recorded corpus.

The source mapping answers the one platform the corpus names - `bigquery` - with the deployment's
declared source, which is what lets a model on that platform be opened. This is the constructor
the conformance registry uses to register the adapter; it is `pub` because an integration suite is
a separate crate and cannot reach a `#[cfg(test)]` item.

## Module `http`

The real `crate::AspectReader`: three paged reads over `DataHub`'s versioned `OpenAPI` v3
entity surface, assembled into one `Snapshot`.

Behind the crate's default-off `http` feature - see `Cargo.toml`'s own comment on why - so a
build that does not ask for this reader links no outbound TLS stack.

# What is measured, and what is not - stated here because it decides how this module is written

**The `metric` entity's wire shape is confirmed against a live instance.**
`tests/provisioned.rs`'s *Revision, 2026-09-04* round-tripped the recorded fixture's own document
through a real `DataHub` 1.7.0 and compared what came back against the fixture byte-for-byte, so
`harvest_metric` maps exactly the envelope that suite measured: `entities[]`, each carrying
`metricInfo.value` and `structuredProperties.value.properties[]`.

**The `dataset` and `semanticModel` entities are NOT measured against a live instance.** Their
field lists come from `docs/adr/0016`'s "Field by field" table, which was read from the platform's
own `.pdl` schema sources rather than from a served response. This is stated once here and
repeated at each mapping function, because the two facts have different consequences: a wrong
guess about `metric`'s envelope would be a regression against a proven round trip, and a wrong
guess about the other two would be the FIRST claim this crate has made about them. So both
mapping functions refuse an unexpected shape as a typed `HttpReaderError::UnexpectedShape`
naming the entity and the field, rather than reading past a missing or mistyped key with a
default - a guess that happened to be wrong would otherwise certify a bundle silently missing a
model or a relationship. **Do not cite this reader as proof the structural half works against a
real `DataHub` until an acceptance leg like `tests/provisioned.rs`'s measures it.**

# What every read is bounded by

`ReadBounds` carries a request timeout and a response-size cap, both **settings with defaults,
not constants** - `DEFAULT_TIMEOUT_SECONDS` and `DEFAULT_MAX_RESPONSE_BYTES` are the values a
composition root's settings default to, following `sutura-config`'s own convention of a default
function per optional key, not a value baked into this type. `read` makes
up to three requests and shares ONE deadline across them - opened once, and what is left after
the first two requests is what the third gets - the same shape `sutura-exec-bigquery`'s
`CallDeadline` holds for a job's token exchange and its query, and for the same reason: a budget
opened per request lets three independent timeouts sum to three times what a deployment declared.

# Auth

A personal access token as a `Secret`, sent as `Authorization: Bearer <token>` on every
request. The token is a constructor argument here; a composition root reads it from a settings-
declared file at boot (`token_file`, the naming convention `credential_file`/`password_file`
already hold), never inline in a settings document.

# Paging

One page per entity type, at a generous count. A page that SIGNALS more results exist - a
`scrollId`, or a returned count below a reported `total` - is refused
(`HttpReaderError::MorePages`) rather than silently read as complete: the same "one page or a
refusal" shape `sutura-exec-bigquery`'s wire holds for `jobs.query`, because a caller must not
certify a bundle built from a `Snapshot` that silently dropped a model, a relationship or a
metric. **Unmeasured: whether a real v3 last page ever carries a `scrollId` of its own.** If it
does, every read of a real instance is a refusal, and the follow-up acceptance leg (shaped like
`tests/provisioned.rs`) has to measure this before PR2 wires the composition - the `scrollId` arm
is a defensible guess against the platform's own "there is more" convention, not something this
crate has watched a real GMS answer.

# TLS and the endpoint

`Endpoint::parse` is the ONLY way to obtain an `Endpoint`, and `HttpAspectReader::new` takes
one rather than a `String` - a caller cannot dial an endpoint this module has not validated. What
`Endpoint::parse` accepts, exactly: a `Uri` (`ureq`'s own re-export of the `http` crate's
parser, `ureq::http::Uri` - the SAME type `ureq` itself parses a request URL into before
dialling) naming `http` or `https` as its scheme, an authority with no `user[:pass]@` prefix, and
nothing past the bare root - no path, no query, no fragment. `https://` is accepted for any host;
`http://` is accepted ONLY when the host is an IP loopback LITERAL -
`sutura_domain::source::host_is_loopback`, the ONE predicate `sutura_config::sources::transport`
also calls for Postgres's `transport_mode: plaintext` (issue 124's fail-closed rule,
`github.com/telekom/sutura#653`): a hostname is not an address, so `localhost` does not count
either, and only something that parses as `IpAddr` and answers `is_loopback()` does. Both crates
call the same function - not a copy each holds - so a divergence between the two is a compile
error, not something a review has to notice.

**This section has been wrong twice, and both corrections are worth keeping visible rather than
silently fixed - the second because the first one's OWN reasoning had a gap in it.**

The first draft removed `https_only(true)` (`BigQuery`'s own pin) entirely, arguing from this
record's own measurement tier reaching its `DataHub` over loopback plaintext "by construction" -
true, but an argument for LOOPBACK plaintext, not for plaintext to any host a deployment might
type. With no parse at all, a bearer was dialled in clear text to
`http://datahub.example.internal` exactly as readily as to `http://127.0.0.1`, refused only by a
connection timeout. The first fix was `Endpoint`, parsed by splitting the string by hand.

**The hand-split parse was itself the second gap, and a second review measured it.** For
`http://[::1]:1@localhost:<port>`, the hand-rolled host extraction took the text before the
LAST `:` in the authority - `::1` for the bracketed case - so the endpoint parsed as loopback
while the REAL host, `localhost` (everything after the userinfo's `@`), is exactly the name
`Endpoint::parse` is supposed to refuse in plaintext. A reader built from that string dialled
`localhost` with the bearer prepared. Parsing with `Uri` - the SAME parser `ureq` itself uses -
closes this the way it should have been closed the first time: `Authority::host` already
resolves past userinfo correctly, and `Endpoint::parse` additionally refuses any `user[:pass]@`
prefix outright rather than trusting that resolution to stay correct.

`ureq`'s compiled-in default root set (for an `https://` endpoint), `max_redirects(0)` and the
proxy left on (`Proxy::try_from_env()`) are the other three pins
`sutura_exec_bigquery::wire::WireAgent::pinned` states for `BigQuery`. **Follow-up, not built
here:** issue #125 PR2's `security.outbound.transport_anchors` is the future seam for a
deployment's own CA, for the endpoints that do use TLS.

### `enum InvalidReadBounds`

```rust
pub enum InvalidReadBounds
```

Why a declared bound is not usable.

#### Variants

- `Zero` - Zero would refuse every read rather than bounding one.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct ReadBounds`

```rust
pub struct ReadBounds
```

What one `HttpAspectReader::read` call may spend: a request timeout and a response-size cap.

A newtype rather than two loose arguments, so a reader cannot be built with an unchecked pair -
`sutura_exec_bigquery::wire::JobBounds`'s own shape, minus the money bound this read has no use
for (a metadata read is not billed).

#### Methods

```rust
pub const fn max_response_bytes(&self) -> u64
```

```rust
pub const fn parse(timeout_seconds: u64, max_response_bytes: u64) -> Result<Self, InvalidReadBounds>
```

Parses a declared timeout and cap, refusing either at zero.

```rust
pub const fn timeout(&self) -> Duration
```

#### Implements

`Clone`, `Copy`, `Debug`

### `struct EndpointMessage`

```rust
pub struct EndpointMessage
```

`DataHub`'s own message on a refusal.

Redacted the way `sutura_exec_bigquery::wire::EndpointMessage` is: bounded, filtered, and
reachable only through `Self::as_str` - never through `Debug`, which is the rendering a
cause-chain walk uses.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

The message itself, for a caller that has decided it may render it.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum HttpReaderError`

```rust
pub enum HttpReaderError
```

Why one of the three entity reads did not produce the aspects it names.

Reaches `crate::DataHubCatalog` boxed inside `DataHubError::Read` - the port's own coarse
variant - so this stays inspectable by a caller that knows to downcast, the `ErasedCause` shape
`.agents/skills/sutura/secure-by-design/SKILL.md` argues for at a boundary.

#### Variants

- `DeadlineSpent` - The shared budget was gone before this entity's page could be requested.
- `Unreachable` - The entity's page was not reached.
- `Unreadable` - The entity's page was reached and its answer could not be read.
- `Refused` - `DataHub` refused the request. `Display` renders the status and never `detail` - the same rule `sutura_exec_bigquery::wire::WireError::Refused` holds, and for the same reason: a cause-chain walk that flattens every link with `Display` must not carry endpoint-owned text.
- `TooLarge` - The page was larger than the cap this reader will read.
- `NotADocument` - The page was not a JSON document.
- `UnexpectedShape` - One entity's aspect did not carry a field this reader expects, or carried it in a shape it does not recognise.

  **Refused rather than guessed** - see the module header on which of the three entity shapes
  this applies to. `field` is a dotted path (`"schemaMetadata.value.fields[].fieldPath"`) so a
  refusal names exactly where the document stopped matching this reader's expectation.
- `NotTheCanonicalShape` - The page's own field mapped into this crate's canonical aspect shape and that decode failed - a defect in this reader's mapping rather than in the page, since every field reaching `serde_json::from_value` here was already read out of the page by name above.
- `MorePages` - The page stated or implied more results exist than the one page this reader will read.

#### Implements

`Debug`, `Display`, `Error`

### `enum InvalidEndpoint`

```rust
pub enum InvalidEndpoint
```

Why a declared endpoint is not usable.

#### Variants

- `NotAnHttpUrl` - Not a parseable URL, or a parseable URL naming neither `http` nor `https`, or one naming no authority at all.
- `CredentialsInUrl` - The authority carries `user[:pass]@` - refused outright. **This is not merely defence in depth against a spoofed host**: the round-2 review measured a reader built from `http://[::1]:1@localhost:<port>` dialling `localhost` in clear text with the bearer prepared, because a hand-rolled host extraction split on the wrong delimiter. Parsing with `Uri` closes that specific bypass on its own - `Authority::host` resolves to the text AFTER the last `@`, which is `localhost` here, so the loopback check below already sees the real target - but a declared endpoint has no legitimate use for embedded credentials, so this refuses the shape by name rather than relying on that resolution being correct forever.
- `PathBeyondRoot` - A path, a query or a fragment beyond the bare root. **Not supported in this revision, and that is a stated limit rather than an oversight:** a GMS behind a reverse proxy with a path prefix is a real deployment shape this crate has not measured a use case for, so the grammar stays exactly `scheme://host[:port]` until one is.
- `PlaintextBeyondLoopback` - `http://` to a host that is not an IP loopback literal - see `sutura_domain::source::host_is_loopback`.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct Endpoint`

```rust
pub struct Endpoint
```

A validated `DataHub` endpoint: `scheme://host[:port]`, and NOTHING past the authority - no
path, query or fragment (see `InvalidEndpoint::PathBeyondRoot`).

`https://` is accepted for any host; `http://` is accepted only for an IP loopback literal
(`sutura_domain::source::host_is_loopback`); an authority carrying `user[:pass]@` is refused
outright. `Self::parse` is the only constructor - see the module header's "TLS and the
endpoint" section for the rule and why an earlier draft did not hold it.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: &str) -> Result<Self, InvalidEndpoint>
```

Parses and validates a declared endpoint against `Uri` - the SAME parser `ureq` itself
uses to dial, re-exported as `ureq::http::Uri` - rather than a hand-rolled split, which is
what let the round-2 review's userinfo form (`http://[::1]:1@localhost`) reach
`host_is_loopback` with the wrong string. Refuses a scheme other than `http`/`https`, an
authority carrying `user[:pass]@`, anything past the bare root, and `http://` to a
non-loopback host. The stored form is rebuilt from the parsed `scheme` and `authority`
rather than kept as the trimmed input, so a trailing slash (or any other root spelling)
normalises to the same string.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct HttpAspectReader`

```rust
pub struct HttpAspectReader
```

A `DataHub` GMS, reached over HTTP.

Not generic over its credential the way `sutura-exec-bigquery`'s transport is: there is exactly
one credential shape here, a personal access token, so a type parameter would buy nothing a
second constructor would not.

#### Methods

```rust
pub fn new(endpoint: Endpoint, property: String, token: Secret, bounds: ReadBounds) -> Self
```

Opens a reader. `endpoint` is already validated - a caller reaches one only through
`Endpoint::parse`, so a reader cannot be built pointed at a plaintext non-loopback host.
`property` is the deployment's; `token` is read from a settings-declared file at boot by the
composition root, never inline; `bounds` is `ReadBounds::parse`'s output, so a reader
cannot be built with an unchecked pair either.

#### Implements

`AspectReader`, `Clone`, `Debug`

### `constant DEFAULT_TIMEOUT_SECONDS`

The recommended default request timeout, in seconds, for a composition root's settings default.

Matches `server.request_timeout_seconds`'s own shipped default: a metadata read that outlives the
request timeout in front of it cannot answer inside the budget the caller was promised anyway.
**Not read by anything in this module** - a caller passes the number it resolved, through
`ReadBounds::parse`, the same single-owner shape `BytesBilledCeiling::parse` holds for
`BigQuery`'s ceiling: this crate owns the range, a settings tree owns that the key was written.

### `constant DEFAULT_MAX_RESPONSE_BYTES`

The recommended default response-size cap, in bytes, for a composition root's settings default.

One quarter of `sutura_exec_bigquery::wire::MAX_ANSWER_BYTES`: a metadata page is descriptions,
column names and one metric document, not query rows, and what this defends against is the same
case that constant does - something that is not the endpoint answering - rather than a
realistic upper bound on a legitimate page.
