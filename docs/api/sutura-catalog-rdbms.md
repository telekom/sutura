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

This crate implements the dictionary conversion from `github.com/telekom/sutura#151`. The
runtime derives the zero-metric physical-schema guidance from the pinned bundle rather than
coupling the application to this adapter.

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

**Update, 2026-09-24 (#966).** The measurement above counted TABLE comments; a real dictionary's
`information_schema.columns` / `pg_catalog` COULD also supply a `data_type` per column, a
per-COLUMN comment through `col_description`, and its own primary/unique key - a real reader
could read all three the same way it reads the rest of the dictionary, and none of it existed
at this port before this issue. `Table` now CAN carry a type and a comment per column, as
`ColumnMetadata` attached with `Table::with_column_metadata`, and a key as
`Table::with_primary_key` - both additive rather than `Table::new` parameters, so every
existing reader and every test fixture here keeps compiling. Neither licenses anything a
cardinality or a measure would: a type is descriptive text
(`sutura_domain::catalog::ColumnType`) and a primary key is evidence
(`sutura_domain::catalog::Model::with_primary_key`).

The fixture carries a type, a comment and a key for two columns, so the converter's mapping and
byte accounting are exercised without a socket. The feature-gated live reader also reads these
fields from a declared Postgres documentation schema; its provisioned test is the socket venue.

# The declaration, and what it means for the bundle

`SemanticCatalog::capabilities` provides `Structure` and may provide `Descriptions`,
`Relationships`, `ColumnTypes` and `ColumnDescriptions`. A sparse dictionary - structure with no
comments, foreign keys, column types or column comments - is therefore faithful without making
structure optional. What is declared is nothing more: no `Cardinality`
(a foreign key vouches for no metric fan-out), no `Metrics`, no
`Grains`, no `RequiredFilters`, no `AllowedValues`, no `Anchors`, and an empty knowledge half.
A bundle from this source therefore **loads, pins and validates with zero metrics**, and answers
no certified question - which is issue #115's shape and the whole reason the declaration exists:
a deployment whose whole model is a physical schema must not be told it has metrics it does not.

# What is built here, and what is NOT

This crate contains the conversion `RdbmsCatalog` applies to dictionary records, and it is
tested against a fake reader that serves a recorded dictionary - the port gets a fake,
not mocked SQL (`github.com/telekom/sutura#151`'s thing 4). Since #972, it also contains the
live implementors over a Postgres (`postgres_reader`) and an Oracle (`oracle_reader`)
documentation schema, behind a default-off `live` feature so the library closure stays
domain + thiserror and no build links either driver's stack without asking for it.

**The fake dominates the suite; the live reader is the production half, feature-gated.**
`DictionaryReader` is the seam they implement (`fixture::FixtureReader` the recorded corpus,
`AnyDictionaryReader` a real connection), and the conversion is the same for each.
A composition root that links the `live` feature serves `catalog.kind: rdbms`; a build without
it refuses by name. (Its only other dependant is `sutura-app`, as a dev-dependency.)

## `enum AnyDictionaryReader`

```rust
pub enum AnyDictionaryReader
```

The live reader a declared `connection.dialect` selects, so one `RdbmsCatalog` type holds
either without a trait object.

### Variants

- `Postgres` - The Postgres documentation-schema reader, boxed because it is several times the Oracle one.
- `Oracle` - The Oracle documentation-schema reader.

### Methods

```rust
pub const fn bounds(&self) -> DictionaryBounds
```

### Implements

`Clone`, `Debug`, `DictionaryReader`

## `trait DictionaryReader`

```rust
pub trait DictionaryReader
```

Where a dictionary's records come from.

**The fake seam.** Everything above this trait is decided and tested against a recorded
dictionary served by `fixture::FixtureReader`. The feature-gated
`postgres_reader::PostgresReader` reads a declared documentation schema over a Postgres
socket and maps its failures into `RdbmsError::Read`. Both use the same conversion.

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
- `ColumnDescription` - A column comment did not pass the authored-prose rule.
- `TargetUniquenessUnknown` - The referenced column had no single-column primary or unique-key evidence.
- `Inconsistent` - The assembled definitions did not hold together.
- `Digest` - Pinning failed.
- `ExceedsBounds` - The dictionary carries more tables and relationships than the declared row cap.

  Checked on the decoded dictionary, whatever the reader did. The byte cap is not checked here:
  a `Dictionary` carries no byte count, so it is the reader's to hold against the response.

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
pub fn new(name: SourceName, version: DefinitionVersion, reader: R) -> Self
```

Opens a catalog over a dictionary reader.

```rust
pub const fn with_bounds(self, bounds: DictionaryBounds) -> Self
```

Holds the dictionary read to `bounds`; without it the row count is unchecked here.

The composition root that links the `live` reader passes the declared
`max_dictionary_rows`/`max_dictionary_bytes` here (see `open_one_rdbms_catalog`); a reader
over a live socket also enforces them inline, but this stays the conversion's own post-decode
guard over a `Dictionary` whatever the reader did.

**The reading reader holds the caps inline too** (`postgres_reader` abandons a stream that
crosses the ceiling); this guard is the second, non-network half that a recorded or fetched
dictionary gets regardless of the transport.

```rust
pub fn with_source_alias(self, source_alias: SourceName) -> Self
```

Binds models from this dictionary to a separately declared data source.

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

## `struct ColumnMetadata`

```rust
pub struct ColumnMetadata
```

What a dictionary's own `information_schema`/catalog view says about one column beyond its
name: its declared data type, and a column comment if a human wrote one.

A separate type from the domain's `sutura_domain::catalog::Column` rather than that type
itself, because this crate's own identifiers are still bare dictionary strings at this point -
the same reason `Table`'s own fields are `String` rather than `sutura_domain::model::ColumnName`.
`RdbmsCatalog::convert_model` is where the parse happens for all of them together.

**`datahub::document` and `openmetadata::document` declare the identical two fields, and stay
separate on purpose.** Each is that adapter's own reading of a wire shape this crate has no
business depending on - one adapter importing another's type crosses the boundary
`sutura-catalog-*` crates are not supposed to cross, for a coincidence of shape between three
sources whose actual dictionaries have no reason to keep matching.

### Methods

```rust
pub fn data_type(&self) -> Option<&str>
```

```rust
pub fn description(&self) -> Option<&str>
```

```rust
pub const fn new(data_type: Option<String>, description: Option<String>) -> Self
```

### Implements

`Clone`, `Debug`, `Default`, `Eq`, `PartialEq`

## `struct Table`

```rust
pub struct Table
```

One semantic model, the physical table it selects, and the prose written against it.

`column_metadata` and `primary_key` are both additive - see `Self::with_column_metadata` and
`Self::with_primary_key` - rather than `Self::new` parameters, so a reader that has neither
(or a test fixture built before either existed) keeps compiling unchanged.

### Methods

```rust
pub const fn address(&self) -> &TableAddress
```

The physical table address, as the dictionary spells each part.

```rust
pub fn column_metadata(&self, column: &str) -> Option<&ColumnMetadata>
```

One column's type/comment evidence, by the dictionary's own spelling of its name.

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

```rust
pub fn primary_key(&self) -> &[String]
```

Which columns the dictionary's own constraint names as this table's primary key.

```rust
pub fn with_column_metadata(self, metadata: impl IntoIterator<Item>) -> Self
```

Attaches per-column type and comment evidence, keyed by the dictionary's own column spelling.

A column named here that is not in `Self::columns` is dropped rather than refused: the
conversion reads metadata only for a column it is already about to declare, and a stray key
says nothing this crate's error vocabulary is set up to report against a table.

```rust
pub fn with_primary_key(self, primary_key: Vec<String>) -> Self
```

Declares which of this table's columns the dictionary's own primary or unique-key constraint
names - evidence only, the same as `sutura_domain::catalog::Model::with_primary_key`, which
is where this arrives once converted.

**Uncorrelated with `SingleColumnTargetUniqueness`, and that is stated rather than
reconciled.** A `Relationship`'s target-uniqueness evidence licenses one specific foreign
key's `ManyToOne` direction; this evidence describes the table's OWN key, independent of
whether anything references it. Nothing here checks the two against each other - a column
this method names could be the primary key, a unique key that is not the primary one, or
(a reader's bug) neither, and this type cannot tell which from the name alone. A reader is
the only thing that could keep them consistent, because only a reader sees the dictionary's
own labelling of which constraint is which.

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

## `struct DictionaryBounds`

```rust
pub struct DictionaryBounds
```

What one dictionary read may spend: a row cap and a byte cap.

Each cap is non-zero BY TYPE - a zero cap would refuse every read rather than bound one - and
`sutura-config` refuses a written zero at load with the same type, so a declared bound arrives
here with nothing left to check. Which default an absent bound takes is the reader's to say.

### Methods

```rust
pub const fn max_bytes(&self) -> NonZeroU64
```

```rust
pub const fn max_rows(&self) -> NonZeroU64
```

```rust
pub const fn new(max_rows: NonZeroU64, max_bytes: NonZeroU64) -> Self
```

### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## Module `fixture`

The recorded dictionary corpus and the fake reader that serves it.

This is the recorded-corpus implementor of `crate::DictionaryReader`, and it is the **fake** the
port is tested against - a recorded dictionary, not mocked SQL (`github.com/telekom/sutura#151`'s
thing 4).
The feature-gated `crate::postgres_reader::PostgresReader` is the live implementor the
composition root serves; this fixture is what the test suite and the conformance registry read.

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
pub fn over_fixture_source(name: sutura_domain::model::SourceName, version: sutura_domain::pinned::DefinitionVersion) -> crate::RdbmsCatalog<FixtureReader>
```

A `crate::RdbmsCatalog` over the recorded corpus.

The constructor the conformance registry uses to register the adapter; it is `pub` because an
integration suite is a separate crate and cannot reach a `#[cfg(test)]` item.

## Module `oracle_reader`

The live Oracle documentation-schema reader, behind the default-off `live` feature.

`crate::postgres_reader`'s twin over an Oracle connection: the same documented `columns` view
(that module's header carries the column table), the same constructor checks, inline caps and
assembly - all held once in `crate::documentation` - so the conversion in `crate::RdbmsCatalog`
cannot tell the two apart. What differs is the SQL dialect and the driver.

# The contract's Oracle spelling

`is_primary_key` and `is_deleted` are `NUMBER(1)`, `1` for true and `0` for false; any other
value of `is_primary_key` is refused as missing evidence rather than read as either. The
schema, the view and the predicate column are validated identifiers, **folded to upper case and
then quoted** - Oracle folds an unquoted name to upper case when the object is created, so
`sutura_dictionary.columns` is found as written. A schema created under a quoted lower-case
name is not. The environment and an `equals` value are bound as `:1`/`:2`, never interpolated.

# The limits, next to the claims

- **No live Oracle run is observed or cited.** No venue that runs `just validate` reaches an
  Oracle server (`compose.services.yaml`'s `oracle` row says why). The live cells in
  `tests/oracle_provisioned.rs` exist and the `oracle-tier` CI job runs them; until a run of
  them is cited here, the unit cells prove the constructor refusals, the rendered statement, the flag
  decode, and a golden of the dictionary assembled from positional values handed to the
  decoder - never a read. The driver cannot build a row outside a session, so the cursor, the
  transaction and the driver's own type conversion stay unexercised.
- **Read-only by statement, not by driver flag.** The pinned driver has no read-only option;
  the reader issues `SET TRANSACTION READ ONLY` before its one `SELECT` and rolls back after it.
  Unobserved against a server, like every other line of the read path.
- **The byte cap is an estimate.** The driver exposes no row's wire size, so a row spends the
  UTF-8 length of its decoded text plus one byte - bounding the decoded payload, not the bytes
  on the wire. Neither cap limits elapsed read time, and neither bounds a fetch: the pinned
  driver prefetches 2 rows on execute and fetches 100 per round trip by default - read off its
  source, not observed against a server - so up to one batch is in memory before a cap refuses.
- **The connection is plaintext and confined only at its first dial.** `sutura-config`'s
  `OracleCatalogConnection` refuses TLS and a non-loopback host, because the driver takes no
  caller-built trust store; the driver still follows a listener's TNS redirect to any address
  it names - the limit `sutura-cli`'s `oracle` module holds for the source kind.
- **`SharedServiceUser`, not leg 2.** A catalog read has no caller to run as; it is one login
  under the user the deployment declared.

### `struct OracleLogin`

```rust
pub struct OracleLogin
```

Where and as whom the reader logs in: an EZCONNECT `host:port/service_name` dial.

#### Methods

```rust
pub const fn new(host: String, port: u16, service_name: String, user: String, password: Secret) -> Self
```

#### Implements

`Clone`, `Debug`

### `struct OracleReader`

```rust
pub struct OracleReader
```

A live `DictionaryReader` over an Oracle documentation schema.

#### Methods

```rust
pub const fn bounds(&self) -> DictionaryBounds
```

```rust
pub fn new(login: OracleLogin, documentation_schema: &str, environment: String, predicate: RowPredicate, row_cap: Option<NonZeroU64>, byte_cap: Option<NonZeroU64>) -> Result<Self, InvalidReaderConfig>
```

Builds the reader without dialling. An absent `row_cap`/`byte_cap` selects the documented
defaults `crate::postgres_reader::PostgresReader` uses.

# Errors

The documentation schema or the predicate column is not an identifier.

#### Implements

`Clone`, `Debug`, `DictionaryReader`

## Module `postgres_reader`

The live Postgres documentation-schema reader, behind the default-off `live` feature.

This is the reader the crate's module header has said, since #151, "a real implementor" would
be: one that speaks to a Postgres socket, reads a documentation schema, decodes into
`crate::Dictionary`, and maps its own failures into `crate::RdbmsError::Read`. It is the
companion to `crate::fixture::FixtureReader` - the fake is the recorded corpus this port was
tested against, and this is the live implementor over a real connection.

# The documentation-schema contract

A **documented fixed schema** - one column per documented dictionary row, so the reader never
guesses at structure, named `columns` inside the declared documentation schema:

| Column | Meaning |
| --- | --- |
| `environment` | The deployment environment key this row's descriptions apply to. |
| `catalog_name` | The physical catalog above the schema, when generated statements need one. |
| `schema_name` | The physical schema the described table lives in. |
| `table_name` | The physical table. |
| `model_name` | The semantic model name to bind the table to. |
| `table_description` | Authored table prose; `NULL` for none. |
| `column_name` | The physical column. |
| `column_ordinal` | The column's stable order within the table. |
| `column_type` | The physical data type, as `information_schema` reports it. |
| `column_description` | Authored column prose; `NULL` for none. |
| `is_primary_key` | A `boolean`: is this column the sole column of a primary or unique key. |
| `is_deleted` | A soft-delete marker: `false` selects live rows on every read. |

The reader selects `is_deleted = false` unconditionally and binds the declared `environment` and
the equals-predicate value as SQL parameters - **never interpolating configuration values into
the statement**. The schema and predicate column are checked at the reader's constructor,
then quoted as identifiers, so neither can be the vehicle for SQL. The predicate
operator comes from a closed set rendered as fixed text.

# Foreign keys are unsupported in this first slice - and the reader does not pretend otherwise

The documented schema carries no foreign-key columns, so this reader emits a
`crate::Dictionary` with **no `crate::Relationship`s**. That is a real, explicit limit,
stated here. It is not an invented uniqueness assertion: single-column primary-key evidence is
read per column (`is_primary_key`) and that alone is ever emitted; nothing in this reader
fabricates a foreign key or a target-uniqueness claim on the reader's behalf.

# Read-only, streamed, bounded

The read runs inside a single read-only transaction (`read_only`, `RepeatableRead`). Rows are
streamed with `query_raw` - the driver's extended-protocol portal, which does not materialise
the result set up front - and the row cap and byte cap are enforced **inline**, abandoning the
stream the moment the declared ceiling is crossed. This bounds the streamed row payload;
the converter's separate post-decode guard bounds the assembled dictionary. The driver still
materialises one row before its size is checked. Neither bound limits elapsed read time.

# Connection and transport policy

The reader uses the catalog's own declared connection and transport policy. Anchor/identity
material is resolved by a composition root into a `rustls::ClientConfig` for `verified`/`mutual`
channels, or `None` for `plaintext`. When a `ClientConfig` is supplied the reader forces
`SslMode::Require` so a server declining TLS cannot silently downgrade the verifier to
cleartext - the same hardening `sutura-exec-postgres::connect_secured` applies. There is no
unconditional `NoTls`: plaintext is reached only through the declared `plaintext` mode, which
the transport layer already refuses for a remote host.

# Feature gating

This module is `#[cfg(feature = "live")]`. A build without the feature links no
`tokio-postgres`/`tokio-postgres-rustls`/`rustls`/`ring` stack, and the composition root refuses
the catalog by name.

### `enum InvalidReaderConfig`

```rust
pub enum InvalidReaderConfig
```

#### Variants

- `Schema`
- `PredicateColumn`
- `DefaultBound`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum RowPredicate`

```rust
pub enum RowPredicate
```

A live-row predicate rendered into the dictionary query and bound as parameters.

`PostgresReader::new` validates the column identifier before it can reach SQL text. The
operator is from a fixed set rendered as fixed text; an `equals` value is bound as a parameter.

#### Variants

- `None` - No live-row filter beyond the soft-delete marker.
- `IsNull` - `column IS NULL`.
- `IsNotNull` - `column IS NOT NULL`.
- `Equals` - `column = $N`, value bound as a parameter.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct PostgresReader`

```rust
pub struct PostgresReader
```

A live `crate::DictionaryReader` over a Postgres documentation schema.

Owns the driver configuration and the optional TLS verifier, plus the environment key, the
optional live-row predicate and the read bounds. The TLS `ClientConfig` is supplied by a
composition root that resolved the declared `transport_mode`; `None` selects the `plaintext`
channel. The read-only transaction, parameter binding and inline caps are all this reader's own.

#### Methods

```rust
pub const fn bounds(&self) -> DictionaryBounds
```

```rust
pub fn new(config: tokio_postgres::Config, tls: Option<rustls::ClientConfig>, documentation_schema: String, environment: String, predicate: RowPredicate, row_cap: Option<NonZeroU64>, byte_cap: Option<NonZeroU64>) -> Result<Self, InvalidReaderConfig>
```

Builds the reader. `tls` is `Some(rustls::ClientConfig)` for a `verified`/`mutual` channel
and `None` for a declared `plaintext` one. `documentation_schema` and `environment` are
validated identifiers supplied by the composition root. An absent `row_cap`/`byte_cap`
selects the reader's own documented defaults.

#### Implements

`Clone`, `Debug`, `DictionaryReader`

### `constant DEFAULT_DOCUMENTATION_SCHEMA`

The documented default documentation schema, used when no `dictionary_schema` is configured.

### `constant DEFAULT_MAX_DICTIONARY_ROWS`

The documented default row cap on one dictionary read.

### `constant DEFAULT_MAX_DICTIONARY_BYTES`

The documented default byte cap on one dictionary read.
