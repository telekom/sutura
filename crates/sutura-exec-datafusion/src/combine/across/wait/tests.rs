//! The bounded wait over a resolver that names the nodes a test chooses, poll by poll.

use core::num::NonZeroU64;
use core::time::Duration;
use std::time::Instant;

use sutura_domain::plan::{FederationCombiner as _, WorkersShort};
use url::Url;

use super::{others_wanted, ready};
use crate::combine::{CombineError, DataFusionCombiner};

const SHORT: Duration = Duration::from_millis(50);

fn node(host: &str) -> Url {
    Url::parse(&format!("https://{host}:8443")).expect("a host and a port are a URL")
}

#[test]
fn a_wait_no_other_node_resolves_within_is_refused_as_workers_not_ready_once_it_ends() {
    let mut asked = 0_u32;
    let started = Instant::now();
    let refused = ready(2, SHORT, || {
        asked += 1;
        Vec::new()
    })
    .expect_err("no other node resolved, so the question is refused");

    assert!(
        started.elapsed() >= SHORT,
        "refused after {:?}, before the wait ended",
        started.elapsed()
    );
    assert!(
        asked > 1,
        "the nodes were resolved {asked} time(s), so the wait never asked again"
    );
    let CombineError::WorkersNotReady { wanted, ready } = refused else {
        panic!("refused for another reason: {refused}");
    };
    assert_eq!((wanted, ready), (2, 0));
}

#[test]
fn a_wait_ends_as_soon_as_the_nodes_wanted_resolve() {
    let wanted = vec![node("b.example"), node("c.example")];
    let mut asked = 0_u32;
    let wait = Duration::from_secs(30);
    let started = Instant::now();
    let resolved = ready(2, wait, || {
        asked += 1;
        if asked > 1 { wanted.clone() } else { Vec::new() }
    })
    .expect("both nodes resolved on the second poll");

    assert_eq!(resolved, wanted);
    assert!(
        started.elapsed() < wait,
        "the wait ran to its end although both nodes had resolved"
    );
}

#[test]
fn when_the_wait_ends_with_fewer_nodes_than_wanted_the_question_goes_across_those_that_resolved() {
    let mut asked = 0_u32;
    let resolved = ready(3, SHORT, || {
        asked += 1;
        vec![node("b.example")]
    })
    .expect("one node resolved, which is enough to go across");

    assert_eq!(resolved, vec![node("b.example")]);
    assert!(
        asked > 1,
        "the nodes were resolved {asked} time(s), so the wait never asked again"
    );
}

#[test]
fn a_question_wants_one_other_node_for_each_share_beyond_this_nodes_and_one_at_least() {
    assert_eq!(others_wanted(NonZeroU64::new(10), 5, 1_000), 1);
    assert_eq!(others_wanted(NonZeroU64::new(10), 25, 1_000), 2);
    assert_eq!(others_wanted(None, 250, 100), 2);
    assert_eq!(others_wanted(None, 3, 0), 2, "a zero ceiling shares by one byte");
}

#[test]
fn workers_not_ready_is_read_as_the_nodes_wanted_and_ready() {
    let combiner = DataFusionCombiner::new().expect("a combiner builds");
    let short = combiner.workers_not_ready(&CombineError::WorkersNotReady { wanted: 2, ready: 0 });
    assert_eq!(short, Some(WorkersShort::new(2, 0)));
}
