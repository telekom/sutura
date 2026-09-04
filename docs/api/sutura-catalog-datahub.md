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
**declaring** adapter: it says which kinds it provides and which it does not, and it is measured
against that declaration rather than against the golden adapters' oracle.

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
HTTP. What it does not contain is an HTTP client: `AspectReader` is the seam a real reader over
`DataHub`'s versioned `OpenAPI` v3 entity surface will implement (with a personal access token as a
bearer). Until that lands, the only implementor of the port is the recorded fixture source in
`fixture`, so no code here shapes a request or maps a response. **And nothing serves it:** no
composition root links this crate (its only dependant is `sutura-app`, as a dev-dependency), and
`sutura-serve` refuses `catalog.kind: datahub` by name. Everything here is decided and tested;
what is not is the reader itself and a served composition - the *Built and not wired* register in
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
`document::Snapshot`, and maps its own failures into `DataHubError::Read`. The only
implementor today is the recorded source in `fixture`. The RESPONSE SHAPE that implementor has
to map, and the surface's consistency, are measured rather than guessed - see the crate header.

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
- `UnknownPlatform` - A model named a platform this deployment declared no `sources.<alias>` for.
- `CardinalityUnrepresentable` - A relationship carries no cardinality, or one this adapter cannot represent.
- `Identifier` - A name on a snapshot did not parse as the identifier kind it claims to be.
- `Sutura` - The `sutura` structured property's scalar value is not the metric content it claims to be.
- `Description` - Prose on a snapshot is not a usable description.
- `Inconsistent` - The models, relationships and columns did not hold together.
- `Knowledge` - The bundle's knowledge does not hold together.
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
as an Elasticsearch keyword, so it is an index setting a deployment raises. This crate adds no
bound of its own.

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
pub const fn new(range: TimeRange, value: String) -> Self
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
