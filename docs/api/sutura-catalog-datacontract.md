<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-datacontract

The public API of `sutura-catalog-datacontract`, rendered from rustdoc JSON.

A `SemanticCatalog` over a directory of Open Data Contract Standard v3 contract documents.

One YAML document per contract, each an [ODCS v3 / Bitol](https://bitol.io/open-data-contract-standard/)
data contract: the `schema` array's `SchemaObject`s are the contract's physical tables, each
`properties[].name` is that table's column set, and a `SchemaObject.relationships` array (v3.1+)
records the joins between them. `docs/what-a-data-contract-can-carry.md` is the finding that
decides this adapter's whole shape.

**The ODCS vocabulary is richer than OKF's on the schema side, and just as silent on the
measure.** It names primary keys, `required` and `unique` per column, and carries a free-text
per-column `classification`; from v3.1.0 it carries a foreign key whose single-column target
`primaryKey`/`unique` evidence can licence a join under the same rule `sutura-catalog-rdbms`
applies. What it never carries, at any version, is a metric entity - so an adapter over it
**declares** `Structure`; **may-provides** `Descriptions`, `ColumnTypes`, `ColumnDescriptions`
(each optional in the schema) and `Relationships` (v3.1+, and only where a relationship's single-
column target carries `primaryKey`/`unique` evidence); **never declares** `Cardinality` (a
contract vouches for no metric fan-out, and there is no metric to own a dimension here); and
**reports-not-defines** the quality rules, the SLA and the `classification` while **declaring**
`Metrics`, `RequiredFilters`, `Grains`, `AllowedValues` and `Anchors` absent. Being a
`declaring` source measured against that declaration is the whole point of the
`CatalogKind::Declaring` class.

Three properties of the load worth stating, because each is a mechanism rather than a wish:

**The walk is sorted and refused-when-empty.** `read_dir` returns entries in filesystem order,
and a digest that moves between two runs over unchanged files is a digest nobody can act on, so
the walk collects into a `BTreeSet` and yields sorted. An empty or missing directory is an error
(`DataContractError::Empty`, `DataContractError::NotADirectory`) rather than a silently-empty
catalog, for the same reason the OKF and local catalogs refuse one. This walk and the read are
the bounded, single-open shape `sutura-catalog-okf` carries after `#1022`'s hardening - shared
now through one crate, `sutura_bounded_read`, rather than a third copy of one mechanism
(`github.com/telekom/sutura#1045`).

**A contract without a self-report of its version is refused by name.** `apiVersion` and `kind`
are required by the standard and this adapter reads both: an unknown `apiVersion` (a `v2.x` or
a typo'd `v3`) and an unknown `kind` are refused rather than guessed at, because which fields a
contract may carry - in particular whether `relationships` exist - is a property of the version
it stamps itself with.

**`deny_unknown_fields` at the depths this adapter decodes, and validated names.** A contract,
`SchemaObject`, `SchemaProperty` or relationship that carries a key this adapter does not read
is refused rather than silently ignored, and each column, table and model name is parsed
through the domain's validated newtypes. What this does NOT reach: `servers`, `quality`,
`description` (contract-level), `team`, `support`, `slaProperties` and `context` are accepted as
opaque `serde_norway::Value` subtrees and are never themselves schema-validated - reported and
ignored by declaration, `docs/what-a-data-contract-can-carry.md`. A duplicate column name (two
`properties` entries with one `name`) is an error, because `BTreeMap` would deduplicate and a
silently-shrinking column set is a digest that lies.

**A relationship this adapter cannot safely convert is refused, not dropped.** A schema-level
`relationships` array present before v3.1.0 (invalid against the published schema for that
version, but structurally decodable by this adapter's own type) is refused by name
(`DataContractError::RelationshipsBeforeV3_1`), and a property-level relationship (v3.1+,
`from` implicit) is refused by name (`DataContractError::PropertyLevelRelationshipUnsupported`)
rather than accepted as opaque and silently vanishing from the bundle.

## `struct DataContractCatalog`

```rust
pub struct DataContractCatalog
```

A catalog read from a directory of ODCS v3 contract documents.

Like the OKF and local catalogs, it carries a declared NAME, a root and a version: the name is
the key the contribution manifest records this contributor under, the root is the directory of
contracts, and the version identifies which snapshot of that directory this is.

### Methods

```rust
pub const fn name(&self) -> &SourceName
```

The declared name this contributor is recorded under in a bundle's contribution manifest.

```rust
pub const fn new(name: SourceName, root: PathBuf, version: DefinitionVersion) -> Self
```

Points a catalog at a directory of ODCS v3 contract documents.

The version is supplied rather than derived, because what identifies a snapshot of a directory
is not something the directory knows - it is a commit id or a tag that only the caller has.

```rust
pub fn root(&self) -> &Path
```

### Implements

`Clone`, `Debug`, `SemanticCatalog`

## `enum UnsupportedApiVersion`

```rust
pub enum UnsupportedApiVersion
```

Why a contract's `apiVersion` could not be read.

Every variant carries the word, so a message names the value a reader wrote and the version
line this adapter reads.

### Variants

- `Unknown`

### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## `enum DataContractError`

```rust
pub enum DataContractError
```

Why a directory could not be read as a data-contract catalog.

Every variant carries the path, because a catalog is many files and a message that names no file
sends a reader to read all of them.

### Variants

- `NotADirectory`
- `Io`
- `Open`
- `Malformed`
- `UnsupportedVersion`
- `UnknownKind`
- `UnknownRelationshipType`
- `CompositeKeyUnrepresentable`
- `UnsupportedReference`
- `RelationshipsBeforeV3_1`
- `PropertyLevelRelationshipUnsupported`
- `InvalidName`
- `InvalidColumn`
- `DuplicateColumn`
- `InvalidDescription`
- `InvalidColumnDescription`
- `TargetUniquenessUnknown`
- `RelationshipName`
- `Inconsistent`
- `UncheckableKnowledge`
- `Empty`
- `TooManyDocuments`
- `TooLarge`
- `NotARegularFile`
- `Digest`

### Implements

`Debug`, `Display`, `Error`
