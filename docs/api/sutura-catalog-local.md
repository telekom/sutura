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

**The variant is the contract, so a variant that names the wrong failure is a false contract even
with a truthful `#[source]` beneath it.** This enum had one that did: every failure of the
kind-probe deserialization became `UnknownKind`, whose message was "declares no `kind`". A
document saying `kind: dashboard`, one saying `kind: 3`, and one whose YAML did not parse at all
were three different problems reported as the same missing key - and a caller matching on the
variant, which is the only thing a caller can match on, was told something untrue in two cases out
of three. Keeping the parse error reachable through the chain did not fix that; it only meant the
truth was available to whoever thought to look past the variant.

It is two variants now, and the line between them is a mechanism rather than a guess at an error
message: `Self::MalformedFrontmatter` is raised when the block does not parse as YAML at all,
and `Self::IdentifyKind` when it parses and still does not identify the document. They are two
rather than four - missing, unrecognised, wrong type - because nothing in this workspace matches
on any of them, so a split finer than the remedy is a branch nobody takes: "your frontmatter is
not YAML" and "your frontmatter does not say what this is" send a reader to different places, and
"the `kind` key is missing" versus "its value is not one of three" send them to the same one. A
finer split is a cheap change if a caller ever needs the branch.

### Variants

- `NotADirectory`
- `Io`
- `Malformed`
- `Frontmatter`
- `MalformedFrontmatter`
- `IdentifyKind`
- `Metric`
- `Description` - The prose of a definition document is not a usable description.
- `NoteBody` - The prose of a knowledge document is not a usable note body: nothing at all, or more of it than a note may carry.
- `Inconsistent`
- `UncheckableKnowledge` - The notes do not hold together with the definitions they are about.
- `Empty`
- `Digest` - The domain could not hash the definitions.

### Implements

`Debug`, `Display`, `Error`

## `struct LocalCatalog`

```rust
pub struct LocalCatalog
```

A catalog read from a directory of documents.

Carries a declared NAME, the way a `sources:` entry or a `catalogs:` entry carries an alias: it
is the key the contribution manifest records this contributor under. `sutura-serve` hands it the
configured `catalogs:.<key>`; `sutura-cli` names its single directory a constant. The adapter
can no more guess it than a data adapter can guess its source alias.

### Methods

```rust
pub const fn name(&self) -> &SourceName
```

The declared name this contributor is recorded under in a bundle's contribution manifest.

```rust
pub const fn new(name: SourceName, root: PathBuf, version: DefinitionVersion) -> Self
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

`sutura_domain::measure` is the one exception, and the `measure` field of `MetricDoc` argues
for it where a reader will be standing when they wonder. In short: those types already carry
exactly this format's representation, and mirroring its variants here would buy nothing but a
place to forget the next one.

### `enum DocumentKind`

```rust
pub enum DocumentKind
```

What a document declares itself to be.

Required in every document rather than inferred from the directory it sits in. A file in the
wrong directory is then an error naming the mismatch, instead of a metric that was quietly never
loaded, and the loader can walk one tree instead of trusting a layout convention.

**Seven kinds now, and the split between them is worth reading as two groups.** The first three
are definitions: they decide what executes, and `sutura_domain::catalog` checks them. The last
four are knowledge: they decide what a reader understands, and `sutura_domain::knowledge` checks
them. Nothing in the loader treats the two groups differently - one walk, one tag, one dispatch -
which is what keeps "which directory is this in" from becoming part of the format.

#### Variants

- `Model`
- `Relationship`
- `Metric`
- `Glossary` - One entry of the business glossary.
- `Caveat` - Something a reader has to know before trusting a number.
- `NotDefined` - A term this catalog deliberately does not define.
- `Example` - A worked question: how somebody asked it, and what to send.

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
pub fn into_domain(self, description: Description) -> Model
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
pub fn into_domain(self, description: Description) -> Result<Metric, InvalidMetricDocument>
```

#### Implements

`Debug`, `Deserialize<'de>`

### `enum InvalidMetricDocument`

```rust
pub enum InvalidMetricDocument
```

Why a metric document cannot become a metric.

Only what belongs to the DOCUMENT: the two conversions this file performs that the domain's own
constructors can refuse. Everything about whether a metric holds together is checked in
`sutura_domain::catalog`, once, for every adapter - **including the duplicated dimension this
enum used to carry.** That variant existed because `Metric::new` took a map, so the domain could
not see the pair; it takes a vector now, and the refusal is
`InconsistentDefinitions::DuplicateDimension`, which `Self::Inconsistent` carries. A copy of
a check in one adapter is a check the other adapter does not have, which is exactly what
happened.

#### Variants

- `Inconsistent` - The domain refused the metric this document describes.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### Module `knowledge`

The on-disk shape of the four knowledge documents: the glossary, the caveats, the terms
deliberately left undefined, and the worked examples.

Same rules as the definition documents next door, and one addition worth stating on its own.

**`example`'s `question:` field deserializes `sutura_domain::query::Query` DIRECTLY, and that is
the most useful line in this file.** `Query` carries `deny_unknown_fields` and has no field for
SQL, so an author who copies a verified-query file out of a reference implementation - `nl:` plus
`sql:` is exactly the shape those files have - gets an error naming `sql`, at load, in the
document. `document.rs`'s `a_document_carrying_sql_is_refused_by_name` therefore extends to this
kind for nothing, and an example cannot describe a request the surface would not accept, because
the thing in the document IS the request.

**Prose is the markdown body**, as for a model and a metric, and it becomes a
`NoteBody` - which is where the size caps are. A document over the cap does not load; nothing
anywhere truncates it.

**Nothing here reads the directory a document sits in.** The `kind:` tag in the frontmatter is
what says what a file is - `crate::document::DocumentKind` argues why - so
`catalog/knowledge/glossary/` is a convenience for whoever browses the tree and not a fact the
loader depends on. That is also the part of this adapter a metadata-service adapter can borrow: a
document kind is a concept, not a path.

#### `struct GlossaryDoc`

```rust
pub struct GlossaryDoc
```

One glossary entry: the words, and the one thing they mean.

`means` needs no `singleton_map` adapter, unlike `MetricDoc`'s `measure`. A `Referent` is read
through a flat `try_from` representation rather than by an external tag - `sutura_domain::knowledge`
gives the argument - so what an author writes is one mapping of plain keys, which is what YAML is
good at.

##### Methods

```rust
pub fn into_domain(self, body: NoteBody) -> GlossaryEntry
```

##### Implements

`Debug`, `Deserialize<'de>`

#### `struct CaveatDoc`

```rust
pub struct CaveatDoc
```

One caveat, and everything it is about.

`about` has no default. A caveat scoped to nothing is refused by
`sutura_domain::knowledge::InconsistentKnowledge::CaveatAboutNothing`, and that refusal is what
keeps the catalog from having an unscoped channel into the prompt - so the field being required
here means the author is told about the missing key rather than about the empty list.

##### Methods

```rust
pub fn into_domain(self, body: NoteBody) -> Caveat
```

##### Implements

`Debug`, `Deserialize<'de>`

#### `struct NotDefinedDoc`

```rust
pub struct NotDefinedDoc
```

One term this catalog deliberately does not define.

`NotDefinedDoc` rather than `AbsenceDoc`, because the two names are for two audiences: `not_defined`
is the word an author writes in `kind:`, and it says what they are doing; `Absence` is what the
domain calls the thing once it exists. Naming the document after the word in the file is what makes
an error about it findable.

##### Methods

```rust
pub fn into_domain(self, body: NoteBody) -> Absence
```

##### Implements

`Debug`, `Deserialize<'de>`

#### `struct ExampleDoc`

```rust
pub struct ExampleDoc
```

One worked question: how it was asked, and what to send.

##### Methods

```rust
pub fn into_domain(self, body: NoteBody) -> Example
```

##### Implements

`Debug`, `Deserialize<'de>`

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
