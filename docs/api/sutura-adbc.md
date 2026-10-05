<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-adbc

The public API of `sutura-adbc`, rendered from rustdoc JSON.

The linked-driver FFI and the shared helpers every ADBC adapter needs.

This crate exists so the workspace's one `unsafe` declaration has one home
rather than one per adapter. `linked` declares the per-driver C ABI init function
each statically linked driver exports, and `ManagedDriver::load_static`
opens it through that pointer - the only route a STATIC musl artefact has, because
it has no dynamic loader. The `#[expect(unsafe_code)]` on that declaration is the
one lowering `cargo xtask check-unsafe` excepts, and this crate's root is the one
root that omits `#![forbid(unsafe_code)]`.

**Why a shared crate and not a per-adapter `linked.rs`** (`telekom/sutura#913` PR 2):
a second ADBC adapter that links its own driver needs the same FFI, and a list of
excepted roots widens the invariant from "one site" to "N sites". One crate keeps
the single exception and removes the duplication - the same shape `sutura-tls` has
for the TLS-bundle read two adapters share.

**The prefix is the role, not an adapter**: `sutura-adbc` joins no `-exec-` class
(it opens no data system and renders no dialect), so `xtask/src/boundaries/adapters.rs`'s
`data systems` prefix rule does not match it. An adapter depends on it behind a
default-off feature, the way `sutura-exec-postgres` depends on `sutura-tls`.

# What lives here and what does not

- `linked` (the `unsafe`, cfg-gated on `adbc_driver_linked`) and `location` (where a
  driver is, parsed once) are fully generic: any ADBC adapter that links or mounts a
  driver uses both.
- `bind` builds the one-row Arrow batch a positional-parameter driver binds from.
  It returns the Arrow error directly; each adapter wraps it into its own error
  type, because the error vocabulary is the adapter's and not the FFI's.
- The driver-option constants, the transport, the identity decision and the result
  drain stay in each adapter. This crate names no data system.

## `fn linked_driver`

```rust
pub fn linked_driver() -> Result<adbc_driver_manager::ManagedDriver, adbc_core::error::Error>
```

The `BigQuery` driver this artefact's own link carries, or an error where it carries none.

**The one route a STATIC musl binary has**, because it has no dynamic loader at all:
`linked`'s header carries what makes the declaration sound. A build that linked no
archive gets an `Err` here, which a caller renders rather than panicking on - the two
`cfg` halves have one signature, so an adapter's `load` needs no branch of its own.

# Errors

`CoreError` where a linked-in driver's own initialisation refused, and where this
build linked no archive at all - the same error type `ManagedDriver::load_dynamic_from_filename`
returns, so a caller cannot tell the two routes apart and does not have to.

## `fn linked_postgres_driver`

```rust
pub fn linked_postgres_driver() -> Result<adbc_driver_manager::ManagedDriver, adbc_core::error::Error>
```

The PostgreSQL driver this artefact's own link carries - `linked_driver`'s contract, for the
archive `nix/postgres-adbc.nix` builds.

A build that linked only the `BigQuery` archive gets an `Err` here and never that driver: the two
are separate symbols, so no build can hand one out under the other's name.

# Errors

As `linked_driver`.

## `fn linked_duckdb_driver`

```rust
pub fn linked_duckdb_driver() -> Result<adbc_driver_manager::ManagedDriver, adbc_core::error::Error>
```

The `DuckDB` driver this artefact's own link carries - `linked_driver`'s contract, for the
archive `nix/duckdb-adbc.nix` builds.

# Errors

As `linked_driver`.

## `fn mounted_duckdb_driver`

```rust
pub fn mounted_duckdb_driver(named: &str) -> Result<adbc_driver_manager::ManagedDriver, adbc_core::error::Error>
```

The `DuckDB` library a deployment mounted at `named`, opened as an ADBC driver.

**The path is parsed, never handed to the loader on trust.** `DriverLocation::parse` refuses an
empty or relative path before `dlopen` sees it - `telekom/sutura#929`'s sixth finding - so a
relative path is a refusal, not a library that depends on this process's working directory.

**The entrypoint is passed, never derived.** Given none, the driver manager derives
`AdbcDuckdbInit` from `libduckdb.so` and falls back to `AdbcDriverInit`, and `DuckDB` defines
neither: `duckdb_adbc_init` is its one C-linkage ADBC name, the symbol the linked route declares
too.

# Errors

`CoreError` where the named path is unusable (empty or relative), where the library does not
load, or where its initialisation refused.

## `use parameter_batch`

One question's values, as the batch this driver binds from - or `None` where there are none.

**`None` rather than an empty batch, and the driver decides that too:** `newRecordReader` takes
the `runPlainQuery` path when nothing is bound, and an empty-schema batch with one row would
instead take the parameter path and set `query.Parameters` to an empty slice. The two are not the
same request. Every boot-path call - `verify_anchor`, the identity read, a fixture load - carries
no values and must take the plain path.

# Errors

`ArrowError` where Arrow refuses the batch. Unreachable as written - the schema and
the columns are built from one walk of one slice, so their lengths and types agree by
construction - and answered for rather than unwrapped, because `unwrap_used` is denied and a
panic here would be process death under `panic = "abort"`. Each adapter wraps this into its
own error type; the failure is on THIS side of the C ABI and the vocabulary is the adapter's.

## `use DriverLocation`

Where the driver is, once something has decided that it is reachable at all.

**There is no third state and no `Option`.** A source cannot be opened without one of these,
so a composition root either resolved a driver or refused to serve - the shape
`sutura_exec_bigquery::transport::JobIdentity` uses for the same reason.

## `use UnusableDriverPath`

Why a named driver path is not one this process will open.

## `constant LINKS_POSTGRES_DRIVER`

Whether this artefact's own link carries the PostgreSQL driver, asked without initialising it.
