<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-postgres

The public API of `sutura-exec-postgres`, rendered from rustdoc JSON.

A `Warehouse` adapter over PostgreSQL, through the ADBC
driver - `adbc::AdbcPostgres`, one connection string under the deployment's declared identity
(`SharedServiceUser`). The static half of Postgres: no OAuth, no impersonation.

**ADBC is the only transport** (`telekom/sutura#913` stage 2). The driver is the self-built
`libadbc_driver_postgresql` (`nix/postgres-adbc.nix`): linked into every musl release, mounted
from `SUTURA_POSTGRES_ADBC_DRIVER` everywhere else. SQL renders through `sutura-sql`
(`Dialect::Postgres`); nothing here is compiled or translated. `adbc`'s header says what holds
the channel, the deadline and the single-statement guarantee, and what does not.

## Limits

- **One declared identity, only.** A Postgres source signs in solely as the deployment's
  declared shared service account: a password, or one Kerberos principal from the deployment's
  keytab. OAuth and per-caller sign-in are not supported: `AdbcPostgres::IMPERSONATION` is
  `NoPlaceForASubject`, and `Conninfo` pins `require_auth` to `password,md5,scram-sha-256,none`
  and `gssencmode` to `disable` everywhere but `Conninfo::kerberos`, which writes
  `require_auth='gss'` and the `gssencmode` its declaration names. No settings key selects
  Kerberos yet.
- **`transport_anchors: system` is refused** (`adbc::UnusableChannel::HostStore`): libpq's
  `system` store is OpenSSL's compiled-in default, not the host store sutura reads.

## `enum PostgresError`

```rust
pub enum PostgresError
```

Why this data system could not answer.

### Variants

- `DivisionByZero` - The server refused a statement as `division by zero` (SQLSTATE `22012`) - how Postgres honours `zero_denominator: fails`, kept typed rather than folded into `Self::Adbc`.
- `UnsupportedType` - A column came back as a type this adapter does not map. An error, not a stringified value.
- `NotFinite` - A floating-point (or `NUMERIC`) column came back as a value that is not a number.
- `NotADate` - A day came back that is not a date this build can represent.
- `Shape`
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.

  A defect in the rendering or in this adapter's value mapping rather than anything about the
  data - two aggregates over no group produce one row of two integers - and it travels as an
  `Err` from the port, which the boot path reads as *this declaration went unchecked*.
- `Render`
- `Fixture` - A fixture import failed at the server.
- `FixtureRead`
- `InvalidColumnName` - A CSV header named a column that is not a valid identifier. Refused, not interpolated.
- `FixtureSchema` - The shared conformance fixture schema could not be inferred.
- `InvalidStatementTimeout` - The dev-only `statement_timeout` tuning value is not a `u32` millisecond count.

  The value becomes a `SET LOCAL statement_timeout = N` ceiling, so it is parsed at the
  boundary and refused if it is not a number or exceeds the `u32` ceiling - a value that
  cannot be a timeout must not reach the statement as uninterpreted text. The cause survives
  so the operator sees the number did not parse, not a plain refusal.
- `DeadlineSpent` - The deadline was already spent before anything was sent - refused locally, no round trip.
- `Adbc` - The ADBC transport's own failure; its refusals before any SQL are the variants above.
- `NoPlaceForASubject` - The credential broker handed this adapter subject material it has nowhere to put.
- `PresentedDisagreesWithPosture`

### Implements

`Debug`, `Display`, `Error`

## Module `adbc`

The PostgreSQL adapter's one transport, the second adapter on `sutura-adbc` (`telekom/sutura#913`).

The `Warehouse` port over the self-built driver
(`nix/postgres-adbc.nix`), answering with `PostgresError`. A certified
answer's Arrow batches are handed on as they arrive, except a `NUMERIC` column, which `numeric`
re-reads per cell. **Every `kind: postgres` source is answered here**: the composition root
constructs `AdbcPostgres` for each, over the driver this artefact
links (both musl triples) or the one `SUTURA_POSTGRES_ADBC_DRIVER` names.

# What holds what

- **Caller text reaches the driver only through `execute`.** The pinned driver
  (`apache-arrow-adbc-24`) sends a parameterless `execute_update` through `PQexec`, the simple
  protocol, which runs every statement of a multi-statement string; `execute` asks for a result
  stream and goes through `PQprepare`, where the server refuses a second statement at `Parse`.
  So `session`'s `Setting::apply` is the one shipped `execute_update` here, its text is fixed
  literals and a `NonZeroU32` (the `fixtures` loader is the other, in no
  release), and `clippy.toml` bans every other call
  in the workspace - except one written inside an existing `disallowed_methods` expectation's
  scope, which that expectation covers too (the ban's own entry states it).
- **The per-request deadline is `SET LOCAL statement_timeout`** in the transaction the driver
  opens when autocommit is switched off, clamped to the deployment's ceiling
  (`SUTURA_DEV_STATEMENT_TIMEOUT_MS`), and always rolled back. A statement the server cancelled for it
  (`57014`), or a stream that failed once that timeout had run out, is the deadline to
  `Warehouse::deadline_exceeded`.
- **The channel is the declared one.** `Conninfo` builds the libpq
  connection string from the declaration and classifies every libpq keyword, pinning each one the
  environment could weaken; its own header carries what it refuses and what it cannot pin.
- **The driver is the linked one where there is one.**
  `PostgresDriver` is the archive this artefact links (both musl
  triples) or a mounted `.so` (every other build).

# Limits

- **`NUMERIC` is read exactly** (`numeric`): scale-zero text that fits an `i64` is an integer,
  the rest exact text - except a domain over it, which the driver tags with the domain's name and
  so stays text. `tests/types.rs` holds each mapped type against the tier through the real driver.
- **The cells here are a fake connection**; the integration tests are what reach a server through
  a real driver (mounted on a host, linked in `nix/shipped.nix`'s musl test build). The server
  refusing a multi-statement string at `Parse` is held there; the driver's `BEGIN` on
  autocommit-off is read off its source and observed only through `SET LOCAL` taking effect.
  `tests/kerberos.rs`'s Kerberos sign-in and its refused negative control (a declared service the
  KDC does not know) run through the linked `x86_64` musl driver, in its CI venue alone.
- **Loading and connecting are outside the deadline**: the driver is loaded and a connection
  opened per call, and only the statement runs under `SET LOCAL`.
- **Every port method.** `session`'s header says what each sends.
- **One declared identity.** A source signs in only as its declared shared service account: with
  a password or a client certificate, or as the one Kerberos principal the process's named
  credential cache holds, when `Conninfo::kerberos` declares it
  (the linked libpq through a static MIT krb5, `nix/postgres-adbc.nix`). No settings key selects
  Kerberos yet. OAuth and per-caller sign-in are not supported: `Conninfo`
  refuses SSPI and OAuth on either driver, and the linked libpq is built without libcurl.

### `enum AdbcError`

```rust
pub enum AdbcError
```

Why this transport could not answer.

#### Variants

- `Load`
- `Adbc`
- `Batch`
- `TimedOut` - The stream failed once the statement's timeout had run out - the server cancelling it, read by the clock where the stream carries no SQLSTATE (`session::timed_out`).
- `Unannounced`
- `Parameters`
- `Unreadable`
- `DeadlineSpent` - Spent once the connection was open - refused locally, as `PostgresError::DeadlineSpent`.

#### Implements

`Debug`, `Display`, `Error`

### `struct PostgresDriver`

```rust
pub struct PostgresDriver
```

Where the PostgreSQL driver comes from: this artefact's own link, or a mounted `.so`.

Not `sutura_adbc::DriverLocation`, whose linked route is the `BigQuery` archive; a mounted path is
parsed by it, so an empty or relative one is refused exactly as for every ADBC adapter.

#### Methods

```rust
pub fn from_host() -> Result<Self, NoDriver>
```

The driver this process opens: the one this artefact links, else the one
`MOUNTED_DRIVER` names.

# Errors

`NoDriver` where neither is there, or the named path is not one.

```rust
pub fn linked_in() -> Option<Self>
```

The driver this artefact links, or `None` where it links none.

```rust
pub fn parse(named: &str) -> Result<Self, UnusableDriverPath>
```

Parses a mounted driver's path.

# Errors

`UnusableDriverPath` for an empty or relative path. Whether a driver is there is the
load's question, asked by the first call.

```rust
pub fn probe(&self) -> Result<(), AdbcError>
```

Loads and initialises the driver, opening no database - what `sutura doctor` asks.

# Errors

`AdbcError::Load`, from either route.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum NoDriver`

```rust
pub enum NoDriver
```

Why this process has no PostgreSQL driver to open.

#### Variants

- `Unset`
- `Unusable`

#### Implements

`Debug`, `Display`, `Error`

### `struct AdbcPostgres`

```rust
pub struct AdbcPostgres
```

A PostgreSQL source reached through its ADBC driver, behind the `Warehouse` port.

#### Methods

```rust
pub fn create_schema(&self, schema: &str) -> Result<(), PostgresError>
```

Creates `schema` if it is not there - the per-cell isolation a fixture opens with
`Conninfo::in_schema`.

# Errors

`PostgresError::Fixture` where the server refused it, and the driver's load or connect.

```rust
pub fn load_csv(&self, table: &sutura_domain::model::TableName, path: &std::path::Path) -> Result<(), PostgresError>
```

Exposes a CSV as a table: infers column types, recreates the table, then inserts the rows -
re-inferred from the committed file on every run, so it cannot drift from it.

# Errors

`PostgresError::FixtureRead`, `PostgresError::InvalidColumnName`, or
`PostgresError::Fixture` where the server refused the load.

```rust
pub fn load_fixture_csv(&self, table: &sutura_domain::model::TableName, path: &std::path::Path) -> Result<(), PostgresError>
```

Exposes a conformance fixture with the same exact types as the other adapter bindings.

# Errors

`PostgresError::FixtureRead`, `PostgresError::FixtureSchema`, or
`PostgresError::Fixture` where the server refused the load.

```rust
pub fn new(source: SourceName, posture: SourcePosture, driver: PostgresDriver, conninfo: Conninfo) -> Result<Self, PostgresError>
```

Takes the source, the driver and the connection string it connects with.

Reads the deployment's statement-timeout ceiling (`SUTURA_DEV_STATEMENT_TIMEOUT_MS`), which a
request's own budget may only narrow.

# Errors

`PostgresError::InvalidStatementTimeout` where that tuning value is not a millisecond count.

#### Implements

`Debug`, `Warehouse`

### `struct FixtureAdmin`

```rust
pub struct FixtureAdmin
```

A plain second connection to the fixture tier, for what no port method may do.

It creates a view, or holds a lock inside a transaction it leaves open until it is dropped. It
runs whatever it is handed, through the simple protocol - which is why it exists only under the
`fixtures` feature.

#### Methods

```rust
pub fn open(driver: &PostgresDriver, conninfo: &Conninfo) -> Result<Self, AdbcError>
```

One connection over `conninfo`.

# Errors

The driver's load or connect.

```rust
pub fn run(&mut self, sql: &str) -> Result<(), AdbcError>
```

Runs `sql`, every statement in it, on this connection.

# Errors

`AdbcError::Adbc` where the server refused it.

### `use Channel`

How the channel to the source is secured, as the composition root resolved the declaration.

### `use Conninfo`

The connection string for one source. Only `Conninfo::new` and `Conninfo::kerberos` make
one, and its `Debug` is the `Secret`'s, so the password it carries is never printed.

### `use GssEncryption`

Whether GSSAPI encrypts the channel - libpq's `gssencmode`.

### `use InvalidKerberosService`

A declared Kerberos service name that is not one.

### `use Kerberos`

A Kerberos sign-in through GSSAPI, as the declaration names it.

### `use KerberosService`

The service half of the server's principal, `<service>/<host>` - libpq's `krbsrvname`.

### `use UnusableChannel`

A declared connection the ADBC transport cannot hold to, refused before anything dials.

### `use UnusableDriverPath`

### `constant MOUNTED_DRIVER`

The variable a host that links no driver names a mounted one with.

**Not a settings key**: which driver file a host carries is a property of the host rather than of
the semantic deployment, and a release artefact that links one never reads it - the order
`bigquery_driver` in `sutura-cli` gives for the other ADBC adapter, for its reason: a mounted
path must not be able to displace the driver a published artefact carries.

## Module `connection`

What a composition root reads out of one PostgreSQL declaration before a `Conninfo` is built:
the address it dials and the password, read once at boot.

### `enum ConnectionTarget`

```rust
pub enum ConnectionTarget<'a>
```

The address a PostgreSQL source is dialled through.

#### Variants

- `Host` - A TCP host name or address.
- `UnixSocket` - A unix socket directory.

#### Implements

`Clone`, `Copy`

### `struct PasswordFileUnreadable`

```rust
pub struct PasswordFileUnreadable
```

The declared password file could not be read.

#### Implements

`Debug`, `Display`, `Error`

### `fn read_password`

```rust
pub fn read_password(password_file: &std::path::Path) -> Result<sutura_domain::identity::Secret, PasswordFileUnreadable>
```

Reads the declared password file into a `Secret`.

The password is trimmed exactly once after reading, so a trailing newline from a mounted secret
is not part of the credential. The read `String` is shadowed by that `Secret`, not dropped - it
is not zeroised, and it lives unzeroised until this function returns (`docs/adr/0020`'s "not
claimed" list).

# Errors

`PasswordFileUnreadable` when `password_file` cannot be read.

## Module `fixture`

The fixture tier's credential and its loader - a value that cannot exist unconfigured.

**Behind the default-off `fixtures` feature**, because every caller is a test and
`nix/shipped.nix` builds cargo's DEFAULT set: so no artefact a release publishes contains this
module, which deletes the *reachable from a consumer* half rather than hardening it.
`--all-features` compiles, lints and tests it on every run.
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
`FixtureCredential::conninfo` can be handed - it is unrepresentable rather than rejected -
and there is no branch left for a fallback to live in.

# Where the values come from, because nothing set them before

**The provisioner publishes them, and it is not the compose file.** There is no Postgres
service in `compose.services.yaml` at all - this tier is nixpkgs' `postgresql_18`, started by
`nix/postgres-tier.nix`, which now generates a password per worktree and prints the three
credential exports from `sutura-postgres-tier credentials`, plus the loopback listener's CA and
client pair. Those are read by the TLS cells rather than here.
`nix/with-tier.sh` evaluates them exactly where it already exports `SUTURA_DEV_REQUIRE_TIER`, so
*the server is there* and *the client knows how to log in* cannot come apart, and
`checks.postgres-tier` drives that subcommand's two answers.

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
`sutura_domain::identity::Secret`, which has neither `Display` nor `==`, and the values leave
this crate only inside a `Conninfo`.

#### Methods

```rust
pub fn as_role(&self, role: &str) -> Self
```

The same credential, signing in as `role` - the tier's certificate-authenticated role.

```rust
pub fn conninfo(&self, source: &SourceName, host: &str, port: u16, channel: Channel<'_>) -> Result<Conninfo, UnusableChannel>
```

The tier at `host:port` as this credential, over `channel`.

# Errors

`Conninfo::new`'s.

```rust
pub fn conninfo_in(&self, source: &SourceName, host: &str, port: u16, schema: &str) -> Result<Conninfo, UnusableChannel>
```

The tier at `host:port` as this credential, every unqualified name resolved in `schema`.

# Errors

`Conninfo::in_schema`'s.

```rust
pub fn from_env() -> Result<Self, UnconfiguredFixture>
```

This process's credential, as the tier exported it.

The production entry point, and a one-line adapter over `FixtureCredential::parse` on
purpose - see that function for why the seam is there.

#### Implements

`Clone`, `Debug`
