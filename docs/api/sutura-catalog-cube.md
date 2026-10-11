<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-catalog-cube

The public API of `sutura-catalog-cube`, rendered from rustdoc JSON.

A `SemanticCatalog` over a Cube metrics serving layer: the definitions Cube's metadata API
(`/cubejs-api/v1/meta`) serves, read as a **declaring** source - `github.com/telekom/sutura#1340`.

# What a cube becomes

Each cube or view is a `Model` on the deployment's Cube source: the model and the table are the
cube's own name, which is how Cube addresses its members, and each dimension is a column under
its short name. The column type is Cube's own type word (`string`, `number`, `time`, `boolean`),
because that is all Cube says about it. A cube's description is required, as `Descriptions` is a
provided kind; a cube without one is refused by name.

# What stays in Cube

A measure's computation, a segment's condition and a join's condition are SQL text in Cube's own
model, and the plain metadata answer carries none of it. So a measure is decoded and **reported,
not defined**: no domain `Metric` is minted, because sutura cannot compute it and never computes
a Cube metric itself. The execution path that answers a question over a Cube measure through Cube
is not in this crate. A join is Cube's to make when one query names members of two cubes, so no
`Relationship` is declared either.

## `trait MetaReader`

```rust
pub trait MetaReader
```

Where the metadata answer a `CubeCatalog` decides over comes from.

The fake seam: `fixture::FixtureReader` serves the recorded answer, and `http::HttpMetaReader`
(behind the `http` feature) asks a running Cube.

## `enum CubeError`

```rust
pub enum CubeError
```

Why a metadata answer could not be read as a catalog.

### Variants

- `Read`
- `Identifier`
- `ForeignMember`
- `MissingDescription`
- `Description`
- `ColumnDescription`
- `Inconsistent`
- `Knowledge`
- `Digest`

### Implements

`Debug`, `Display`, `Error`

## `struct CubeCatalog`

```rust
pub struct CubeCatalog<R>
```

A catalog read from one Cube deployment's metadata answer.

`source` is the `sources.<alias>` every cube's model is on: one Cube deployment is one source.

### Methods

```rust
pub const fn new(name: SourceName, version: DefinitionVersion, source: SourceName, reader: R) -> Self
```

### Implements

`Clone`, `Debug`, `SemanticCatalog`

## Module `document`

The answer of Cube's metadata API (`GET /cubejs-api/v1/meta`), decoded into the fields this
adapter maps.

Cube's answer also carries display fields (`title`, `shortTitle`, `formatDescription`,
`drillMembers`, `folders`, …) that this adapter does not read. Unknown fields are ignored rather
than refused: each Cube release adds display fields, and a refusal there would stop a deployment on
an upgrade that changed nothing it reads. Every field it does read is required in the shape the
pinned Cube serves (`src/fixture/meta.json` is that answer, recorded).

The reader asks for the plain answer, never `?extended=true`: the extended one adds each member's
SQL text and each join's SQL condition, and this adapter takes no SQL text out of a serving layer.

### `struct Meta`

```rust
pub struct Meta
```

The whole answer: every cube and view the token may see.

#### Methods

```rust
pub fn cubes(&self) -> &[Cube]
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `enum CubeKind`

```rust
pub enum CubeKind
```

Whether an entry of the answer is a cube or a view over cubes.

#### Variants

- `Cube`
- `View`

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct Cube`

```rust
pub struct Cube
```

One cube or view.

#### Methods

```rust
pub fn description(&self) -> Option<&str>
```

```rust
pub fn dimensions(&self) -> &[Dimension]
```

```rust
pub const fn kind(&self) -> CubeKind
```

```rust
pub fn measures(&self) -> &[Measure]
```

```rust
pub fn name(&self) -> &str
```

```rust
pub fn segments(&self) -> &[Segment]
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct Measure`

```rust
pub struct Measure
```

A measure, named `<cube>.<measure>`. Its computation stays in Cube.

#### Methods

```rust
pub fn agg_type(&self) -> &str
```

Cube's own aggregation word (`sum`, `count`, `countDistinct`, `number`, …), as served.

```rust
pub fn description(&self) -> Option<&str>
```

```rust
pub fn name(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct Dimension`

```rust
pub struct Dimension
```

A dimension, named `<cube>.<dimension>`.

#### Methods

```rust
pub fn data_type(&self) -> &str
```

Cube's own type word (`string`, `number`, `time`, `boolean`, …), as served.

```rust
pub fn description(&self) -> Option<&str>
```

```rust
pub fn name(&self) -> &str
```

```rust
pub const fn primary_key(&self) -> bool
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

### `struct Segment`

```rust
pub struct Segment
```

A segment: a named filter, named `<cube>.<segment>`. Its condition stays in Cube.

#### Methods

```rust
pub fn description(&self) -> Option<&str>
```

```rust
pub fn name(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Deserialize<'de>`, `Eq`, `PartialEq`

## Module `fixture`

The recorded metadata answer and the fake reader that serves it.

`fixture/meta.json` is the answer the pinned Cube image in `compose.services.yaml` serves for
`examples/cube/model`, recorded from the `cube` profile and pretty-printed. `tests/provisioned.rs`
reads the live tier and asserts it still decodes to the same definitions.

### `struct FixtureReader`

```rust
pub struct FixtureReader
```

The fake `MetaReader`: it decodes `META` on every read, through the same path a live answer takes.

#### Implements

`Clone`, `Copy`, `Debug`, `MetaReader`

### `fn over_fixture_source`

```rust
pub fn over_fixture_source(name: sutura_domain::model::SourceName, version: sutura_domain::pinned::DefinitionVersion) -> crate::CubeCatalog<FixtureReader>
```

A `CubeCatalog` over the recorded answer, every cube on `name`. The golden registry opens it.

### `constant META`

The recorded answer, as served.

## Module `http`

The real `crate::MetaReader`: one `GET /cubejs-api/v1/meta` against a running Cube.

Behind the crate's default-off `http` feature, so a build that does not ask for it links no
outbound TLS stack.

# Auth

The deployment's Cube token as a `Secret`, sent as `Authorization: Bearer <token>`. Cube
verifies it as a JSON Web Token and answers `403` without one or with one it refuses. The
catalog is read as the deployment, never as a caller: the definitions are the same for every
caller, and per-caller rules belong to the execution path.

# Bounds, TLS and the endpoint

`ReadBounds` gives the request its timeout and the answer its size cap. `Endpoint::parse`
accepts `https://` for any host and `http://` only for an IP loopback literal, so a token is never
sent in plain text to a host that is not this one. The trust anchors are `ureq`'s compiled-in
roots, or exactly the deployment's `security.outbound.transport_anchors`; redirects are not
followed.

### `enum HttpReaderError`

```rust
pub enum HttpReaderError
```

Why the metadata answer was not read.

Reaches `crate::CubeCatalog` boxed inside `crate::CubeError::Read`. No variant carries the token,
and `HttpReaderError::Refused` renders the status and never Cube's own text.

#### Variants

- `Unreachable`
- `Unreadable`
- `Refused`
- `TooLarge`
- `Shape`

#### Implements

`Debug`, `Display`, `Error`

### `struct HttpMetaReader`

```rust
pub struct HttpMetaReader
```

A Cube deployment's metadata API, reached over HTTP.

#### Methods

```rust
pub fn new(endpoint: Endpoint, token: Secret, bounds: ReadBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self
```

A reader over a fixed agent: `anchors` `None` keeps `ureq`'s compiled-in roots, `Some`
replaces them with exactly the declared certificates.

```rust
pub const fn rotating(endpoint: Endpoint, token: Secret, bounds: ReadBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self
```

A reader over the rotating agent a composition root built with `Self::rotating_agent`.

```rust
pub fn rotating_agent(bounds: ReadBounds, declared: Option<sutura_tls::Declared>) -> Result<OutboundAgent, sutura_tls::LoadError>
```

The rotating agent for a declared `security.outbound` set, and its poll handle.

# Errors

The declared bundle or client identity cannot be loaded.

#### Implements

`Clone`, `Debug`, `MetaReader`

### `use DEFAULT_MAX_RESPONSE_BYTES`

### `use DEFAULT_TIMEOUT_SECONDS`

### `use Endpoint`

### `use EndpointMessage`

### `use InvalidEndpoint`

### `use InvalidReadBounds`

### `use OutboundAgent`

### `use ReadBounds`

### `constant META_PATH`

The metadata API's path under a Cube server's root.
