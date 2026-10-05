//! The connection the live Postgres dictionary reader dials with.
//!
//! The `tokio-postgres` configuration for one declared connection, and the `rustls::ClientConfig`
//! a `verified`/`mutual` channel verifies (and presents) with.
//!
//! **Here because this reader is the one `tokio-postgres` client left** (`telekom/sutura#913`):
//! `sutura-exec-postgres` answers every source through ADBC, so the client and its channel live
//! beside the reader that still uses them, until the reader moves to ADBC too.
//!
//! What fails here is what only a file and a TLS implementation can answer, each fail-closed and
//! naming the path: anchors that cannot be read or parse to nothing, an identity half that cannot be
//! read or holds the wrong kind (all `sutura_tls`'s reads), a certificate rustls cannot use as a
//! root, and a key that does not match its certificate. An untrusted chain is the handshake's
//! refusal, not this module's.

use std::sync::Arc;

use rustls::RootCertStore;
use sutura_domain::identity::Secret;

/// Why a declared channel could not become a client config.
#[derive(Debug, thiserror::Error)]
pub enum UnusableChannel {
    #[error("the declared TLS material could not be read")]
    Material(#[source] sutura_tls::LoadError),
    #[error("a declared anchor is not a certificate this TLS implementation can use as a root")]
    Root(#[source] rustls::Error),
    #[error("the TLS client verifier could not be constructed")]
    Verifier(#[source] rustls::Error),
    #[error("the client certificate and key could not be combined into one identity this build can present")]
    Identity(#[source] rustls::Error),
}

/// The address the reader dials.
#[derive(Clone, Copy)]
pub enum Target<'a> {
    /// A TCP host name or address.
    Host(&'a str),
    /// A unix socket directory.
    UnixSocket(&'a std::path::Path),
}

/// The driver configuration for one declared connection. It selects no TLS; the reader makes a
/// supplied client config mandatory.
#[must_use]
pub fn config(target: Target<'_>, port: u16, database: &str, user: &str, credential: &Secret) -> tokio_postgres::Config {
    let mut config = tokio_postgres::Config::new();
    match target {
        Target::Host(host) => config.host(String::from(host)),
        // A `/`-prefixed host is the driver's representation of a unix socket directory.
        Target::UnixSocket(directory) => config.host(directory.display().to_string()),
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "the credential's destination is a connection handshake, which is the one place the value itself is the payload"
    )]
    config
        .port(port)
        .dbname(String::from(database))
        .user(String::from(user))
        .password(credential.expose_secret());
    config
}

/// The `rustls::ClientConfig` a TLS channel verifies against `anchors` with, presenting `identity`.
///
/// # Errors
///
/// [`UnusableChannel`]: material `sutura_tls` cannot read, a root rustls cannot use, or a pair it
/// will not present together.
pub fn client_config(
    anchors: &sutura_tls::Anchors,
    identity: Option<&sutura_tls::Identity>,
) -> Result<rustls::ClientConfig, UnusableChannel> {
    let loaded = sutura_tls::load_anchors(anchors).map_err(UnusableChannel::Material)?;
    let identity = identity
        .map(sutura_tls::load_identity)
        .transpose()
        .map_err(UnusableChannel::Material)?;
    let mut roots = RootCertStore::empty();
    for certificate in loaded {
        roots.add(certificate).map_err(UnusableChannel::Root)?;
    }
    // An explicit provider rather than the process-global default, so this reader does not depend
    // on whether something else installed one first. `ring` is the one compiled in.
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(UnusableChannel::Verifier)?
        .with_root_certificates(roots);
    match identity {
        None => Ok(builder.with_no_client_auth()),
        Some(loaded) => {
            let (chain, key) = loaded.into_parts();
            builder.with_client_auth_cert(chain, key).map_err(UnusableChannel::Identity)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{UnusableChannel, client_config};

    /// A generated self-signed pair, as PEM, written to a scratch directory.
    struct Scratch {
        directory: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir().join(format!("sutura-rdbms-tls-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
            Self { directory }
        }

        /// A certificate plus its key, written to `prefix.crt` and `prefix.key`.
        fn pair(&self, prefix: &str) -> (PathBuf, PathBuf) {
            let issued = rcgen::generate_simple_self_signed([String::from("localhost")]).expect("a self-signed pair generates");
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
    fn a_bundle_and_a_matching_pair_build_a_mutual_client_config() {
        let scratch = Scratch::new("mutual");
        let (root, _) = scratch.pair("root");
        let (certificate, key) = scratch.pair("client");
        let anchors = sutura_tls::Anchors::Bundle(root);
        client_config(&anchors, None).expect("a bundle with one certificate builds");
        client_config(&anchors, Some(&sutura_tls::Identity::new(certificate, key))).expect("a pair builds");
    }

    #[test]
    fn a_missing_anchor_file_is_refused_naming_the_path() {
        let refused = client_config(&sutura_tls::Anchors::Bundle(PathBuf::from("/definitely/not/here.pem")), None);
        assert!(
            matches!(
                refused,
                Err(UnusableChannel::Material(sutura_tls::LoadError::AnchorsRead { ref path, .. })) if path == "/definitely/not/here.pem"
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_key_that_does_not_match_its_certificate_is_refused() {
        let scratch = Scratch::new("mismatched");
        let (root, _) = scratch.pair("root");
        let (certificate, _) = scratch.pair("a");
        let (_, key) = scratch.pair("b");
        let refused = client_config(
            &sutura_tls::Anchors::Bundle(root),
            Some(&sutura_tls::Identity::new(certificate, key)),
        );
        assert!(
            matches!(
                refused,
                Err(UnusableChannel::Identity(rustls::Error::InconsistentKeys(
                    rustls::InconsistentKeys::KeyMismatch
                )))
            ),
            "{refused:?}"
        );
    }
}
