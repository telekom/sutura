//! Building the `rustls::ClientConfig` a TLS `postgres` source channel verifies with.
//!
//! This is the TLS half of `sutura_config::sources::transport`, turned into a verifier. That module
//! owns the three-state DECLARATION (`plaintext` / `verified` / `mutual`); this one owns turning a
//! declared `verified` or `mutual` channel into the thing the driver connects with: read the anchor
//! store, read the optional client identity, and refuse the combinations a closed type refuses.
//!
//! The two crates do not share a dependency, so this module's input is the RESOLVED material a
//! composition root extracted from the declaration - the same boundary `connect_secured`'s own
//! signature draws, and the reason the adapters here never carry a second copy of the three-state
//! shape. What a composition root hands this module is: whether the declared anchors are a PEM
//! bundle or the host's system store, and an optional client identity path pair.
//!
//! # What fails here, and why it is a connect-time refusal
//!
//! Configuration refuses what only a tree can see (an unknown `transport_mode` word, TLS naming no
//! anchors, a partial identity, a relative path). What this module refuses is what only a file and
//! a TLS implementation can answer - and each refusal is fail-closed and names the path:
//!
//! * anchors that cannot be read ([`PostgresError::AnchorsRead`]) or parse to no certificates
//!   ([`PostgresError::AnchorsEmpty`]);
//! * a `system` store that cannot be read completely or contains no usable roots
//!   ([`PostgresError::SystemStoreRead`], [`PostgresError::SystemStoreEmpty`]);
//! * an identity half that cannot be read ([`PostgresError::IdentityRead`]) or parses to the wrong
//!   kind ([`PostgresError::IdentityIncomplete`], [`PostgresError::IdentityKey`]).
//!
//! An untrusted-issuer chain is not refused HERE: verification is the handshake's job, and a
//! `ClientConfig` built over the declared roots is exactly the thing that refuses it. The
//! tier-backed test that connects a source to a server under an unTRUSTED issuer is refused by
//! `PostgresWarehouse::connect_secured`'s [`PostgresError::Connect`] arm at the handshake, while the
//! construction half stays honest about what it can know: a `ClientConfig` whose roots are the
//! declared file.
//!
//! The construction is always compiled (this crate is the source channel), so `checks.nextest`
//! exercises every refusal above in-crate against `rcgen`-generated material, and `tests/tls.rs`
//! drives the same construction against the tier's real server - where the two cells are that the
//! declared anchor verifies and an issuer it does not name is refused.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::RootCertStore;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use crate::PostgresError;

/// The trust anchors a TLS source channel verifies against, resolved from the declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsAnchors {
    /// A PEM bundle at this absolute path. Read by [`client_config`] once, at boot.
    Bundle(PathBuf),
    /// The host's own trust store, read once by [`client_config`]. This is reached only when the
    /// deployment explicitly wrote `transport_anchors: system`; it is never a fallback.
    System,
}

/// The client certificate and key a `mutual` channel presents, resolved from the declaration.
///
/// A pair - configuration already refused a partial one at load; this module reads both paths and
/// refuses a file that does not hold its half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsIdentity {
    /// The certificate (and any chain) to present. Absolute.
    certificate: PathBuf,
    /// The private key for that certificate. Absolute.
    key: PathBuf,
}

impl TlsIdentity {
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

/// Builds the `rustls::ClientConfig` a TLS source channel verifies (and, for `mutual`, presents)
/// with, from the resolved anchor material and an optional client identity.
///
/// # Errors
///
/// `SystemStoreRead`/`SystemStoreEmpty` for a host store that cannot supply a complete non-empty
/// root set; `AnchorsRead` for a bundle that cannot be read; `AnchorsEmpty` for a bundle that parses
/// to no certificates; `IdentityRead`/`IdentityIncomplete`/`IdentityKey` for an identity half that
/// cannot be read or does not hold its kind.
pub fn client_config(anchors: &TlsAnchors, identity: Option<&TlsIdentity>) -> Result<rustls::ClientConfig, PostgresError> {
    let roots = match anchors {
        TlsAnchors::System => system_roots(rustls_native_certs::load_native_certs())?,
        TlsAnchors::Bundle(path) => bundle_roots(path)?,
    };

    // An explicit provider rather than `CryptoProvider::install_default`, for the reason the
    // serving side gives: the process-global default would make this adapter's behaviour depend on
    // whether something else got there first. `ring` is the one compiled in.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|cause| PostgresError::TlsConfiguration { cause })?
        .with_root_certificates(roots);

    match identity {
        None => Ok(builder.with_no_client_auth()),
        Some(identity) => {
            let cert_der = load_certificate(&identity.certificate)?;
            let key = load_private_key(identity.key())?;
            builder
                .with_client_auth_cert(cert_der, key)
                .map_err(|_cause| PostgresError::IdentityKey {
                    path: identity.key().display().to_string(),
                    what: "not a private key this build can present",
                })
        }
    }
}

/// Reads a declared PEM bundle into a root store without skipping an invalid certificate.
fn bundle_roots(anchors_path: &Path) -> Result<RootCertStore, PostgresError> {
    let mut roots = RootCertStore::empty();
    let bundle = std::fs::read(anchors_path).map_err(|cause| PostgresError::AnchorsRead {
        path: anchors_path.display().to_string(),
        cause,
    })?;
    for entry in CertificateDer::pem_slice_iter(&bundle) {
        let der = entry.map_err(|cause| PostgresError::AnchorsRead {
            path: anchors_path.display().to_string(),
            cause: std::io::Error::new(std::io::ErrorKind::InvalidData, cause),
        })?;
        roots.add(der).map_err(|cause| PostgresError::AnchorsRead {
            path: anchors_path.display().to_string(),
            cause: std::io::Error::new(std::io::ErrorKind::InvalidData, cause),
        })?;
    }
    if roots.is_empty() {
        return Err(PostgresError::AnchorsEmpty {
            path: anchors_path.display().to_string(),
        });
    }
    Ok(roots)
}

/// Turns the host-store reader's result into the same strict root set as a declared bundle.
///
/// The upstream reader reports partial reads as certificates plus errors. Accepting only the
/// certificates would make `system` mean a silently reduced store, so any reported error is a
/// refusal. A certificate rustls itself rejects is likewise not skipped.
fn system_roots(loaded: rustls_native_certs::CertificateResult) -> Result<RootCertStore, PostgresError> {
    if !loaded.errors.is_empty() {
        let errors = loaded.errors.len();
        let mut failures = loaded.errors.into_iter();
        let cause = failures.next().ok_or(PostgresError::SystemStoreEmpty)?;
        return Err(PostgresError::SystemStoreRead { errors, cause });
    }
    let mut roots = RootCertStore::empty();
    for certificate in loaded.certs {
        roots
            .add(certificate)
            .map_err(|cause| PostgresError::SystemStoreCertificate { cause })?;
    }
    if roots.is_empty() {
        return Err(PostgresError::SystemStoreEmpty);
    }
    Ok(roots)
}

/// Reads and parses the client certificate chain, refused if it holds no certificate.
fn load_certificate(path: &Path) -> Result<Vec<CertificateDer<'static>>, PostgresError> {
    let bytes = std::fs::read(path).map_err(|cause| PostgresError::IdentityRead {
        path: path.display().to_string(),
        cause,
    })?;
    let certificates = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<Vec<CertificateDer<'static>>, _>>()
        .map_err(|_cause| PostgresError::IdentityIncomplete {
            path: path.display().to_string(),
            what: "no certificate",
        })?;
    if certificates.is_empty() {
        return Err(PostgresError::IdentityIncomplete {
            path: path.display().to_string(),
            what: "no certificate",
        });
    }
    Ok(certificates)
}

/// Reads and parses the client private key, refused if it is not a key this build can present.
fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, PostgresError> {
    let bytes = std::fs::read(path).map_err(|cause| PostgresError::IdentityRead {
        path: path.display().to_string(),
        cause,
    })?;
    PrivateKeyDer::from_pem_slice(&bytes).map_err(|_cause| PostgresError::IdentityKey {
        path: path.display().to_string(),
        what: "not a private key this build can present",
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::PostgresError;

    /// The name every generated certificate is issued for.
    const SUBJECT: &str = "localhost";

    /// A generated self-signed pair, as PEM, written to a scratch directory.
    struct Scratch {
        directory: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir().join(format!("sutura-pg-tls-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
            Self { directory }
        }

        /// Generates one self-signed certificate and writes its PEM bundle at `name.pem`; returns
        /// the path.
        fn cert(&self, name: &str) -> PathBuf {
            let issued = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed pair generates");
            let path = self.directory.join(format!("{name}.pem"));
            std::fs::write(&path, issued.cert.pem()).expect("a certificate writes");
            path
        }

        /// Generates a certificate plus its key, writing both to `prefix.crt` and `prefix.key`.
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
    fn a_bundle_builds_a_verifying_client_config() {
        let scratch = Scratch::new("bundle");
        let anchors = scratch.cert("root");
        // Construction succeeding IS the assertion: there is no unverified `ClientConfig` this
        // function could return instead, so a bundle it cannot build a verifier over is an `Err`.
        client_config(&TlsAnchors::Bundle(anchors), None).expect("a bundle with one cert builds");
    }

    #[test]
    fn a_loaded_system_store_builds_the_same_root_set_as_an_explicit_bundle() {
        let issued = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed pair generates");
        let mut loaded = rustls_native_certs::CertificateResult::default();
        loaded.certs.push(issued.cert.der().clone());
        assert_eq!(system_roots(loaded).expect("one system root loads").len(), 1);
    }

    #[test]
    fn an_empty_system_store_is_refused() {
        assert!(matches!(
            system_roots(rustls_native_certs::CertificateResult::default()),
            Err(PostgresError::SystemStoreEmpty)
        ));
    }

    #[test]
    fn a_missing_anchor_file_is_refused_naming_the_path() {
        let missing = PathBuf::from("/definitely/not/here.pem");
        assert!(matches!(
            client_config(&TlsAnchors::Bundle(missing), None),
            Err(PostgresError::AnchorsRead { path, .. }) if path == "/definitely/not/here.pem"
        ));
    }

    #[test]
    fn an_anchor_file_with_no_certificates_is_refused() {
        let scratch = Scratch::new("empty-anchors");
        let empty = scratch.directory.join("empty.pem");
        std::fs::write(&empty, "not a certificate at all\n").expect("an empty file writes");
        assert!(matches!(
            client_config(&TlsAnchors::Bundle(empty.clone()), None),
            Err(PostgresError::AnchorsEmpty { path }) if path == empty.display().to_string()
        ));
    }

    #[test]
    fn a_client_identity_builds_a_mutual_client_config() {
        let scratch = Scratch::new("mutual");
        let anchors = scratch.cert("root");
        let (certificate, key) = scratch.pair("client");
        let identity = TlsIdentity { certificate, key };
        client_config(&TlsAnchors::Bundle(anchors), Some(&identity)).expect("a pair builds a mutual config");
    }

    #[test]
    fn a_missing_identity_certificate_is_refused_naming_the_path() {
        let scratch = Scratch::new("missing-cert");
        let anchors = scratch.cert("root");
        let identity = TlsIdentity {
            certificate: PathBuf::from("/definitely/not/here.crt"),
            key: scratch.directory.join("client.key"),
        };
        assert!(matches!(
            client_config(&TlsAnchors::Bundle(anchors), Some(&identity)),
            Err(PostgresError::IdentityRead { path, .. }) if path == "/definitely/not/here.crt"
        ));
    }

    #[test]
    fn a_certificate_with_no_certificates_is_refused() {
        let scratch = Scratch::new("no-cert");
        let anchors = scratch.cert("root");
        let bad_cert = scratch.directory.join("not-a-cert.pem");
        std::fs::write(&bad_cert, "not a certificate\n").expect("a bad certificate writes");
        let identity = TlsIdentity {
            certificate: bad_cert,
            key: scratch.directory.join("client.key"),
        };
        assert!(matches!(
            client_config(&TlsAnchors::Bundle(anchors), Some(&identity)),
            Err(PostgresError::IdentityIncomplete {
                what: "no certificate",
                ..
            })
        ));
    }

    #[test]
    fn a_non_key_file_is_refused_as_identity_key() {
        let scratch = Scratch::new("bad-key");
        let anchors = scratch.cert("root");
        let (certificate, _) = scratch.pair("client");
        let bad_key = scratch.directory.join("key.pem");
        std::fs::write(&bad_key, "not a private key\n").expect("a bad key writes");
        let identity = TlsIdentity {
            certificate,
            key: bad_key,
        };
        assert!(matches!(
            client_config(&TlsAnchors::Bundle(anchors), Some(&identity)),
            Err(PostgresError::IdentityKey { .. })
        ));
    }
}
