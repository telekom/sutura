<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-oracle

The public API of `sutura-exec-oracle`, rendered from rustdoc JSON.

A `Warehouse` adapter over Oracle Database. `github.com/telekom/sutura#127` PR 2, over PR 1's
`Dialect::Oracle` rendering.

**Two postures.** A `shared-service-user` source answers every question on one connection under
the deployment's declared user. An `impersonation-at-source` source opens a session of its own
for each question, with the asker's own verified token (`TokenSessions`), so the database
authenticates the asker and runs the statement as the user it maps that token to. The boot
connection under the declared user stays, for the boot path's own probes only
(`Warehouse::verify_anchor`, `Warehouse::declared_key`); a question never runs on it.

**Synchronous.** `oracledb::Connection`'s own
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

- **TLS verifies against the declared anchors only.** `Channel::Verified` hands the driver
  PEM certificates that REPLACE its bundled public certificate authorities, so a server is
  admitted only under a certificate those anchors issue. The driver still builds its own
  `rustls::ClientConfig`, so the architecture decision's host store (`transport_anchors: system`) and a client
  certificate (`mutual`) have nothing here to reach, and `sutura-config` refuses both on a
  `kind: oracle` source.
- **A listener's redirect is refused before authentication**, as
  `OracleError::RedirectRefused`: the driver is told not to follow one, so the connection
  stays on the address the source declared. A clustered listener that redirects every client is
  therefore refused too; declare the address that answers.
- **One `parking_lot::Mutex` serializes every call**, and for good reason:
  `Connection`'s own methods
  take `&self`, so the port's shared reference alone does not prove the driver tolerates two
  overlapping calls - and nothing here measured that it does.
- **The session lifecycle, and the bound on sessions.** A question at an impersonating source
  opens its session under that lock, runs on it, and closes it before the lock is released, so a
  source holds at most one asker's session at a time beside its boot connection, and no session
  outlives the question or serves a second asker. The dial bound covers each session's TCP
  connect; the handshake and the close after it are not bounded, so a database that stops
  answering there holds the source.
- **The token is copied once, into the driver's configuration**, as an ordinary `String` the
  driver keeps masked; nothing here zeroes that copy.

## `enum OracleError`

```rust
pub enum OracleError
```

Why this data system could not answer.

### Variants

- `TrustAnchors` - The declared trust anchors are not PEM certificates the driver can verify a server against.
- `Connect`
- `RedirectRefused` - The listener answered with a redirect. Every redirect is refused before authentication: the address it names is one no source declared, so no declared transport governs it.
- `Execute`
- `DivisionByZero` - The server refused a statement as `ORA-01476: divisor is equal to zero`.
- `UnsupportedType` - A column came back as a type this adapter does not map. An error, not a stringified value.
- `ValueDecode` - A value this adapter asked the driver to decode did not decode.
- `NotFinite` - A `BINARY_DOUBLE` column came back as a value that is not a number.
- `Shape`
- `OverBudget` - The collected result would cost more than this adapter's materialisation budget to hold.

  The sibling of `Self::Shape` for the byte budget the port's
  `result_did_not_fit` reads: a
  result refused for crossing it is *the result did not fit*, never a data-system failure, so
  a caller is refused rather than told to retry.
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.

  A defect in the rendering or in this adapter's value mapping, never anything about the data:
  the probe projects two aggregates over no group, so one row of two integers is the only
  shape it can have. It travels as an `Err` from the port, which the boot path reads as *this
  declaration went unchecked* rather than as a violated one.
- `Render`
- `Undeliverable` - The credential broker handed this adapter a leg it cannot open a session for.
- `PresentedDisagreesWithPosture`
- `DeadlineSpent` - The deadline was already spent before this call ever reached the driver - see `refuse_if_spent` for why this is checked rather than forwarded.
- `CallTimeout` - `Connection::set_call_timeout` itself refused the value.
- `PacketTraceOn` - The driver's packet trace is switched on - see `refuse_packet_trace`.

### Implements

`Debug`, `Display`, `Error`

## `struct OracleWarehouse`

```rust
pub struct OracleWarehouse
```

An Oracle connection, behind the `Warehouse` port.

### Methods

```rust
pub fn connect(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, dial: Dial<'_>, user: &str, password: &str, result_budget: sutura_domain::warehouse::ResultBudget) -> Result<Self, OracleError>
```

Opens one connection to the listener `dial` names, under the declared user.

A redirect from the listener is refused before authentication as
`OracleError::RedirectRefused`, so the connection stays on the declared address. For an
`impersonation-at-source` `posture` the same dial, with no user or password, is kept to open
each question's own session.

```rust
pub fn connect_fixture(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, host: &str, port: u16, credential: &fixture::FixtureCredential, result_budget: sutura_domain::warehouse::ResultBudget) -> Result<Self, OracleError>
```

A connection config's host/port/credential for the fixture tier - the counterpart of
`sutura_exec_postgres::adbc::Conninfo`. `service_name` is fixed at
`FREEPDB1`, the community image's own pluggable database, which is not a secret the tier
publishes - it is the image's name for itself.

### Implements

`Debug`, `Warehouse`

## `enum Channel`

```rust
pub enum Channel<'pem>
```

How the connection to the listener is secured.

### Variants

- `Plaintext` - No transport security. A composition root reaches this only for a loopback host.
- `Verified` - TLS, verified against these PEM certificates and no others: they replace the driver's bundled public certificate authorities rather than adding to them.

### Implements

`Clone`, `Copy`, `Debug`

## `struct Dial`

```rust
pub struct Dial<'dial>
```

Where one connection goes and how it is secured: an EZCONNECT `host:port/service_name`.

### Methods

```rust
pub const fn new(host: &'dial str, port: u16, service_name: &'dial str, channel: Channel<'dial>) -> Self
```

A dial bounded by `DIAL_DEADLINE`.

```rust
pub const fn within(self, deadline: std::time::Duration) -> Self
```

The same dial with its TCP connect bounded by `deadline` instead.

### Implements

`Clone`, `Copy`, `Debug`

## `struct TokenSessions`

```rust
pub struct TokenSessions
```

The declared dial with no user or password: where each question's own session is opened, with
the asker's own token, at an `impersonation-at-source` source.

The driver refuses a token over anything but TLS, so a `Channel::Plaintext` dial opens no
session; the settings parse refuses that declaration before a dial exists.

### Methods

```rust
pub fn new(dial: Dial<'_>) -> Result<Self, OracleError>
```

The driver's configuration for `dial`: its address, its channel and anchors, the dial bound, and a refused redirect.

# Errors

`OracleError::PacketTraceOn` while the driver's packet trace is switched on,
`OracleError::TrustAnchors` for anchors that are not usable certificates, and
`OracleError::Connect` for an address the driver cannot parse.

```rust
pub fn open(&self, token: &Secret) -> Result<oracledb::Connection, OracleError>
```

Opens one session that authenticates with `token`. Dropping it closes the session.

# Errors

`OracleError::RedirectRefused` for a listener that redirects, and `OracleError::Connect`
for every other refused dial or login. Neither carries the token.

### Implements

`Clone`, `Debug`

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

## `use refuse_packet_trace`

Refuses while the driver's packet trace is switched on. Every dial this adapter makes asks here
first.

# Errors

`OracleError::PacketTraceOn` while the variable is set.

## `constant DIAL_DEADLINE`

How long the TCP connect to the listener may take before the dial is refused.

The bound covers the connect only: a listener that accepts and then never answers is not bounded
by it, because the driver reads its handshake with no timeout.

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
