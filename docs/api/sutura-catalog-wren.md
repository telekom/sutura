<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-wren

The public API of `sutura-catalog-wren`, rendered from rustdoc JSON.

`sutura import wren <dir> <out>`.

`<dir>` is a `WrenAI` project directory - read as `<dir>/manifest.json`, the MDL manifest
`wren-core-base::mdl::manifest` defines (layout version 2; `wire`'s own header names the
upstream source this was read from). Everything this converter knows about a wren project is in
that one file: no second reader for a `views/` directory or a knowledge export, because nothing
upstream of the manifest declares one that this converter's own module header does not already
name as out of scope.

This crate is not only an exporter. `catalog` reads the same `manifest.json` straight into a
pinned sutura bundle, so wren can be registered as a `declaring` catalog in the conformance
matrix as well as converting to markdown.

## `struct Summary`

```rust
pub struct Summary
```

What ran, for the two lines the command prints.

### Methods

```rust
pub const fn metrics(&self) -> usize
```

```rust
pub const fn models(&self) -> usize
```

```rust
pub const fn refusals(&self) -> usize
```

```rust
pub const fn relationships(&self) -> usize
```

### Implements

`Debug`

## `enum ImportError`

```rust
pub enum ImportError
```

Why an import wrote nothing, or stopped part-way through writing.

### Variants

- `Read`
- `NotAManifest`
- `DestinationNotEmpty` - `<out>` already holds files. Refused rather than written into, because a document an earlier run wrote and this manifest no longer produces would sit beside the new output as if this run had converted it - and deleting the operator's files is not this command's call.
- `Write`

### Implements

`Debug`, `Display`, `Error`

## `fn import`

```rust
pub fn import(source: &std::path::Path, destination: &std::path::Path) -> Result<Summary, ImportError>
```

Converts the wren project at `source` into markdown catalog documents and a refusal report
under `destination`, which must be absent or empty.

# Errors

If `<source>/manifest.json` cannot be read or is not a wren MDL manifest this converter's `wire`
module can parse, if `destination` already holds a file, if it cannot be read, or if writing
into it fails.

## `use WrenCatalog`

A catalog read from one `WrenAI` `manifest.json`.

Like the other on-disk adapters, it carries a declared NAME (the key the contribution manifest
records this contributor under), the manifest path and a version: the version identifies which
snapshot of the manifest this is supplied from, exactly as `sutura_domain::pinned` expects.

## Module `catalog`

A `SemanticCatalog` over a `WrenAI` MDL `manifest.json`.

This is the reader half of the wren crate. `super::import` converts a manifest into markdown
catalog documents; this module reads the same manifest bytes (the committed fixture under
`testdata/manifest.json`) straight into a pinned sutura bundle, so the crate is not merely an
exporter but can also be registered as a `declaring` metadata source in the conformance matrix.

**What this reader supplies is the physical model and nothing of the semantic layer.** A wren
`Model` with a `tableReference` is a physical table: its name is the model (and table) name and
every non-computed, non-relationship column carries a declared type, so the bundle provides
exactly `DefinitionKind::Structure` and - per column, since a wren `Column.type` is required
but a spelling this crate cannot represent is dropped per `Column::new`'s rule - may-provide
`DefinitionKind::ColumnTypes`. Everything else a wren project writes about joins, cubes and
metrics is a deliberate, declared absence for the same reason `sutura-catalog-okf` declares its
own: this adapter is measured against what it says it carries, and it says it carries only the
columns-as-defined.

**What cannot be represented is refused, never dropped.** A `Model` whose rows come from an
authored statement (`refSql`) is not a physical table, and a column that navigates a relation or
carries a computation is not a physical field - each is a refusal naming the item rather than a
row silently missing from the model, mirroring `super::convert`'s own "everything maps or is
refused by name" rule.

### `struct WrenCatalog`

```rust
pub struct WrenCatalog
```

A catalog read from one `WrenAI` `manifest.json`.

Like the other on-disk adapters, it carries a declared NAME (the key the contribution manifest
records this contributor under), the manifest path and a version: the version identifies which
snapshot of the manifest this is supplied from, exactly as `sutura_domain::pinned` expects.

#### Methods

```rust
pub const fn name(&self) -> &SourceName
```

The declared name this contributor is recorded under in a bundle's contribution manifest.

```rust
pub const fn new(name: SourceName, manifest: PathBuf, version: DefinitionVersion) -> Self
```

Points a catalog at one wren `manifest.json`.

The version is supplied rather than derived, because what identifies a snapshot of a
manifest is not something the manifest knows - it is a commit id or a tag that only the
caller has.

#### Implements

`Clone`, `Debug`, `SemanticCatalog`

### `enum WrenCatalogError`

```rust
pub enum WrenCatalogError
```

Why one `manifest.json` could not be read as a wren catalog.

Every variant but three (`Inconsistent`, `UncheckableKnowledge`, `Digest`) carries the manifest
path, because a catalog is one file and the file is what failed.

#### Variants

- `Read`
- `NotAManifest`
- `InvalidModelName`
- `InvalidColumnName`
- `RefSqlModel`
- `UnrepresentableColumn`
- `Inconsistent`
- `UncheckableKnowledge`
- `Digest`

#### Implements

`Debug`, `Display`, `Error`
