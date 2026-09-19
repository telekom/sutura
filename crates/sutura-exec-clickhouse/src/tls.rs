//! Building the `ureq::tls::TlsConfig` a TLS `clickhouse` source channel verifies with.
//!
//! This is the TLS half of `sutura_config::sources::transport`, turned into a verifier - the same
//! job `sutura_exec_postgres::tls` does for `rustls::ClientConfig`, so that module's own header is
//! this one's rather than restated: configuration owns the three-state DECLARATION (`plaintext` /
//! `verified` / `mutual`), and this module owns turning a declared `verified` or `mutual` channel
//! into the thing the client connects with. **The declared-trust-store rule extends rather than
//! forks** (`docs/adr/0010`): a PEM bundle or the host's system store, read once by [`config`] and
//! never a fallback, exactly as the Postgres source channel reads it.
//!
//! The read itself lives in `sutura-tls`, shared with the Postgres source channel and the
//! `BigQuery` wire so the refusals for an unreadable, empty or malformed bundle/identity are not a
//! third hand-written copy. What is this crate's own is folding the read bytes into the shape
//! `ureq::tls::TlsConfig` wants - `ureq::tls::Certificate`/`ureq::tls::PrivateKey` rather than
//! `rustls`'s own DER newtypes, because `ureq` never hands this adapter a `rustls::ClientConfig` to
//! build.
//!
//! # Why this is a fallible step SEPARATE from opening the transport
//!
//! `transport::Http::connect`/`connect_secured` take an already-built `ureq::tls::TlsConfig` and
//! cannot fail, because `ureq::Agent::new_with_config` does no I/O - `ureq` dials lazily, on the
//! first request. So [`TlsError`] is its own type here rather than a variant folded into
//! `crate::ClickHouseError`: nothing about it can arrive from `Warehouse::execute`, only from
//! whoever resolves a declared channel into a config before opening the adapter.

use std::path::PathBuf;

/// Why this crate could not build a `ureq::tls::TlsConfig` from a declared channel.
///
/// The same six refusals `sutura_exec_postgres::PostgresError` carries for the identical read,
/// field for field - [`from_load_error`] is the mapping.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("the declared trust anchors could not be read as a PEM bundle at {path}")]
    AnchorsRead {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    #[error("the trust-anchor bundle at {path} parsed to no certificates - a store of nothing verifies nothing")]
    AnchorsEmpty { path: String },
    #[error("the declared client identity could not be read at {path}")]
    IdentityRead {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    #[error("the client identity pair is incomplete: expected a certificate and a key, and found {what} at {path}")]
    IdentityIncomplete { path: String, what: &'static str },
    #[error("the client private key at {path} is not a private key this build can present")]
    IdentityKey { path: String, what: &'static str },
    #[error("the host trust store reported {errors} errors while it was read")]
    SystemStoreRead {
        errors: usize,
        #[source]
        cause: rustls_native_certs::Error,
    },
    #[error("the host trust store held no certificates - a store of nothing verifies nothing")]
    SystemStoreEmpty,
}

/// The trust anchors a TLS source channel verifies against, resolved from the declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsAnchors {
    /// A PEM bundle at this absolute path.
    Bundle(PathBuf),
    /// The host's own trust store. Reached only when the deployment explicitly wrote
    /// `transport_anchors: system`; never a fallback.
    System,
}

/// The client certificate and key a `mutual` channel presents, resolved from the declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsIdentity {
    certificate: PathBuf,
    key: PathBuf,
}

impl TlsIdentity {
    /// A client identity from its declared paths.
    #[must_use]
    pub const fn new(certificate: PathBuf, key: PathBuf) -> Self {
        Self { certificate, key }
    }
}

/// Builds the `ureq::tls::TlsConfig` a TLS source channel verifies (and, for `mutual`, presents)
/// with, from the resolved anchor material and an optional client identity.
///
/// # Errors
///
/// The six refusals [`TlsError`] carries: an unreadable or empty bundle, an unreadable or empty
/// system store, and an unreadable or malformed identity.
pub fn config(anchors: &TlsAnchors, identity: Option<&TlsIdentity>) -> Result<ureq::tls::TlsConfig, TlsError> {
    let loaded = sutura_tls::load_anchors(&resolved_anchors(anchors)).map_err(from_load_error)?;
    let identity_loaded = identity
        .map(|id| sutura_tls::load_identity(&resolved_identity(id)).map_err(from_load_error))
        .transpose()?;
    let roots = ureq::tls::RootCerts::new_with_certs(&owned_certificates(loaded));
    let builder = ureq::tls::TlsConfig::builder().root_certs(roots);
    // `identity.zip(identity_loaded)` is `Some` exactly when both halves are - `identity_loaded`
    // is derived FROM `identity` above, so there is no third case where one is present and the
    // other is not for a match arm to have to refuse.
    match identity.zip(identity_loaded) {
        None => Ok(builder.build()),
        Some((declared, loaded)) => {
            let (chain, _key) = loaded.into_parts();
            let certificates = owned_certificates_from(chain);
            let key = owned_key(declared)?;
            Ok(builder
                .client_cert(Some(ureq::tls::ClientCert::new_with_certs(&certificates, key)))
                .build())
        }
    }
}

/// `sutura_tls::LoadedAnchors`'s `CertificateDer` bytes, each turned into an owned
/// `ureq::tls::Certificate` - the same conversion `sutura_exec_bigquery::wire::tls` makes for its
/// own (anchors-only) TLS configuration.
fn owned_certificates(loaded: sutura_tls::LoadedAnchors) -> Vec<ureq::tls::Certificate<'static>> {
    owned_certificates_from(loaded)
}

/// The shared conversion, over anything that yields owned `CertificateDer`s - both the root anchor
/// set and a `mutual` channel's own presented chain are exactly this shape.
fn owned_certificates_from(
    loaded: impl IntoIterator<Item = rustls_pki_types::CertificateDer<'static>>,
) -> Vec<ureq::tls::Certificate<'static>> {
    loaded
        .into_iter()
        .map(|der| ureq::tls::Certificate::from_der(der.as_ref()).to_owned())
        .collect()
}

/// The declared client key, as `ureq::tls::PrivateKey` wants it.
///
/// **Read a second time, deliberately.** `sutura_tls::load_identity` already validated the file at
/// `declared`'s key path parses to a complete certificate-and-key pair - that is what
/// [`TlsError::IdentityIncomplete`]/[`TlsError::IdentityKey`] hold - but its own return type is
/// `rustls_pki_types::PrivateKeyDer`, whose three variants (`Pkcs1`/`Sec1`/`Pkcs8`) name which shape
/// the PEM held using `ureq::tls::KeyKind`, a type `ureq` does not make public. `PrivateKey::
/// from_pem` is the only externally reachable constructor left, and it re-derives the same shape
/// from the same bytes rustls's reader already validated - so this re-read cannot newly refuse
/// anything `sutura_tls::load_identity` did not already accept.
fn owned_key(declared: &TlsIdentity) -> Result<ureq::tls::PrivateKey<'static>, TlsError> {
    let pem = std::fs::read(&declared.key).map_err(|cause| TlsError::IdentityRead {
        path: declared.key.display().to_string(),
        cause,
    })?;
    ureq::tls::PrivateKey::from_pem(&pem).map_err(|_cause| TlsError::IdentityKey {
        path: declared.key.display().to_string(),
        what: "not a private key this build can present",
    })
}

/// This crate's declared anchors, as the plain `Bundle`-or-`System` shape `sutura-tls` reads.
fn resolved_anchors(anchors: &TlsAnchors) -> sutura_tls::Anchors {
    match anchors {
        TlsAnchors::Bundle(path) => sutura_tls::Anchors::Bundle(path.clone()),
        TlsAnchors::System => sutura_tls::Anchors::System,
    }
}

/// This crate's declared identity, as the plain path pair `sutura-tls` reads.
fn resolved_identity(identity: &TlsIdentity) -> sutura_tls::Identity {
    sutura_tls::Identity::new(identity.certificate.clone(), identity.key.clone())
}

/// Maps `sutura-tls`'s load refusal onto this crate's own error type, field for field.
fn from_load_error(cause: sutura_tls::LoadError) -> TlsError {
    match cause {
        sutura_tls::LoadError::AnchorsRead { path, cause } => TlsError::AnchorsRead { path, cause },
        sutura_tls::LoadError::AnchorsEmpty { path } => TlsError::AnchorsEmpty { path },
        sutura_tls::LoadError::SystemStoreRead { errors, cause } => TlsError::SystemStoreRead { errors, cause },
        sutura_tls::LoadError::SystemStoreEmpty => TlsError::SystemStoreEmpty,
        sutura_tls::LoadError::IdentityRead { path, cause } => TlsError::IdentityRead { path, cause },
        sutura_tls::LoadError::IdentityIncomplete { path, what } => TlsError::IdentityIncomplete { path, what },
        sutura_tls::LoadError::IdentityKey { path, what } => TlsError::IdentityKey { path, what },
    }
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
            let directory = std::env::temp_dir().join(format!("sutura-ch-tls-{name}-{}", std::process::id()));
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
    fn a_bundle_builds_a_verifying_tls_config() {
        let scratch = Scratch::new("bundle");
        let anchors = scratch.cert("root");
        config(&TlsAnchors::Bundle(anchors), None).expect("a bundle with one cert builds");
    }

    #[test]
    fn a_missing_anchor_file_is_refused_naming_the_path() {
        let missing = PathBuf::from("/definitely/not/here.pem");
        assert!(matches!(
            config(&TlsAnchors::Bundle(missing), None),
            Err(TlsError::AnchorsRead { path, .. }) if path == "/definitely/not/here.pem"
        ));
    }

    #[test]
    fn an_anchor_file_with_no_certificates_is_refused() {
        let scratch = Scratch::new("empty-anchors");
        let empty = scratch.directory.join("empty.pem");
        std::fs::write(&empty, "not a certificate at all\n").expect("an empty file writes");
        assert!(matches!(
            config(&TlsAnchors::Bundle(empty.clone()), None),
            Err(TlsError::AnchorsEmpty { path }) if path == empty.display().to_string()
        ));
    }

    #[test]
    fn a_client_identity_builds_a_mutual_tls_config() {
        let scratch = Scratch::new("mutual");
        let anchors = scratch.cert("root");
        let (certificate, key) = scratch.pair("client");
        let identity = TlsIdentity { certificate, key };
        config(&TlsAnchors::Bundle(anchors), Some(&identity)).expect("a pair builds a mutual config");
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
            config(&TlsAnchors::Bundle(anchors), Some(&identity)),
            Err(TlsError::IdentityRead { path, .. }) if path == "/definitely/not/here.crt"
        ));
    }
}
