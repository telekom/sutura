//! A link whose call ended without a reply is dialled again by the next call, and one whose call
//! was answered is not. Each cell dials a node that answers over TLS with headers alone, and counts
//! the connections it was dialled on.
//!
//! # Limits
//!
//! The node lets any peer in, so mutual TLS is not exercised here.

use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::time::Duration;
use std::sync::Arc;

use datafusion_distributed::{ChannelResolver as _, GetWorkerInfoRequest};
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tonic::codegen::http::Response;
use url::Url;

use super::{Bounded, Links, NodeIdentity};
use crate::pool::WorkingSet;

const WAIT: Duration = Duration::from_secs(10);

/// A node on localhost, what it was sent, and the links that dial it.
struct Serving {
    url: Url,
    /// The connections it accepted.
    dials: Arc<AtomicUsize>,
    /// The calls it received, answered or not.
    asked: Arc<AtomicUsize>,
    links: Bounded,
}

/// A node whose first `silent` connections never answer a call and whose others answer each with
/// headers alone.
async fn serving(silent: usize) -> Serving {
    let mut params = CertificateParams::new(Vec::<String>::new()).expect("no names are valid parameters");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let authority =
        CertifiedIssuer::self_signed(params, KeyPair::generate().expect("a key generates")).expect("an authority signs itself");
    let key = KeyPair::generate().expect("a key generates");
    let certificate = CertificateParams::new(vec![String::from("127.0.0.1")])
        .expect("an address is valid parameters")
        .signed_by(&key, &authority)
        .expect("the authority signs a node");
    let pkcs8 = || PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    let identity = NodeIdentity::new(vec![certificate.der().clone()], pkcs8(), [authority.der().clone()])
        .expect("a signed node is an identity");
    let mut config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring offers the safe versions")
        .with_no_client_auth()
        .with_single_cert(vec![certificate.der().clone()], pkcs8())
        .expect("a signed node is a listener's identity");
    config.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("a port binds");
    let port = listener.local_addr().expect("a bound port has an address").port();
    let (dials, asked) = (Arc::<AtomicUsize>::default(), Arc::<AtomicUsize>::default());
    let (counted, heard) = (Arc::clone(&dials), Arc::clone(&asked));
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let nth = counted.fetch_add(1, Ordering::SeqCst);
            let (acceptor, heard) = (acceptor.clone(), Arc::clone(&heard));
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else { return };
                let Ok(mut connection) = h2::server::handshake(tls).await else {
                    return;
                };
                while let Some(Ok((request, mut respond))) = connection.accept().await {
                    heard.fetch_add(1, Ordering::SeqCst);
                    if nth < silent {
                        tokio::spawn(async move {
                            let _unanswered = (request, respond);
                            core::future::pending::<()>().await;
                        });
                    } else {
                        let reply = Response::builder()
                            .header("content-type", "application/grpc")
                            .header("grpc-status", "0")
                            .body(())
                            .expect("a reply is a response");
                        drop(respond.send_response(reply, true));
                    }
                }
            });
        }
    });
    Serving {
        url: Url::parse(&format!("https://127.0.0.1:{port}")).expect("a socket address is a URL's authority"),
        dials,
        asked,
        links: Links::of(identity).bounded(WorkingSet::of_bytes(NonZeroUsize::MIN)),
    }
}

/// A question's call on `node`, running until it is answered or aborted.
fn call(node: &Serving) -> JoinHandle<()> {
    let (links, url) = (node.links.clone(), node.url.clone());
    tokio::spawn(async move {
        drop(
            links
                .get_worker_client_for_url(&url)
                .await
                .expect("a node URL is a link")
                .get_worker_info(GetWorkerInfoRequest {})
                .await,
        );
    })
}

/// Waits until `count` reaches `wanted`.
async fn reached(count: &AtomicUsize, wanted: usize) {
    tokio::time::timeout(WAIT, async {
        while count.load(Ordering::SeqCst) < wanted {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the node was sent what the cell waits for");
}

/// Aborts `call`, returning once the task has dropped it.
async fn given_up_on(call: JoinHandle<()>) {
    call.abort();
    assert!(
        call.await.is_err_and(|ended| ended.is_cancelled()),
        "a call ended before it was given up on"
    );
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime builds")
}

#[test]
fn a_call_after_one_given_up_on_while_it_dialled_dials_the_node_again() {
    runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("a port binds");
        let address = listener.local_addr().expect("a bound port has an address");
        let node = Serving {
            url: Url::parse(&format!("https://{address}")).expect("a socket address is a URL's authority"),
            dials: Arc::default(),
            asked: Arc::default(),
            links: Links::of(NodeIdentity::signed_for_localhost()).bounded(WorkingSet::of_bytes(NonZeroUsize::MIN)),
        };

        let first = call(&node);
        // Accepted and never answered, so the first call's handshake waits on it.
        let (held, _) = listener.accept().await.expect("the first call dials the node");
        given_up_on(first).await;
        // The dial the first call gave up on now fails, with no call waiting for it.
        drop(held);

        let second = call(&node);
        let dialled = tokio::time::timeout(WAIT, listener.accept()).await;
        second.abort();
        assert!(
            dialled.is_ok(),
            "the next call was handed how the given-up dial ended, and dialled nothing"
        );
    });
}

#[test]
fn a_call_that_got_a_reply_keeps_its_channel_so_the_next_call_dials_nothing() {
    runtime().block_on(async {
        let node = serving(0).await;

        call(&node).await.expect("the first call ends");
        call(&node).await.expect("the second call ends");

        assert_eq!(
            node.dials.load(Ordering::SeqCst),
            1,
            "an answered call forgot its channel, so the next one dialled again"
        );
        assert_eq!(node.asked.load(Ordering::SeqCst), 2, "each call reached the node");
    });
}

#[test]
fn a_call_given_up_on_forgets_only_the_channel_it_was_dialled_on_and_not_one_dialled_since() {
    runtime().block_on(async {
        // Its first connection never answers, so both calls on it wait, each holding that dial.
        let node = serving(1).await;
        let (older, other) = (call(&node), call(&node));
        reached(&node.asked, 2).await;

        given_up_on(older).await;
        // The channel the older call forgot is dialled again, and the node answers on it.
        call(&node).await.expect("the call on the new channel ends");
        assert_eq!(
            node.dials.load(Ordering::SeqCst),
            2,
            "the forgotten channel was not dialled again"
        );

        // The other call, still holding the forgotten dial, is given up on after the new one was made.
        given_up_on(other).await;
        call(&node).await.expect("the call after it ends");
        assert_eq!(
            node.dials.load(Ordering::SeqCst),
            2,
            "a call given up on forgot a channel dialled after its own"
        );
    });
}
