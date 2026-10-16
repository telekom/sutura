<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-oracle-connect

The public API of `sutura-oracle-connect`, rendered from rustdoc JSON.

The Oracle connector `sutura-exec-oracle` dials a source through and `sutura-catalog-rdbms`'s
Oracle reader dials its documentation schema through: one EZCONNECT address, one TLS posture,
one dial bound and one driver configuration, so the two dials cannot drift apart. It cannot live
in the exec adapter, because a catalog adapter may not reach `sutura-sql` and the exec adapter
renders through it.

# Limits

- **TLS verifies against the declared anchors only.** `Channel::Verified` hands the driver
  PEM certificates that REPLACE its bundled public certificate authorities. The driver builds
  its own `rustls::ClientConfig`, so a host store and a client certificate have nothing here to
  reach.
- **The bound covers the TCP connect only.** The driver reads the TLS and TNS handshakes with no
  timeout, so a listener that accepts and then never answers is not bounded by `DIAL_DEADLINE`.
- **A listener's redirect is refused**: the driver is told not to follow one, so the connection
  stays on the declared address.

## `enum Channel`

```rust
pub enum Channel<'pem>
```

How the connection to the listener is secured.

### Variants

- `Plaintext` - No transport security. No composition root builds this; the adapter's own unit cells do.
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
pub fn config(self) -> Result<oracledb::Config, DialError>
```

The driver's configuration for this dial, with no credential: its address, its channel and
anchors, the dial bound, and a refused redirect.

# Errors

`DialError::PacketTraceOn` while the driver's packet trace is switched on, before anything
else; otherwise `DialError::Prepare` for what `Dial::prepare` refuses.

```rust
pub const fn deadline(&self) -> std::time::Duration
```

The bound on the TCP connect.

```rust
pub const fn new(host: &'dial str, port: u16, service_name: &'dial str, channel: Channel<'dial>) -> Self
```

A dial bounded by `DIAL_DEADLINE`.

```rust
pub fn prepare(self) -> Result<PreparedDial, PrepareError>
```

The same configuration as `Dial::config`, parsed once and asking the packet-trace refusal
at each `PreparedDial::config` instead.

# Errors

`PrepareError::TrustAnchors` for anchors that are not usable certificates, and
`PrepareError::Address` for an address the driver cannot parse.

```rust
pub const fn within(self, deadline: std::time::Duration) -> Self
```

The same dial with its TCP connect bounded by `deadline` instead.

### Implements

`Clone`, `Copy`, `Debug`

## `enum DialError`

```rust
pub enum DialError
```

Why a `Dial` has no driver configuration.

### Variants

- `PacketTraceOn` - The driver's packet trace is switched on - see `crate::oracle_trace::refuse`.
- `Prepare` - The dial's anchors or address are refused - see `Dial::prepare`.

### Implements

`Debug`, `Display`, `Error`

## `enum PrepareError`

```rust
pub enum PrepareError
```

Why a `Dial` cannot be prepared.

### Variants

- `TrustAnchors` - The declared anchors are not PEM certificates the driver can verify a server against.
- `Address` - The driver cannot parse the address.

### Implements

`Debug`, `Display`, `Error`

## `struct PreparedDial`

```rust
pub struct PreparedDial
```

A `Dial` whose address and anchors the driver has parsed, kept so that each connect clones
them instead of parsing them again.

### Methods

```rust
pub fn config(&self) -> Result<oracledb::Config, crate::oracle_trace::PacketTraceOn>
```

The driver's configuration for one connect, with no credential.

# Errors

`crate::oracle_trace::PacketTraceOn` while the driver's packet trace is switched on.

### Implements

`Clone`, `Debug`

## `fn ezconnect`

```rust
pub fn ezconnect(host: &str, port: u16, service_name: &str) -> String
```

An EZCONNECT `host:port/service_name`, with an IPv6 literal in brackets.

The driver's EZCONNECT parser reads a host as a bracketed literal or a run of name characters,
so an unbracketed `::1:1521/FREEPDB1` is not an EZCONNECT string to it at all: it falls through
to a `tnsnames.ora` alias lookup and fails - or, where a configuration directory is set in the
environment, resolves whatever that file names. Bracketing is decided on a parsed address, not
on the presence of a `:`, so a name is never wrapped.

## `constant DIAL_DEADLINE`

How long the TCP connect to the listener may take before the dial is refused.

The bound covers the connect only: a listener that accepts and then never answers is not bounded
by it, because the driver reads its handshake with no timeout.

## Module `oracle_trace`

The vendored Oracle driver's packet trace, which no Oracle dial in this workspace runs under.

The variable is a process global: every `crate::Dial::config` asks `refuse` before it
builds a configuration, and the composition root asks it before any command.

### `struct PacketTraceOn`

```rust
pub struct PacketTraceOn
```

The driver's packet trace is switched on.

#### Implements

`Debug`, `Display`, `Error`

### `fn refuse`

```rust
pub fn refuse() -> Result<(), PacketTraceOn>
```

Refuses while the Oracle driver's packet trace is switched on.

# Errors

`PacketTraceOn` while the variable is set, at any value.
