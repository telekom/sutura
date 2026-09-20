<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-tls

The public API of `sutura-tls`, rendered from rustdoc JSON.

Reading a declared outbound trust anchor and client identity into loadable `rustls` material.

**Extracted out of `sutura-exec-postgres/src/tls.rs`**, which is where this logic first landed
and where its own header used to record why it stayed there: *"The two crates do not share a
dependency, so this module's input is the RESOLVED material a composition root extracted from the
declaration."* That argument is about `sutura-config` - a settings crate parses a path and never
reads it, so the file read belongs to whichever adapter opens the connection - and it says nothing
about two *adapters* sharing the read. `github.com/telekom/sutura#125`'s remaining half needs the
same bundle-or-system-store read a second time, for the `BigQuery` wire, and copying
`bundle_roots`/`system_roots`/the identity loaders a second time is exactly the duplication
`AGENTS.md` asks not to hold twice.

# What is shared, and what deliberately is not

This crate reads bytes and returns `rustls_pki_types::CertificateDer` /
`rustls_pki_types::PrivateKeyDer` - the DER material every rustls-based consumer starts from,
and the exact type a `rustls`-depending caller already has: `rustls` itself re-exports this crate
verbatim as `rustls::pki_types` (`pub use pki_types::*;`), so nothing converts at the seam. This
crate builds no `RootCertStore` and installs no crypto provider, because neither is shared:

- `sutura-exec-postgres::tls` folds the returned certificates into a `RootCertStore` (the step
  that also catches a certificate rustls itself cannot use as a root) and builds a
  `rustls::ClientConfig` with the `ring` provider IT already depends on, for
  `tokio-postgres-rustls`.
- A `ureq`-based adapter turns the same `CertificateDer` bytes into `ureq::tls::Certificate` (via
  `Certificate::from_der(der.as_ref()).to_owned()`) and hands `RootCerts::Specific` to
  `ureq::tls::TlsConfig` - the workspace's own pinned `ureq` takes that shape directly, so no new
  outbound HTTP client enters the graph for this.

So the crate this loader lives in depends on `rustls-pki-types` (not `rustls` itself - that pulls
the `ring` provider on this workspace's feature pin, and this crate must not) and
`rustls-native-certs`, and nothing that names a network client or a crypto provider - a data
system's own outbound wire chooses those, not this. `cargo tree -p sutura-tls -e normal -i ring`
prints nothing, held by `xtask/src/boundaries.rs`'s `FORBIDDEN_EDGES` entry naming this pair.

# What refuses here, and why it is fail-closed the same way twice

- A bundle path that cannot be read, or reads to no certificates at all: a store of nothing
  verifies nothing.
- The host store (`transport_anchors: system` / `security.outbound.transport_anchors: system`)
  read with any reported error, or with no certificates: the upstream reader reports a PARTIAL
  read as certificates plus errors, and accepting the certificates alone would make `system` mean
  a silently reduced store.
- A client identity certificate that cannot be read or holds no certificate, or a key that cannot
  be read or does not parse as a private key this build can present.

An untrusted-issuer chain is never refused here - verification is the handshake's job, and a
caller that folds these bytes into its own verifier is exactly the thing that refuses it.

## `enum Anchors`

```rust
pub enum Anchors
```

Where a declared trust anchor bundle is read from.

### Variants

- `Bundle` - A PEM bundle at this absolute path.
- `System` - The host's own trust store, read once.

### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## `struct Identity`

```rust
pub struct Identity
```

The client certificate and key a `mutual` channel presents, as paths to read.

A pair - a caller has already refused a partial one before reaching this crate (both
`sutura_config::sources::transport::ClientIdentity` and any deployment-wide equivalent are pairs
by construction), so this type carries no partial state either.

### Methods

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

### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## `enum LoadError`

```rust
pub enum LoadError
```

Why a declared anchor or identity could not be loaded.

### Variants

- `AnchorsRead` - The declared trust anchors could not be read or parsed.
- `AnchorsEmpty` - The declared trust anchors parsed to no certificates.
- `SystemStoreRead` - The explicitly selected host trust store could not be read completely.
- `SystemStoreEmpty` - The explicitly selected host trust store held no roots.
- `IdentityRead` - The declared client identity could not be read.
- `IdentityIncomplete` - The declared client certificate parsed to no certificate, or the key to no key.
- `IdentityKey` - The client key was not an RSA/EC key this build can present.

### Implements

`Debug`, `Display`, `Error`

## `struct LoadedAnchors`

```rust
pub struct LoadedAnchors
```

A loaded trust-anchor set - never empty, by construction.

The property `load_anchors` promises is now a type rather than a comment at the call site: a
store of nothing verifies nothing, and `LoadedAnchors::parse` is the only constructor, refusing
an empty list with the caller's own refusal (`AnchorsEmpty` for a bundle, `SystemStoreEmpty` for
the host store) rather than letting each source repeat the check.

**`Clone`, unlike `LoadedIdentity`.** `CertificateDer` is public material by construction (a
certificate, never a key), and a deployment-wide declaration is read ONCE and then handed to every
fixed-host client that needs it - `github.com/telekom/sutura#125`'s `security.outbound` covers the
`BigQuery` wire and the STS exchange from a single boot-time read, so a composition root needs one
loaded value it can give to more than one `crate`-external constructor without re-reading the
bundle or the host store per call site.

### Implements

`Clone`, `Debug`, `IntoIterator`

## `struct LoadedIdentity`

```rust
pub struct LoadedIdentity
```

A loaded client identity: the certificate chain, and the private key for it.

Private fields behind named accessors, not a tuple and not `pub` fields - a struct literal built
from outside this crate could pair any chain with any key, which is exactly the invariant
`load_identity` exists to hold (each half read from the SAME declared `Identity`).

### Methods

```rust
pub fn chain(&self) -> &[CertificateDer<'static>]
```

The certificate chain, for inspection without giving up the key.

```rust
pub fn into_parts(self) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)
```

The chain and the key, consumed together - `PrivateKeyDer` implements no `Clone`, so there is
no `&self` accessor for it that would not lie about ownership.

```rust
pub fn key_kind(&self) -> KeyKind
```

The private key's DER encoding kind - see `KeyKind`.

**Why this exists rather than a caller matching `PrivateKeyDer` itself.** Matching
`rustls_pki_types::PrivateKeyDer`'s variant directly would make `rustls-pki-types` a
dependency of every caller merely to ask which kind a key is - `sutura_exec_bigquery`'s own
`wire::tls::client_cert` needs exactly this, to choose the PEM label a re-armored key is
written under (`ureq`'s own key type recovers its kind from that label, not from a value a
caller passes it). This crate already depends on `rustls-pki-types` for the read; this
method is the answer in this crate's own vocabulary.

### Implements

`Debug`

## `enum KeyKind`

```rust
pub enum KeyKind
```

The three private-key DER encodings `load_identity` can present - `LoadedIdentity::key_kind`.

### Variants

- `Pkcs1` - PKCS#1 (RSA).
- `Sec1` - SEC1 (EC).
- `Pkcs8` - PKCS#8.

### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## `struct Declared`

```rust
pub struct Declared
```

A resolved `security.outbound` declaration: the anchors always present once the block is
written, and the optional client identity beside them - `github.com/telekom/sutura#911`.

**Why one type and not two independently-optional parameters.** A composition root threads
this from boot to every fixed-host consumer (the `BigQuery` wire, the `datahub` reader); an
identity can never be declared without anchors (`security.outbound` always requires
`transport_anchors` once the block itself exists), so pairing them is a type that cannot
disagree with that invariant rather than two values a call site could thread inconsistently.

### Methods

```rust
pub const fn anchors(&self) -> &Anchors
```

The declared anchors.

```rust
pub const fn identity(&self) -> Option<&Identity>
```

The declared client identity, if any.

```rust
pub fn into_parts(self) -> (Anchors, Option<Identity>)
```

Consumes into its parts - what `Rotator::new` takes separately.

```rust
pub const fn new(anchors: Anchors, identity: Option<Identity>) -> Self
```

Pairs a resolved anchor declaration with its optional client identity.

### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## `fn load_anchors`

```rust
pub fn load_anchors(anchors: &Anchors) -> Result<LoadedAnchors, LoadError>
```

Loads the declared trust anchors as raw certificate DER, refusing an empty or unreadable store.

# Errors

`LoadError::AnchorsRead` for a bundle that cannot be read or parsed;
`LoadError::AnchorsEmpty` for a bundle that parses to no certificates;
`LoadError::SystemStoreRead`/`LoadError::SystemStoreEmpty` for a host store that cannot
supply a complete, non-empty set.

## `fn load_identity`

```rust
pub fn load_identity(identity: &Identity) -> Result<LoadedIdentity, LoadError>
```

Loads the declared client identity, refusing a half that cannot be read or does not hold its
kind.

# Errors

`LoadError::IdentityRead` for a half that cannot be read; `LoadError::IdentityIncomplete` for
a certificate file with no certificate; `LoadError::IdentityKey` for a key file that does not
parse as a private key.

## `use Outcome`

What one look at the declared material decided.

## `use POLL_INTERVAL`

## `use Rotating`

The read side a consumer holds: clones out the in-use `T`.

`Clone` is sharing, not copying: every clone observes the same channel, so a rotation adopted by
`poll_once` reaches every consumer that holds a clone. `current()` is an `Arc` clone under a
`watch` borrow - no lock a request path waits on. `Debug` prints no path.

## `use Rotator`

The poll handle a composition root drives on `POLL_INTERVAL`.

Owns the declared source (so it can re-read it), the `sender` half of the channel `Rotating`
reads, the rebuild closure (the consumer's own materialization), and the `seen` marker that makes
identical bytes silent. `poll_once` is synchronous, like `sutura-http::tls::Renewal::poll_once`;
*starting* the loop is the composition root's choice, and this type does not spawn.
