//! What the TLS suite needs to have a certificate at all: a generated pair, a scratch
//! directory, a client that trusts exactly one root, and one HTTP/1.1 request by hand.
//!
//! **The HARNESS, and only the harness.** It lives out here because `tls.rs` reached the
//! unexemptable 1000-line cap and `xtask test-causality`'s own remedy for that is to move the
//! harness out and leave every assertion where it is: a `mod` declaration in a file the gate
//! reverts orphans whatever it declares, and Rust builds no unreferenced file - so a suite moved
//! out wholesale is never compiled against base and reads as green there.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sutura_config::TlsMaterial;
use tokio_rustls::rustls::pki_types::pem::PemObject as _;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

/// The name every generated certificate is issued for, and the name the client asks for.
pub(super) const SUBJECT: &str = "localhost";

/// A generated self-signed pair, as PEM.
pub(super) struct Pair {
    pub(super) certificate: String,
    pub(super) key: String,
}

/// Generates a pair. A different one every call, which is what the mismatch test needs.
pub(super) fn generate() -> Pair {
    let issued = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed pair generates");
    Pair {
        certificate: issued.cert.pem(),
        key: issued.signing_key.serialize_pem(),
    }
}

/// Writes a pair into a directory and returns the material pointing at it.
///
/// Written through a temporary name and renamed, because that is how a certificate manager
/// replaces one and it is the case the reload path has to survive: a reader must never see half
/// a file.
pub(super) fn write(directory: &Path, pair: &Pair) -> TlsMaterial {
    let certificate = directory.join("chain.pem");
    let key = directory.join("key.pem");
    atomically(&certificate, pair.certificate.as_bytes());
    atomically(&key, pair.key.as_bytes());
    TlsMaterial::parse(certificate.to_str(), key.to_str())
        .expect("a written pair is a pair")
        .expect("a written pair is material")
}

pub(super) fn atomically(path: &Path, bytes: &[u8]) {
    let staged = path.with_extension("staged");
    let mut file = std::fs::File::create(&staged).expect("a test file is creatable");
    file.write_all(bytes).expect("a test file is writable");
    file.sync_all().expect("a test file syncs");
    drop(file);
    std::fs::rename(&staged, path).expect("a test file renames");
}

/// A directory of this test's own, removed when the test ends.
pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sutura-tls-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("a scratch directory is creatable");
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A failure here would mask the assertion that failed first, and a leftover directory
        // under the temporary directory is not a problem worth that.
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// A client that trusts exactly the certificate it was given, and nothing else.
///
/// A real verifier over a root store of one, rather than a verifier that accepts anything: the
/// point of the handshake test is that the chain the listener presents is the chain this pair
/// describes, and a client that skips verification could not tell.
pub(super) fn client_trusting(certificate: &str) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    for entry in CertificateDer::pem_slice_iter(certificate.as_bytes()) {
        roots
            .add(entry.expect("a generated certificate parses"))
            .expect("a generated certificate is a root");
    }
    Arc::new(
        ClientConfig::builder_with_provider(Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("the default protocol versions are safe")
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// Speaks HTTP/1.1 over a TLS connection, by hand, and returns what came back.
///
/// By hand because this crate has no HTTP client and does not need one for this: the assertion
/// is that a real handshake completed and that the bytes on the other side of it are a real
/// response, and thirty lines of `write_all` prove that better than a client library would.
pub(super) async fn get_over_tls(port: u16, path: &str, certificate: &str) -> String {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let connector = tokio_rustls::TlsConnector::from(client_trusting(certificate));
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("the listener is accepting");
    let name = ServerName::try_from(SUBJECT).expect("the subject is a server name");
    let mut tls = connector.connect(name, tcp).await.expect("the TLS handshake completes");
    tls.write_all(format!("GET {path} HTTP/1.1\r\nHost: {SUBJECT}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .expect("the request is writable");
    tls.flush().await.expect("the request flushes");
    let mut answer = Vec::new();
    // `read_to_end` terminates because the request asked for `Connection: close`.
    tls.read_to_end(&mut answer).await.expect("the response is readable");
    String::from_utf8_lossy(&answer).into_owned()
}

/// The first base64 line of a PEM block.
///
/// A whole block is the wrong thing to search a `Debug` rendering for: `Debug` escapes a newline
/// as two characters, so the block never appears verbatim even where it leaked. One body line is
/// long, specific to this key, and survives the escaping.
pub(super) fn a_body_line(pem: &str) -> &str {
    pem.lines()
        .find(|line| line.len() > 32 && !line.starts_with('-'))
        .expect("a generated PEM block has a body")
}

/// The same line as a derived `Debug` over `Vec<u8>` would print it.
///
/// **The hard half of the assertion.** A redaction checked against the PEM text alone passes
/// while the bytes are printed as `[45, 45, 45, ..]` - which is exactly what the derive over
/// `Vec<u8>` did, and exactly what a reader would not recognise as a private key.
pub(super) fn as_a_byte_vector(line: &str) -> String {
    line.as_bytes().iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
}
