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
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.
- `Render`
- `Fixture` - A fixture import failed.
- `FixtureRead`
- `InvalidColumnName` - A CSV header named a column that is not a valid identifier. Refused, not interpolated.
- `InvalidSchemaName` - A schema name this adapter was asked to open that is not a word. Refused, not interpolated.
- `InvalidStatementTimeout` - The dev-only `statement_timeout` tuning value is not a `u32` millisecond count.
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
pub fn local_config(host: &str, port: u16, credential: &fixture::FixtureCredential) -> tokio_postgres::Config
```

A connection config for the fixture tier, over a credential that has already been parsed.

**It TAKES the credential and reads no environment of its own**, which is the whole change:
`host` and `port` are parameters, so this function cannot know it is talking to an ephemeral
local server, and the shape it replaced offered `sutura`/`sutura`/`sutura` to whatever host
it was handed whenever nothing was set. There is no unconfigured state to substitute for now
- `fixture::FixtureCredential` cannot hold one - so this stays infallible.

### Implements

`Debug`, `Warehouse`

## Module `fixture`

The fixture tier's credential - a value that cannot exist unconfigured.

**Behind the default-off `fixtures` feature**, because both callers are tests
(`crates/sutura-exec-postgres/tests/conformance.rs` and
`crates/sutura-app/tests/adapters/mod.rs`) and `nix/shipped.nix` builds cargo's DEFAULT set: so
no artefact a release publishes contains this module or the connection config over it, which
deletes the *reachable from a consumer* half rather than hardening it. `--all-features` compiles,
lints and tests it on every run.
The fixture tier's credential: **configured, or refused by name.**

# What this replaces, and why the old shape was wrong in a way its strength cannot fix

`local_config` used to read `SUTURA_DEV_USER` / `SUTURA_DEV_PASSWORD` / `SUTURA_DEV_DB` through
an `unwrap_or_else(|_| "sutura")` fallback, in a `pub fn` that was neither `#[cfg(test)]` nor
feature-gated. The reasoning beside the compose file - *these are fixtures, nothing outside the
host can reach them, every port is published ephemerally* - is sound, and it is sound **about a
container.** It does not reach a function whose `host` and `port` are PARAMETERS: this code
cannot know it is talking to an ephemeral local server, so the property *"that credential only
ever reaches one"* was held by the function's name and by the discipline of its callers.
`AGENTS.md`: invariants are held by a type, a lint, a hook or a gate, never by recall.

And the doc comment inverted the default - *"so a host that objects to a weak default can change
one value"* makes hardening **opt-in**. That inversion is the defect, more than the strength of
the password: a weak default a caller must object to is the opposite of secure by design.

# The shape now

`crate::fixture::FixtureCredential` is the only way to hold one, its three values are private, and
`FixtureCredential::parse` is the only way in. So *unconfigured* is not a value
`PostgresWarehouse::local_config` can be handed - it is unrepresentable rather than rejected -
and there is no branch left for a fallback to live in.

# Where the values come from, because nothing set them before

**The provisioner publishes them, and it is not the compose file.** There is no Postgres
service in `compose.services.yaml` at all - this tier is nixpkgs' `postgresql_18`, started by
`nix/postgres-tier.nix`, which now generates a password per worktree and prints the three
exports from `sutura-postgres-tier credentials`. `nix/with-tier.sh` evaluates them exactly where
it already exports `SUTURA_DEV_REQUIRE_TIER`, so *the server is there* and *the client knows how
to log in* cannot come apart, and `checks.postgres-tier` drives that subcommand's two answers.

The variables are `SUTURA_POSTGRES_TIER_*` rather than the old `SUTURA_DEV_*`: the names were
shared with the compose fixture credential while nothing in that file provisions a Postgres, and
a shared name is what made a coupling look real. The compose anchors keep their own defaults and
their own argument.

# Two limits, stated with the claim

- **A unix socket under this tier authenticates by `trust`.** `nix/postgres-tier.nix` runs
  `initdb` with no `--auth`, so the password is not what admits a client to *that* server. What
  the refusal buys is that no OTHER host is offered a guessable one, which is the half the old
  `pub fn` could not hold.
- **The role name and the database name are still `sutura`.** They are names the provisioner
  CHOOSES and publishes, not values a client guesses when nothing is set - which is the
  distinction this module is about. Only the password is generated.

### `enum FixtureVariable`

```rust
pub enum FixtureVariable
```

One of the three values the Postgres fixture tier publishes into the environment.

An enum rather than a `&'static str`, because a refusal has to carry WHICH value was missing as
data: a caller that read the variable's name out of the message would be matching on prose.

#### Variants

- `User` - The login role the tier created.
- `Password` - That role's password, generated per worktree by `nix/postgres-tier.nix`.
- `Database` - The database the tier created and owns.

#### Methods

```rust
pub const fn name(self) -> &'static str
```

The environment variable's name, spelled once.

One definition, because a refusal that tells somebody to set a name the reader spells
differently is a fix that does not work and looks like it should.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Hash`, `PartialEq`

### `enum UnconfiguredFixture`

```rust
pub enum UnconfiguredFixture
```

Why there is no fixture credential to connect with.

**Two variants, and no third that carries a substitute.** The failure is that this process was
not told the credential, and the only honest answers to it are *say which variable* and *stop* -
so there is nowhere in this type for a default to be returned from.

#### Variants

- `Unset` - The variable is not in the environment at all.
- `Blank` - The variable is set to nothing, which a shell script exporting `""` is the ordinary way to reach. Read as a credential it is a login attempt as the empty user, so it is refused here rather than at the server.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct FixtureCredential`

```rust
pub struct FixtureCredential
```

The fixture tier's credential.

Exists only if all three values were present, so nothing downstream asks again. No `Display`,
no `PartialEq`, no public field and no public accessor: the password is a
`sutura_domain::identity::Secret`, which has neither `Display` nor `==`, and the two names
leave this crate only as arguments to `PostgresWarehouse::local_config`.

#### Methods

```rust
pub fn from_env() -> Result<Self, UnconfiguredFixture>
```

This process's credential, as the tier exported it.

The production entry point, and a one-line adapter over `FixtureCredential::parse` on
purpose - see that function for why the seam is there.

#### Implements

`Debug`
