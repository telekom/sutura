//! A stage another node refused for rows past the federated row ceiling crosses its link as the code
//! it was refused under, and is refused here; a code that is not exactly that one, or is not a code
//! at all, stays the engine's failure.

use core::num::NonZeroU64;
use std::sync::Arc;

use datafusion_distributed::Worker;
use futures_util::stream;
use sutura_domain::plan::FederationCombiner as _;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tonic::service::InterceptorLayer;
use tonic::transport::Server;
use tonic::{Code, Status};
use url::Url;

use super::{Authority, Tls, UNBOUNDED, assigned, legs, listening, roomy, served, servers, sum_plan};
use crate::combine::{CombineError, DataFusionCombiner, Nodes};

/// The code a node refuses a stage over the federated row ceiling under, as it crosses a link.
const CODE: &str = "sutura.refusal.rows-over-ceiling";

/// One length-delimited protobuf field: its key, its length in one byte, then `body`.
fn field(tag: u8, body: &[u8]) -> Vec<u8> {
    let length = u8::try_from(body.len()).expect("a field body here is short");
    assert!(length < 0x80, "a field body here is one length byte long");
    [&[(tag << 3) | 2, length][..], body].concat()
}

/// The status a node relays an engine failure in: a stage's own failure under the context `code`.
fn refused_under(code: &str) -> Status {
    let failure = field(11, b"a stage refused its rows");
    let context = [field(1, &failure), field(2, code.as_bytes())].concat();
    Status::with_details(Code::Internal, "DataFusionError", field(15, &context).into())
}

/// `worker` served over `tls` on a free localhost port, every call but a stage's install failed with
/// `status` before the worker reads it.
fn refusing(servers: &tokio::runtime::Runtime, tls: Arc<ServerConfig>, worker: Worker, status: Status) -> Url {
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
    let refused = InterceptorLayer::new(move |request: tonic::Request<()>| {
        if request.metadata().contains_key("x-sutura-task-key") {
            Ok(request)
        } else {
            Err(status.clone())
        }
    });
    servers.spawn(
        Server::builder()
            .layer(refused)
            .add_service(worker.into_worker_server())
            .serve_with_incoming(incoming),
    );
    Url::parse(&format!("https://127.0.0.1:{port}")).expect("a socket address is a URL's authority")
}

/// What a combine planned across this node and one other fails with, when the other fails every call
/// but a stage's install with `status`.
fn failed_across(status: Status) -> CombineError {
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    let other = authority.sign();
    let worker = Nodes::worker(roomy(), authority.identity(&other)).expect("a worker builds");
    let other_url = refusing(&servers, listening(&other, authority.peers()), worker, status);
    let across = DataFusionCombiner::across(
        Nodes::declared(own, own_url, [other_url], authority.identity(&node)).across_above(NonZeroU64::new(1)),
    )
    .expect("a combiner builds");

    let plan = sum_plan(true);
    let results = legs(&plan);
    across
        .combine(&plan, assigned(&plan, &results), UNBOUNDED)
        .expect_err("a combine with a stage another node failed answered")
}

#[test]
fn a_stage_another_node_refused_over_the_row_ceiling_is_refused_here_and_no_other_code_is() {
    let refused = failed_across(refused_under(CODE));
    // By name rather than by pattern, so this compiles, and fails, where the variant does not exist.
    assert_eq!(format!("{refused:?}"), "RowsOverCeiling");
    for status in [
        refused_under("sutura.refusal.rows-over-ceiling.v2"),
        Status::with_details(Code::Internal, "DataFusionError", field(11, CODE.as_bytes()).into()),
        Status::with_details(Code::Internal, "DataFusionError", vec![0xff].into()),
    ] {
        let failure = failed_across(status);
        assert!(matches!(failure, CombineError::Execute { .. }), "{failure:?}");
    }
}
