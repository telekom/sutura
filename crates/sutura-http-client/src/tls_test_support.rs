//! A real loopback TLS server and an `rcgen`-issued self-signed leaf.
//!
//! The fixture shape every one of this workspace's outbound-TLS suites rests on, `pub` behind this
//! crate's `tls-test` feature.
//!
//! **Moved here (issue #970's review) from `sutura-catalog-datahub`'s and
//! `sutura-catalog-openmetadata`'s own `tests/http_reader.rs::tls_anchors` modules**, which
//! `cargo xtask check-jscpd` measured as byte-for-byte identical wire plumbing once the second
//! reader carried its own copy.
//!
//! **A dev-dependency-only feature, unlike [`crate::test_support`].** Nothing outside a reader's
//! OWN `tests/http_reader.rs` needs this - `sutura-cli`'s served suite only dials the plaintext
//! fake behind [`crate::test_support`] - so a catalog crate takes `sutura-http-client` with this
//! feature in `[dev-dependencies]` only, never folded into its own `http` feature: no `rcgen`/
//! `rustls` object code reaches a `--features http` build that did not already carry `rustls`
//! transitively through `ureq`.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test: a TLS loopback harness shared by both catalog HTTP readers' own \
              `tests/http_reader.rs::tls_anchors` cells. Dev-dependency-only, so its panics and \
              slices are how a test asserts invariants and never reach a shipped binary."
)]

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use crate::test_support::Scripted;

/// A self-signed leaf whose SAN names the IP literal the reader dials (`127.0.0.1`).
///
/// Freshly generated per call. `security.outbound` verification is by dial, so the SAN must match
/// the dialed address - a DNS-name-only cert would not verify against `https://127.0.0.1:<port>`.
pub struct Issued {
    certificate: rcgen::Certificate,
    key: rcgen::KeyPair,
}

#[must_use]
pub fn issue() -> Issued {
    // `CertificateParams::new` reads a string SAN as an IP when it parses as one, so "127.0.0.1"
    // becomes `SanType::IpAddress` - the SAN rustls checks an IP dial against.
    let params =
        rcgen::CertificateParams::new([String::from("127.0.0.1")]).expect("an IP subject alternative name parameterizes");
    let key = rcgen::KeyPair::generate().expect("a key pair generates");
    let certificate = params.self_signed(&key).expect("a self-signed leaf signs");
    Issued { certificate, key }
}

#[must_use]
pub fn server_config(issued: &Issued) -> Arc<rustls::ServerConfig> {
    let chain: Vec<CertificateDer<'static>> = vec![issued.certificate.der().clone()];
    let key = PrivateKeyDer::try_from(issued.key.serialize_der()).expect("a generated key is a usable private key");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the default protocol versions are safe")
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .expect("a freshly generated chain and its own key are a usable pair"),
    )
}

/// A scratch directory a test owns, removed when it ends.
pub struct Scratch(PathBuf);

impl Scratch {
    #[must_use]
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sutura-catalog-tls-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("a scratch directory is creatable");
        Self(path)
    }

    /// Writes a declared PEM bundle of exactly one certificate and returns its path - what a
    /// composition root would read via `sutura_tls::load_anchors(&Anchors::Bundle(path))`.
    #[must_use]
    pub fn bundle(&self, name: &str, certificate: &rcgen::Certificate) -> PathBuf {
        let path = self.0.join(format!("{name}.pem"));
        std::fs::write(&path, certificate.pem()).expect("a bundle writes");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// A loopback TLS server presenting one leaf and answering the scripted pages a `read()` makes.
///
/// The SAME happy-path corpus [`crate::test_support::FakeServer`] serves, over TLS, so these cells
/// prove the same read every other test certifies completes under a declared bundle.
///
/// Bounded by the answer count AND a deadline: a refused-handshake cell (the client never sends a
/// request and never reconnects after the refusal) must not hang the serve thread.
pub struct TlsFakeServer {
    addr: SocketAddr,
    handle: Option<thread::JoinHandle<()>>,
}

impl TlsFakeServer {
    pub fn start(issued: &Issued, answers: Vec<Scripted>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let addr = listener.local_addr().expect("a bound listener has a local address");
        let config = server_config(issued);
        let handle = thread::spawn(move || serve_until(&listener, &config, &answers));
        Self {
            addr,
            handle: Some(handle),
        }
    }

    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("https://{}", self.addr)
    }
}

impl Drop for TlsFakeServer {
    fn drop(&mut self) {
        let _ignored = self.handle.take();
    }
}

fn serve_until(listener: &TcpListener, config: &Arc<rustls::ServerConfig>, answers: &[Scripted]) {
    let _ignored = listener.set_nonblocking(true);
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut next = 0;
    while next < answers.len() && Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, _)) => {
                // On Darwin, an accepted socket inherits the LISTENER's non-blocking flag - undone
                // here so the read/write calls below block normally rather than racing a
                // `WouldBlock` mid-handshake or mid-response.
                let _ignored = stream.set_nonblocking(false);
                serve_connection(stream, config, &answers[next]);
                next += 1;
            }
            Err(cause) if cause.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn serve_connection(stream: TcpStream, config: &Arc<rustls::ServerConfig>, answer: &Scripted) {
    let mut tcp: TcpStream = stream;
    let mut connection = rustls::ServerConnection::new(Arc::clone(config)).expect("a server connection builds");
    {
        let mut tls = rustls::Stream::new(&mut connection, &mut tcp);
        let mut request = [0_u8; 2048];
        // Reading first drives the handshake to completion and consumes the client's request; a
        // client that refused the handshake (an untrusted issuer) errors here, which is exactly
        // what the negative cells below provoke - discarded rather than panicked on.
        let _ignored = tls.read(&mut request);
        let head = format!(
            "HTTP/1.1 {} OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            answer.status_code(),
            answer.body().len()
        );
        let _ignored = tls.write_all(head.as_bytes());
        let _ignored = tls.write_all(answer.body());
    }
    // A graceful `close_notify` before the socket drops - without it, a client whose read completes
    // the whole body still sees an `UnexpectedEof` from rustls's own truncation guard (RFC 8446
    // 6.1) rather than a clean end of stream.
    connection.send_close_notify();
    let _ignored = connection.complete_io(&mut tcp);
}

/// Loads a scratch-written bundle carrying `issued`'s own certificate, the way a composition root
/// would from a declared `security.outbound.transport_anchors` bundle path.
///
/// # Panics
///
/// The freshly written bundle fails to load - a test invariant, never a production path.
#[must_use]
pub fn declared_anchors(scratch: &Scratch, name: &str, issued: &Issued) -> sutura_tls::LoadedAnchors {
    let bundle = scratch.bundle(name, &issued.certificate);
    sutura_tls::load_anchors(&sutura_tls::Anchors::Bundle(bundle)).expect("the freshly written bundle loads")
}
