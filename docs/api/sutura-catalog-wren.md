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
module can parse, if `destination` already holds a file, or if it cannot be written to.
