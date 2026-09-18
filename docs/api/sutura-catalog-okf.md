<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-okf

The public API of `sutura-catalog-okf`, rendered from rustdoc JSON.

A `SemanticCatalog` over a directory of OKF Frictionless Table Schema descriptors.

One YAML file per physical table, each a [Table Schema](https://specs.frictionlessdata.io/table-schema/)
descriptor: the `fields[].name` column set is the model's columns, the descriptor's `title` or
`description` is the model's description, and the file's own stem is the table (and model) name -
because the Table Schema vocabulary declares no table name of its own, and a file name is the one
thing an author wrote whose stem is deterministic and unique in a directory.

`docs/what-okf-can-carry.md` is the finding that decides this adapter's whole shape. The OKF
vocabulary (Table Schema version 1) is **strictly narrower than Wren** - it carries a table's
column set and free-text descriptions, and nothing of a metric layer. A `foreignKey` declares no
cardinality, so it can licence no relationship (a relationship carries a required `JoinType`,
and Table Schema gives no way to choose one) - which is why this adapter **declares**
`Relationships` out rather than supplying an unlicensed one. What it provides is exactly
`DefinitionKind::Structure` and `DefinitionKind::Descriptions`; everything else - the metric,
the required filter, the grain, the allowlist, the anchor, and the referent-bearing knowledge - is
a deliberate, declared absence. Being a `declaring` source measured against that declaration is
the whole point of the `CatalogKind::Declaring` class.

Three properties of the load worth stating, because each is a mechanism rather than a wish:

**The walk is sorted and refused-when-empty.** `read_dir` returns entries in filesystem order, and
a digest that moves between two runs over unchanged files is a digest nobody can act on, so the
walk collects into a `BTreeSet` and yields sorted. An empty or missing directory is an error
(`OkfCatalogError::Empty`, `OkfCatalogError::NotADirectory`) rather than a silently-empty
catalog, for the same reason `sutura-catalog-local` refuses one: a mistyped root that happens to
exist would otherwise load a catalog with no models and its declaration would then claim
`Structure` that `produced` does not observe.

**A descriptor without a self-report must fail, not default.** Each model is required to carry a
non-empty `title` or `description`, because the adapter declares `DefinitionKind::Descriptions`
and `MetadataCapabilities::produced` observes that kind only through a non-empty description - so
a document that supplied none would make the adapter's own declaration unfaithful. The requirement
is a refusal (`OkfCatalogError::MissingDescription`), not a default, which is this repository's
rule that a default is indistinguishable from a decision.

**`deny_unknown_fields` at every depth, and validated names.** A Table Schema descriptor that
carries a key this adapter does not read is refused rather than silently ignored, and each column,
table and model name is parsed through the domain's validated newtypes - so a document cannot
write past a `parse`. A duplicate column name (two `fields` entries with one `name`) is an error,
because `BTreeSet` deduplicates and a silently-shrinking column set is a digest that lies.

## `struct OkfCatalog`

```rust
pub struct OkfCatalog
```

A catalog read from a directory of Table Schema descriptors.

Like the local catalog's `LocalCatalog`, it carries a declared NAME, a root and a version: the
name is the key the contribution manifest records this contributor under, the root is the
directory of descriptors, and the version identifies which snapshot of that directory this is.

### Methods

```rust
pub const fn name(&self) -> &SourceName
```

The declared name this contributor is recorded under in a bundle's contribution manifest.

```rust
pub const fn new(name: SourceName, root: PathBuf, version: DefinitionVersion) -> Self
```

Points a catalog at a directory of Table Schema descriptors.

The version is supplied rather than derived, because what identifies a snapshot of a directory
is not something the directory knows - it is a commit id or a tag that only the caller has.

```rust
pub fn root(&self) -> &Path
```

### Implements

`Clone`, `Debug`, `SemanticCatalog`

## `enum OkfCatalogError`

```rust
pub enum OkfCatalogError
```

Why a directory could not be read as an OKF catalog.

Every variant carries the path, because a catalog is many files and a message that names no file
sends a reader to read all of them.

### Variants

- `NotADirectory`
- `Io`
- `Malformed`
- `Unnamed`
- `InvalidName`
- `InvalidColumn`
- `DuplicateColumn`
- `MissingDescription`
- `InvalidDescription`
- `Inconsistent`
- `UncheckableKnowledge`
- `Empty`
- `TooManyDocuments`
- `Digest`

### Implements

`Debug`, `Display`, `Error`
