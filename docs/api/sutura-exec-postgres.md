<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-postgres

The public API of `sutura-exec-postgres`, rendered from rustdoc JSON.

A `Warehouse` adapter over PostgreSQL - one connection under the deployment's declared
identity (`SharedServiceUser`). The static half of Postgres: no OAuth, no impersonation.

The client is async and the `Warehouse` port is not, so this adapter owns a `tokio` runtime
and `block_on`s each call. `tokio-postgres` is pure Rust and links nothing native. SQL renders
through `sutura-sql` (`Dialect::Postgres`); nothing here is compiled or translated.

## Limits

- `NoTls`, unconditional: a `hostssl`-only server refuses this connection.
- A `statement_timeout` is set at connect, so a slow server statement cannot hold a
  blocking-pool thread past the caller's request deadline.

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
- `NumericNotCarryable` - A `NUMERIC` wider than this build can carry exactly. Refused, not rounded.
- `UnsupportedType` - A column came back as a type this adapter does not map. An error, not a stringified value.
- `NotFinite` - A floating-point (or `NUMERIC`) column came back as a value that is not a number.
- `NotADate` - A day came back that is not a date this build can represent.
- `Shape`
- `Render`
- `Fixture` - A fixture import failed.
- `FixtureRead`
- `InvalidColumnName` - A CSV header named a column that is not a valid identifier. Refused, not interpolated.
- `InvalidSchemaName` - A schema name this adapter was asked to open that is not a word. Refused, not interpolated.
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

Opens one connection under the supplied `tokio_postgres::Config` and keeps it for this
adapter's life.

```rust
pub fn connect_in_schema(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, config: &tokio_postgres::Config, schema: &str) -> Result<Self, PostgresError>
```

Like `connect`, but every unqualified table name resolves to a fresh,
private schema - so several warehouses can share one Postgres without clobbering each other.
The caller-supplied schema name is validated to a word before it reaches `CREATE SCHEMA`.

```rust
pub fn load_csv(&self, table: &TableName, path: &Path) -> Result<(), PostgresError>
```

Exposes a fixture CSV as a table: infers column types, recreates the table, then pushes the
rows through `COPY ... FROM STDIN`. Re-inferring from the committed CSV each run cannot
drift from it, and recreating makes a run idempotent.

```rust
pub fn local_config(host: &str, port: u16) -> tokio_postgres::Config
```

A connection config for the fixture tier, honouring the `SUTURA_DEV_*` overrides the
compose file reads, so a host that objects to a weak default can change one value.

### Implements

`Debug`, `Warehouse`
