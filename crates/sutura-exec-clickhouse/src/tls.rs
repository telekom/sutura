//! The TLS material a TLS `clickhouse` source channel hands the ADBC driver.
//!
//! This is the TLS half of `sutura_config::sources::transport`, turned into the driver's PEM
//! options: configuration owns the three-state DECLARATION (`plaintext` / `verified` / `mutual`),
//! and this module owns reading a declared `verified` or `mutual` channel's material. **The
//! declared-trust-store rule extends rather than forks** (`docs/adr/0010`): a PEM bundle or the
//! host's system store, validated once by [`adbc_options`] and never a fallback, exactly as the
//! Postgres source channel reads it.
//!
//! The read itself lives in `sutura-tls`, shared with the Postgres source channel and the
//! `BigQuery` wire so the refusals for an unreadable, empty or malformed bundle/identity are not a
//! third hand-written copy. What is this crate's own is handing the validated bytes on as the PEM
//! text the driver builds its TLS configuration from.
//!
//! # Why this is a fallible step SEPARATE from opening the adapter
//!
//! [`TlsError`] is its own type rather than a variant folded into [`crate::adbc::AdbcError`]:
//! nothing about it can arrive from `Warehouse::execute`, only from whoever resolves a declared
//! channel before opening the adapter.

use std::path::PathBuf;

/// Why this crate could not read the TLS material of a declared channel.
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
    #[error("the client key at {path} holds no readable `PRIVATE KEY`, `RSA PRIVATE KEY` or `EC PRIVATE KEY` PEM section")]
    IdentityKey { path: String },
    #[error("the host trust store reported {errors} errors while it was read")]
    SystemStoreRead {
        errors: usize,
        #[source]
        cause: rustls_native_certs::Error,
    },
    #[error("the host trust store held no certificates - a store of nothing verifies nothing")]
    SystemStoreEmpty,
    #[error("the declared TLS PEM at {path} is not UTF-8")]
    PemNotUtf8 { path: String },
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

/// PEM options handed to the ADBC driver: roots, client certificate chain and private key.
pub type AdbcTlsOptions = (Option<String>, Option<String>, Option<String>);

impl TlsIdentity {
    /// A client identity from its declared paths.
    #[must_use]
    pub const fn new(certificate: PathBuf, key: PathBuf) -> Self {
        Self { certificate, key }
    }
}

/// Loads a declared channel for the ADBC driver's PEM options.
///
/// A system trust store is represented by `None`: the vendored driver is built with its native
/// roots feature. A declared bundle or mutual identity is validated through `sutura-tls` first,
/// then passed on as the original PEM bytes so the driver performs the actual TLS configuration.
pub fn adbc_options(anchors: &TlsAnchors, identity: Option<&TlsIdentity>) -> Result<AdbcTlsOptions, TlsError> {
    let root = match anchors {
        TlsAnchors::System => {
            drop(sutura_tls::load_anchors(&resolved_anchors(anchors)).map_err(from_load_error)?);
            None
        }
        TlsAnchors::Bundle(path) => {
            drop(sutura_tls::load_anchors(&resolved_anchors(anchors)).map_err(from_load_error)?);
            Some(read_anchor_pem(path)?)
        }
    };
    let (chain, key) = match identity {
        None => (None, None),
        Some(identity) => {
            drop(sutura_tls::load_identity(&resolved_identity(identity)).map_err(from_load_error)?);
            (Some(read_pem(&identity.certificate)?), Some(read_pem(&identity.key)?))
        }
    };
    Ok((root, chain, key))
}

fn read_pem(path: &std::path::Path) -> Result<String, TlsError> {
    let bytes = std::fs::read(path).map_err(|cause| TlsError::IdentityRead {
        path: path.display().to_string(),
        cause,
    })?;
    String::from_utf8(bytes).map_err(|_cause| TlsError::PemNotUtf8 {
        path: path.display().to_string(),
    })
}

fn read_anchor_pem(path: &std::path::Path) -> Result<String, TlsError> {
    let bytes = std::fs::read(path).map_err(|cause| TlsError::AnchorsRead {
        path: path.display().to_string(),
        cause,
    })?;
    String::from_utf8(bytes).map_err(|_cause| TlsError::PemNotUtf8 {
        path: path.display().to_string(),
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
        sutura_tls::LoadError::IdentityKey { path } => TlsError::IdentityKey { path },
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
    fn a_bundle_is_handed_on_as_the_drivers_root_certificates() {
        let scratch = Scratch::new("bundle");
        let anchors = scratch.cert("root");
        let (root, chain, key) = adbc_options(&TlsAnchors::Bundle(anchors), None).expect("a bundle with one cert reads");
        assert!(root.is_some_and(|pem| pem.contains("BEGIN CERTIFICATE")));
        assert_eq!((chain, key), (None, None));
    }

    #[test]
    fn a_missing_anchor_file_is_refused_naming_the_path() {
        let missing = PathBuf::from("/definitely/not/here.pem");
        assert!(matches!(
            adbc_options(&TlsAnchors::Bundle(missing), None),
            Err(TlsError::AnchorsRead { path, .. }) if path == "/definitely/not/here.pem"
        ));
    }

    #[test]
    fn an_anchor_file_with_no_certificates_is_refused() {
        let scratch = Scratch::new("empty-anchors");
        let empty = scratch.directory.join("empty.pem");
        std::fs::write(&empty, "not a certificate at all\n").expect("an empty file writes");
        assert!(matches!(
            adbc_options(&TlsAnchors::Bundle(empty.clone()), None),
            Err(TlsError::AnchorsEmpty { path }) if path == empty.display().to_string()
        ));
    }

    #[test]
    fn a_client_identity_is_handed_on_as_the_drivers_chain_and_key() {
        let scratch = Scratch::new("mutual");
        let anchors = scratch.cert("root");
        let (certificate, key) = scratch.pair("client");
        let identity = TlsIdentity { certificate, key };
        let (_, chain, key) =
            adbc_options(&TlsAnchors::Bundle(anchors), Some(&identity)).expect("a pair reads as a mutual identity");
        assert!(chain.is_some_and(|pem| pem.contains("BEGIN CERTIFICATE")));
        assert!(key.is_some_and(|pem| pem.contains("PRIVATE KEY")));
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
            adbc_options(&TlsAnchors::Bundle(anchors), Some(&identity)),
            Err(TlsError::IdentityRead { path, .. }) if path == "/definitely/not/here.crt"
        ));
    }
}
