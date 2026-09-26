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
every one of the eleven `DefinitionKind`s is covered, either provided unconditionally or as a
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
this crate reads against. **A composition root now serves it, behind `sutura-cli`'s default-off
`datahub` feature:** that feature links this crate (with `http`) and opens `catalog.kind: datahub`,
reading the entry's token file once at boot and carrying its PAT as the bearer on every request. What
is still not is a live-`DataHub` read path in CI and the rest of the wire mapping - the fixed
`bigquery` platform alias and the last page unmeasured against a live instance, both stated in
`http`'s own module header. The *Built and not wired* register in
`.agents/skills/sutura/query-surface/SKILL.md` keeps those limits; it no longer records "no
composition root".

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
- `ColumnDescription` - A column's own description did not pass the authored-prose rule.
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

### `struct ColumnMetadata`

```rust
pub struct ColumnMetadata
```

What `schemaMetadata.fields[]` says about one column beyond its `fieldPath`: its
`nativeDataType` and its own `description`, both optional per field.

A canonical, flat shape rather than `DataHub`'s own tagged `SchemaFieldDataType` union - the
same simplification `DatasetAspect` already makes for the aspect as a whole, and the reason is
the same: a real `nativeDataType` is already the flat string a dictionary's own `data_type`
would be (`"VARCHAR(255)"`, `"BIGINT"`), so nothing here needs the union to read it.

Identical in shape to `openmetadata::document::ColumnMetadata` and `rdbms::ColumnMetadata`,
and kept separate for the reason the latter's own doc gives: each is one adapter's reading of
a wire shape none of the others should depend on.

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

### `struct DatasetAspect`

```rust
pub struct DatasetAspect
```

What a `dataset` entity supplies a model: a table, its columns, the platform it lives on, and a
description.

`column_metadata` and `primary_key` are both additive - see `Self::with_column_metadata` and
`Self::with_primary_key` - rather than `Self::new` parameters, so every existing fixture and
test in this crate keeps compiling. Both are `#[serde(default)]`, so a recorded document that
predates either still deserializes.

#### Methods

```rust
pub fn column_metadata(&self, column: &str) -> Option<&ColumnMetadata>
```

One column's type/description evidence, by `fieldPath`.

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
pub fn primary_key(&self) -> &[String]
```

Which fields `schemaMetadata` marks `isPartOfKey`.

```rust
pub fn table(&self) -> &str
```

```rust
pub fn with_column_metadata(self, metadata: impl IntoIterator<Item>) -> Self
```

Attaches per-column `nativeDataType`/`description` evidence, keyed by `fieldPath`.

```rust
pub fn with_primary_key(self, primary_key: Vec<String>) -> Self
```

Declares which fields `schemaMetadata` marks `isPartOfKey` - evidence only, the same as
`sutura_domain::catalog::Model::with_primary_key`, which is where this arrives once
converted.

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

Part of the promotion candidate's other half; read by the same `test_support` page builder
`expression` is, and nowhere on the read path.

```rust
pub fn expression(&self) -> &str
```

The promotion candidate's other half - the raw expression string in a dialect nothing here
renders.

One consumer today, outside the read path: `crate::test_support`'s `metric_page` builder
reads it (and `Self::dialect`) to make its fake serve the recorded fixture's OWN content,
so the wire page cannot drift from the corpus it certifies against. `docs/adr/0016` still
says the read path never converts it: the aspect is decoded and a metric without the
`sutura` property is set aside. These accessors are the readable shape a future reporter
would use.

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

**`via` is the same scalar-or-seq shape the markdown document spells, in and out.** The custom
`Serialize` writes a scalar when the chain has one hop and a sequence otherwise, and the input
side accepts both, so a stored chain reads the same as a stored hop.

**The byte-stability that buys holds in ONE direction.** A deployment that stored a scalar gets a
scalar back, which is the case that matters because it is what every document in the field already
holds. A deployment that stored a one-element SEQUENCE gets a scalar back instead - same meaning,
different bytes - because the writer is keyed on the chain's length rather than on what was read.
Round-tripping is therefore idempotent but not byte-preserving, and nothing here records the
original spelling to make it so.

#### Methods

```rust
pub fn into_domain(self) -> Dimension
```

Into the domain type `Definitions::assemble` holds.

The expect is on `ViaChain`'s own invariant, not on anything this adapter parsed: the
property carries only chains built through `ViaChain::of`, so a non-empty chain is what
arrives, and the domain type keeps its parse-only construction honest.

```rust
pub fn new(name: DimensionName, column: ColumnName, via: Option<ViaChain>, allowed_values: Option<std::collections::BTreeSet<DimensionValue>>, description: Description) -> Self
```

A dimension, in the order `Definitions::assemble` wants it.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`, `Serialize`

### `enum ViaDoc`

```rust
pub enum ViaDoc
```

The hops a dimension is reached through, as the property spells them.

One name, or a sequence of them. The same untagged shape the markdown adapter's `via:` takes, so
the two adapters agree on what a stored chain means - #266's one-content-one-meaning rule, at
this field.

#### Variants

- `One`
- `Chain`

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

**The `semanticModel` entity's relationship shape IS measured against a live instance, the
`dataset` entity's is NOT.** Real `DataHub` 1.7.0 carries a joined relationship NOT as a
top-level `semanticModelRelationship` aspect (which GMS drops on write) but nested inside the
`semanticModel` entity's own `semanticModelInfo.value.relationships[]` array - measured on this
repository's docker tier (2026-09-16) by upserting the recorded corpus's `orders_to_customer`
relationship under `semanticModelInfo` and reading it back. So
`HttpAspectReader::read_relationships` maps the aspect it fetches to the array
`harvest_relationship` walks, and the fake `happy_path_answers` page serves the same nested
shape - one content over two transports. The `dataset` entity's field
list is still from `docs/adr/0016`'s "Field by field" table, read from the platform's own `.pdl`
schema rather than from a served response. The two facts have different consequences: a wrong
guess about `metric`'s envelope would be a regression against a proven round trip, and a wrong
guess about `dataset` would be a FIRST claim this crate has made about it. Both mapping functions
refuse an unexpected shape as a typed `HttpReaderError::UnexpectedShape` naming the entity and
the field, rather than reading past a missing or mistyped key with a default - a guess that
happened to be wrong would otherwise certify a bundle silently missing a model or a relationship.
**Do not cite this reader as proof the `dataset` half works against a real `DataHub` until an
acceptance leg like `tests/provisioned.rs`'s measures it.**

# What every read is bounded by

`ReadBounds` carries a request timeout and a response-size cap, both **settings with defaults,
not constants** - `DEFAULT_TIMEOUT_SECONDS` and `DEFAULT_MAX_RESPONSE_BYTES` are the values a
composition root's settings default to, following `sutura-config`'s own convention of a default
function per optional key, not a value baked into this type. `read` makes
up to three requests and shares ONE deadline across them - opened once, and what is left after
the first two requests is what the third gets - the same shape `sutura_domain::warehouse::deadline::Deadline`
holds for a job's execution, and for the same reason: a budget opened per request lets three
independent timeouts sum to three times what a deployment declared.

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

`Endpoint` and its `Endpoint::parse` now live in `sutura-http-client`, shared with
`sutura-catalog-openmetadata`'s identical reader since issue #970's review found the two
byte-for-byte the same (`cargo xtask check-jscpd`). `HttpAspectReader::new` takes one rather
than a `String` - a caller cannot dial an endpoint this module has not validated. What
`Endpoint::parse` accepts, exactly:
`scheme://host[:port]`, scheme `http` or `https` (case-folded), on a `ureq::http::Uri` (`ureq`'s own re-export of the `http` crate's
parser, the SAME type `ureq` itself parses a request URL into before dialling), an OPTIONAL nonzero valid `:port`, an
OPTIONAL trailing `/`, and NOTHING else: a path, query or fragment is `InvalidEndpoint::PathBeyondRoot` (fragment
checked on the RAW text, because `http::Uri` silently discards a `#`), a bad port is `InvalidEndpoint::NotAnHttpUrl`,
and a `user[:pass]@` prefix is `InvalidEndpoint::CredentialsInUrl`. `https://` is accepted for any host; `http://` only
for an IP loopback LITERAL - `sutura_domain::source::host_is_loopback`, the ONE predicate
`sutura_config::sources::transport` also calls for Postgres's `transport_mode: plaintext` (issue 124's fail-closed rule,
`github.com/telekom/sutura#653`): a hostname is not an address, so `localhost` does not count either, and only something
that parses as `IpAddr` and answers `is_loopback()` does - the same function both crates call, so a divergence between
them is a compile error, not a review's job to notice.

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
`localhost` with the bearer prepared. Parsing with `ureq::http::Uri` - the SAME parser `ureq` itself uses -
closes this the way it should have been closed the first time: `Authority::host` already
resolves past userinfo correctly, and `Endpoint::parse` additionally refuses any `user[:pass]@`
prefix outright rather than trusting that resolution to stay correct.

`ureq`'s compiled-in default root set (for an `https://` endpoint), `max_redirects(0)` and the
proxy left on (`Proxy::try_from_env()`) are the other pins; a deployment MAY replace the
compiled-in roots with its own CA via `security.outbound.transport_anchors` (`#125`), folded in
`sutura_http_client::tls` - anchors only, no client identity.

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
- `Refused` - `DataHub` refused the request. `Display` renders the status and never `detail`, because a cause-chain walk that flattens every link with `Display` must not carry endpoint-owned text.
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
pub fn new(endpoint: Endpoint, property: String, token: Secret, bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self
```

Opens a reader. `endpoint` (only `Endpoint::parse`), `property`, `token` (read from a
settings-declared file at boot) and `bounds` (only `ReadBounds::parse`) are all checked first.

**`anchors` is `security.outbound.transport_anchors` (`#125`), resolved once at boot**: `None`
leaves `ureq`'s compiled-in `RootCerts::WebPki` (every deployment before `security.outbound`),
`Some` replaces it with `RootCerts::Specific` from exactly the declared certificates - never a
union of the two (see `sutura_http_client::tls`). This constructor never presents a client
identity - `Self::rotating_agent` is the one that does, over the same declaration
(`security.outbound.client_certificate`/`client_key`, `github.com/telekom/sutura#911`).

```rust
pub const fn rotating(endpoint: Endpoint, property: String, token: Secret, bounds: ReadBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self
```

The rotation-lane constructor: holds the rotating agent handle a composition root built (via
`Self::rotating_agent`) and drove to re-read on `sutura_tls::POLL_INTERVAL`. The reader is
per-request, so the agent `current()` resolves to on the next `read` is the latest that loaded -
a replaced bundle (`security.outbound.transport_anchors`, `github.com/telekom/sutura#125`) is
adopted by the next read, no drain (per `docs/adr/0010`).

```rust
pub fn rotating_agent(bounds: ReadBounds, declared: Option<sutura_tls::Declared>) -> Result<OutboundAgent, sutura_tls::LoadError>
```

Builds the reader's rotating agent handle for a declared `security.outbound` set, and (when
one is declared) the `sutura_tls::Rotator` the composition root drives on
`sutura_tls::POLL_INTERVAL`. `None` (no declaration) returns a fixed handle over `ureq`'s
compiled-in roots, presenting no identity, and no poll handle. Rebuilt over
`RootCerts::Specific` from each freshly loaded bundle and, when
`sutura_tls::Declared::identity` is declared, the freshly loaded identity too - never a
union, never a second external read.

# Errors

The declared bundle or client identity cannot be loaded at boot.

#### Implements

`AspectReader`, `Clone`, `Debug`

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

The happy-path `DataHub` wire pages this crate's own tests need.

Over the loopback fake `sutura-http-client::test_support` now hosts (issue #970's review: this
file's own `FakeServer`/`Scripted`/plumbing was byte-for-byte identical to
`sutura-catalog-openmetadata`'s copy, `cargo xtask check-jscpd` measured, once a second reader
crate carried one). Re-exported here so `sutura-cli`'s served-binary suite
(`crates/sutura-cli/tests/served/datahub.rs`) keeps building `test_support::FakeServer` off
THIS crate's public API - it takes `sutura-catalog-datahub` as a dependency, not
`sutura-http-client` directly.

`#[cfg(feature = "http")]`, not `#[cfg(test)]`, for the reason `sutura_http_client::test_support`'s
own header gives: an integration test binary cannot see another crate's `tests/` directory, so
the only way to share a fake across crates is through a library, `pub`, reachable at compile
time from whichever feature both a reader and its composition root's tests turn on.

### `fn dataset_page`

```rust
pub fn dataset_page() -> serde_json::Value
```

One `dataset` page, over the two models the certified fixture metric needs: `orders` (carrying
every column the metric's measure, time column and required filter name) and `customers`
(carrying the dimension's column). Model name and table are the same string, because
`HttpAspectReader::read_datasets`'s own doc names that as a real limit rather than hiding it.

### `fn relationship_page`

```rust
pub fn relationship_page() -> serde_json::Value
```

One `semanticModel` page, over the one relationship the certified fixture metric's dimension
reaches `customers` through. Served in the shape a REAL `DataHub` carries - the relationship
nested inside the `semanticModel` entity's own `semanticModelInfo.value.relationships[]`, not as
a top-level aspect (GMS drops that on write; measured against the docker tier, 2026-09-16) - so
the fake and the live tier serve one content over two transports, and
`HttpAspectReader::read_relationships` walks both identically.

### `fn metric_page`

```rust
pub fn metric_page() -> serde_json::Value
```

One `metric` page carrying the recorded fixture's OWN certified metric, read through the crate's
public port rather than restated here - the recorded corpus and this page cannot drift.

### `fn happy_path_answers`

```rust
pub fn happy_path_answers() -> Vec<Scripted>
```

The three pages a `read()` call makes, in order, all answering `200` - what a real `DataHub`
carrying exactly the recorded fixture's content would serve.

### `use CapturedAuthorizations`

### `use FakeServer`

### `use Scripted`

### `constant DEPLOYMENT_PROPERTY`

The structured property name the happy-path pages below register the certified metric's content
under - a fixed test constant, deliberately independent of the deployment's OWN choice, the same
way `tests/provisioned.rs`'s `DEPLOYMENT_PROPERTY` is: a fake registering the adapter's own field
name would pass equally whether the name were the deployment's choice or a constant this crate
requires.
