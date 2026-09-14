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

This crate reads bytes and returns `rustls::pki_types::CertificateDer` /
`rustls::pki_types::PrivateKeyDer` - the DER material every rustls-based consumer starts from.
It builds no `rustls::RootCertStore` and installs no crypto provider, because neither is shared:

- `sutura-exec-postgres::tls` folds the returned certificates into a `RootCertStore` (the step
  that also catches a certificate rustls itself cannot use as a root) and builds a
  `rustls::ClientConfig` with the `ring` provider it already depends on, for
  `tokio-postgres-rustls`.
- A `ureq`-based adapter turns the same `CertificateDer` bytes into `ureq::tls::Certificate` (via
  `Certificate::from_der(der.as_ref()).to_owned()`) and hands `RootCerts::Specific` to
  `ureq::tls::TlsConfig` - the workspace's own pinned `ureq` takes that shape directly, so no new
  outbound HTTP client enters the graph for this.

So the crate this loader lives in depends on `rustls` (for `pki_types` only) and
`rustls-native-certs`, and nothing that names a network client or a crypto provider - a data
system's own outbound wire chooses those, not this.

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

## `fn load_anchors`

```rust
pub fn load_anchors(anchors: &Anchors) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, LoadError>
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

## `type_alias LoadedIdentity`

A loaded client identity: the certificate chain, and the private key for it.

Named rather than left as a bare tuple - `clippy::type_complexity` is over the workspace's own
threshold at the return position, and a name is also what a caller destructures against instead
of a positional `.0`/`.1`.
