//! A combine planned across three nodes on localhost, against the same combine on one node, every
//! link over mutual TLS.

use core::num::{NonZeroU64, NonZeroUsize};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use datafusion::arrow::array::{ArrayRef, StringArray};
use datafusion::arrow::datatypes::DataType;
use datafusion::error::DataFusionError;
use datafusion::prelude::{CsvReadOptions, SessionContext};
use datafusion_distributed::{DistributedConfig, MaybeEncoded, SetPlanRequest, TaskKey, Worker};
use futures_util::stream;
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use sutura_domain::plan::{FederatedPlan, FederationCombiner as _, LegResult};
use sutura_domain::warehouse::{Accumulating, ResultBatches, ResultBudget, Value};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::server::danger::ClientCertVerifier;
use tokio_rustls::rustls::{RootCertStore, ServerConfig};
use tonic::codegen::http::HeaderMap;
use tonic::service::InterceptorLayer;
use tonic::transport::Server;
use tonic::transport::server::Connected;
use url::Url;

use super::{UNBOUNDED, assigned, combiner, fact, fact_linked, fact_row, lookup, lookup_linked, sum_plan, text};
use crate::combine::across::StageReadsASource;
use crate::combine::{DataFusionCombiner, NodeIdentity, Nodes};
use crate::pool::WorkingSet;

/// A deployment's node authority.
struct Authority(CertifiedIssuer<'static, KeyPair>);

/// A node certificate the authority signed for 127.0.0.1, where every node here listens, and its key.
struct Signed {
    certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
}

impl Authority {
    fn generate() -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("no names are valid parameters");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        Self(
            CertifiedIssuer::self_signed(params, KeyPair::generate().expect("a key generates"))
                .expect("an authority signs itself"),
        )
    }

    fn sign(&self) -> Signed {
        let key = KeyPair::generate().expect("a key generates");
        let certificate = CertificateParams::new(vec![String::from("127.0.0.1")])
            .expect("an address is valid parameters")
            .signed_by(&key, &self.0)
            .expect("the authority signs a node");
        Signed {
            certificate: certificate.der().clone(),
            key: PrivatePkcs8KeyDer::from(key.serialize_der()),
        }
    }

    fn identity(&self, node: &Signed) -> NodeIdentity {
        NodeIdentity::new(
            vec![node.certificate.clone()],
            PrivateKeyDer::Pkcs8(node.key.clone_key()),
            [self.0.der().clone()],
        )
        .expect("a signed node is an identity")
    }

    /// A peer accepted only with a certificate this authority signed.
    fn peers(&self) -> Arc<dyn ClientCertVerifier> {
        let mut roots = RootCertStore::empty();
        roots.add(self.0.der().clone()).expect("an authority is a trust anchor");
        WebPkiClientVerifier::builder_with_provider(Arc::new(roots), Arc::new(ring::default_provider()))
            .build()
            .expect("an authority verifies its nodes")
    }
}

/// `node`'s listener, checking a peer's certificate with `peers`.
fn listening(node: &Signed, peers: Arc<dyn ClientCertVerifier>) -> Arc<ServerConfig> {
    let mut config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring offers the safe versions")
        .with_client_cert_verifier(peers)
        .with_single_cert(vec![node.certificate.clone()], PrivateKeyDer::Pkcs8(node.key.clone_key()))
        .expect("a signed node is a listener's identity");
    config.alpn_protocols = vec![b"h2".to_vec()];
    Arc::new(config)
}

/// A pool no stage here comes near.
fn roomy() -> WorkingSet {
    WorkingSet::of_bytes(core::num::NonZeroUsize::new(1 << 30).expect("a nonzero size"))
}

fn servers() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a server runtime builds")
}

/// `worker` served over `tls` on a free localhost port, every request it is sent counted in `sent`.
fn served(servers: &tokio::runtime::Runtime, tls: Arc<ServerConfig>, worker: Worker, sent: Arc<AtomicUsize>) -> Url {
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
    let counted = InterceptorLayer::new(move |request| {
        sent.fetch_add(1, Ordering::SeqCst);
        Ok(request)
    });
    servers.spawn(
        Server::builder()
            .layer(counted)
            .add_service(worker.into_worker_server())
            .serve_with_incoming(incoming),
    );
    Url::parse(&format!("https://127.0.0.1:{port}")).expect("a socket address is a URL's authority")
}

/// The legs of `sum_plan`: two facts of A and one of B, both customers in the north.
fn legs(plan: &FederatedPlan) -> Vec<LegResult> {
    let fact = fact(vec![
        fact_row("A", text("c1"), Value::Integer(100)),
        fact_row("A", text("c2"), Value::Integer(200)),
        fact_row("B", text("c1"), Value::Integer(50)),
    ]);
    let lookup = lookup(vec![vec![text("c1"), text("north")], vec![text("c2"), text("north")]]);
    plan.legs()
        .into_iter()
        .zip([fact, lookup])
        .map(|(leg, batches)| LegResult::of(leg, batches))
        .collect()
}

#[test]
fn a_combine_across_three_nodes_answers_what_one_node_answers_and_sends_both_others_work() {
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    let sent: [Arc<AtomicUsize>; 2] = Default::default();
    let others: Vec<Url> = sent
        .iter()
        .map(|count| {
            let other = authority.sign();
            let worker = Nodes::worker(roomy(), authority.identity(&other)).expect("a worker builds");
            served(&servers, listening(&other, authority.peers()), worker, Arc::clone(count))
        })
        .collect();
    let declared = || Nodes::declared(own.clone(), own_url.clone(), others.clone(), authority.identity(&node));
    let here = DataFusionCombiner::across(declared()).expect("a combiner builds");
    let across = DataFusionCombiner::across(declared().across_above(NonZeroU64::new(1))).expect("a combiner builds");

    let plan = sum_plan(true);
    let results = legs(&plan);
    let answer = |combiner: &DataFusionCombiner| {
        combiner
            .combine(&plan, assigned(&plan, &results), UNBOUNDED)
            .expect("this combine answers")
            .to_rows()
            .expect("a combined answer decodes")
    };

    let one = answer(&combiner());
    assert_eq!(
        one.rows(),
        &[
            vec![text("A"), text("north"), text("2026-06"), Value::Integer(300)],
            vec![text("B"), text("north"), text("2026-06"), Value::Integer(50)],
        ]
    );
    // Legs this node holds within the question's ceiling are combined here, and no link is dialled.
    assert_eq!(answer(&here).rows(), one.rows());
    for (other, count) in sent.iter().enumerate() {
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "the other node {other} was sent a stage of a question this node holds"
        );
    }
    // Every listener here refuses a peer without a certificate the authority signed, so this answers
    // only if each link presents one.
    assert_eq!(answer(&across).rows(), one.rows());
    // A runner with one core plans no repartition, so nothing crosses a link: this fails rather than
    // passing as a combine on one node.
    for (other, count) in sent.iter().enumerate() {
        assert!(
            count.load(Ordering::SeqCst) > 0,
            "the other node {other} was sent no request, so its stage ran here"
        );
    }
}

/// Link values in a question one node is too small for: each leg is past the engine's thresholds
/// for broadcasting a join's smaller side, so the join is hash-partitioned and each node builds a part.
const LINKS: usize = 1 << 17;

/// Rows per batch, as a source streams them.
const BATCH: usize = 8192;

/// `sum_plan`'s legs over [`LINKS`] link values from `c0`, in batches of [`BATCH`] rows: one fact of A
/// per link value, each in the north.
fn wide(plan: &FederatedPlan) -> Vec<LegResult> {
    let parts: Vec<ArrayRef> = (0..LINKS)
        .step_by(BATCH)
        .map(|first| -> ArrayRef {
            Arc::new(StringArray::from_iter_values(
                (first..LINKS.min(first + BATCH)).map(|link| format!("c{link}")),
            ))
        })
        .collect();
    let streamed = |batches: Vec<ResultBatches>| {
        let schema = Arc::clone(batches.first().expect("a question has a link value").schema());
        let mut accumulating = Accumulating::announcing(schema, LINKS, ResultBudget::of_bytes(NonZeroUsize::MAX));
        for batch in batches.into_iter().flat_map(ResultBatches::into_batches) {
            accumulating
                .push(batch)
                .expect("a part agrees with the schema it was built under");
        }
        accumulating.finish()
    };
    plan.legs()
        .into_iter()
        .zip([
            streamed(
                parts
                    .iter()
                    .map(|part| fact_linked(DataType::Utf8, Arc::clone(part)))
                    .collect(),
            ),
            streamed(
                parts
                    .iter()
                    .map(|part| lookup_linked(DataType::Utf8, Arc::clone(part)))
                    .collect(),
            ),
        ])
        .map(|(leg, batches)| LegResult::of(leg, batches))
        .collect()
}

#[test]
fn a_question_one_node_refuses_for_its_working_set_three_nodes_answer_exactly() {
    let plan = sum_plan(true);
    let results = wide(&plan);
    let one = combiner();
    let answer = one
        .combine(&plan, assigned(&plan, &results), UNBOUNDED)
        .expect("one roomy node answers")
        .to_rows()
        .expect("a combined answer decodes");
    let sum = i64::try_from(LINKS).expect("the link count is an integer") * 100;
    assert_eq!(
        answer.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), Value::Integer(sum)]]
    );

    // The widest ceiling, halving from a roomy one, that one node refuses for its working set.
    let mut working_set = UNBOUNDED;
    let refused = loop {
        match one.combine(&plan, assigned(&plan, &results), working_set) {
            Ok(_) => working_set >>= 1,
            Err(refused) => break refused,
        }
    };
    assert_eq!(one.working_set_exhausted(&refused), Some(working_set), "{refused:?}");

    // Each node's pool, and the question's own ceiling, is the one a single node refused.
    let pool = || {
        let bytes = usize::try_from(working_set).expect("a ceiling under one node's roomy one fits a pointer");
        WorkingSet::of_bytes(NonZeroUsize::new(bytes).expect("a refused ceiling halved from a roomy one is not zero"))
    };
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(pool(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    let others: Vec<Url> = std::iter::repeat_with(|| {
        let other = authority.sign();
        let worker = Nodes::worker(pool(), authority.identity(&other)).expect("a worker builds");
        served(&servers, listening(&other, authority.peers()), worker, Arc::default())
    })
    .take(2)
    .collect();
    let across =
        DataFusionCombiner::across(Nodes::declared(own, own_url, others, authority.identity(&node))).expect("a combiner builds");
    let spread = across
        .combine(&plan, assigned(&plan, &results), working_set)
        .expect("three nodes answer what one refused")
        .to_rows()
        .expect("a combined answer decodes");
    assert_eq!(spread.rows(), answer.rows());
}

#[test]
fn a_question_across_three_nodes_lets_each_link_hold_a_twelfth_of_the_working_set() {
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let at = |port: u16| Url::parse(&format!("https://127.0.0.1:{port}")).expect("a node is a URL");
    let nodes = Nodes::declared(own, at(1), [at(2), at(3)], authority.identity(&node));
    let working_set = WorkingSet::of_bytes(NonZeroUsize::new(12 << 20).expect("a nonzero size"));

    let session = nodes.session(working_set).expect("a session builds");
    let state = session.state();
    let links = DistributedConfig::from_session_config(state.config()).expect("the session plans across nodes");

    assert_eq!(
        links.worker_connection_buffer_budget_bytes,
        1 << 20,
        "each link holds a quarter of the working set shared by three nodes"
    );
    assert_eq!(state.config().target_partitions(), 3, "a task plans one partition per node");
}

#[test]
fn a_node_another_authority_signed_is_sent_no_stage() {
    let servers = servers();
    let ours = Authority::generate();
    let theirs = Authority::generate();
    let node = ours.sign();
    let own = Nodes::worker(roomy(), ours.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, ours.peers()), own.clone(), Arc::default());
    let sent: [Arc<AtomicUsize>; 2] = Default::default();
    // Each accepts any peer, so what refuses them is this node's link and nothing else.
    let others: Vec<Url> = sent
        .iter()
        .map(|count| {
            let other = theirs.sign();
            let worker = Nodes::worker(roomy(), theirs.identity(&other)).expect("a worker builds");
            served(
                &servers,
                listening(&other, WebPkiClientVerifier::no_client_auth()),
                worker,
                Arc::clone(count),
            )
        })
        .collect();
    let across =
        DataFusionCombiner::across(Nodes::declared(own, own_url, others, ours.identity(&node)).across_above(NonZeroU64::new(1)))
            .expect("a combiner builds");

    let plan = sum_plan(true);
    let results = legs(&plan);
    assert!(
        across.combine(&plan, assigned(&plan, &results), UNBOUNDED).is_err(),
        "a combine whose stages were planned onto another authority's nodes answered"
    );
    for (other, count) in sent.iter().enumerate() {
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "the node {other} another authority signed was sent a request"
        );
    }
}

#[test]
fn a_node_refuses_a_stage_that_reads_a_source() {
    let authority = Authority::generate();
    let worker = Nodes::worker(roomy(), authority.identity(&authority.sign())).expect("a worker builds");
    let path = std::env::temp_dir().join(format!("sutura-stage-source-{}.csv", std::process::id()));
    std::fs::write(&path, "region\nnorth\n").expect("a temporary file writes");
    // The stage arrives decoded, as the node's own stages do; one sent over a link is decoded first,
    // and the same check runs on the decoded plan.
    let refused = servers().block_on(async {
        let scan = SessionContext::new()
            .read_csv(path.to_str().expect("a temporary path is text"), CsvReadOptions::new())
            .await
            .expect("a file reads")
            .create_physical_plan()
            .await
            .expect("a file scan plans");
        #[expect(
            clippy::default_trait_access,
            reason = "the question id is a `uuid`, which this crate does not name; the check reads no id"
        )]
        let stage = SetPlanRequest {
            task_key: TaskKey {
                query_id: Default::default(),
                stage_id: 1,
                task_number: 0,
            },
            task_count: 1,
            plan: MaybeEncoded::Decoded(scan),
            work_unit_feed_declarations: vec![],
            target_worker_url: Url::parse("https://127.0.0.1:1").expect("an address is a URL"),
            query_start_time_ns: 0,
        };
        worker
            .coordinator_channel(HeaderMap::new(), stage, Box::pin(stream::empty()))
            .await
            .err()
    });
    std::fs::remove_file(&path).expect("a temporary file removes");
    let refused = refused.expect("a node ran a stage that reads a file");
    assert!(
        matches!(
            refused.find_root(),
            DataFusionError::External(cause)
                if cause.downcast_ref::<StageReadsASource>().is_some_and(|refusal| refusal.leaf == "DataSourceExec")
        ),
        "{refused}"
    );
}

/// A served TLS connection, as the gRPC server takes one: its own crate's `Connected` for a TLS
/// stream sits behind a feature this workspace leaves off.
struct Tls(tokio_rustls::server::TlsStream<tokio::net::TcpStream>);

impl Connected for Tls {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

impl AsyncRead for Tls {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Tls {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
