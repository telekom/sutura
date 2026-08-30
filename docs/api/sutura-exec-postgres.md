<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-postgres

The public API of `sutura-exec-postgres`, rendered from rustdoc JSON.

A `Warehouse` adapter over `PostgreSQL`, reached over the wire.

The static-credential half of a Postgres data system: this adapter holds one connection under the
shared service identity a deployment declared - the `SharedServiceUser` posture the example ships -
and nothing here exchanges a caller's token for a per-subject grant. The OAuth half is deliberately
out of scope (`docs/adr/0008`, `docs/adr/0014`); row 18, where a source executes as the asking
subject, changes this adapter rather than arriving beside it.

## The deployability limit, stated so the title is not read as more than it is

The connection is **`NoTls`**, unconditionally: SCRAM keeps the password off the wire, but every
row travels in plaintext, and a server configured `hostssl`-only refuses this adapter outright.
That is the right shape for a localhost compose tier (which is all this ships for), and the row
in `AGENTS.md` carries the same sentence. What a deployment wants before a real Postgres is
reached is TLS at the very least, which is a change to `connect`.

Why it exists at all is the two rows in `AGENTS.md` it answers:

- **The artifact question, in code.** `tokio-postgres` is pure Rust and links nothing, so Postgres
  is the first source whose driver does not pull a native library - which is what keeps the musl
  cross-build matrix a non-issue. `duckdb` is a dev-dependency for exactly the opposite reason
  (nixpkgs has no musl `libduckdb`).
- **A second data system vouching for acceptance, over the wire for the first time.** `DuckDB` was
  the only thing confirming that a rendered statement is not just well formed but accepted, and
  parse-checked is explicitly narrower than accepted. This adapter makes it two, over the Postgres
  protocol.

Rendering is `sutura-sql`'s job, exactly as it is for `sutura-exec-duckdb`: this adapter asks
`generate(plan, Dialect::Postgres)` and binds the parameters by hand. It compiles nothing.

## Why `tokio-postgres`, chosen and recorded

The issue this row is answering leaves the client open, subject to "the choice is also row 18's
inheritance". `sqlx` and `tokio-postgres` both owe nothing at link time. `tokio-postgres` is the
one whose protocol- and SASL-support underpins the OAuth verification already done against a live
Postgres, so choosing it keeps that door open with the least churn. The cost is that the
`Warehouse` port is SYNCHRONOUS, so this adapter owns a `tokio` runtime and `block_on`s each
call - the engine's own "it holds its runtime" precedent, applied to a driver rather than to a
plan executor.

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
- `NumericNotCarryable` - A `NUMERIC` whose exact value is wider than this build can carry.
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

 The corpus runs several independent warehouses against ONE shared Postgres, in parallel
 threads. If two of them loaded the same tables into the same schema, one dropping and
 recreating a table would clobber the other mid-query. A per-connection schema makes each cell
's tables its own, so the cells cannot collide - which is the same reason the repository gives
 each worktree its own compose project.

 The schema name is taken on trust from the caller here (a name a corpus generated), so it is
 validated to a word character to keep the `CREATE SCHEMA` from becoming an injection.

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
