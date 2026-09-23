<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-oracle

The public API of `sutura-exec-oracle`, rendered from rustdoc JSON.

A `Warehouse` adapter over Oracle Database - one connection under the deployment's declared
identity (`SharedServiceUser`). `github.com/telekom/sutura#127` PR 2, over PR 1's
`Dialect::Oracle` rendering.

**Synchronous, unlike `sutura_exec_postgres::PostgresWarehouse`.** `oracledb::Connection`'s own
methods (`execute`, `query`, `set_call_timeout`) are plain blocking `fn`s over a
`std::net::TcpStream` - measured by reading `oracle/rust-oracledb`'s own source, not assumed -
so this adapter owns no `tokio` runtime and calls the driver directly. `oracledb` is
`#![forbid(unsafe_code)]` with no `build.rs` and no `links` key: nothing here can `dlopen` an
Instant Client the way an ODPI-C wrapper would, which was `#127`'s original blocker.

SQL renders through `sutura-sql` (`Dialect::Oracle`); nothing here is compiled or translated.

## The one caveat this crate exists to hold, per the spike behind `#127`

`Connection::set_call_timeout` is a PER-READ idle timeout (`TcpStream::set_read_timeout`), not a
total-call budget, and there is no cancellation API - a slow statement that keeps the socket
busy is not stopped by it. Worse, `sutura_domain::warehouse::deadline::Deadline::remaining_at`
returns `None` when the deadline is EXPIRED, and `set_call_timeout(None)` means *wait forever* -
so naively forwarding `deadline.remaining_at(now)` into `set_call_timeout` turns an expired
deadline into an unbounded wait, the opposite of a refusal. `refuse_if_spent` is the guard:
every call site asks it FIRST and never calls the driver, let alone `set_call_timeout`, once it
answers `None`. This module's own `#[cfg(test)]` cell,
`an_expired_deadline_refuses_before_reaching_the_connection`, is the refusal;
`a_live_deadline_answers_the_remaining_duration` is its predicate half.

## Limits

- **No transport of its own, and less than `sutura_exec_postgres` carries.** ADR 0010's
  declared-trust-store rule (a bundle path, or the host store, resolved once by a composition
  root and handed to the adapter as a `rustls::ClientConfig`) has NOWHERE to attach here:
  `oracledb::Connection` builds its OWN `rustls::ClientConfig` internally, from a wallet
  directory's `ewallet.pem` when one is configured and from the bundled `webpki-roots` set when
  one is not (measured by reading `oracle/rust-oracledb/src/transport.rs`) - there is no
  constructor that takes an external root store or a caller-built `ClientConfig` at all. So
  `OracleWarehouse::connect_secured` takes a wallet directory rather than the
  `sutura_tls::Rotating<rustls::ClientConfig>` handle Postgres's own `connect_secured` takes, and
  a `transport_anchors: system` declaration has nothing on this adapter to reach: there is no
  "read the host trust store" option in the driver at all. This is a real fork in ADR 0010, not
  an oversight - and it is why `sutura-config` refuses any `transport_mode` but `plaintext` on
  a `kind: oracle` source, and its shared rule confines `plaintext` to a loopback host.
  `OracleWarehouse::connect_secured` therefore has no composition-root caller.
- **Wired behind a default-off feature, and in no release.** `sutura-cli`'s `oracle` feature
  links this crate into both composition roots through `OracleWarehouse::connect`;
  `nix/shipped.nix` does not carry that feature - see its entry in `sutura-cli`'s manifest.
- **Every mapping below is reasoned from the driver's documented wire types, not measured
  against a live Oracle** - no docker socket was available while this adapter was written. The
  golden matrix's `oracle` cells (`crates/sutura-app/tests/adapters/adapters.rs`) skip rather
  than run wherever that is still true - that file's own `DataSystemUnderTest::available`
  decides which venues those are.
- **No venue that runs `just validate` can reach a live Oracle.** `compose.services.yaml`'s
  `oracle` service is a docker-compose tier brought up by hand (`just dev-up-oracle`); the nix
  sandbox has no docker socket and no `oracle-tier.nix` exists, so a gate leg cannot
  provision one - and Oracle Database is proprietary, so no nix-native tier could take
  `nix/postgres-tier.nix`'s shape even in principle. The render goldens this suite pins for
  Oracle therefore assert what `sutura-sql` emitted and nothing a data system said back; that
  is what `crates/sutura-app/tests/golden/dialects.rs`'s `Venue::ByHandOnly` arm declares. The
  check there holds this path, never this prose - a header that stops arguing this stays green.
- **One `parking_lot::Mutex` serializes every call**, the same shape
  `PostgresWarehouse::execution_lock` holds and for a matching reason: `Connection`'s own methods
  take `&self`, so the port's shared reference alone does not prove the driver tolerates two
  overlapping calls - and nothing here measured that it does.

## `enum OracleError`

```rust
pub enum OracleError
```

Why this data system could not answer.

### Variants

- `Connect`
- `Execute`
- `DivisionByZero` - The server refused a statement as `ORA-01476: divisor is equal to zero`.
- `UnsupportedType` - A column came back as a type this adapter does not map. An error, not a stringified value.
- `ValueDecode` - A value this adapter asked the driver to decode did not decode.
- `NotFinite` - A `BINARY_DOUBLE` column came back as a value that is not a number.
- `Shape`
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.
- `Render`
- `NoPlaceForASubject` - The credential broker handed this adapter subject material it has nowhere to put.
- `PresentedDisagreesWithPosture`
- `DeadlineSpent` - The deadline was already spent before this call ever reached the driver - see `refuse_if_spent` for why this is checked rather than forwarded.
- `CallTimeout` - `Connection::set_call_timeout` itself refused the value.

### Implements

`Debug`, `Display`, `Error`

## `struct OracleWarehouse`

```rust
pub struct OracleWarehouse
```

An Oracle connection, behind the `Warehouse` port.

### Methods

```rust
pub fn connect(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, host: &str, port: u16, service_name: &str, user: &str, password: &str) -> Result<Self, OracleError>
```

Opens one connection over a plain TCP EZCONNECT string (`host:port/service_name`), with no
transport security at all.

```rust
pub fn connect_fixture(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, host: &str, port: u16, credential: &fixture::FixtureCredential) -> Result<Self, OracleError>
```

A connection config's host/port/credential for the fixture tier - the counterpart of
`sutura_exec_postgres::PostgresWarehouse::local_config`. `service_name` is fixed at
`FREEPDB1`, the community image's own pluggable database, which is not a secret the tier
publishes - it is the image's name for itself.

```rust
pub fn connect_secured(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, host: &str, port: u16, service_name: &str, user: &str, password: &str, wallet: Option<&OracleWallet>) -> Result<Self, OracleError>
```

Opens one connection over `tcps://host:port/service_name`, with the driver's own wallet-based
TLS - see the module header's limit on how far this reaches ADR 0010's declared-trust-store
rule. `wallet` is a directory containing an `ewallet.pem`; `None` verifies against the
driver's bundled `webpki-roots` set rather than against a declared anchor.

### Implements

`Debug`, `Warehouse`

## `struct DriverError`

```rust
pub struct DriverError
```

Wraps `oracledb::Error` so it can be a `thiserror` `#[source]`.

**Measured, not assumed:** `oracledb::Error` implements `Debug` and `Display` but not
`std::error::Error` - `oracledb::error::ErrorKind` and `DbError` are its own typed detail, and
nothing upstream ties the type into `core::error::Error`'s chain. `#[source]` needs that trait,
and there is no orphan-rule obstacle to implementing it for a LOCAL wrapper around a foreign
type, so this is the newtype rather than a second, string-only error shape.

### Implements

`Debug`, `Display`, `Error`

## `struct OracleWallet`

```rust
pub struct OracleWallet
```

A wallet directory for `OracleWarehouse::connect_secured` - a path to a directory containing
`ewallet.pem`, and the password protecting the private key inside it (if any).

### Methods

```rust
pub fn at(location: impl Into<String>, password: impl Into<String>) -> Self
```

## Module `fixture`

The fixture tier's credential - a value that cannot exist unconfigured.

Behind the default-off `fixtures` feature - see that module's own header for why.
The fixture tier's credential: **configured, or refused by name.**

Behind the default-off `fixtures` feature - `sutura-exec-postgres::fixture`'s own reason, one
size smaller: nothing a release publishes links this crate at all (see the workspace manifest's
member-list entry), so the feature is about keeping a `pub fn` that reads `std::env::var` out of
the default cargo feature set rather than about hiding a shipped artefact's credential.

**`SUTURA_DEV_USER`/`SUTURA_DEV_PASSWORD`, not a third-tier name.** `compose.services.yaml`'s
`oracle` service reuses the SAME `x-fixture-credentials` anchor every compose-only service in
that file shares - `clickhouse` included - because this tier, unlike Postgres, is provisioned by
that file rather than by a nix-native script. `sutura_exec_postgres::fixture` deliberately does
NOT share that name, for the opposite reason: nothing in `compose.services.yaml` provisions a
Postgres, so a shared name there would have implied a coupling that did not exist. Here the
coupling is real - the compose block IS the definition - so sharing the name is the honest
spelling rather than a third naming scheme nobody chose.

### `enum UnconfiguredFixture`

```rust
pub enum UnconfiguredFixture
```

Why there is no fixture credential to connect with.

#### Variants

- `Unset` - The variable is not in the environment at all.
- `Blank` - The variable is set to nothing - a shell script exporting `""` is the ordinary way to reach this, and read as a credential it is a login attempt as the empty user.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct FixtureCredential`

```rust
pub struct FixtureCredential
```

The fixture tier's credential.

No `Display`, no `PartialEq`, no public field: the password is a
`sutura_domain::identity::Secret`, which has neither, and the two names leave this crate only
as arguments to `crate::OracleWarehouse::local_config`.

#### Methods

```rust
pub fn from_env() -> Result<Self, UnconfiguredFixture>
```

This process's credential, as `compose.services.yaml`'s anchors export it.

#### Implements

`Debug`
