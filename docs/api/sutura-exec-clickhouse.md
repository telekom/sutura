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
alone: `sutura-exec-bigquery`'s own `Cargo.toml` states the reasoning for its `ureq`-based wire
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
crate's own unit tests. The split exists for the reason `sutura-exec-bigquery`'s `JobTransport` does: no
venue that runs `just validate` can reach a live `ClickHouse` (`compose.services.yaml`'s
`clickhouse` service is a docker-compose tier; the nix sandbox has no docker socket and no
`clickhouse-tier.nix` exists), so a binding that dialled out would either panic in every
such venue or have to declare itself absent - and `sutura_conformance::venue::
refuse_a_declared_absence` fires against an absent fixture whenever `SUTURA_DEV_REQUIRE_TIER`
is set, which it is inside `checks.nextest` because the Postgres tier is up there, a fact
about THAT tier and not about this one. A canned transport sidesteps the question entirely:
Nothing here claims to be a live endpoint.

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

# What is NOT here

**No composition root links this crate.** Nothing in `sutura-cli`'s `sources.rs` or `serve`
module names a `kind: clickhouse`, so no served deployment can reach a `ClickHouse` source
today. `crate-map`'s rule is a default-off feature on whichever composition root wants to
serve one, and none does yet. Wiring that in is a `sutura-cli` change, out of this crate's
own scope.

**No raw-SQL tool support** (`Warehouse::ACCEPTS_RAW_STATEMENTS` stays at its `false` default)
and **no leg execution** (`Warehouse::EXECUTES_LEGS` stays at its `false` default, so
`Executable::Leg` answers `ClickHouseError::LegWithoutCombiner` exactly as
`sutura_exec_postgres` and `sutura_exec_bigquery` both do).

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
pub const fn of(source: SourceName, posture: SourcePosture, transport: T) -> Self
```

Opens an adapter over an already-constructed transport - `T = transport::Http` for a real
connection, and a fake for the conformance pack.

### Implements

`Debug`, `Warehouse`

## Module `deadline`

`docs/adr/0029`'s per-request budget, this adapter's own mechanism: `ClickHouse`'s
`max_execution_time` setting.

# Is the deadline honourable here? What was checked, and what was not

**`max_execution_time` is a TOTAL wall-clock budget the server enforces, not the per-read idle
timeout `#127`'s Oracle work found in that driver.** `ClickHouse`'s own documentation
(`https://clickhouse.com/docs/en/operations/settings/query-complexity#max-execution-time`)
states the check runs against elapsed time since the query started, and a query that has run
longer than the setting is stopped and the server answers `Code: 159. DB::Exception: Timeout
exceeded` - the same shape as Postgres's own `statement_timeout`, and unlike the idle-read
timeout the Oracle record names as the trap to check for. **What this adapter has NOT measured
against a real server: the exact granularity of that check for a query with no natural
"stage" boundary** (a single unbounded aggregate scan, say) - `ClickHouse`'s own documentation
notes the check happens at block boundaries during execution, so a single block that itself
runs long could, in principle, overrun the setting by one block's worth of work before the
next check point. That is the same shape of limit `docs/adr/0029`'s Postgres row already
accepts for `SET LOCAL statement_timeout` (checked between statements of the interpreter, not
mid-expression), stated here rather than assumed.

`Deadline::remaining_at` returning `None` when expired is read as *stop now*, never as *wait
forever*: `refuse_if_spent` refuses locally before a request is ever sent, and
`max_execution_time_seconds` answers `None` (which `crate::transport::Http::run` reads as
*do not send the setting at all*) only after `refuse_if_spent` has already refused a spent
deadline - there is no path where `None` reaches the wire as *no limit*. A `0` is never sent
either: `ClickHouse` reads `max_execution_time = 0` as *no limit*, the same "zero reads as
unbounded" trap `sutura_exec_postgres::deadline` documents for `statement_timeout`. The
mechanism that prevents a `0` from reaching the wire is `remaining_at`'s `None` itself: a
spent deadline has `None`, which `refuse_if_spent` catches before
`max_execution_time_seconds` is ever asked, so the remaining budget is always strictly
positive when this function runs. The `ceiling.max(1)` floor in
`max_execution_time_seconds` is therefore a dead defensive bound - kept because a future
caller that bypassed `refuse_if_spent` should still never send a `0`, but unreachable on
every path that goes through the transport.

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
`sutura-exec-bigquery`'s `JobTransport` already established for the identical reason (no live
server this repository can reach in every venue that runs the suite; see this crate's own
`lib.rs` header).

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
