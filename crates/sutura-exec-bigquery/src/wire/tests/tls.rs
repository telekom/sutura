//! `security.outbound.transport_anchors` (`github.com/telekom/sutura#125`): the hermetic cells
//! `wire.rs`'s own module header says every OTHER test in this suite cannot reach.
//!
//! **Why this is a `WireAgent`-level cell and not a `BigQueryWire` one.** `HOST` is a compile-time
//! constant and the agent is built `https_only`, so nothing that goes through `BigQueryWire::submit`
//! can be pointed at a loopback listener - `wire.rs`'s header and `wire/tests.rs`'s own state that.
//! What every cell below asks instead is the narrower, still load-bearing question underneath it:
//! does the `ureq::Agent` `WireAgent::secured` builds actually verify a peer against the anchors it
//! was given? Reached through [`WireAgent::agent`], `pub(crate)` and visible here because this file
//! is a unit test of the crate that owns it, not an integration test in a separate crate - which is
//! also why it is a submodule under `#[cfg(test)] mod tests;` rather than `crates/.../tests/tls.rs`.
//!
//! **A real TLS handshake, never a mocked one** - `AGENTS.md`'s "ports get fakes, not mocked HTTP".
//! Each cell binds a real loopback `TcpListener`, drives a real `rustls::ServerConnection` over it by
//! hand, and dials it with the real `ureq::Agent` a declared `security.outbound` produces - the same
//! `rcgen`-issued self-signed fixture shape `sutura-tls`'s own suite and
//! `sutura-exec-postgres/src/tests.rs`'s hermetic negative both already rest on. What is asserted is
//! the SHAPE of a real handshake outcome (accepted or refused), never a canned HTTP response - the
//! server answers just enough bytes to let `ureq`'s own `.call()` return, and no cell here reads the
//! body.
//!
//! **The limit, next to the claim.** These cells are `sutura_tls::load_anchors` and
//! `WireAgent::secured` proven together, over a loopback server. They are not proof that Google's own
//! endpoint is reachable under a declared bundle - no local test can be, for the reason `HOST` being
//! a compile-time constant already gives.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sutura_tls::Anchors;

use crate::wire::WireAgent;

use super::bounds;

/// Every certificate issued below carries this name, and every URL in this file dials it: `ureq`'s
/// hostname-derived `ServerName` verification needs the dialed name and the certificate's name to
/// agree, and `"localhost"` resolves to the loopback interface with no hosts-file entry needed on
/// every venue this runs on.
const SUBJECT: &str = "localhost";

/// A scratch directory this test owns, removed when it ends - the same fixture shape
/// `sutura_tls`'s own suite uses, reused rather than copied a third time in this workspace.
pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sutura-bq-outbound-tls-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("a scratch directory is creatable");
        Self(path)
    }

    /// Writes a declared PEM bundle of exactly one certificate and returns its path - what a
    /// composition root would read via `sutura_tls::load_anchors(&Anchors::Bundle(path))`.
    pub(super) fn bundle(&self, name: &str, certificate: &rcgen::Certificate) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, certificate.pem()).expect("a bundle writes");
        path
    }

    /// The absolute path a bundle named `name` would be written to - for the rotation cells that
    /// REWRITE a bundle in place.
    pub(super) fn path(&self, name: &str) -> PathBuf {
        self.0.join(format!("{name}.pem"))
    }

    /// Writes `issued`'s certificate and private key as a declared client identity pair and
    /// returns both paths - what `sutura_tls::Identity::new` reads.
    pub(super) fn identity(&self, prefix: &str, issued: &Issued) -> (PathBuf, PathBuf) {
        let certificate = self.0.join(format!("{prefix}.crt"));
        let key = self.0.join(format!("{prefix}.key"));
        std::fs::write(&certificate, issued.certificate.pem()).expect("a certificate writes");
        std::fs::write(&key, issued.key.serialize_pem()).expect("a key writes");
        (certificate, key)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// A self-signed issuer and the leaf it signs for [`SUBJECT`], both freshly generated - a different
/// pair every call, which is what lets a test declare one root and present a chain from another.
pub(super) struct Issued {
    pub(super) certificate: rcgen::Certificate,
    key: rcgen::KeyPair,
}

pub(super) fn issue() -> Issued {
    let generated = rcgen::generate_simple_self_signed([String::from(SUBJECT)]).expect("a self-signed pair generates");
    Issued {
        certificate: generated.cert,
        key: generated.signing_key,
    }
}

/// A `rustls::ServerConfig` presenting exactly this one certificate, no client authentication asked
/// for - `security.outbound` carries no client identity, so nothing here needs to test one.
pub(super) fn server_config(issued: &Issued) -> Arc<rustls::ServerConfig> {
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

/// A `rustls::ServerConfig` presenting `server`'s certificate and REQUIRING a client certificate
/// signed by `client_root` - the mutual-TLS peer `identity.rs`'s cells dial, to prove a declared
/// `security.outbound` client identity is actually PRESENTED rather than merely built.
///
/// `client_root` is a self-signed certificate added directly to the verifier's root store, the
/// same "trust it directly" shape [`server_config`]'s own caller uses for the server side: a
/// self-signed certificate is its own issuer, so it is a usable root with no separate CA needed.
pub(super) fn server_config_requiring_client_auth(
    server: &Issued,
    client_root: &rcgen::Certificate,
) -> Arc<rustls::ServerConfig> {
    let chain: Vec<CertificateDer<'static>> = vec![server.certificate.der().clone()];
    let key = PrivateKeyDer::try_from(server.key.serialize_der()).expect("a generated key is a usable private key");
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(client_root.der().clone())
        .expect("the client's self-signed certificate is a usable root");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    // `builder_with_provider`, not the bare `builder`: the bare form asks rustls to AUTO-DETECT the
    // process-level `CryptoProvider` from which crate feature is on, and it panics rather than
    // guessing once more than one is - which `github.com/telekom/sutura#127`'s `oracledb` dependency
    // makes true workspace-wide (`oracledb` requires `rustls`'s own default features, and those
    // default to `aws_lc_rs`, alongside the `ring` this crate's own fixtures already pin explicitly
    // two lines below). Naming the SAME provider here is what keeps this fixture's behaviour fixed
    // to `ring` regardless of which other providers a `--workspace --all-features` build compiles
    // into the same test binary.
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
        .build()
        .expect("a non-empty root store builds a client verifier");
    Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the default protocol versions are safe")
            .with_client_cert_verifier(verifier)
            .with_single_cert(chain, key)
            .expect("a freshly generated chain and its own key are a usable pair"),
    )
}

/// Binds a loopback listener and returns it with the port a dial reaches it on.
pub(super) fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener binds");
    let port = listener.local_addr().expect("a bound listener has a local address").port();
    (listener, port)
}

/// Accepts ONE connection, completes the TLS handshake and answers with just enough bytes for
/// `ureq`'s own `.call()` to return - never a canned document a test then has to read, because the
/// claim here is about the HANDSHAKE, not about a response body.
pub(super) fn serve_one(listener: TcpListener, config: Arc<rustls::ServerConfig>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("the client dials this listener");
        let mut connection = rustls::ServerConnection::new(config).expect("a server connection builds");
        let mut tcp: TcpStream = stream;
        let mut tls = rustls::Stream::new(&mut connection, &mut tcp);
        // Reading first is what drives the handshake to completion and consumes the client's
        // request; the byte count and the request text are not this file's claim, so both are
        // discarded - a request that never arrives (the client refused the handshake before
        // sending one) is exactly the negative cells below exercise, and this read simply returns
        // an error in that case, which this thread also discards rather than panicking on.
        let mut buffer = [0_u8; 1024];
        let _ignored = tls.read(&mut buffer);
        let _ignored = tls.write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
    })
}

/// The URL every cell dials: this crate's own [`WireAgent`], pointed at the loopback server rather
/// than at `HOST` - which `WireAgent` itself never allows; what is dialed here is the `ureq::Agent`
/// underneath it, reached through the crate-visible accessor.
pub(super) fn dial(agent: &WireAgent, port: u16) -> Result<(), Box<ureq::Error>> {
    agent
        .agent()
        .get(format!("https://{SUBJECT}:{port}/"))
        .call()
        .map(|_response| ())
        .map_err(Box::new)
}

#[test]
fn a_declared_bundle_is_trusted_and_the_handshake_completes() {
    let scratch = Scratch::new("trusted");
    let issued = issue();
    let bundle = scratch.bundle("root", &issued.certificate);
    let (listener, port) = listener();
    let server = serve_one(listener, server_config(&issued));

    let anchors = sutura_tls::load_anchors(&Anchors::Bundle(bundle)).expect("the freshly written bundle loads");
    let agent = WireAgent::secured(bounds(), Some(anchors));

    let answered = dial(&agent, port);
    server.join().expect("the fake server thread does not panic");
    answered.expect("a peer signed by the declared bundle is trusted");
}

#[test]
fn absent_anchors_are_the_compiled_in_default_and_refuse_a_self_signed_peer() {
    // The regression floor `WireAgent::pinned` always held, restated over `secured(.., None)` now
    // that it is written in terms of it: `ureq`'s own `RootCerts::WebPki` trusts a real certificate
    // authority and refuses a random self-signed peer - which this loopback server always is, so a
    // real "connects to a real host" cell needs no live network to make the same point on the
    // refusal side. `HOST` being unreachable from a test is exactly why only the refusal half of
    // that claim is provable locally; the acceptance half is `tests/acceptance.rs`'s own claim,
    // against the real endpoint, unchanged by this file.
    let issued = issue();
    let (listener, port) = listener();
    let server = serve_one(listener, server_config(&issued));

    let agent = WireAgent::secured(bounds(), None);

    let answered = dial(&agent, port);
    server.join().expect("the fake server thread does not panic");
    // The message text is `ureq`'s and `rustls`'s own, so only the SHAPE is pinned here: a refusal,
    // and not `ConnectionFailed` - the generic fallback `ureq` reaches only after a lower-level cause
    // could not be classified at all, which is not what an untrusted-issuer handshake is.
    let error = answered.expect_err("no declared anchor trusts an ad hoc self-signed peer");
    assert!(
        !matches!(*error, ureq::Error::ConnectionFailed),
        "an untrusted-issuer handshake should be classified, not fall back to the generic failure: {error}"
    );
}

#[test]
fn a_declared_bundle_still_refuses_an_issuer_it_does_not_name() {
    // What the cell above alone could not rule out: a store that trusted whatever came back
    // regardless of issuer would pass it for the wrong reason. The server presents a chain signed by
    // a DIFFERENT freshly generated root than the one declared.
    let scratch = Scratch::new("untrusted-issuer");
    let presented = issue();
    let declared = issue();
    let bundle = scratch.bundle("declared-root", &declared.certificate);
    let (listener, port) = listener();
    let server = serve_one(listener, server_config(&presented));

    let anchors = sutura_tls::load_anchors(&Anchors::Bundle(bundle)).expect("the freshly written bundle loads");
    let agent = WireAgent::secured(bounds(), Some(anchors));

    let answered = dial(&agent, port);
    server.join().expect("the fake server thread does not panic");
    answered.expect_err("a chain signed by an issuer the declared bundle does not name must be refused");
}
