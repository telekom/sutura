<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-postgres

The public API of `sutura-exec-postgres`, rendered from rustdoc JSON.

A `Warehouse` adapter over PostgreSQL.

One connection, under the deployment's declared identity (`SharedServiceUser`). The static
half of Postgres: no OAuth, no impersonation. That is row 18.

Two things this adapter proves:

1. **The artifact question.** `tokio-postgres` is pure Rust and links nothing, so Postgres is
   the first source whose driver does not pull a native library.
2. **Acceptance.** `DuckDB` was the only data system that showed a rendered statement is *accepted*
   rather than just parsed. This makes it two, over the wire.

Rendering is `sutura-sql`'s (`generate(plan, Dialect::Postgres)`); the parameters are bound by
hand. Nothing here is compiled or translated.

The client is async; the `Warehouse` port is not, so this adapter owns a `tokio` runtime and
`block_on`s each call - the engine's own pattern. It was chosen over `sqlx` because its
protocol and SASL support underpin the OAuth work already done against a live Postgres, which
keeps row 18's door open.

## Limits

- `NoTls`, unconditional. SCRAM protects the password, not the rows; a `hostssl`-only server
  refuses this connection. Fine for a localhost tier (all this ships for). TLS is a change to
  `connect`.
- The corpus cells run only where a tier is provisioned, and skip loudly elsewhere. The signal
  is `SUTURA_DEV_REQUIRE_TIER`, not `CI` - whoever provisions the tier sets it and gets
  fail-closed.
- A `statement_timeout` is set at connect so a slow server statement cannot hold a blocking-pool
  thread past the caller's request deadline.

## `enum PostgresError`

```rust
pub enum PostgresError
```

Why this data system could not answer.

### Variants

- `Runtime`
- `Connect`
- `Prepare`
- `Execute`
- `DivisionByZero` - The server refused a statement as `division by zero` (SQLSTATE `22012`).
- `NumericNotCarryable` - A `NUMERIC` wider than this build can carry exactly.
- `UnsupportedType` - A column came back as a type this adapter does not map.
- `NotFinite` - A floating-point (or `NUMERIC`) column came back as a value that is not a number.
- `NotADate` - A day came back that is not a date this build can represent.
- `Shape`
- `Render`
- `Fixture` - A fixture import failed.
- `FixtureRead`
- `InvalidColumnName` - A CSV header named a column that is not a valid identifier. A fixture is repo-committed, so there is no live hole - but `load_csv` is a `pub` DDL renderer, and a name reaching the `CREATE TABLE` / `COPY` unparsed is the shape "a document read off disk gets the treatment a question off the wire gets" is about.
- `InvalidSchemaName` - A schema name this adapter was asked to open that is not a word, so it is refused rather than interpolated into `CREATE SCHEMA`.
- `NoPlaceForASubject` - The credential broker handed this adapter subject material it has nowhere to put.
- `PresentedDisagreesWithPosture`
- `LegWithoutCombiner` - One leg of a federated answer, which nothing here can assemble above.

### Implements

`Debug`, `Display`, `Error`

## `struct PostgresWarehouse`

```rust
pub struct PostgresWarehouse
```

A `PostgreSQL` connection, behind the `Warehouse` port.

### Methods

```rust
pub fn connect(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, config: &tokio_postgres::Config) -> Result<Self, PostgresError>
```

Opens a connection under the supplied `tokio_postgres::Config` - host and the ephemeral port
the compose tier allocated, user, password and database - and keeps one connection under it.

```rust
pub fn connect_in_schema(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, config: &tokio_postgres::Config, schema: &str) -> Result<Self, PostgresError>
```

Opens a connection whose every unqualified table name resolves to a fresh, private schema.

The corpus runs several warehouses against ONE shared Postgres, in parallel threads; a shared
schema would let one cell's drop-and-recreate clobber another mid-query. A per-connection
schema makes each cell's tables its own.

The schema name is caller-supplied (a corpus-generated name), so it is validated to a word
before it reaches `CREATE SCHEMA`.

```rust
pub fn load_csv(&self, table: &TableName, path: &Path) -> Result<(), PostgresError>
```

Exposes a fixture CSV as a table: infer column types from the values, drop and recreate the
table, then push the rows through `COPY ... FROM STDIN`. Re-inferring every run from a
committed CSV cannot drift from it, and recreating makes a run idempotent.

```rust
pub fn local_config(host: &str, port: u16) -> tokio_postgres::Config
```

A connection config for the fixture tier, honouring the same `SUTURA_DEV_*` overrides the
compose file reads, so a host that objects to a weak default can change one value.

### Implements

`Debug`, `Warehouse`
