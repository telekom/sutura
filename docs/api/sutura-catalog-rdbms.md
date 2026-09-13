<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-rdbms

The public API of `sutura-catalog-rdbms`, rendered from rustdoc JSON.

A `SemanticCatalog` over an RDBMS dictionary - the narrowest declaration, and the one that
needs no service.

`docs/adr/0011-pluggable-by-declaration.md` defines the role this conversion implements. A
database dictionary is mainly DDL and comments: the tables, the columns, their constraints, and
table prose. It is not a
semantic layer and does not pretend to be one - which is the whole point of a **declaring**
adapter. It says which kinds it provides and which it does not, and it is measured against that
declaration rather than against the golden adapters' oracle.

This crate implements the dictionary conversion from `github.com/telekom/sutura#151`. A live
reader remains outside it; the runtime prompt derives the zero-metric physical-schema guidance
from the pinned bundle rather than coupling the application to this adapter.

# What a real dictionary yields

ADR 0016's method, applied here: read a real dictionary before writing the adapter. A throwaway
reader, measured once against a two-table Postgres 18 schema (two tables, a primary key each, one
foreign key, and table comments), reported the following:

| What | Count |
| --- | --- |
| tables | 2 |
| columns | 6 |
| tables carrying a comment | 2 |
| foreign keys | 1 |
| primary/unique constraints | 2 |

Three findings, and two of them are the declaration's content:

1. **A dictionary yields structure and prose, and nothing else.** Tables and columns are the
   `Structure` half; table comments are
   the `Descriptions`
   half. A dictionary carries **no measure, no grain, no definitional filter, no value allowlist
   and no anchor** - those are declared by a human in a semantic layer, which is what ADR 0011's
   table says ("certified metrics, measures, grains, allowed values: **no** - a human declares
   those elsewhere").
2. **A foreign key carries no metric cardinality.** It names the source and target columns, so
   this adapter declares the `Cardinality` *capability* absent: a dictionary carries no metric to
   reach a dimension `via` a relationship.
3. **A single-column primary or unique key is evidence, and only the safe direction.** ADR
   0011's "part worth having this connector for" is the one-direction uniqueness argument: a
   reader must supply a `SingleColumnTargetUniqueness` before the foreign key maps to
   `JoinType::ManyToOne`. Membership in a composite constraint is not evidence that one column
   is unique. Without the single-column evidence, loading refuses rather than asserting the
   relationship. The variant records what the reader found; this converter does not re-derive
   it from the constraint itself.

# The declaration, and what it means for the bundle

`SemanticCatalog::capabilities` provides `Structure` and may provide `Descriptions` and
`Relationships`. A sparse dictionary - structure with no comments or foreign keys - is therefore
faithful without making structure optional. What is declared is nothing more: no `Cardinality`
(a foreign key vouches for no metric fan-out), no `Metrics`, no
`Grains`, no `RequiredFilters`, no `AllowedValues`, no `Anchors`, and an empty knowledge half.
A bundle from this source therefore **loads, pins and validates with zero metrics**, and answers
no certified question - which is issue #115's shape and the whole reason the declaration exists:
a deployment whose whole model is a physical schema must not be told it has metrics it does not.

# What is built here, and what is NOT

This crate contains the conversion `RdbmsCatalog` applies to dictionary records, and it is
tested against a fake reader that serves a recorded dictionary - the port gets a fake,
not mocked SQL (`github.com/telekom/sutura#151`'s thing 4). What it does not contain is a
database client in the library closure: `DictionaryReader` is the seam a real reader over a
Postgres socket will implement, and the only implementor today is the recorded fixture source in
`fixture`. A production reader is outside this crate's current scope.

**And nothing serves it:** no composition root links this crate (its only dependant is
`sutura-app`, as a dev-dependency), so this is a registered, declaring catalog rather than a
served one - exactly the state `sutura-catalog-datahub` holds, which is the precedent copied.

## `trait DictionaryReader`

```rust
pub trait DictionaryReader
```

Where a dictionary's records come from.

**The fake seam.** Everything above this trait is decided and tested against a recorded
dictionary served by `fixture::FixtureReader`; a real implementor speaks to a Postgres socket,
reads `information_schema` / `pg_catalog`, decodes into `Dictionary`, and maps its own failures
into `RdbmsError::Read`. A port rather than a method on `RdbmsCatalog` for the same reason
the warehouse port exists: a catalog that could be swapped for a live source without the
conversion changing is the point.

A `Relationship` carries exactly one origin column and one target column, so a composite
(multi-column) foreign key is not representable in it. A real implementor must therefore either
refuse the whole read or omit that one foreign key when it encounters one, and must document
which of the two it does - the conversion below never sees a foreign key that a reader omitted,
so it cannot enforce or even detect either choice.

## `enum RdbmsError`

```rust
pub enum RdbmsError
```

Why a dictionary could not be read as a catalog.

Every variant is a typed contract rather than a message; the message is for a human and the
variant is what a caller can branch on. The mapping variants carry the table or column they were
refused on, because a dictionary is many rows and "invalid identifier" with no table name sends
a reader back to all of them.

### Variants

- `Read` - The reader could not fetch the dictionary.
- `NoVisibleTables` - The reader returned no table, so this bundle cannot honour its required `Structure` claim.
- `ModelName` - A table's semantic name did not parse as a model name.
- `CatalogName` - A table's catalog did not parse as the top part of a qualified table name.
- `SchemaName` - A table's schema did not parse as the middle part of a qualified table name.
- `TableName` - A physical table's own name did not parse.
- `DuplicateTable` - Two semantic models named the same physical table.
- `UnknownRelationshipTable` - A foreign key named a physical table absent from the dictionary.
- `ColumnName` - A column's name did not parse.
- `RelationshipName` - A foreign key's name did not parse.
- `Description` - A table description did not pass the authored-prose rule.
- `TargetUniquenessUnknown` - The referenced column had no single-column primary or unique-key evidence.
- `Inconsistent` - The assembled definitions did not hold together.
- `Digest` - Pinning failed.

### Implements

`Debug`, `Display`, `Error`

## `struct RdbmsCatalog`

```rust
pub struct RdbmsCatalog<R>
```

A catalog read from an RDBMS dictionary.

Generic in its reader rather than holding a boxed one, matching the warehouse adapters: there is
one per composition and a generic keeps the chosen reader visible. It carries the declared name
and version it is recorded under, the same way the other catalog adapters carry their own.

### Methods

```rust
pub const fn new(name: SourceName, version: DefinitionVersion, reader: R) -> Self
```

Opens a catalog over a dictionary reader.

### Implements

`Clone`, `Debug`, `SemanticCatalog`

## `struct TableAddress`

```rust
pub struct TableAddress
```

A table's physical address as a dictionary reports it.

A schema is required because it is the first part that distinguishes same-named tables in one
database. The catalog is optional because not every target renders a three-part table path.

### Methods

```rust
pub fn catalog(&self) -> Option<&str>
```

The catalog above the schema, when the dictionary reports one for generated statements.

```rust
pub const fn in_schema(schema: String, table: String) -> Self
```

A table in a schema of the connected catalog.

```rust
pub const fn new(catalog: Option<String>, schema: String, table: String) -> Self
```

A physical table address, split into the dictionary fields that own its identity.

```rust
pub fn schema(&self) -> &str
```

The schema immediately above the table.

```rust
pub fn table(&self) -> &str
```

The table's own name.

### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

## `struct Table`

```rust
pub struct Table
```

One semantic model, the physical table it selects, and the prose written against it.

### Methods

```rust
pub const fn address(&self) -> &TableAddress
```

The physical table address, as the dictionary spells each part.

```rust
pub fn columns(&self) -> &[String]
```

The columns the table exposes.

```rust
pub fn description(&self) -> Option<&str>
```

The table's comment, if a human wrote one.

```rust
pub fn model(&self) -> &str
```

The semantic model name assigned to this table.

```rust
pub const fn new(model: String, address: TableAddress, columns: Vec<String>, description: Option<String>) -> Self
```

A semantic model over a physical table.

### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## `struct Dictionary`

```rust
pub struct Dictionary
```

A dictionary: the tables, and the relationships a foreign key between them records.

### Methods

```rust
pub const fn new(tables: Vec<Table>, relationships: Vec<Relationship>) -> Self
```

A dictionary assembled from what a reader fetched.

```rust
pub fn relationships(&self) -> &[Relationship]
```

The relationships a foreign key records.

```rust
pub fn tables(&self) -> &[Table]
```

The tables the dictionary names.

### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## `enum SingleColumnTargetUniqueness`

```rust
pub enum SingleColumnTargetUniqueness
```

Why the target column of a foreign key is known to be individually unique.

This variant records what the reader found in the dictionary; it carries no constraint name and
no column list, so this converter checks only that a variant is present and cannot re-derive or
verify that the underlying constraint is truly single-column.

### Variants

- `PrimaryKey` - The target column is the sole column of a primary key.
- `UniqueConstraint` - The target column is the sole column of a unique constraint.

### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## `struct Relationship`

```rust
pub struct Relationship
```

A join a foreign key records: the two endpoints, their columns, and single-column target-key
evidence.

`SingleColumnTargetUniqueness` is evidence for the safe `ManyToOne` direction, not a metric
cardinality. Without it, loading refuses before a domain relationship is emitted.

### Methods

```rust
pub fn name(&self) -> Option<&str>
```

The foreign key's name, if the dictionary named it.

```rust
pub const fn new(name: Option<String>, origin_table: TableAddress, origin_column: String, target_table: TableAddress, target_column: String, target_uniqueness: Option<SingleColumnTargetUniqueness>) -> Self
```

A relationship's endpoints, with target-key evidence if the reader has any.

Loading refuses this value when `target_uniqueness` is `None`.

```rust
pub fn origin_column(&self) -> &str
```

The column the foreign key starts from.

```rust
pub const fn origin_table(&self) -> &TableAddress
```

The table the foreign key starts from.

```rust
pub fn target_column(&self) -> &str
```

The column the foreign key points to.

```rust
pub const fn target_table(&self) -> &TableAddress
```

The table the foreign key points to.

### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## Module `fixture`

The recorded dictionary corpus and the fake reader that serves it.

This is the only `crate::DictionaryReader` implementor today, and it is the **fake** the port is
tested against - a recorded dictionary, not mocked SQL (`github.com/telekom/sutura#151`'s thing 4).
Until a real reader exists this is what a `crate::RdbmsCatalog` reads.

The corpus mirrors exactly what a throwaway reader measured once against a two-table Postgres
18 schema: two tables (one fact, one lookup), a column set per table, table comments, and one
foreign key from the fact table to the lookup. There are **no metrics** - that is the whole
point of the narrowest metadata source, and what makes
`a_bundle_from_a_dictionary_loads_validates_and_answers_no_certified_question` pass.

### `struct FixtureReader`

```rust
pub struct FixtureReader
```

The fake `DictionaryReader` that serves the recorded corpus.

#### Implements

`Clone`, `Debug`, `DictionaryReader`

### `fn corpus`

```rust
pub fn corpus() -> crate::Dictionary
```

The recorded dictionary: the two tables the spike named, with their comments, and the one
foreign key between them.

### `fn over_fixture_source`

```rust
pub const fn over_fixture_source(name: sutura_domain::model::SourceName, version: sutura_domain::pinned::DefinitionVersion) -> crate::RdbmsCatalog<FixtureReader>
```

A `crate::RdbmsCatalog` over the recorded corpus.

The constructor the conformance registry uses to register the adapter; it is `pub` because an
integration suite is a separate crate and cannot reach a `#[cfg(test)]` item.
