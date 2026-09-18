<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-openmetadata

The public API of `sutura-catalog-openmetadata`, rendered from rustdoc JSON.

A `SemanticCatalog` over an `OpenMetadata` deployment: the richest of the three measured metadata
sources, and still a **declaring** one.

`docs/what-openmetadata-can-carry.md` is the finding that decides this adapter's whole shape.
`OpenMetadata` carries a typed metric entity and declared relationship cardinality — both richer
than `DataHub` or `Frictionless Table Schema` — but keeps **two half-a-definition slots**: a measure's
aggregation-to-column binding lives in `measures[].expression` / `metricExpression.code` as free
text in a dialect set (`SQL`/`Java`/`JavaScript`/`Python`/`External`) that does not intersect this
repository's typed `Term`, and the required filter is a raw SQL `where`. ADR 0016's refusal fires
on both: a measure carried as a raw expression string is half a definition, and taking it would
certify a foreign dialect's free text.

So this adapter is a `CatalogKind::Declaring` source that provides the physical model, the
descriptions, and the declared non-duplicating joins, and treats the kind it cannot faithfully
express exactly as the finding says: a metric whose measure is an expression string is
**reported and not defined** — its `metricType` is decidable but its bound column is not
resolvable from the foreign text, so it is decoded and set aside, never minted into a domain
`Measure`. `capabilities()` declares the deployment-dependent kinds (`Metrics`, `Grains` reached
through those metrics, and `Cardinality` observed only through a dimension reached via a
relationship) as **declared-and-empty may-provide kinds**, the 0011 state
`DefinitionCapabilities::of_may_provide` exists for.

# What is built here, and what is NOT

This crate contains everything `OpenMetadataCatalog` DECIDES about the documents a reader
extracts. It is tested against a fake reader that serves recorded documents — the port gets a fake,
not mocked HTTP. `SnapshotReader` is the seam a real reader over `OpenMetadata`'s `REST` API
(`/api/v1/tables`, `/api/v1/metrics`, …) implements, with a bearer credential; that HTTP reader is
deliberately NOT in this first PR, so the crate stays green (a service has no network in the nix
sandbox). The `http` reader + the live provisioned leg are the recorded follow-up.

# The declaration, and what it means for the bundle

`Structure`, `Descriptions` and `Relationships` are provided unconditionally: a `Table` with its
`columns[]` becomes a model, its `description` the model's description, and a `tableConstraint` /
`foreignKey` whose `relationshipType` is declared `ONE_TO_ONE` / `MANY_TO_ONE` / `ONE_TO_MANY` a
relationship. A relationship whose cardinality is absent **or** `MANY_TO_MANY` is refused naming
it — this is the pleasant surprise the finding records: `OpenMetadata` declares cardinality when it
is there and stays silent when it is not, so an undeclared relationship licenses nothing, and a
row-duplicating one is refused by this adapter rather than defaulted in either direction. A bundle
whose relationships are all unconstrained therefore carries none, lawfully.

`Metrics`, `Grains` and `Cardinality` are declared-and-empty may-provide kinds: whether a bundle
carries any is the deployment's decision (it defined a metric whose binding resolves, or it did
not), so absence is faithful rather than an aspirational claim. The metric entity IS decoded and
`metricType` + `granularity` + `dimensions[].type` are read, but `Measure` is minted only where a
column binding resolves to a domain `Term` without certifying a foreign dialect's free text — which
the recorded fixtures deliberately do not — so today those kinds arrive empty and the expression
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
source mapping — which service/database alias answers to which `SourceName` — and the declared
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
adapter's own canonical statement of the entity CONTENT a reader must extract and decode — the
decision side of `docs/what-openmetadata-can-carry.md`'s transport note — and `deny_unknown_fields`
holds over THIS shape and over the recorded fixtures a reader decodes, rather than over
`OpenMetadata`'s envelope. A real HTTP reader maps the service's document into one of these.

The fields track the entities `docs/what-openmetadata-can-carry.md` measured: a `Table` and its
`columns[]`, a `tableConstraint` / `foreignKey` with a `relationshipType` whose cardinality is
named when present and silent when not, and a `Metric` whose `metricType` is decidable but whose
bound column is not resolvable from its free-text expression. Everything an adapter DECIDES below
this shape is tested against a fake reader that serves recorded documents, which is the port's own
rule.

`RelationshipType` mirrors `OpenMetadata`'s cardinality enumeration: `ONE_TO_ONE`, `MANY_TO_ONE`,
`ONE_TO_MANY` are non-duplicating and license a `JoinType`, while `MANY_TO_MANY` — or a silent
absence — licences nothing and is refused by the conversion, because `JoinType` has no
many-to-many shape and a relationship nobody vouched for licenses no join. The fields are private
with accessors, per the workspace's `check-boundaries` rule that a library crate's types are its
contract; an untyped value (a `name`, a `column`, a `service`) is parsed during the conversion,
not in these carriers.

### `struct Snapshot`

```rust
pub struct Snapshot
```

Everything a reader fetched, before any of it is converted.

`deny_unknown_fields` here too — the three entity groups are the whole of what a reader must
extract, and a snapshot carrying a fourth is a reader this adapter has not been told to expect.

#### Methods

```rust
pub fn metrics(&self) -> &[Metric]
```

The metrics this snapshot carries.

Read so the reported-not-defined cell can prove a metric a snapshot carries never becomes a
domain `Metric`.

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

### `struct Table`

```rust
pub struct Table
```

What a `Table` entity supplies a model: a table, its columns, the service it lives on, and a
description.

#### Methods

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
but the bound column is not resolvable from `metricExpression` / `measures[].expression` — free
text in a foreign dialect — and a measure carried as such is reported and not defined. The metric
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
against — recorded documents, not mocked HTTP. The corpus is a bundle of two models and one
declared non-duplicating join, plus one metric whose measure is an expression string; the metric
is carried and never minted, because its bound column is not resolvable from a foreign-dialect
expression (the reported-not-defined half this crate's declaration promises).

The documents are decoded through `serde_json` at read time, so the same deserialization path a
real reader over OpenMetadata's REST API would use is exercised, and `deny_unknown_fields` on
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

The source mapping answers the one service the corpus names — `warehouse` — with the deployment's
declared source, which is what lets a model on that platform be opened. This is the constructor
the conformance registry uses to register the adapter.
