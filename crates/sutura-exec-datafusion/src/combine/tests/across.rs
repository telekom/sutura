//! A combine planned across three nodes on localhost, against the same combine on one node.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use datafusion_distributed::Worker;
use sutura_domain::plan::{FederationCombiner as _, LegResult};
use sutura_domain::warehouse::Value;
use tonic::service::InterceptorLayer;
use tonic::transport::Server;
use tonic::transport::server::TcpIncoming;
use url::Url;

use super::{UNBOUNDED, assigned, combiner, fact, fact_row, lookup, sum_plan, text};
use crate::combine::{DataFusionCombiner, Nodes};
use crate::pool::WorkingSet;

/// A pool no stage here comes near.
fn roomy() -> WorkingSet {
    WorkingSet::of_bytes(core::num::NonZeroUsize::new(1 << 30).expect("a nonzero size"))
}

/// `worker` served on a free localhost port, every request it is sent counted in `sent`.
fn served(servers: &tokio::runtime::Runtime, worker: Worker, sent: Arc<AtomicUsize>) -> Url {
    let _entered = servers.enter();
    let incoming = TcpIncoming::bind(([127, 0, 0, 1], 0).into()).expect("a localhost port binds");
    let address = incoming.local_addr().expect("a bound port has an address");
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
    Url::parse(&format!("http://{address}")).expect("a socket address is a URL's authority")
}

#[test]
fn a_combine_across_three_nodes_answers_what_one_node_answers_and_sends_both_others_work() {
    let servers = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a server runtime builds");
    let own = Nodes::worker(roomy()).expect("a worker builds");
    let own_url = served(&servers, own.clone(), Arc::default());
    let sent: [Arc<AtomicUsize>; 2] = Default::default();
    let others: Vec<Url> = sent
        .iter()
        .map(|count| served(&servers, Nodes::worker(roomy()).expect("a worker builds"), Arc::clone(count)))
        .collect();
    let across = DataFusionCombiner::across(Nodes::declared(own, own_url, others)).expect("a combiner builds");

    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row("A", text("c1"), Value::Integer(100)),
        fact_row("A", text("c2"), Value::Integer(200)),
        fact_row("B", text("c1"), Value::Integer(50)),
    ]);
    let lookup = lookup(vec![vec![text("c1"), text("north")], vec![text("c2"), text("north")]]);
    let results: Vec<LegResult> = plan
        .legs()
        .into_iter()
        .zip([fact, lookup])
        .map(|(leg, batches)| LegResult::of(leg, batches))
        .collect();
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
