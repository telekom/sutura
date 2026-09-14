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
//!
//! **The READ itself lives in `sutura-tls`**, a leaf crate with no dependency on a crypto provider,
//! a network client, or `sutura-config` - extracted here because `github.com/telekom/sutura#125`'s
//! remainder needs the identical bundle-or-system-store read a second time, for a `ureq`-based
//! outbound adapter, and copying `bundle_roots`/`system_roots`/the identity loaders a second time
//! is exactly the duplication `AGENTS.md` asks not to hold twice. What stays HERE, and is this
//! crate's own, is folding the read bytes into a `RootCertStore` (the step that also catches a
//! certificate rustls itself cannot use as a root - [`PostgresError::AnchorsRead`] for a bundle
//! entry, [`PostgresError::SystemStoreCertificate`] for a system-store one, unchanged from before
//! the extraction) and building the `ring`-backed `rustls::ClientConfig` `tokio-postgres-rustls`
//! wants. Every error variant this module can produce is unchanged; only where the read happens did.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::RootCertStore;

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
    let roots = certificate_roots(anchors)?;

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
            let (cert_der, key) = sutura_tls::load_identity(&resolved_identity(identity)).map_err(convert_load_error)?;
            builder
                .with_client_auth_cert(cert_der, key)
                .map_err(|_cause| PostgresError::IdentityKey {
                    path: identity.key().display().to_string(),
                    what: "not a private key this build can present",
                })
        }
    }
}

/// Reads the declared anchors through `sutura_tls::load_anchors` and folds the certificates into a
/// `RootCertStore`, which is the one step `sutura-tls` deliberately does not take - it has no crypto
/// provider to validate against, and which anchors a `ClientConfig` trusts is this adapter's own
/// decision.
///
/// A certificate rustls itself rejects is refused rather than skipped, in the same shape each of the
/// two sources used before this read moved: a bundle entry's failure is
/// [`PostgresError::AnchorsRead`] (wrapping the rejection as the read failure it effectively is for
/// that source), and a system-store entry's is [`PostgresError::SystemStoreCertificate`] - the two
/// error identities a caller already matches on are unchanged.
fn certificate_roots(anchors: &TlsAnchors) -> Result<RootCertStore, PostgresError> {
    let certificates = sutura_tls::load_anchors(&resolved_anchors(anchors)).map_err(convert_load_error)?;
    let mut roots = RootCertStore::empty();
    for certificate in certificates {
        roots.add(certificate).map_err(|cause| match anchors {
            TlsAnchors::System => PostgresError::SystemStoreCertificate { cause },
            TlsAnchors::Bundle(path) => PostgresError::AnchorsRead {
                path: path.display().to_string(),
                cause: std::io::Error::new(std::io::ErrorKind::InvalidData, cause),
            },
        })?;
    }
    // `sutura_tls::load_anchors` already refuses an empty result (`AnchorsEmpty`/`SystemStoreEmpty`),
    // so `roots` is non-empty here whenever every `add` above succeeded - no second empty check.
    Ok(roots)
}

/// This crate's declared anchors, as the plain `Bundle`-or-`System` shape `sutura-tls` reads.
fn resolved_anchors(anchors: &TlsAnchors) -> sutura_tls::Anchors {
    // `TlsAnchors::Bundle` holds an owned `PathBuf`; `sutura_tls::Anchors` needs one too, so this
    // clones the path rather than borrowing it - the same cost `TlsIdentity`'s accessors already
    // pay by returning `&Path` from an owned field.
    match anchors {
        TlsAnchors::Bundle(path) => sutura_tls::Anchors::Bundle(path.clone()),
        TlsAnchors::System => sutura_tls::Anchors::System,
    }
}

/// This crate's declared identity, as the plain path pair `sutura-tls` reads.
fn resolved_identity(identity: &TlsIdentity) -> sutura_tls::Identity {
    sutura_tls::Identity::new(identity.certificate.clone(), identity.key.clone())
}

/// Maps `sutura-tls`'s load refusal onto this crate's own error type, field for field - the two
/// enums were designed to line up exactly, so a caller matching on `PostgresError::AnchorsRead` (or
/// any of the other six) sees no difference from before the read moved crates.
fn convert_load_error(cause: sutura_tls::LoadError) -> PostgresError {
    match cause {
        sutura_tls::LoadError::AnchorsRead { path, cause } => PostgresError::AnchorsRead { path, cause },
        sutura_tls::LoadError::AnchorsEmpty { path } => PostgresError::AnchorsEmpty { path },
        sutura_tls::LoadError::SystemStoreRead { errors, cause } => PostgresError::SystemStoreRead { errors, cause },
        sutura_tls::LoadError::SystemStoreEmpty => PostgresError::SystemStoreEmpty,
        sutura_tls::LoadError::IdentityRead { path, cause } => PostgresError::IdentityRead { path, cause },
        sutura_tls::LoadError::IdentityIncomplete { path, what } => PostgresError::IdentityIncomplete { path, what },
        sutura_tls::LoadError::IdentityKey { path, what } => PostgresError::IdentityKey { path, what },
    }
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

    // The system-store loading cells (a loaded store builds the same root set as a bundle, an empty
    // store is refused, a partially-read store is refused naming how many entries failed) moved to
    // `sutura-tls`'s own suite with the function they exercised - `system_roots` no longer exists in
    // this crate. What stays here is the postgres-specific half those tests never reached: that a
    // `client_config` refusal still comes back as THIS crate's own `PostgresError` variant.

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
