<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-local

The public API of `sutura-catalog-local`, rendered from rustdoc JSON.

A `SemanticCatalog` over a directory of markdown documents with YAML frontmatter.

One document per model, relationship and metric: the frontmatter is the definition and the prose
is the description that travels with an answer. It is the catalog for the case where there is no
upstream semantic layer to take a rendered statement from, which is the case a person is in the
first time they try this. `docs/adr/0001-first-party-semantic-models.md` is the decision.

Three properties of the load are worth stating, because each is a mechanism rather than an
intention:

**`LocalCatalog::load` takes no request context**, because the trait does not have one to give
it. A catalog that could see the caller could return a different definition per caller, and the
digest that travels with an answer would then describe something other than what produced it.

**The walk is sorted**, so the same directory produces the same bundle. `read_dir` returns
entries in whatever order the filesystem chose, and a digest that moves between two runs over
unchanged files is a digest nobody can act on.

**An empty directory is an error.** A mistyped root that happens to exist would otherwise load a
catalog with no metrics and refuse every question, and the refusal would name the metric rather
than the path.

## `enum LocalCatalogError`

```rust
pub enum LocalCatalogError
```

Why a directory could not be read as a catalog.

Every variant carries the path, because a catalog is many files and "invalid type: integer" with
no file name is a message that sends the reader to read all of them.

### Variants

- `NotADirectory`
- `Io`
- `Malformed`
- `Frontmatter`
- `UnknownKind`
- `Metric`
- `Inconsistent`
- `Empty`
- `Canonicalize`
- `Digest`

### Implements

`Debug`, `Display`, `Error`

## `struct LocalCatalog`

```rust
pub struct LocalCatalog
```

A catalog read from a directory of documents.

### Methods

```rust
pub const fn new(root: PathBuf, version: DefinitionVersion) -> Self
```

Points a catalog at a directory.

The version is supplied rather than derived, because what identifies a snapshot of a
directory is not something the directory knows: it is a commit id, a build number or a tag,
and only the caller has it. Deriving it from the digest would make the two say the same thing
twice and leave no way to tell two builds of identical content apart.

```rust
pub fn root(&self) -> &Path
```

### Implements

`Clone`, `Debug`, `SemanticCatalog`

## `fn canonical_form`

```rust
pub fn canonical_form(definitions: &sutura_domain::catalog::Definitions) -> Result<Vec<u8>, serde_json::Error>
```

The canonical byte form of a set of definitions: what the digest is taken over.

JSON rather than the YAML it was read from, and that is the whole point. Reformatting a document,
reordering two files or rewording a comment must not move the digest; changing what a metric means
must. Serializing the *parsed* definitions gives exactly that, because everything that survives
parsing is meaning and everything that does not is layout.

Deterministic for two reasons that both have to hold: `Definitions` uses `BTreeMap` throughout,
so collection order is content order rather than hash order, and `serde_json` writes struct
fields in declaration order.

It lives in this adapter because `sutura-domain` cannot hash - `sha2` is not on its allowlisted
dependency tree, deliberately. When a second real catalog adapter lands, this moves to something
both can depend on rather than being reimplemented; a second implementation of a canonical form
is two canonical forms.

## `fn digest_of`

```rust
pub fn digest_of(definitions: &sutura_domain::catalog::Definitions) -> Result<sutura_domain::definitions::DefinitionDigest, LocalCatalogError>
```

The digest of a set of definitions.

## Module `document`

The on-disk shape of a catalog document, and its conversion into domain types.

These structs exist so the file format is a separate thing from the model. A domain type with
`Deserialize` on it would make every rename in a catalog file a breaking change to the hexagon's
interior, and it would put the file format's defaults inside the types the business rules are
written in.

`deny_unknown_fields` is on every one of them, and it is the most useful line in this module. A
misspelled key would otherwise be dropped in silence, and the definition that loads is not the
one the author wrote: `colums:` yields a model with no columns, which then refuses every question
about it for a reason that says nothing about a typo.

### `enum DocumentKind`

```rust
pub enum DocumentKind
```

What a document declares itself to be.

Required in every document rather than inferred from the directory it sits in. A file in the
wrong directory is then an error naming the mismatch, instead of a metric that was quietly never
loaded, and the loader can walk one tree instead of trusting a layout convention.

#### Variants

- `Model`
- `Relationship`
- `Metric`

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct KindProbe`

```rust
pub struct KindProbe
```

Just enough of a document to know which shape to parse it as.

A separate pass over the same few lines. The alternative is an internally tagged enum, and serde
cannot combine one with `deny_unknown_fields`, which is the check that makes a typo an error. Two
parses of a frontmatter block is not a cost worth trading that for.

#### Methods

```rust
pub const fn kind(&self) -> DocumentKind
```

What the document says it is.

An accessor rather than a public field, because the boundary gate fails a public field on a
public struct in a library crate: a struct literal can build a value a constructor would
have rejected, and the rule does not get to make an exception for a type that currently has
no invariant to protect.

#### Implements

`Debug`, `Deserialize<'de>`

### `enum AnchorLiteral`

```rust
pub enum AnchorLiteral
```

A value an anchor may be written as.

Untagged so `value: 197122` and `value: "197122"` both work. Without it the unquoted form fails
with "invalid type: integer, expected a string", which is a true statement about a file that
looks correct to whoever wrote it. Everything becomes text either way, because that is what an
anchor comparison uses.

#### Variants

- `Integer`
- `Text`

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`

### `struct ModelDoc`

```rust
pub struct ModelDoc
```

#### Methods

```rust
pub fn into_domain(self, description: String) -> Model
```

#### Implements

`Debug`, `Deserialize<'de>`

### `struct EndpointDoc`

```rust
pub struct EndpointDoc
```

One end of a relationship.

#### Implements

`Debug`, `Deserialize<'de>`

### `struct RelationshipDoc`

```rust
pub struct RelationshipDoc
```

#### Methods

```rust
pub fn into_domain(self) -> Relationship
```

#### Implements

`Debug`, `Deserialize<'de>`

### `struct MeasureDoc`

```rust
pub struct MeasureDoc
```

#### Implements

`Debug`, `Deserialize<'de>`

### `struct DimensionDoc`

```rust
pub struct DimensionDoc
```

A dimension, as a list entry with its own `name`.

A sequence rather than a map keyed by name, and that is not a style choice. A YAML mapping with
the same key twice keeps the last value and reports nothing, so a metric declaring `region`
twice would load with whichever definition came second. As a list the duplication survives to
where `MetricDoc::into_domain` can refuse it.

#### Implements

`Debug`, `Deserialize<'de>`

### `struct AnchorDoc`

```rust
pub struct AnchorDoc
```

#### Implements

`Debug`, `Deserialize<'de>`

### `struct MetricDoc`

```rust
pub struct MetricDoc
```

#### Methods

```rust
pub fn into_domain(self, description: String) -> Result<Metric, InvalidMetricDocument>
```

#### Implements

`Debug`, `Deserialize<'de>`

### `enum InvalidMetricDocument`

```rust
pub enum InvalidMetricDocument
```

Why a metric document cannot become a metric.

Only the things `sutura_domain::catalog::Definitions` cannot see, because by the time it runs
the duplication has already been collapsed by the map it holds. Everything else is checked there,
once, for every adapter.

#### Variants

- `DuplicateDimension`

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `frontmatter`

Splitting a catalog document into its YAML frontmatter and its prose.

Its own module because it is the only text scanning in this crate, and because the failure
modes are the interesting part: a document whose frontmatter is silently treated as prose
loads as a metric with no definition, and one whose prose is silently treated as frontmatter
fails with a YAML error that names a line nobody wrote.

### `enum MalformedDocument`

```rust
pub enum MalformedDocument
```

Why a document could not be split.

#### Variants

- `Unfenced` - The document does not open with a fence.
- `Unterminated` - The opening fence is never closed.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct Split`

```rust
pub struct Split<'a>
```

A document split into the part that is parsed and the part that is read.

#### Methods

```rust
pub const fn body(&self) -> &'a str
```

The prose, trimmed. It becomes a description that travels with an answer, so leading and
trailing blank lines are noise rather than content.

```rust
pub const fn frontmatter(&self) -> &'a str
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `fn split`

```rust
pub fn split(text: &str) -> Result<Split<'_>, MalformedDocument>
```

Splits `text` at its frontmatter fences.

Line-oriented rather than a search for the next `---` anywhere: a `---` inside the YAML (a
horizontal rule in a quoted string, or a nested document marker) must not end the block, and a
fence is only a fence when it is the whole line.
