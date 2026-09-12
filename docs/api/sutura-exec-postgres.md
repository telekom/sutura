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

- **No transport of its own.** `PostgresWarehouse::connect` opens with no TLS at all; the
  verifying path is `PostgresWarehouse::connect_secured`, which takes the
  `rustls::ClientConfig` a composition root built from the declared channel
  (`tls::client_config`). Which source gets which is `sutura_config::sources::transport`'s
  decision and never this adapter's, so a caller that builds no config gets a cleartext
  connection - including to a server that offers TLS.
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
- `UnsupportedType` - A column came back as a type this adapter does not map. An error, not a stringified value.
- `NotFinite` - A floating-point (or `NUMERIC`) column came back as a value that is not a number.
- `NotADate` - A day came back that is not a date this build can represent.
- `Shape`
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.
- `Render`
- `Fixture` - A fixture import failed.
- `FixtureRead`
- `InvalidColumnName` - A CSV header named a column that is not a valid identifier. Refused, not interpolated.
- `FixtureSchema` - The shared conformance fixture schema could not be inferred.
- `InvalidSchemaName` - A schema name this adapter was asked to open that is not a word. Refused, not interpolated.
- `InvalidStatementTimeout` - The dev-only `statement_timeout` tuning value is not a `u32` millisecond count.
- `NoPlaceForASubject` - The credential broker handed this adapter subject material it has nowhere to put.
- `PresentedDisagreesWithPosture`
- `LegWithoutCombiner` - A leg without a combiner.
- `AnchorsRead` - The declared trust anchors could not be read or parsed.
- `AnchorsEmpty` - The declared trust anchors parsed to no certificates.
- `IdentityRead` - The declared client identity could not be read.
- `IdentityIncomplete` - The declared client certificate parsed to no certificate, or the key to no key.
- `IdentityKey` - The client key was not an RSA/EC key this build can present.
- `SystemStoreRead` - The explicitly selected host trust store could not be read completely.
- `SystemStoreEmpty` - The explicitly selected host trust store held no roots.
- `SystemStoreCertificate` - A certificate returned by the host trust-store reader was not a usable root.
- `TlsConfiguration` - The cryptographic provider could not construct a client verifier.

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
adapter's life, over no transport security. The fixture tier's path (unix socket, loopback),
and the composition root's `plaintext` choice - the caller has already refused a
non-loopback plaintext host.

```rust
pub fn connect_in_schema(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, config: &tokio_postgres::Config, schema: &str) -> Result<Self, PostgresError>
```

Like `connect`, but every unqualified table name resolves to a fresh,
private schema - so several warehouses can share one Postgres without clobbering each other.
The caller-supplied schema name is validated to a word before it reaches `CREATE SCHEMA`.

```rust
pub fn connect_secured(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, config: &tokio_postgres::Config, tls: Option<rustls::ClientConfig>) -> Result<Self, PostgresError>
```

Opens one connection under the supplied `config`, secured as the caller resolved.

`tls` is `None` for a `plaintext` channel and a ready-built `rustls::ClientConfig` for
`verified` and `mutual` channels. Both are produced by the composition root, which is the
only place that can see the declared `sutura_config::sources::transport::SourceTransport` -
this adapter takes the resolved material rather than a second copy of the three-state shape.

```rust
pub fn load_csv(&self, table: &TableName, path: &Path) -> Result<(), PostgresError>
```

Exposes a fixture CSV as a table: infers column types, recreates the table, then pushes the
rows through `COPY ... FROM STDIN`. Re-inferring from the committed CSV each run cannot
drift from it, and recreating makes a run idempotent.

```rust
pub fn load_fixture_csv(&self, table: &TableName, path: &Path) -> Result<(), PostgresError>
```

Exposes a conformance fixture with the same exact types as the other adapter bindings.

Available only with the default-off `fixtures` feature.

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

## Module `connection`

The driver configuration for one declared PostgreSQL connection.

A composition root owns mapping its source declaration into these values. This module owns the
driver-specific half: how TCP and unix-socket targets are represented to `tokio-postgres`, and
reading the password file once at boot. Keeping that here means both shipped composition roots
reach the same driver behaviour without depending on each other.

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

The declared password file could not be read while the connection was built.

#### Implements

`Debug`, `Display`, `Error`

### `fn config`

```rust
pub fn config(target: ConnectionTarget<'_>, port: u16, database: &str, user: &str, password_file: &std::path::Path) -> Result<tokio_postgres::Config, PasswordFileUnreadable>
```

Builds the driver configuration for one declared PostgreSQL connection.

The password is trimmed exactly once after reading, so a trailing newline from a mounted secret
is not part of the credential, then parsed into `Secret`. The read `String` is shadowed by
that `Secret`, not dropped - it is not zeroised, and it lives unzeroised until this function
returns. The returned config does not select TLS; `crate::PostgresWarehouse::connect_secured`
makes a supplied TLS client mandatory before it dials.

# Errors

Returns `PasswordFileUnreadable` when `password_file` cannot be read.

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

## Module `tls`

Building the `rustls::ClientConfig` a TLS `postgres` source channel verifies with.

This is the TLS half of `sutura_config::sources::transport`, turned into a verifier. That module
owns the three-state DECLARATION (`plaintext` / `verified` / `mutual`); this one owns turning a
declared `verified` or `mutual` channel into the thing the driver connects with: read the anchor
store, read the optional client identity, and refuse the combinations a closed type refuses.

The two crates do not share a dependency, so this module's input is the RESOLVED material a
composition root extracted from the declaration - the same boundary `connect_secured`'s own
signature draws, and the reason the adapters here never carry a second copy of the three-state
shape. What a composition root hands this module is: whether the declared anchors are a PEM
bundle or the host's system store, and an optional client identity path pair.

# What fails here, and why it is a connect-time refusal

Configuration refuses what only a tree can see (an unknown `transport_mode` word, TLS naming no
anchors, a partial identity, a relative path). What this module refuses is what only a file and
a TLS implementation can answer - and each refusal is fail-closed and names the path:

* anchors that cannot be read (`PostgresError::AnchorsRead`) or parse to no certificates
  (`PostgresError::AnchorsEmpty`);
* a `system` store that cannot be read completely or contains no usable roots
  (`PostgresError::SystemStoreRead`, `PostgresError::SystemStoreEmpty`);
* an identity half that cannot be read (`PostgresError::IdentityRead`) or parses to the wrong
  kind (`PostgresError::IdentityIncomplete`, `PostgresError::IdentityKey`).

An untrusted-issuer chain is not refused HERE: verification is the handshake's job, and a
`ClientConfig` built over the declared roots is exactly the thing that refuses it. The
tier-backed test that connects a source to a server under an unTRUSTED issuer is refused by
`PostgresWarehouse::connect_secured`'s `PostgresError::Connect` arm at the handshake, while the
construction half stays honest about what it can know: a `ClientConfig` whose roots are the
declared file.

The construction is always compiled (this crate is the source channel), so `checks.nextest`
exercises every refusal above in-crate against `rcgen`-generated material, and `tests/tls.rs`
drives the same construction against the tier's real server - where the two cells are that the
declared anchor verifies and an issuer it does not name is refused.

### `enum TlsAnchors`

```rust
pub enum TlsAnchors
```

The trust anchors a TLS source channel verifies against, resolved from the declaration.

#### Variants

- `Bundle` - A PEM bundle at this absolute path. Read by `client_config` once, at boot.
- `System` - The host's own trust store, read once by `client_config`. This is reached only when the deployment explicitly wrote `transport_anchors: system`; it is never a fallback.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct TlsIdentity`

```rust
pub struct TlsIdentity
```

The client certificate and key a `mutual` channel presents, resolved from the declaration.

A pair - configuration already refused a partial one at load; this module reads both paths and
refuses a file that does not hold its half.

#### Methods

```rust
pub fn certificate(&self) -> &Path
```

The declared client certificate path.

```rust
pub fn key(&self) -> &Path
```

The declared client key path.

```rust
pub const fn new(certificate: PathBuf, key: PathBuf) -> Self
```

A client identity from its declared paths.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `fn client_config`

```rust
pub fn client_config(anchors: &TlsAnchors, identity: Option<&TlsIdentity>) -> Result<rustls::ClientConfig, crate::PostgresError>
```

Builds the `rustls::ClientConfig` a TLS source channel verifies (and, for `mutual`, presents)
with, from the resolved anchor material and an optional client identity.

# Errors

`SystemStoreRead`/`SystemStoreEmpty` for a host store that cannot supply a complete non-empty
root set; `AnchorsRead` for a bundle that cannot be read; `AnchorsEmpty` for a bundle that parses
to no certificates; `IdentityRead`/`IdentityIncomplete`/`IdentityKey` for an identity half that
cannot be read or does not hold its kind.
