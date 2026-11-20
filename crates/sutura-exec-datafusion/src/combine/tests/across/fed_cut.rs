//! A feed cut on a served link, under an operator above the fed leaf, reaches the planning node as a
//! lost node: the refusal's code survives the worker's own gRPC reply and the consumer stage.

use core::num::NonZeroU64;
use core::time::Duration;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::task::{Context, Poll};

use datafusion_distributed::Worker;
use futures_util::stream;
use sutura_domain::plan::FederationCombiner as _;
use tokio::sync::watch;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tonic::body::Body;
use tonic::codegen::http;
use tower::util::MapRequestLayer;
use url::Url;

use super::{Authority, Tls, UNBOUNDED, assigned, legs, listening, roomy, served, servers, sum_plan};
use crate::combine::{DataFusionCombiner, Nodes};
use tonic::transport::Server;

/// What a cut waits for.
type Gate = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A call's body that passes its first gRPC message, the stage's plan, and then ends once `released`
/// is true and a moment has passed: whatever the planning node feeds after the plan never arrives, and
/// the stream ends as one the planning node closed.
struct CutAfterThePlan {
    inner: Body,
    prefix: Vec<u8>,
    total: Option<usize>,
    passed: usize,
    cut: Gate,
}

impl http_body::Body for CutAfterThePlan {
    type Data = tonic::codegen::Bytes;
    type Error = <Body as http_body::Body>::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = &mut *self;
        if this.total.is_some_and(|total| this.passed >= total) {
            return this.cut.as_mut().poll(cx).map(|()| None);
        }
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                Ok(mut data) => {
                    if this.total.is_none() {
                        let missing = 5_usize.saturating_sub(this.prefix.len());
                        this.prefix.extend(data.iter().take(missing));
                        if this.prefix.len() == 5
                            && let Some(length) = this.prefix.get(1..).and_then(|tail| <[u8; 4]>::try_from(tail).ok())
                        {
                            #[expect(
                                clippy::big_endian_bytes,
                                reason = "the gRPC frame length prefix is always big-endian on the wire"
                            )]
                            let length = u32::from_be_bytes(length);
                            this.total = Some(5_usize.saturating_add(usize::try_from(length).unwrap_or(usize::MAX)));
                        }
                    }
                    let allowed = this.total.map_or(data.len(), |total| total.saturating_sub(this.passed));
                    data.truncate(allowed);
                    this.passed = this.passed.saturating_add(data.len());
                    Poll::Ready(Some(Ok(http_body::Frame::data(data))))
                }
                Err(trailers) => Poll::Ready(Some(Ok(trailers))),
            },
            other => other,
        }
    }
}

/// `worker` served over `tls` on a free localhost port, every stage installed on it cut after its
/// plan once a task has been asked for there.
fn cutting(servers: &tokio::runtime::Runtime, tls: Arc<ServerConfig>, worker: Worker) -> Url {
    let listener = servers
        .block_on(tokio::net::TcpListener::bind(("127.0.0.1", 0)))
        .expect("a localhost port binds");
    let port = listener.local_addr().expect("a bound port has an address").port();
    let incoming = stream::unfold((listener, TlsAcceptor::from(tls)), |(listener, acceptor)| async move {
        let accepted = match listener.accept().await {
            Ok((tcp, _)) => acceptor.accept(tcp).await.map(Tls),
            Err(cause) => Err(cause),
        };
        Some((accepted, (listener, acceptor)))
    });
    let (released, _) = watch::channel(false);
    let cut = MapRequestLayer::new(move |request: http::Request<Body>| match request.uri().path() {
        "/worker.WorkerService/ExecuteTask" => {
            released.send_replace(true);
            request
        }
        "/worker.WorkerService/CoordinatorChannel" => {
            let mut released = released.subscribe();
            request.map(|inner| {
                Body::new(CutAfterThePlan {
                    inner,
                    prefix: Vec::new(),
                    total: None,
                    passed: 0,
                    cut: Box::pin(async move {
                        // A consumer asked for the task, so a scan is reading its feed when it ends.
                        drop(released.wait_for(|asked| *asked).await);
                        tokio::time::sleep(Duration::from_millis(300)).await;
                    }),
                })
            })
        }
        _ => request,
    });
    servers.spawn(
        Server::builder()
            .layer(cut)
            .add_service(worker.into_worker_server())
            .serve_with_incoming(incoming),
    );
    Url::parse(&format!("https://127.0.0.1:{port}")).expect("a socket address is a URL's authority")
}

#[test]
fn a_feed_cut_on_a_served_link_under_an_operator_is_a_lost_node_here() {
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(
        &servers,
        listening(&node, authority.peers()),
        own.clone(),
        Arc::<AtomicUsize>::default(),
    );
    let others: Vec<Url> = std::iter::repeat_with(|| {
        let other = authority.sign();
        let worker = Nodes::worker(roomy(), authority.identity(&other)).expect("a worker builds");
        cutting(&servers, listening(&other, authority.peers()), worker)
    })
    .take(2)
    .collect();
    let across = DataFusionCombiner::across(
        Nodes::declared(own, own_url, others, authority.identity(&node)).across_above(NonZeroU64::new(1)),
    )
    .expect("a combiner builds");

    let plan = sum_plan(true);
    let results = legs(&plan);
    let refused = across
        .combine(&plan, assigned(&plan, &results), UNBOUNDED)
        .expect_err("a combine whose feeds were cut answered");
    assert!(across.node_lost(&refused), "{refused:?}");
}
