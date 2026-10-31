//! A question that wants other nodes, with a bounded wait for them declared and without one.

use core::num::NonZeroU64;
use core::time::Duration;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use sutura_domain::plan::{FederationCombiner as _, WorkersShort};
use url::Url;

use super::{Authority, legs, listening, roomy, served, servers, signed_for};
use crate::combine::tests::{UNBOUNDED, assigned, combiner, sum_plan};
use crate::combine::{CombineError, DataFusionCombiner, Nodes};

const WAIT: Duration = Duration::from_millis(300);

fn nowhere() -> Url {
    Url::parse("https://nowhere.invalid:8443").expect("a reserved name is a URL")
}

#[test]
fn a_question_no_other_node_resolves_for_within_the_wait_is_refused_as_workers_not_ready() {
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = Url::parse("https://127.0.0.1:1").expect("an address is a URL");
    let nodes = Nodes::declared(own, own_url, [nowhere()], authority.identity(&node))
        .across_above(NonZeroU64::new(1))
        .wait_for_workers(Some(WAIT));
    let across = DataFusionCombiner::across(nodes).expect("a combiner builds");
    let plan = sum_plan(true);
    let results = legs(&plan);

    let started = Instant::now();
    let refused = across
        .combine(&plan, assigned(&plan, &results), UNBOUNDED)
        .expect_err("no other node resolved, so the question is refused");

    assert!(
        started.elapsed() >= WAIT,
        "refused after {:?}, before the wait ended",
        started.elapsed()
    );
    let CombineError::WorkersNotReady { wanted, ready } = refused else {
        panic!("refused for another reason: {refused}");
    };
    assert_eq!(ready, 0);
    assert!(wanted >= 1, "a question going across wants one other node at least");
    assert_eq!(across.workers_not_ready(&refused), Some(WorkersShort::new(wanted, ready)));
}

#[test]
fn with_no_wait_declared_a_question_no_other_node_resolves_for_is_answered_here() {
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    let nodes = Nodes::declared(own, own_url, [nowhere()], authority.identity(&node))
        .across_above(NonZeroU64::new(1))
        .wait_for_workers(None);
    let across = DataFusionCombiner::across(nodes).expect("a combiner builds");
    let plan = sum_plan(true);
    let results = legs(&plan);

    let answer = |combiner: &DataFusionCombiner| {
        combiner
            .combine(&plan, assigned(&plan, &results), UNBOUNDED)
            .expect("this combine answers")
            .to_rows()
            .expect("a combined answer decodes")
    };
    assert_eq!(answer(&across).rows(), answer(&combiner()).rows());
}

#[test]
fn when_the_wait_ends_a_question_is_planned_across_the_nodes_that_resolved() {
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    let named = signed_for(&authority, "localhost");
    let worker = Nodes::worker(roomy(), authority.identity(&named)).expect("a worker builds");
    let sent = Arc::<AtomicUsize>::default();
    let mut localhost = served(&servers, listening(&named, authority.peers()), worker, Arc::clone(&sent));
    localhost.set_host(Some("localhost")).expect("a name is a host");
    let nodes = Nodes::declared(own, own_url, [localhost, nowhere()], authority.identity(&node))
        .across_above(NonZeroU64::new(1))
        .wait_for_workers(Some(WAIT));
    let across = DataFusionCombiner::across(nodes).expect("a combiner builds");
    let plan = sum_plan(true);
    let results = legs(&plan);

    let answer = |combiner: &DataFusionCombiner| {
        combiner
            .combine(&plan, assigned(&plan, &results), UNBOUNDED)
            .expect("one node of the two wanted resolved, so this combine answers")
            .to_rows()
            .expect("a combined answer decodes")
    };
    assert_eq!(answer(&across).rows(), answer(&combiner()).rows());
    assert!(
        sent.load(Ordering::SeqCst) > 0,
        "the node that resolved was sent no request, so the wait planned nothing across"
    );
}
