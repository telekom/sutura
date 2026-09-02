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
adapter will execute, no reliable cardinality, no definitional filter, no grain, no value
allowlist and no anchor. That is the shape of a **declaring** adapter: it says which kinds it
provides and which it does not, and it is measured against that declaration rather than against
the golden adapters' oracle.

# What is built here, and what is NOT

This crate contains everything `DataHubCatalog` DECIDES about the aspects it reads, and it is
tested against a fake reader that serves recorded documents - the port gets a fake, not mocked
HTTP. What it does not contain is an HTTP client: `AspectReader` is the seam a real reader over
`DataHub`'s versioned `OpenAPI` v3 entity surface will implement (with a personal access token as a
bearer), and the read path's cost - how many requests a whole bundle takes - is explicitly the
opening engineering question `docs/adr/0016` leaves open, to be measured against a provisioned
instance the way `sutura-exec-bigquery`'s acceptance leg was. Until that lands, the only
implementor of the port is the recorded fixture source in `fixture`.

# The declaration, and what it means for the bundle

`SemanticCatalog::KIND` is `CatalogKind::Declaring` and `SemanticCatalog::capabilities`
provides `Structure`, `Descriptions` and `Relationships`, and no knowledge capability. A bundle a
`DataHub`-only deployment loads therefore uses the physical model, the prose and the join columns,
and carries no certified metric layer - which the prompt states as a fact derived from the bundle,
exactly as `docs/adr/0011` decided. It loads and validates, because `Definitions::assemble` has no
minimum-metric refusal.

**The knowledge half is empty on purpose, and why is worth stating rather than glossed.** `DataHub`
does have glossary-like content, but every `Knowledge` referent names a metric, a dimension of one
or a declared value of one - and this adapter provides no metrics, so there is no referent for a
phrase or a caveat to attach to. `docs/adr/0016` decision 3 marks glossary and caveats *provides,
conditionally, where the bundle already declares a metric for a Referent to name*; that condition
is met only by the composition of a separately-authored metric layer, which `docs/adr/0011`'s
assembler (not built) is what would supply. So the honest standalone declaration is no knowledge
at all, and this crate says so rather than advertising a conditional it cannot satisfy alone.

# The two absences that are the point

`Cardinality` and `Metrics` are both *present* in `DataHub` and both declared unsupported here,
which is the interesting kind of absence `docs/adr/0016` calls out: reading them would be reading
something the source does not guarantee. A relationship whose cardinality is absent or many-to-many
is refused naming the relationship (not defaulted in either direction), because the `N_N` default
makes an unconsidered relationship indistinguishable from a considered one. A metric whose measure
is an expression string is read - `document::MetricAspect` is decoded - and never converted into
a `Measure`, so it does not enter the certified bundle.

## `trait AspectReader`

```rust
pub trait AspectReader
```

Where a snapshot's aspects come from.

**The fake seam.** Everything above this trait is decided and tested against recorded documents;
a real implementor speaks to `DataHub`'s versioned `OpenAPI` v3 entity surface, decodes into
`document::Snapshot`, and maps its own failures into `DataHubError::Read`. The only
implementor today is the recorded source in `fixture`, which is why the read path's cost is not
measured here - see the crate header.

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

`Cardinality` mirrors `DataHub`'s `ERModelRelationshipCardinality`, and `N_1`-to-`N_1` is refused by
the conversion rather than carried, because `JoinType` has no many-to-many shape - the declaration
says so (this adapter does NOT provide cardinality), and a relationship nobody vouched for
licenses nothing.

The fields are private with constructors and accessors, per the workspace's `check-boundaries`
rule that a library crate's types are its contract: a `pub` field lets a struct literal build a
value the constructor would have rejected. These carriers are decoded by `serde`, which writes
private fields, and built by a reader through the constructor.

### `struct Snapshot`

```rust
pub struct Snapshot
```

Everything a reader fetched, before any of it is converted.

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
names. Never converted into a `Measure`.

#### Methods

```rust
pub fn dialect(&self) -> &str
```

```rust
pub fn expression(&self) -> &str
```

```rust
pub fn name(&self) -> &str
```

```rust
pub const fn new(name: String, dialect: String, expression: String) -> Self
```

A metric aspect, whose measure is a raw expression string.

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

## Module `fixture`

The recorded fixture corpus and the fake reader that serves it.

This is the only `AspectReader` implementor today, and it is the **fake** the port is tested
against - recorded aspect documents, not mocked HTTP. `docs/adr/0016`'s transport note leaves the
read path's cost open until a provisioned instance exists; until then this is what a
`crate::DataHubCatalog` reads. The corpus is deliberately a **bundle of models and no metrics**:
that is the requirement `docs/adr/0016` checks - a DataHub-only deployment loads, pins and
validates with no certified metric layer - so the standalone declaration (`crate::DataHubCatalog`
provides `Structure`, `Descriptions`, `Relationships` and nothing else) is faithful to it.

The documents are embedded as text and decoded through `serde_json` at read time, so the same
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
