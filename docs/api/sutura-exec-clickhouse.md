<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-clickhouse

The public API of `sutura-exec-clickhouse`, rendered from rustdoc JSON.

A `Warehouse` adapter over `ClickHouse`, over its HTTP interface.

One connection under the deployment's declared identity (`SharedServiceUser`) - the static
half, like `sutura_exec_postgres`: no OAuth, no impersonation.

# Why `ureq` and not an async driver

**The port carries no `async fn`, and `ureq` is natively blocking - so this adapter needs no
`tokio` runtime and no `block_on` at all.** That is not a simplification made for this crate
alone: `sutura-exec-bigquery`'s own `Cargo.toml` states the reasoning for its ADBC driver
at length, and it transfers unchanged - `Warehouse::execute` reaches this adapter through
`sutura_runtime::spawn_carrying_span`, on a blocking-pool thread, and starting a runtime
*there* (which an async driver wrapped the way `sutura_exec_postgres` wraps `tokio-postgres`
would need) is exactly the `reqwest::blocking` shape that entry warns against. `ClickHouse`'s
own HTTP interface asks for nothing more than one request/response per statement, which is
`ureq`'s whole job. **A pure-Rust driver either way**: `ureq` with the `rustls` feature links
no C TLS library, and this crate's own `tls` module (over `sutura-tls`) is what most of
`sutura_exec_postgres::tls`'s reasoning transfers to.

# Why a driver-shaped seam, and not just a hand-rolled client

`transport::ClickHouseTransport` is the port this adapter's own port methods call through -
`transport::Http` for a real connection, and a canned implementor for the
crate's own unit tests. The split exists for the reason `sutura-exec-bigquery`'s `JobTransport` does:
the port's decisions - rendering, the refusals, the decode - are held by cells that need no
server, so they run on every build. What a SERVER answers is held elsewhere: the golden matrix in
`sutura-app`'s tests loads the example corpus into the server `nix/clickhouse-tier.nix` starts
(through `fixture`, behind the default-off `fixtures` feature) and pins the rows, refusals,
error and anchor report it answers - in `checks.nextest` and under `just test`.

`Warehouse::Error` for this adapter is `ClickHouseError`, generic over `T::Error` - the same
shape `sutura_exec_bigquery::BigQueryError<E>`
takes, for the same reason: `transport::ClickHouseTransport::source_refused`/
`transport::ClickHouseTransport::deadline_exceeded` let the TRANSPORT answer the port's own
two predicates about a failure only it can read the wire-level shape of.

# Parameters travel as `ClickHouse`'s own named parameters, never as text

`transport::Http` rewrites each rendered `?` into `{pN:Type}` and sends the value as a
separate `param_pN` field, which the SERVER binds by the declared type - see
`transport::rewrite_placeholders`'s own doc for why counting `?` occurrences against
`params.len()` is safe rather than a guess.

# `Deadline`, and what was checked about it - see `deadline`'s own header

Every `execute`/`declared_key`/`verify_anchor` round trip sends `max_execution_time`, computed
from what the port's `sutura_domain::warehouse::deadline::Deadline` has left. `deadline`'s
module header states what was measured about that setting's semantics and its limit, mirroring
`sutura_exec_postgres::deadline`'s own record for `SET LOCAL statement_timeout`.

# Every request pins the server settings that decide what the rows say

`join_use_nulls=1`: under `ClickHouse`'s default `0`, the unmatched side of an outer join
answers the column type's default - `''` for a `String` - where the SQL standard answers `NULL`.
Measured by hand against the pinned compose server over the example corpus: 6 of the 23
questions with a committed `@duckdb` row golden disagreed without it, 0 with it, under a
non-`Nullable` schema; under `Nullable` columns the setting changes nothing, so a fixture
importer's type choice decides whether the defect shows - `fixture` declares no column
`Nullable` for that reason. `output_format_json_quote_denormals=1`: under the default `0` an
infinite float answers the JSON `null`. `timeout_overflow_mode=throw`: `break` answers a spent
`max_execution_time` with HTTP 200 and the rows read so far. A unit cell holds what is sent; the
executed goldens hold what the first two settings make the server answer, and nothing executed
holds the third.

# What is NOT here

**No release links this crate.** `sutura-cli` opens a `kind: clickhouse` source behind its
default-off `clickhouse` feature, which `nix/shipped.nix` does not enable - see that feature's
own manifest entry for why.

**No raw-SQL tool support** (`Warehouse::ACCEPTS_RAW_STATEMENTS` stays at its `false` default)
and **no leg execution** (`Warehouse::EXECUTES_LEGS` stays at its `false` default, so
`Executable::Leg` answers `ClickHouseError::LegWithoutCombiner` exactly as
`sutura_exec_bigquery` does).

**No `dry_run` override.** `ClickHouse`'s HTTP interface has no cheap "prepare, do not run"
step this adapter could ask for without paying most of the cost of running the statement, so
this stays at the port's own default (`Ok(PreFlight::NotAsked)`) - the same honest absence
`Warehouse::dry_run`'s own doc names for an adapter where checking is not cheaper than running.

## `enum ClickHouseError`

```rust
pub enum ClickHouseError<E>
```

Why this data system could not answer.

Generic in the transport's own error type - the shape `sutura_exec_bigquery::BigQueryError<E>`
takes, for the same reason: `E` is `T::Error`, the one place a failure's wire-level shape
lives, and this adapter's own `source_refused`/`deadline_exceeded` delegate to it through
`ClickHouseError::Endpoint`.

### Variants

- `Endpoint` - The endpoint (or the canned pack) did not answer.
- `Render`
- `LegWithoutCombiner` - A leg without a combiner - `Warehouse::EXECUTES_LEGS` stays at its default; see the crate header.
- `NoPlaceForASubject` - The credential broker handed this adapter subject material it has nowhere to put.
- `PresentedDisagreesWithPosture`
- `UnsupportedType` - A column came back as a `ClickHouse` type this adapter does not map.
- `NotFinite` - A floating-point column came back as a value that is not a number.
- `MalformedResponse` - The response body was not the JSON shape this adapter reads.
- `Shape`
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.

### Implements

`Debug`, `Display`, `Error`

## `struct ClickHouseWarehouse`

```rust
pub struct ClickHouseWarehouse<T>
```

A `ClickHouse` connection, behind the `Warehouse` port.

Generic in `ClickHouseTransport` so this crate's own conformance pack can bind the port to a
canned implementor with no live endpoint; see this crate's own header.

### Methods

```rust
pub fn connect_in_database(source: SourceName, posture: SourcePosture, endpoint: Endpoint, auth: BasicAuth, database: &str) -> Result<Self, FixtureError>
```

Opens the adapter over `endpoint`, resolving every unqualified table name in `database`.

The database is created here if absent, so several opens can share one server without
clobbering each other's tables. `sutura_exec_postgres::PostgresWarehouse::connect_in_schema`'s
shape, over a database because that is `ClickHouse`'s namespace for a table.

```rust
pub fn load_csv(&self, table: &TableName, path: &Path) -> Result<(), FixtureError>
```

Exposes a fixture CSV as a table.

Columns are typed by `csv::infer` (see the module header for the one deliberate
departure), the table is recreated, then the file is sent as the body of an
`INSERT ... FORMAT CSVWithNames` the SERVER parses against those types.

```rust
pub const fn of(source: SourceName, posture: SourcePosture, transport: T) -> Self
```

Opens an adapter over an already-constructed transport - `T = transport::Http` for a real
connection, and a fake for the conformance pack.

### Implements

`Debug`, `Warehouse`

## Module `fixture`

The fixture tier: its credential, a private database per open, and a corpus CSV as a table.

Behind the default-off `fixtures` feature, like `sutura_exec_postgres::fixture`, and for its
reason: nothing here belongs in a composition root, and `--all-features` compiles, lints and
tests it on every run.

# The credential is configured or refused by name

`nix/clickhouse-tier.nix` generates a password per start and prints the two exports from
`sutura-clickhouse-tier credentials`; `nix/with-tier.sh` and `checks.nextest` evaluate them where
they export `SUTURA_DEV_REQUIRE_TIER`. Nothing here defaults a value - the shape
`sutura_exec_postgres::fixture` records the defect of - so an unset or blank variable is a
`UnconfiguredFixture` naming it.

# Column types: the golden matrix's, not the conformance path's

`sutura_domain::warehouse::csv::infer` classifies each column. Its `Decimal` is attached as
`Float64` HERE, deliberately: the matrix's other adapters attach a fractional column as a
double (`DuckDB`'s `read_csv_auto`, `sutura_exec_postgres`'s `load_csv`, the engine's Arrow
inference), so a `Decimal` column here would answer the same question as text rather than as a
real and differ from all three for a reason that is the importer's, not the server's.

**No column is `Nullable`, and an empty cell is refused.** `transport`'s `JOIN_USE_NULLS` is
measured to matter only over a non-`Nullable` schema, so a nullable importer would make the
executed goldens blind to that setting. And a non-`Nullable` column reads an empty CSV cell as
the type's default - `0`, `''` - where every other adapter reads `NULL`: a silently different
fixture. No committed fixture has one; a future one is refused rather than rewritten.

### `enum FixtureVariable`

```rust
pub enum FixtureVariable
```

One of the two values the `ClickHouse` fixture tier publishes into the environment.

#### Variants

- `User` - The user the tier's `users.xml` declares.
- `Password` - That user's password, generated per start by `nix/clickhouse-tier.nix`.

#### Methods

```rust
pub const fn name(self) -> &'static str
```

The environment variable's name, spelled once.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Hash`, `PartialEq`

### `enum UnconfiguredFixture`

```rust
pub enum UnconfiguredFixture
```

Why there is no fixture credential to connect with. No variant carries a substitute.

#### Variants

- `Unset`
- `Blank`

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum FixtureError`

```rust
pub enum FixtureError
```

Why the fixture tier could not be given a table.

#### Variants

- `InvalidDatabaseName` - A database name reaches `CREATE DATABASE` as an identifier, so it has to be a word.
- `Read`
- `Schema`
- `EmptyCell` - See the module header: a non-`Nullable` column would read this cell as a type default.
- `Server`

#### Implements

`Debug`, `Display`, `Error`

### `fn credential_from_env`

```rust
pub fn credential_from_env() -> Result<crate::transport::BasicAuth, UnconfiguredFixture>
```

This process's fixture credential, as the tier exported it.

## Module `tls`

Building the `ureq::tls::TlsConfig` a TLS `clickhouse` source channel verifies with.

This is the TLS half of `sutura_config::sources::transport`, turned into a verifier - the same
job `sutura_exec_postgres::tls` does for `rustls::ClientConfig`, so that module's own header is
this one's rather than restated: configuration owns the three-state DECLARATION (`plaintext` /
`verified` / `mutual`), and this module owns turning a declared `verified` or `mutual` channel
into the thing the client connects with. **The declared-trust-store rule extends rather than
forks** (`docs/adr/0010`): a PEM bundle or the host's system store, read once by `config` and
never a fallback, exactly as the Postgres source channel reads it.

The read itself lives in `sutura-tls`, shared with the Postgres source channel and the
`BigQuery` wire so the refusals for an unreadable, empty or malformed bundle/identity are not a
third hand-written copy. What is this crate's own is folding the read bytes into the shape
`ureq::tls::TlsConfig` wants - `ureq::tls::Certificate`/`ureq::tls::PrivateKey` rather than
`rustls`'s own DER newtypes, because `ureq` never hands this adapter a `rustls::ClientConfig` to
build.

# Why this is a fallible step SEPARATE from opening the transport

`transport::Http::connect`/`connect_secured` take an already-built `ureq::tls::TlsConfig` and
cannot fail, because `ureq::Agent::new_with_config` does no I/O - `ureq` dials lazily, on the
first request. So `TlsError` is its own type here rather than a variant folded into
`crate::ClickHouseError`: nothing about it can arrive from `Warehouse::execute`, only from
whoever resolves a declared channel into a config before opening the adapter.

### `enum TlsError`

```rust
pub enum TlsError
```

Why this crate could not build a `ureq::tls::TlsConfig` from a declared channel.

The same six refusals `sutura_exec_postgres::PostgresError` carries for the identical read,
field for field - `from_load_error` is the mapping.

#### Variants

- `AnchorsRead`
- `AnchorsEmpty`
- `IdentityRead`
- `IdentityIncomplete`
- `IdentityKey`
- `SystemStoreRead`
- `SystemStoreEmpty`

#### Implements

`Debug`, `Display`, `Error`

### `enum TlsAnchors`

```rust
pub enum TlsAnchors
```

The trust anchors a TLS source channel verifies against, resolved from the declaration.

#### Variants

- `Bundle` - A PEM bundle at this absolute path.
- `System` - The host's own trust store. Reached only when the deployment explicitly wrote `transport_anchors: system`; never a fallback.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct TlsIdentity`

```rust
pub struct TlsIdentity
```

The client certificate and key a `mutual` channel presents, resolved from the declaration.

#### Methods

```rust
pub const fn new(certificate: PathBuf, key: PathBuf) -> Self
```

A client identity from its declared paths.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `fn config`

```rust
pub fn config(anchors: &TlsAnchors, identity: Option<&TlsIdentity>) -> Result<ureq::tls::TlsConfig, TlsError>
```

Builds the `ureq::tls::TlsConfig` a TLS source channel verifies (and, for `mutual`, presents)
with, from the resolved anchor material and an optional client identity.

# Errors

The six refusals `TlsError` carries: an unreadable or empty bundle, an unreadable or empty
system store, and an unreadable or malformed identity.

## Module `transport`

The seam this adapter's `execute`/`verify_anchor`/`declared_key` call through.

`Http` is the one implementor a real connection uses, and the crate's own unit tests bind
the port to a CANNED implementor instead, over the exact same trait - the shape
`sutura-exec-bigquery`'s `JobTransport` already established; see this crate's own `lib.rs`
header for where `Http` itself is executed.

`ClickHouseTransport::run` takes the RENDERED statement and its bound `ParamValue`s
untouched - never a rewritten string - so what each implementor does with them is its own
business. `Http` rewrites `?` into `ClickHouse`'s own named-parameter syntax
(`{p0:String}`, ...) and sends each value as a separate `param_p0=...` query field, which
`ClickHouse` binds server-side by the declared type: nothing here concatenates a value into the
statement text - see `rewrite_placeholders` for why counting `?` occurrences is safe rather
than a guess.

### `trait ClickHouseTransport`

```rust
pub trait ClickHouseTransport
```

Where a statement runs, seen from this adapter's own port.

The `Warehouse` port itself is synchronous, and so is this: `ureq` is a blocking client and
needs no runtime of its own - see this crate's `Cargo.toml` for the reasoning `sutura-exec-
bigquery`'s `wire` entry states at length, which applies here unchanged.

### `struct Endpoint`

```rust
pub struct Endpoint
```

A `ClickHouse` HTTP endpoint address.

#### Methods

```rust
pub fn plaintext(host: impl Into<String>, port: u16) -> Self
```

A plaintext (`http://`) endpoint.

```rust
pub fn tls(host: impl Into<String>, port: u16) -> Self
```

A TLS-verified (`https://`) endpoint.

#### Implements

`Clone`, `Debug`

### `struct BasicAuth`

```rust
pub struct BasicAuth
```

The credential a `clickhouse` source presents over HTTP Basic authentication.

#### Methods

```rust
pub fn new(user: impl Into<String>, password: sutura_domain::identity::Secret) -> Self
```

A user/password pair, as the deployment declared it.

#### Implements

`Clone`, `Debug`

### `enum HttpError`

```rust
pub enum HttpError
```

Why `Http` could not answer.

#### Variants

- `Transport` - The endpoint could not be dialled, or its reply could not be read.
- `ServerRefused` - The endpoint answered with a non-2xx status.

  `message` is its response body, which `ClickHouse`'s own error text names its exception
  CODE inside (`Code: 159. DB::Exception: ...`) - see `Http::deadline_exceeded`/
  `Http::source_refused` for the two matches this adapter makes against it, and their own
  stated limit: neither is a typed code, because the driver hands back text and nothing more
  structured.
- `PlaceholderMismatch` - The rendered statement's own `?` count did not match the bound parameters - a defect in this crate's rewrite rather than in the plan; see `rewrite_placeholders`.
- `DeadlineSpent` - The deadline was already spent before a request was ever sent.
- `InvalidEndpoint`

#### Implements

`Debug`, `Display`, `Error`

### `struct Http`

```rust
pub struct Http
```

The real transport: one `ureq::Agent`, built once and kept for this adapter's life.

The same shape `sutura_exec_postgres::PostgresWarehouse` keeps its one connection in. A rotated
TLS config takes effect on the next agent a composition root builds, never on an agent already
standing - `ureq::Agent`'s configuration is fixed at construction, so this is not a choice made
here but the shape the client already has.

#### Methods

```rust
pub fn connect(endpoint: Endpoint, auth: Option<BasicAuth>) -> Self
```

Opens the transport over `endpoint` with no TLS at all - a `plaintext` channel, or the
fixture tier's loopback path.

```rust
pub fn connect_secured(endpoint: Endpoint, auth: Option<BasicAuth>, tls: ureq::tls::TlsConfig) -> Self
```

Opens the transport secured as the caller resolved: `tls` is the `ureq::tls::TlsConfig`
`crate::tls::config` built from the declared channel. Both this and `Self::connect` are
produced by the composition root, which is the only place that can see the declared
`sutura_config::sources::transport::SourceTransport` - the same boundary
`PostgresWarehouse::connect_secured`'s own signature draws.

#### Implements

`ClickHouseTransport`, `Debug`

### `type_alias RunResult`

What one `ClickHouseTransport::run` answers on success.

A `type` alias so the trait's own signature reads as one name rather than as the two-level
generic clippy's `type_complexity` lint asks not to repeat.
