//! The authority whose node certificates the listener accepts from a client.
//!
//! **Asked for, never required.** A caller with no certificate completes the handshake as it did
//! before, so every route keeps answering one. A caller that presents a certificate the authority
//! did not sign is refused in the handshake, before a request is read.
//!
//! **Limits.** The authority is read once, when the listener is prepared: replacing the file needs a
//! restart, unlike the listener's own pair. And a route is told only that a node connected
//! (`crate::server::NodeCertified`), not which one.

use std::sync::Arc;

use sutura_config::TlsMaterial;
use tokio_rustls::rustls::RootCertStore;
use tokio_rustls::rustls::crypto::CryptoProvider;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::pki_types::pem::PemObject as _;
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::server::danger::ClientCertVerifier;

use super::{TlsNotUsable, read_file};

/// How the listener checks a client's certificate.
type Verifier = Arc<dyn ClientCertVerifier>;

/// What the listener asks a client for: a certificate the node authority signed, or none at all.
/// Without an authority it asks for nothing.
pub(super) fn verifier(material: &TlsMaterial, provider: &Arc<CryptoProvider>) -> Result<Verifier, TlsNotUsable> {
    let Some(path) = material.node_authority() else {
        return Ok(WebPkiClientVerifier::no_client_auth());
    };
    let what = "the node authority";
    let pem = read_file(what, path)?;
    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(&pem) {
        let certificate = certificate.map_err(|cause| TlsNotUsable::Malformed {
            what,
            path: path.to_path_buf(),
            cause,
        })?;
        roots.add(certificate).map_err(|cause| TlsNotUsable::NotAnAuthority {
            path: path.to_path_buf(),
            cause,
        })?;
    }
    if roots.is_empty() {
        return Err(TlsNotUsable::NoCertificate {
            path: path.to_path_buf(),
        });
    }
    // The builder refuses only an empty root store, ruled out above, and a revocation list, of
    // which none is given.
    WebPkiClientVerifier::builder_with_provider(Arc::new(roots), Arc::clone(provider))
        .allow_unauthenticated()
        .build()
        .map_err(|cause| TlsNotUsable::NotConfigurable {
            cause: tokio_rustls::rustls::Error::General(cause.to_string()),
        })
}

/// `#[cfg(test)]` on the declaration for `telekom/sutura#657`'s reason.
#[cfg(test)]
mod tests;
