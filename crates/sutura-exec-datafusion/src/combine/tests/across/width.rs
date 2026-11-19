//! How many threads the combiner that plans across nodes runs on.

use core::num::NonZeroUsize;

use url::Url;

use super::{Authority, roomy};
use crate::combine::{DataFusionCombiner, Nodes};

fn threads(workers: Option<usize>) -> usize {
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let url = Url::parse("https://127.0.0.1:1").expect("an address is a URL");
    let declared = Nodes::declared(own, url, [], authority.identity(&node));
    let nodes = match workers {
        Some(count) => declared.engine_workers(NonZeroUsize::new(count).expect("a test width is positive")),
        None => declared,
    };
    let combiner = DataFusionCombiner::across(nodes).expect("a combiner builds");
    combiner
        .runtime()
        .expect("a combiner has its runtime")
        .metrics()
        .num_workers()
}

#[test]
fn a_combiner_across_nodes_runs_on_the_width_its_nodes_were_given_and_on_one_thread_when_given_none() {
    assert_eq!(threads(None), 1);
    assert_eq!(threads(Some(3)), 3);
}
