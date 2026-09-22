#![forbid(unsafe_code)]
//! Reading a declared outbound trust anchor and client identity into loadable `rustls` material.
//!
//! **Extracted out of `sutura-exec-postgres/src/tls.rs`**, which is where this logic first landed
//! and where its own header used to record why it stayed there: *"The two crates do not share a
//! dependency, so this module's input is the RESOLVED material a composition root extracted from the
//! declaration."* That argument is about `sutura-config` - a settings crate parses a path and never
//! reads it, so the file read belongs to whichever adapter opens the connection - and it says nothing
//! about two *adapters* sharing the read. `github.com/telekom/sutura#125`'s remaining half needs the
//! same bundle-or-system-store read a second time, for the `BigQuery` wire, and copying
//! `bundle_roots`/`system_roots`/the identity loaders a second time is exactly the duplication
//! `AGENTS.md` asks not to hold twice.
//!
//! # What is shared, and what deliberately is not
//!
//! This crate reads bytes and returns [`rustls_pki_types::CertificateDer`] /
//! [`rustls_pki_types::PrivateKeyDer`] - the DER material every rustls-based consumer starts from,
//! and the exact type a `rustls`-depending caller already has: `rustls` itself re-exports this crate
//! verbatim as `rustls::pki_types` (`pub use pki_types::*;`), so nothing converts at the seam. This
//! crate builds no `RootCertStore` and installs no crypto provider, because neither is shared:
//!
//! - `sutura-exec-postgres::tls` folds the returned certificates into a `RootCertStore` (the step
//!   that also catches a certificate rustls itself cannot use as a root) and builds a
//!   `rustls::ClientConfig` with the `ring` provider IT already depends on, for
//!   `tokio-postgres-rustls`.
//! - A `ureq`-based adapter turns the same `CertificateDer` bytes into `ureq::tls::Certificate` (via
//!   `Certificate::from_der(der.as_ref()).to_owned()`) and hands `RootCerts::Specific` to
//!   `ureq::tls::TlsConfig` - the workspace's own pinned `ureq` takes that shape directly, so no new
//!   outbound HTTP client enters the graph for this.
//!
//! So the crate this loader lives in depends on `rustls-pki-types` (not `rustls` itself - that pulls
//! the `ring` provider on this workspace's feature pin, and this crate must not) and
//! `rustls-native-certs`, and nothing that names a network client or a crypto provider - a data
//! system's own outbound wire chooses those, not this. `cargo tree -p sutura-tls -e normal -i ring`
//! prints nothing, held by `xtask/src/boundaries.rs`'s `FORBIDDEN_EDGES` entry naming this pair.
//!
//! # What refuses here, and why it is fail-closed the same way twice
//!
//! - A bundle path that cannot be read, or reads to no certificates at all: a store of nothing
//!   verifies nothing.
//! - The host store (`transport_anchors: system` / `security.outbound.transport_anchors: system`)
//!   read with any reported error, or with no certificates: the upstream reader reports a PARTIAL
//!   read as certificates plus errors, and accepting the certificates alone would make `system` mean
//!   a silently reduced store.
//! - A client identity certificate that cannot be read or holds no certificate, or a key that cannot
//!   be read or does not parse as a private key this build can present.
//!
//! An untrusted-issuer chain is never refused here - verification is the handshake's job, and a
//! caller that folds these bytes into its own verifier is exactly the thing that refuses it.

use std::path::{Path, PathBuf};

use rustls_pki_types::pem::PemObject as _;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

mod rotate;
pub use rotate::{Outcome, POLL_INTERVAL, Rotating, Rotator};

/// Where a declared trust anchor bundle is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchors {
    /// A PEM bundle at this absolute path.
    Bundle(PathBuf),
    /// The host's own trust store, read once.
    System,
}

/// The client certificate and key a `mutual` channel presents, as paths to read.
///
/// A pair - a caller has already refused a partial one before reaching this crate (both
/// `sutura_config::sources::transport::ClientIdentity` and any deployment-wide equivalent are pairs
/// by construction), so this type carries no partial state either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    certificate: PathBuf,
    key: PathBuf,
}

impl Identity {
    /// A client identity from its declared paths.
    #[must_use]
    pub const fn new(certificate: PathBuf, key: PathBuf) -> Self {
        Self { certificate, key }
    }

    /// The declared client certificate path.
    #[must_use]
    pub fn certificate(&self) -> &Path {
        &self.certificate
    }

    /// The declared client key path.
    #[must_use]
    pub fn key(&self) -> &Path {
        &self.key
    }
}

/// Why a declared anchor or identity could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The declared trust anchors could not be read or parsed.
    #[error("the declared trust anchors could not be read as a PEM bundle at {path}")]
    AnchorsRead {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    /// The declared trust anchors parsed to no certificates.
    #[error("the trust-anchor bundle at {path} parsed to no certificates - a store of nothing verifies nothing")]
    AnchorsEmpty { path: String },
    /// The explicitly selected host trust store could not be read completely.
    #[error("the host trust store reported {errors} errors while it was read")]
    SystemStoreRead {
        errors: usize,
        #[source]
        cause: rustls_native_certs::Error,
    },
    /// The explicitly selected host trust store held no roots.
    #[error("the host trust store held no certificates - a store of nothing verifies nothing")]
    SystemStoreEmpty,
    /// The declared client identity could not be read.
    #[error("the declared client identity could not be read at {path}")]
    IdentityRead {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    /// The declared client certificate parsed to no certificate, or the key to no key.
    #[error("the client identity pair is incomplete: expected a certificate and a key, and found {what} at {path}")]
    IdentityIncomplete { path: String, what: &'static str },
    /// The client key was not an RSA/EC key this build can present.
    #[error("the client private key at {path} is not a private key this build can present")]
    IdentityKey { path: String, what: &'static str },
}

/// Loads the declared trust anchors as raw certificate DER, refusing an empty or unreadable store.
///
/// # Errors
///
/// [`LoadError::AnchorsRead`] for a bundle that cannot be read or parsed;
/// [`LoadError::AnchorsEmpty`] for a bundle that parses to no certificates;
/// [`LoadError::SystemStoreRead`]/[`LoadError::SystemStoreEmpty`] for a host store that cannot
/// supply a complete, non-empty set.
pub fn load_anchors(anchors: &Anchors) -> Result<LoadedAnchors, LoadError> {
    match anchors {
        Anchors::System => system_certificates(rustls_native_certs::load_native_certs()),
        Anchors::Bundle(path) => bundle_certificates(path),
    }
}

/// A loaded trust-anchor set - never empty, by construction.
///
/// The property [`load_anchors`] promises is now a type rather than a comment at the call site: a
/// store of nothing verifies nothing, and [`LoadedAnchors::parse`] is the only constructor, refusing
/// an empty list with the caller's own refusal (`AnchorsEmpty` for a bundle, `SystemStoreEmpty` for
/// the host store) rather than letting each source repeat the check.
///
/// **`Clone`, unlike [`LoadedIdentity`].** `CertificateDer` is public material by construction (a
/// certificate, never a key), and a deployment-wide declaration is read ONCE and then handed to every
/// fixed-host client that needs it - `github.com/telekom/sutura#125`'s `security.outbound` covers the
/// `BigQuery` wire and the STS exchange from a single boot-time read, so a composition root needs one
/// loaded value it can give to more than one [`crate`]-external constructor without re-reading the
/// bundle or the host store per call site.
#[derive(Debug, Clone)]
pub struct LoadedAnchors(Vec<CertificateDer<'static>>);

impl LoadedAnchors {
    /// Wraps a certificate list, refusing an empty one with `refusal`.
    fn parse(certificates: Vec<CertificateDer<'static>>, refusal: LoadError) -> Result<Self, LoadError> {
        if certificates.is_empty() {
            Err(refusal)
        } else {
            Ok(Self(certificates))
        }
    }

    /// How many certificates are loaded. Test-only: production code takes the whole set via
    /// [`IntoIterator`] and never needs the count on its own.
    #[cfg(test)]
    const fn len(&self) -> usize {
        self.0.len()
    }
}

impl IntoIterator for LoadedAnchors {
    type Item = CertificateDer<'static>;
    type IntoIter = std::vec::IntoIter<CertificateDer<'static>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// A loaded client identity: the certificate chain, and the private key for it.
///
/// Private fields behind named accessors, not a tuple and not `pub` fields - a struct literal built
/// from outside this crate could pair any chain with any key, which is exactly the invariant
/// `load_identity` exists to hold (each half read from the SAME declared [`Identity`]).
#[derive(Debug)]
pub struct LoadedIdentity {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
}

impl LoadedIdentity {
    /// The certificate chain, for inspection without giving up the key.
    #[must_use]
    pub fn chain(&self) -> &[CertificateDer<'static>] {
        &self.chain
    }

    /// The private key's DER encoding kind - see [`KeyKind`].
    ///
    /// **Why this exists rather than a caller matching `PrivateKeyDer` itself.** Matching
    /// `rustls_pki_types::PrivateKeyDer`'s variant directly would make `rustls-pki-types` a
    /// dependency of every caller merely to ask which kind a key is - `sutura_exec_bigquery`'s own
    /// `wire::tls::client_cert` needs exactly this, to choose the PEM label a re-armored key is
    /// written under (`ureq`'s own key type recovers its kind from that label, not from a value a
    /// caller passes it). This crate already depends on `rustls-pki-types` for the read; this
    /// method is the answer in this crate's own vocabulary.
    #[must_use]
    #[expect(
        clippy::unreachable,
        reason = "load_private_key's only constructor, PrivateKeyDer::from_pem_slice, recognizes \
                  exactly the three PEM section kinds matched above and nothing else parses - the \
                  wildcard exists for PrivateKeyDer's #[non_exhaustive], not for a reachable key"
    )]
    pub fn key_kind(&self) -> KeyKind {
        match &self.key {
            PrivateKeyDer::Pkcs1(_) => KeyKind::Pkcs1,
            PrivateKeyDer::Sec1(_) => KeyKind::Sec1,
            PrivateKeyDer::Pkcs8(_) => KeyKind::Pkcs8,
            _ => unreachable!("PrivateKeyDer::from_pem_slice produces only Pkcs1/Sec1/Pkcs8"),
        }
    }

    /// The chain and the key, consumed together - `PrivateKeyDer` implements no `Clone`, so there is
    /// no `&self` accessor for it that would not lie about ownership.
    #[must_use]
    pub fn into_parts(self) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        (self.chain, self.key)
    }
}

/// The three private-key DER encodings [`load_identity`] can present - [`LoadedIdentity::key_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// PKCS#1 (RSA).
    Pkcs1,
    /// SEC1 (EC).
    Sec1,
    /// PKCS#8.
    Pkcs8,
}

/// A resolved `security.outbound` declaration: the anchors always present once the block is
/// written, and the optional client identity beside them - `github.com/telekom/sutura#911`.
///
/// **Why one type and not two independently-optional parameters.** A composition root threads
/// this from boot to every fixed-host consumer (the `BigQuery` wire, the `datahub` reader); an
/// identity can never be declared without anchors (`security.outbound` always requires
/// `transport_anchors` once the block itself exists), so pairing them is a type that cannot
/// disagree with that invariant rather than two values a call site could thread inconsistently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    anchors: Anchors,
    identity: Option<Identity>,
}

impl Declared {
    /// Pairs a resolved anchor declaration with its optional client identity.
    #[must_use]
    pub const fn new(anchors: Anchors, identity: Option<Identity>) -> Self {
        Self { anchors, identity }
    }

    /// The declared anchors.
    #[must_use]
    pub const fn anchors(&self) -> &Anchors {
        &self.anchors
    }

    /// The declared client identity, if any.
    #[must_use]
    pub const fn identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
    }

    /// Consumes into its parts - what [`Rotator::new`] takes separately.
    #[must_use]
    pub fn into_parts(self) -> (Anchors, Option<Identity>) {
        (self.anchors, self.identity)
    }
}

/// Loads the declared client identity, refusing a half that cannot be read or does not hold its
/// kind.
///
/// # Errors
///
/// [`LoadError::IdentityRead`] for a half that cannot be read; [`LoadError::IdentityIncomplete`] for
/// a certificate file with no certificate; [`LoadError::IdentityKey`] for a key file that does not
/// parse as a private key.
pub fn load_identity(identity: &Identity) -> Result<LoadedIdentity, LoadError> {
    let chain = load_certificate(identity.certificate())?;
    let key = load_private_key(identity.key())?;
    Ok(LoadedIdentity { chain, key })
}

/// Reads a declared PEM bundle into raw certificate DER, refusing an invalid entry rather than
/// skipping it.
fn bundle_certificates(anchors_path: &Path) -> Result<LoadedAnchors, LoadError> {
    let bundle = std::fs::read(anchors_path).map_err(|cause| LoadError::AnchorsRead {
        path: anchors_path.display().to_string(),
        cause,
    })?;
    let certificates = CertificateDer::pem_slice_iter(&bundle)
        .collect::<Result<Vec<CertificateDer<'static>>, _>>()
        .map_err(|cause| LoadError::AnchorsRead {
            path: anchors_path.display().to_string(),
            cause: std::io::Error::new(std::io::ErrorKind::InvalidData, cause),
        })?;
    LoadedAnchors::parse(
        certificates,
        LoadError::AnchorsEmpty {
            path: anchors_path.display().to_string(),
        },
    )
}

/// Turns the host-store reader's result into the same strict certificate list a declared bundle
/// gives, refusing a partial read rather than accepting the certificates it did recover.
fn system_certificates(loaded: rustls_native_certs::CertificateResult) -> Result<LoadedAnchors, LoadError> {
    if !loaded.errors.is_empty() {
        let errors = loaded.errors.len();
        let mut failures = loaded.errors.into_iter();
        let cause = failures.next().ok_or(LoadError::SystemStoreEmpty)?;
        return Err(LoadError::SystemStoreRead { errors, cause });
    }
    LoadedAnchors::parse(loaded.certs, LoadError::SystemStoreEmpty)
}

/// Reads and parses the client certificate chain, refused if it holds no certificate.
fn load_certificate(path: &Path) -> Result<Vec<CertificateDer<'static>>, LoadError> {
    let bytes = std::fs::read(path).map_err(|cause| LoadError::IdentityRead {
        path: path.display().to_string(),
        cause,
    })?;
    let certificates = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<Vec<CertificateDer<'static>>, _>>()
        .map_err(|_cause| LoadError::IdentityIncomplete {
            path: path.display().to_string(),
            what: "no certificate",
        })?;
    if certificates.is_empty() {
        return Err(LoadError::IdentityIncomplete {
            path: path.display().to_string(),
            what: "no certificate",
        });
    }
    Ok(certificates)
}

/// Reads and parses the client private key, refused if it is not a key this build can present.
fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, LoadError> {
    let bytes = std::fs::read(path).map_err(|cause| LoadError::IdentityRead {
        path: path.display().to_string(),
        cause,
    })?;
    PrivateKeyDer::from_pem_slice(&bytes).map_err(|_cause| LoadError::IdentityKey {
        path: path.display().to_string(),
        what: "not a private key this build can present",
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const SUBJECT: &str = "localhost";

    struct Scratch {
        directory: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir().join(format!("sutura-tls-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
            Self { directory }
        }

        fn cert(&self, name: &str) -> PathBuf {
            let issued = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed pair generates");
            let path = self.directory.join(format!("{name}.pem"));
            std::fs::write(&path, issued.cert.pem()).expect("a certificate writes");
            path
        }

        fn pair(&self, prefix: &str) -> (PathBuf, PathBuf) {
            let issued = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed pair generates");
            let certificate = self.directory.join(format!("{prefix}.crt"));
            let key = self.directory.join(format!("{prefix}.key"));
            std::fs::write(&certificate, issued.cert.pem()).expect("a certificate writes");
            std::fs::write(&key, issued.signing_key.serialize_pem()).expect("a key writes");
            (certificate, key)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ignored = std::fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn a_bundle_loads_its_certificates() {
        let scratch = Scratch::new("bundle");
        let anchors = scratch.cert("root");
        let loaded = load_anchors(&Anchors::Bundle(anchors)).expect("a bundle with one cert loads");
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn a_missing_anchor_file_is_refused_naming_the_path() {
        let missing = PathBuf::from("/definitely/not/here.pem");
        assert!(matches!(
            load_anchors(&Anchors::Bundle(missing)),
            Err(LoadError::AnchorsRead { path, .. }) if path == "/definitely/not/here.pem"
        ));
    }

    #[test]
    fn an_anchor_file_with_no_certificates_is_refused() {
        let scratch = Scratch::new("empty-anchors");
        let empty = scratch.directory.join("empty.pem");
        std::fs::write(&empty, "not a certificate at all\n").expect("an empty file writes");
        assert!(matches!(
            load_anchors(&Anchors::Bundle(empty.clone())),
            Err(LoadError::AnchorsEmpty { path }) if path == empty.display().to_string()
        ));
    }

    #[test]
    fn a_loaded_system_store_returns_its_certificates() {
        let issued = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed pair generates");
        let mut loaded = rustls_native_certs::CertificateResult::default();
        loaded.certs.push(issued.cert.der().clone());
        assert_eq!(system_certificates(loaded).expect("one system root loads").len(), 1);
    }

    #[test]
    fn an_empty_system_store_is_refused() {
        assert!(matches!(
            system_certificates(rustls_native_certs::CertificateResult::default()),
            Err(LoadError::SystemStoreEmpty)
        ));
    }

    #[test]
    fn a_partially_read_system_store_is_refused_naming_how_many_failed() {
        let mut loaded = rustls_native_certs::CertificateResult::default();
        let issued = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed pair generates");
        loaded.certs.push(issued.cert.der().clone());
        loaded.errors.push(rustls_native_certs::Error {
            context: "one store entry could not be read",
            kind: rustls_native_certs::ErrorKind::Io {
                inner: std::io::Error::other("permission denied"),
                path: PathBuf::from("/etc/ssl/certs/broken.pem"),
            },
        });
        assert!(matches!(
            system_certificates(loaded),
            Err(LoadError::SystemStoreRead { errors: 1, .. })
        ));
    }

    #[test]
    fn a_client_identity_loads_its_certificate_and_key() {
        let scratch = Scratch::new("identity");
        let (certificate, key) = scratch.pair("client");
        let identity = Identity::new(certificate, key);
        let loaded = load_identity(&identity).expect("a pair loads");
        assert_eq!(loaded.chain().len(), 1);
    }

    #[test]
    fn a_loaded_identitys_key_kind_is_pkcs8() {
        // `rcgen`'s own `KeyPair::serialize_pem` always emits PKCS#8 - internally every kind it
        // supports round-trips through `PrivatePkcs8KeyDer` - so this pins the value a real
        // deployment's own openssl/step-issued key would produce just as often, without this test
        // depending on a second key format nothing else here generates.
        let scratch = Scratch::new("key-kind");
        let (certificate, key) = scratch.pair("client");
        let loaded = load_identity(&Identity::new(certificate, key)).expect("a pair loads");
        assert_eq!(loaded.key_kind(), KeyKind::Pkcs8);
    }

    #[test]
    fn declared_pairs_anchors_with_an_optional_identity_and_unpacks_both() {
        let scratch = Scratch::new("declared");
        let anchors = Anchors::Bundle(scratch.cert("root"));
        let (certificate, key) = scratch.pair("client");
        let identity = Identity::new(certificate, key);
        let declared = Declared::new(anchors.clone(), Some(identity.clone()));
        assert_eq!(declared.anchors(), &anchors);
        assert_eq!(declared.identity(), Some(&identity));
        assert_eq!(declared.into_parts(), (anchors, Some(identity)));
    }

    #[test]
    fn a_missing_identity_certificate_is_refused_naming_the_path() {
        let scratch = Scratch::new("missing-cert");
        let identity = Identity::new(
            PathBuf::from("/definitely/not/here.crt"),
            scratch.directory.join("client.key"),
        );
        assert!(matches!(
            load_identity(&identity),
            Err(LoadError::IdentityRead { path, .. }) if path == "/definitely/not/here.crt"
        ));
    }

    #[test]
    fn a_certificate_with_no_certificates_is_refused() {
        let scratch = Scratch::new("no-cert");
        let bad_cert = scratch.directory.join("not-a-cert.pem");
        std::fs::write(&bad_cert, "not a certificate\n").expect("a bad certificate writes");
        let identity = Identity::new(bad_cert, scratch.directory.join("client.key"));
        assert!(matches!(
            load_identity(&identity),
            Err(LoadError::IdentityIncomplete {
                what: "no certificate",
                ..
            })
        ));
    }

    #[test]
    fn a_non_key_file_is_refused_as_identity_key() {
        let scratch = Scratch::new("bad-key");
        let (certificate, _) = scratch.pair("client");
        let bad_key = scratch.directory.join("key.pem");
        std::fs::write(&bad_key, "not a private key\n").expect("a bad key writes");
        let identity = Identity::new(certificate, bad_key);
        assert!(matches!(load_identity(&identity), Err(LoadError::IdentityKey { .. })));
    }
}
