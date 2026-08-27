//! In-process TLS termination: the listener, and the certificate rotation that keeps it up.
//!
//! Compiled only under the `tls` feature. See the manifest for why that feature is default-off -
//! briefly, because TLS is normally terminated by an ingress controller or a sidecar, so plaintext
//! on the pod network is the intended arrangement and the shipped artifacts should not carry a TLS
//! stack they do not use. This module is for the deployment where nothing terminates in front.
//!
//! # rustls, and `tokio-rustls` rather than `axum-server`
//!
//! rustls and not OpenSSL is decided by the shipped artifacts rather than by preference: two of the
//! four are musl, nixpkgs has no musl OpenSSL, and an `openssl-sys` edge would either fail the cross
//! build or need a vendored C OpenSSL per target. rustls needs no system library.
//!
//! Which rustls wrapper was a real choice, and it went to `tokio-rustls` on two counts.
//!
//! **Dependency count.** `tokio-rustls` is the *only* new crate in the graph: `rustls`,
//! `rustls-pki-types`, `rustls-webpki`, `ring` and `untrusted` are already resolved, because
//! `libduckdb-sys` carries `ureq` and `ureq` carries a TLS stack. `axum-server` would have added
//! itself, `hyper-util`, `rustls-pemfile` and `arc-swap` on top of the same rustls.
//!
//! **Graceful shutdown.** This is the heavier reason. `crate::server` has a *bounded* drain - the
//! deadline arms only after shutdown is asked for, and the serve future is dropped when it expires -
//! and it is built on `axum::serve(..).with_graceful_shutdown(..)`. `axum-server` does not compose
//! with that; it replaces it, with its own `Handle::graceful_shutdown(Some(duration))`. Taking it
//! would have given the TLS path a second, separately-implemented drain, so a change to the shutdown
//! semantics would have to be made twice and could be made once. Wrapping the listener instead
//! leaves `axum::serve`, `drain` and `report` exactly as they are: **one serve path and one drain,
//! for plaintext and for TLS.**
//!
//! What `axum-server` would have brought for free is its `RustlsConfig::reload_from_pem_file`. That
//! is replaced here by something narrower - see the rotation section - and the narrower version
//! refuses a pair that does not match, which upstream's does not.
//!
//! Two more decisions live in [`listener`] rather than here, because they are about accepting a
//! connection rather than about what certificate is presented on it: the handshake is done off the
//! accept path, and the peer address is carried through with `tap_io` so the rate limiter keys the
//! same way over TLS as it does over plaintext.
//!
//! # Rotation without dropping a connection
//!
//! A certificate expires and something replaces the files - `cert-manager` writing a Secret, or an
//! operator with a new pair. A restart is not zero downtime, so the listener has to pick the new
//! pair up in place.
//!
//! The mechanism is a [`rustls::server::ResolvesServerCert`] over a `tokio::sync::watch` channel.
//! `ServerConfig` is built **once** and never rebuilt; what changes is the `Arc<CertifiedKey>` the
//! resolver hands back, and it is read once per handshake. So a handshake in flight keeps the pair it
//! resolved and the next one gets the new pair - there is no window in which a connection is
//! serving half of each, and no connection is dropped.
//!
//! `tokio::sync::watch` and not `arc-swap`, which is the crate usually reached for here: `borrow()`
//! is synchronous, which is what the resolver needs, and `tokio` is already a dependency. A crate
//! for one atomic pointer swap is a crate. `std::sync::RwLock` is banned in `clippy.toml` and would
//! have been the wrong reach anyway.
//!
//! **A bad new pair does not take the listener down.** This is the half that matters more than the
//! rotation itself, because reloading into a broken state is worse than not reloading: every new
//! connection would fail and the old, working pair would be gone. So a candidate is fully built
//! before anything is swapped - parsed, the key loaded by the provider, and the key checked against
//! the certificate's `SubjectPublicKeyInfo` - and a candidate that fails any of that is logged at
//! `error` and discarded. The listener keeps serving what it was serving.
//!
//! # Polling, not `inotify`
//!
//! [`Renewal::poll_once`] compares the file *bytes* to the ones in use. That is deliberately not a
//! filesystem watch, and not for want of a crate:
//!
//! * Kubernetes replaces a projected Secret by building a new timestamped directory and swapping a
//!   symlink. An `inotify` watch registered on the *file* path follows the old inode and never fires;
//!   getting it right means watching the directory and interpreting rename events. Polling the path
//!   sees the new content on the next tick, with no cases.
//! * Comparing content rather than `mtime` costs a read of two small files on a slow interval and
//!   answers the question directly. An `mtime` that a writer preserved is a rotation that never
//!   happened.
//! * It adds no dependency.
//!
//! The cost is bounded staleness: up to [`RENEWAL_INTERVAL`] between the write and the swap. For a
//! certificate rotation, which is planned days ahead by whatever issues them, that is nothing.

pub mod listener;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sutura_config::TlsMaterial;
use sutura_runtime::Shutdown;
use tokio::sync::watch;
use tokio_rustls::rustls::crypto::CryptoProvider;
use tokio_rustls::rustls::pki_types::pem::PemObject as _;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::server::{ClientHello, ResolvesServerCert};
use tokio_rustls::rustls::sign::CertifiedKey;
use tokio_rustls::rustls::{self, ServerConfig};

pub use crate::tls::listener::TlsListener;

/// How often the certificate and key files are re-read.
///
/// A constant rather than a configuration key, for the reason `crate::middleware::REAP_INTERVAL` is
/// one: nothing a caller can do changes what the right answer is. Thirty seconds is far below any
/// horizon at which a certificate rotation is urgent - an issuer plans one days ahead - and far
/// above the cost of reading two small files.
pub const RENEWAL_INTERVAL: Duration = Duration::from_secs(30);

/// The one protocol this surface speaks, advertised.
///
/// Set explicitly rather than left empty. `axum` is compiled here with `http1` and no `http2`, so a
/// client that offered `h2` over ALPN and had it accepted would get a connection this process cannot
/// speak. Naming `http/1.1` is what makes the negotiation say so.
const ALPN_HTTP11: &[u8] = b"http/1.1";

/// Why the configured certificate and key are not usable TLS material.
///
/// Every variant names the path. `sutura_config::TlsMaterial` parses the *pair* - both halves or
/// neither - and stops there, because that crate reads no files; everything below is a question only
/// a TLS implementation can answer, and this is where they are all answered. Once, before the socket
/// is bound.
#[derive(Debug, thiserror::Error)]
pub enum TlsNotUsable {
    /// The file could not be read at all: absent, or not readable by this process.
    #[error("could not read {what} at {path}")]
    Unreadable {
        what: &'static str,
        path: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    /// The file was read and held no PEM certificate.
    #[error("{path} contains no PEM certificate")]
    NoCertificate { path: PathBuf },
    /// The file was read and held no PEM private key.
    #[error("{path} contains no PEM private key")]
    NoKey { path: PathBuf },
    /// A PEM block was found and did not parse.
    #[error("{what} at {path} is not valid PEM")]
    Malformed {
        what: &'static str,
        path: PathBuf,
        #[source]
        cause: rustls::pki_types::pem::Error,
    },
    /// The key does not belong to the certificate.
    ///
    /// **The variant this whole module is careful about.** rustls will build a `ServerConfig` from a
    /// mismatched pair without complaint; what fails is every handshake, at the client, with a
    /// signature error that says nothing about a configuration file. Checked here so it is a startup
    /// refusal - or, on a reload, a rejected candidate and a listener that keeps working.
    #[error("the private key at {key} does not match the certificate at {certificate}")]
    KeyDoesNotMatch {
        certificate: PathBuf,
        key: PathBuf,
        #[source]
        cause: rustls::Error,
    },
    /// The server configuration itself would not build.
    ///
    /// Unreachable from a pair that got this far, and an error rather than a panic for the reason
    /// `crate::middleware::LimiterNotBuilt` is one.
    #[error("the TLS server configuration would not build")]
    NotConfigurable {
        #[source]
        cause: rustls::Error,
    },
}

/// The raw bytes of a certificate and key, as read.
///
/// Kept so a reload can tell "the files changed" from "the files are the same" by comparing content,
/// which is the question, rather than by comparing a timestamp, which is a proxy for it. See the
/// module documentation on polling.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pem {
    certificate: Vec<u8>,
    key: Vec<u8>,
}

impl Pem {
    /// Reads both files.
    fn read(material: &TlsMaterial) -> Result<Self, TlsNotUsable> {
        Ok(Self {
            certificate: read_file("the TLS certificate chain", material.certificate())?,
            key: read_file("the TLS private key", material.key())?,
        })
    }

    /// Parses, loads the key and checks the two halves belong together.
    ///
    /// The whole validation, in one place, so a startup and a reload cannot disagree about what
    /// "usable" means.
    fn certify(&self, material: &TlsMaterial, provider: &CryptoProvider) -> Result<Arc<CertifiedKey>, TlsNotUsable> {
        let chain = CertificateDer::pem_slice_iter(&self.certificate)
            .collect::<Result<Vec<CertificateDer<'static>>, _>>()
            .map_err(|cause| TlsNotUsable::Malformed {
                what: "the TLS certificate chain",
                path: material.certificate().to_path_buf(),
                cause,
            })?;
        if chain.is_empty() {
            return Err(TlsNotUsable::NoCertificate {
                path: material.certificate().to_path_buf(),
            });
        }
        let key = PrivateKeyDer::from_pem_slice(&self.key).map_err(|cause| match cause {
            // `NoItemsFound` is a readable file with nothing in it that is a key, which is a
            // different mistake from a mangled one and worth a different sentence: it is what
            // pointing at the certificate twice looks like.
            rustls::pki_types::pem::Error::NoItemsFound => TlsNotUsable::NoKey {
                path: material.key().to_path_buf(),
            },
            cause => TlsNotUsable::Malformed {
                what: "the TLS private key",
                path: material.key().to_path_buf(),
                cause,
            },
        })?;
        // `from_der` loads the key through the provider AND compares the key's public half against
        // the end-entity certificate's `SubjectPublicKeyInfo`. That second half is the check rustls
        // does not make when a `ServerConfig` is built from a chain and a key directly, and its
        // absence is a listener that starts and cannot complete a handshake.
        CertifiedKey::from_der(chain, key, provider)
            .map(Arc::new)
            .map_err(|cause| TlsNotUsable::KeyDoesNotMatch {
                certificate: material.certificate().to_path_buf(),
                key: material.key().to_path_buf(),
                cause,
            })
    }
}

/// One file, with the path in the error.
fn read_file(what: &'static str, path: &Path) -> Result<Vec<u8>, TlsNotUsable> {
    std::fs::read(path).map_err(|cause| TlsNotUsable::Unreadable {
        what,
        path: path.to_path_buf(),
        cause,
    })
}

/// The certificate this listener presents, as rustls asks for it.
///
/// Holds a `watch::Receiver` rather than the key itself, which is the whole rotation mechanism: the
/// `ServerConfig` this lives inside is built once, and what a later handshake resolves to is
/// whatever was last sent. See the module documentation.
#[derive(Debug)]
struct Rotating {
    current: watch::Receiver<Arc<CertifiedKey>>,
}

impl ResolvesServerCert for Rotating {
    /// The pair in use at the moment this handshake asked.
    ///
    /// `Some` always. `None` aborts the handshake, and there is no state this can be in where that
    /// is right: a pair was validated before the socket was bound, and a rotation that did not
    /// validate never replaced it.
    fn resolve(&self, _client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(Arc::clone(&self.current.borrow()))
    }
}

/// A validated TLS configuration, and the means to keep it current.
///
/// **Two values because they have two owners**, the way `crate::router::Assembled` is two: the
/// configuration goes to the listener, and the renewal goes to whatever will poll it. [`prepare`]
/// hands both back rather than starting the poll itself, so a test can drive a rotation a step at a
/// time instead of waiting on a wall clock.
#[derive(Debug)]
pub struct Termination {
    config: Arc<ServerConfig>,
    renewal: Renewal,
}

impl Termination {
    /// Reads the pair, validates it, and builds the configuration around it.
    ///
    /// Called before the socket is bound, so a configuration mistake is a process that does not
    /// start rather than a port that accepts connections and fails every handshake.
    pub fn prepare(material: &TlsMaterial) -> Result<Self, TlsNotUsable> {
        // An explicit provider rather than `CryptoProvider::install_default`, which is process
        // global and would make this library's behaviour depend on whether something else got there
        // first. `ring` is the one compiled in; see the workspace manifest for why it and not
        // `aws-lc-rs`.
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let loaded = Pem::read(material)?;
        let certified = loaded.certify(material, &provider)?;
        let (sender, receiver) = watch::channel(certified);
        let mut config = ServerConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .map_err(|cause| TlsNotUsable::NotConfigurable { cause })?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(Rotating { current: receiver }));
        // See `ALPN_HTTP11`: this surface is compiled with `http1` alone, so `h2` must not be
        // negotiable.
        config.alpn_protocols = vec![ALPN_HTTP11.to_vec()];
        tracing::info!(
            certificate = %material.certificate().display(),
            key = %material.key().display(),
            "in-process TLS termination is configured"
        );
        Ok(Self {
            config: Arc::new(config),
            renewal: Renewal {
                material: material.clone(),
                provider,
                current: sender,
                seen: loaded,
            },
        })
    }

    /// The configuration a listener is built from.
    #[must_use]
    pub fn config(&self) -> Arc<ServerConfig> {
        Arc::clone(&self.config)
    }

    /// The two halves, for a caller that owns them separately.
    #[must_use]
    pub fn into_parts(self) -> (Arc<ServerConfig>, Renewal) {
        (self.config, self.renewal)
    }
}

/// What one look at the files decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renewed {
    /// The files are byte for byte what is already being served.
    Unchanged,
    /// The files changed, the new pair validated, and it is what new handshakes will use.
    Rotated,
    /// The files changed and the new pair is not usable. What was being served still is.
    Rejected,
}

/// Keeps the presented certificate current with what is on disk.
///
/// Owns the sending half of the channel the resolver reads, so this is the only thing that can
/// change what a handshake is offered.
#[derive(Debug)]
pub struct Renewal {
    material: TlsMaterial,
    provider: Arc<CryptoProvider>,
    current: watch::Sender<Arc<CertifiedKey>>,
    /// The last content examined - not necessarily the content in use.
    ///
    /// The distinction is what stops a broken pair from being logged once per interval forever: a
    /// candidate that was rejected is still *examined*, so the next look at identical bytes is
    /// `Unchanged` and says nothing. The first look at it said everything, loudly.
    seen: Pem,
}

impl Renewal {
    /// Looks at the files once.
    ///
    /// Public so a test can rotate a certificate without waiting for an interval to elapse, which
    /// is the difference between asserting that rotation works and asserting that a timer fires.
    pub fn poll_once(&mut self) -> Renewed {
        let Some(found) = self.reread() else {
            return Renewed::Unchanged;
        };
        if found == self.seen {
            return Renewed::Unchanged;
        }
        let outcome = match found.certify(&self.material, &self.provider) {
            Ok(certified) => {
                // `send_replace` rather than `send`: `send` fails when there are no receivers, and
                // "the listener is gone" is not a reason to fail to record the new pair.
                drop(self.current.send_replace(certified));
                self.announce_rotation();
                Renewed::Rotated
            }
            Err(cause) => {
                self.announce_rejection(&cause);
                Renewed::Rejected
            }
        };
        // Recorded whichever way it went. See the field documentation: a rejected candidate is
        // still an examined one, so identical bytes on the next tick are silent.
        self.seen = found;
        outcome
    }

    /// The files as they are now, or `None` if they could not be read.
    ///
    /// Unreadable is deliberately not "changed": a Secret being swapped can make a path briefly
    /// absent, and treating that as a rotation would be a rejection every time one happens. The old
    /// pair keeps serving and the next look sees the new files.
    fn reread(&self) -> Option<Pem> {
        match Pem::read(&self.material) {
            Ok(found) => Some(found),
            Err(cause) => {
                tracing::warn!(error = %cause, "could not re-read the TLS material; keeping the pair in use");
                None
            }
        }
    }

    /// Says a rotation happened.
    fn announce_rotation(&self) {
        tracing::info!(
            certificate = %self.material.certificate().display(),
            "the TLS certificate was replaced; new connections will present the new one"
        );
    }

    /// Says a rotation was refused, and what is still being served.
    ///
    /// Loud, and then carry on with what works. See the module documentation: reloading into a
    /// broken state means every new connection fails and the good pair is gone.
    fn announce_rejection(&self, cause: &TlsNotUsable) {
        tracing::error!(
            error = %cause,
            certificate = %self.material.certificate().display(),
            key = %self.material.key().display(),
            "the TLS material on disk changed and is NOT usable; still serving the previous \
             certificate. Fix the files - nothing will rotate until they parse and match"
        );
    }

    /// Polls on an interval until shutdown is asked for.
    ///
    /// A `tokio` task rather than the OS thread `crate::middleware::spawn_reaper` uses, and the
    /// difference is where it is started from: the reaper is spawned while the router is assembled,
    /// before a runtime exists, and this is spawned from inside `serve`.
    pub fn watch_until_shutdown(self, interval: Duration, shutdown: Shutdown) {
        drop(tokio::spawn(poll_until_shutdown(self, interval, shutdown)));
    }
}

/// The renewal watch's body.
///
/// A named function rather than an inline block so the lint exemption below has something to sit on
/// and a reason a reader can check.
#[expect(
    clippy::integer_division_remainder_used,
    reason = "the `select!` macro expands through remainder arithmetic to pick a poll order; nothing here does"
)]
async fn poll_until_shutdown(mut renewal: Renewal, interval: Duration, shutdown: Shutdown) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(interval) => { let _outcome = renewal.poll_once(); }
            reason = shutdown.requested() => {
                tracing::debug!(%reason, "the TLS renewal watch is stopping");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use sutura_config::TlsMaterial;
    use tokio_rustls::rustls::pki_types::pem::PemObject as _;
    use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
    use tokio_rustls::rustls::{ClientConfig, RootCertStore};

    use super::{Renewed, Termination, TlsListener};

    /// The name every generated certificate is issued for, and the name the client asks for.
    const SUBJECT: &str = "localhost";

    /// A generated self-signed pair, as PEM.
    struct Pair {
        certificate: String,
        key: String,
    }

    /// Generates a pair. A different one every call, which is what the mismatch test needs.
    fn generate() -> Pair {
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
    fn write(directory: &Path, pair: &Pair) -> TlsMaterial {
        let certificate = directory.join("chain.pem");
        let key = directory.join("key.pem");
        atomically(&certificate, pair.certificate.as_bytes());
        atomically(&key, pair.key.as_bytes());
        TlsMaterial::parse(certificate.to_str(), key.to_str())
            .expect("a written pair is a pair")
            .expect("a written pair is material")
    }

    fn atomically(path: &Path, bytes: &[u8]) {
        let staged = path.with_extension("staged");
        let mut file = std::fs::File::create(&staged).expect("a test file is creatable");
        file.write_all(bytes).expect("a test file is writable");
        file.sync_all().expect("a test file syncs");
        drop(file);
        std::fs::rename(&staged, path).expect("a test file renames");
    }

    /// A directory of this test's own, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("sutura-tls-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&path).expect("a scratch directory is creatable");
            Self(path)
        }

        fn path(&self) -> &Path {
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
    fn client_trusting(certificate: &str) -> Arc<ClientConfig> {
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
    async fn get_over_tls(port: u16, path: &str, certificate: &str) -> String {
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

    #[tokio::test(flavor = "multi_thread")]
    async fn a_real_client_completes_a_handshake_and_gets_an_answer_over_tls() {
        // THE test for this module. Not "the configuration parses" - a generated certificate, a
        // client that verifies it against a root store of one, a completed handshake, and a `200`
        // with the liveness body on the other side of it.
        let scratch = Scratch::new("handshake");
        let pair = generate();
        let material = write(scratch.path(), &pair);

        let termination = Termination::prepare(&material).expect("a generated pair is usable material");
        let tcp = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a loopback port is bindable");
        let port = tcp.local_addr().expect("a bound socket has an address").port();
        let listener = TlsListener::wrap(tcp, termination.config()).expect("the listener wraps");

        let router = axum::Router::new().route("/health", axum::routing::get(|| async { r#"{"status":"ok"}"# }));
        let serving = tokio::spawn(async move {
            let _outcome = axum::serve(
                axum::serve::ListenerExt::tap_io(listener, |_io| ()),
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await;
        });

        let answer = get_over_tls(port, "/health", &pair.certificate).await;
        assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
        assert!(answer.contains(r#"{"status":"ok"}"#), "{answer}");
        serving.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_peer_address_survives_the_tls_path_so_the_limiter_still_has_a_key() {
        // The property `client_address` depends on and `tap_io` exists for. Without a `Connected`
        // implementation reaching this listener there is no `ConnectInfo`, the limiter reports that
        // it cannot extract a key, and it bounds nothing - a limiter that looks configured and is
        // not. Asserted through a handler that reads the extractor, because the type checking alone
        // would not catch a listener whose address never arrives.
        let scratch = Scratch::new("connectinfo");
        let pair = generate();
        let material = write(scratch.path(), &pair);
        let termination = Termination::prepare(&material).expect("a generated pair is usable");
        let tcp = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a loopback port is bindable");
        let port = tcp.local_addr().expect("a bound socket has an address").port();
        let listener = TlsListener::wrap(tcp, termination.config()).expect("the listener wraps");

        let router = axum::Router::new().route(
            "/peer",
            axum::routing::get(
                |axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>| async move {
                    peer.ip().to_string()
                },
            ),
        );
        let serving = tokio::spawn(async move {
            let _outcome = axum::serve(
                axum::serve::ListenerExt::tap_io(listener, |_io| ()),
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await;
        });

        let answer = get_over_tls(port, "/peer", &pair.certificate).await;
        assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
        assert!(
            answer.contains("127.0.0.1"),
            "the peer address did not reach the handler: {answer}"
        );
        serving.abort();
    }

    #[test]
    fn a_key_that_does_not_match_its_certificate_is_refused_before_the_socket_is_bound() {
        // The failure rustls does NOT make for us. A `ServerConfig` built from a mismatched pair is
        // accepted and then fails every handshake, at the client, with a signature error that names
        // no file. Two generated pairs crossed is exactly what a certificate manager writing one
        // half of a rotation looks like.
        let scratch = Scratch::new("mismatch");
        let crossed = Pair {
            certificate: generate().certificate,
            key: generate().key,
        };
        let material = write(scratch.path(), &crossed);
        let error = Termination::prepare(&material).expect_err("a crossed pair is not usable material");
        assert!(
            matches!(error, super::TlsNotUsable::KeyDoesNotMatch { .. }),
            "a crossed pair should name the mismatch, not {error:?}"
        );
    }

    #[test]
    fn a_certificate_file_that_is_not_a_certificate_names_the_path() {
        let scratch = Scratch::new("garbage");
        let material = write(
            scratch.path(),
            &Pair {
                certificate: String::from("this is not a certificate\n"),
                key: generate().key,
            },
        );
        let error = Termination::prepare(&material).expect_err("prose is not a certificate");
        let rendered = format!("{error}");
        assert!(rendered.contains("chain.pem"), "{rendered}");
    }

    #[test]
    fn an_absent_file_is_a_refusal_naming_it_rather_than_a_plaintext_listener() {
        // No silent fallback. The whole point of validating before binding is that "TLS was asked
        // for and cannot be established" ends the process instead of opening a port somebody
        // believes is encrypted.
        let scratch = Scratch::new("absent");
        let material = TlsMaterial::parse(
            scratch.path().join("nothing.pem").to_str(),
            scratch.path().join("nokey.pem").to_str(),
        )
        .expect("two paths are a pair")
        .expect("two paths are material");
        let error = Termination::prepare(&material).expect_err("an absent file is not material");
        assert!(matches!(error, super::TlsNotUsable::Unreadable { .. }), "{error:?}");
    }

    #[test]
    fn a_replaced_pair_is_picked_up_and_a_broken_one_leaves_the_old_pair_serving() {
        // Both halves of the renewal requirement in one test, because they are one property: the
        // listener follows the files, EXCEPT when following them would break it.
        let scratch = Scratch::new("renewal");
        let first = generate();
        let material = write(scratch.path(), &first);
        let (_config, mut renewal) = Termination::prepare(&material)
            .expect("the first pair is usable")
            .into_parts();

        // Nothing changed, so nothing happens - and this is what stops a rotation from being
        // announced once a tick forever.
        assert_eq!(renewal.poll_once(), Renewed::Unchanged);

        // A genuine rotation.
        let second = generate();
        let rotated = write(scratch.path(), &second);
        assert_eq!(rotated, material, "the rotation must reuse the same paths");
        assert_eq!(renewal.poll_once(), Renewed::Rotated);
        assert_eq!(renewal.poll_once(), Renewed::Unchanged, "a rotation is announced once");

        // And now the case that must NOT be followed: the certificate is replaced without its key.
        let third = generate();
        let _ignored = write(
            scratch.path(),
            &Pair {
                certificate: third.certificate,
                key: second.key,
            },
        );
        assert_eq!(
            renewal.poll_once(),
            Renewed::Rejected,
            "a certificate whose key does not match it was accepted"
        );
        assert_eq!(
            renewal.poll_once(),
            Renewed::Unchanged,
            "a rejected pair must not be re-reported every tick"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_rotation_changes_what_a_client_is_presented_without_dropping_the_listener() {
        // The zero-downtime claim, asserted rather than reasoned about. The same listener, never
        // rebound, serves the first certificate and then the second - and a client that trusts only
        // the FIRST one stops being able to connect, which is what proves the swap really happened
        // rather than the old configuration still being in use.
        let scratch = Scratch::new("rotate-live");
        let first = generate();
        let material = write(scratch.path(), &first);
        let (config, mut renewal) = Termination::prepare(&material)
            .expect("the first pair is usable")
            .into_parts();
        let tcp = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a loopback port is bindable");
        let port = tcp.local_addr().expect("a bound socket has an address").port();
        let listener = TlsListener::wrap(tcp, config).expect("the listener wraps");
        let router = axum::Router::new().route("/health", axum::routing::get(|| async { r#"{"status":"ok"}"# }));
        let serving = tokio::spawn(async move {
            let _outcome = axum::serve(
                axum::serve::ListenerExt::tap_io(listener, |_io| ()),
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await;
        });

        assert!(
            get_over_tls(port, "/health", &first.certificate)
                .await
                .starts_with("HTTP/1.1 200 OK")
        );

        // Replace the pair on disk and let the renewal see it. Same process, same socket, no
        // rebinding and no restart.
        let second = generate();
        let _same_paths = write(scratch.path(), &second);
        assert_eq!(renewal.poll_once(), Renewed::Rotated);

        // The new certificate is what a handshake now gets.
        let answer = get_over_tls(port, "/health", &second.certificate).await;
        assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");

        // And the old one is genuinely gone, which is the half that makes the assertion above mean
        // something: a client trusting only the first certificate can no longer verify the chain.
        let connector = tokio_rustls::TlsConnector::from(client_trusting(&first.certificate));
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("the listener is still accepting");
        let name = ServerName::try_from(SUBJECT).expect("the subject is a server name");
        assert!(
            connector.connect(name, tcp).await.is_err(),
            "the listener is still presenting the certificate that was rotated away"
        );
        serving.abort();
    }
}
