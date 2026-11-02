//! A combine planned across two nodes whose final stage holds more rows than the federated row
//! ceiling: a worker fails its task under the relay's code, and the node that planned it refuses the
//! question typed.

use core::num::NonZeroU64;
use std::sync::Arc;

use sutura_domain::plan::{FederatedPlan, FederatedRowCeiling, FederationCombiner as _, LegResult};
use sutura_domain::warehouse::Value;

use super::{Authority, UNBOUNDED, assigned, fact, fact_row, listening, lookup, roomy, served, servers, sum_plan, text};
use crate::combine::{DataFusionCombiner, Nodes};

/// The legs of `sum_plan`: five families of one customer in the north, so five answer rows between
/// a final stage of one task per node, and a task holding more than one of them.
fn five_families(plan: &FederatedPlan) -> Vec<LegResult> {
    let fact = fact(Vec::from(
        ["A", "B", "C", "D", "E"].map(|family| fact_row(family, text("c1"), Value::Integer(1))),
    ));
    let lookup = lookup(vec![vec![text("c1"), text("north")]]);
    plan.legs()
        .into_iter()
        .zip([fact, lookup])
        .map(|(leg, batches)| LegResult::of(leg, batches))
        .collect()
}

#[test]
fn a_combine_whose_final_stage_task_passes_the_row_ceiling_is_refused_typed() {
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    let other = authority.sign();
    let worker = Nodes::worker(roomy(), authority.identity(&other)).expect("a worker builds");
    let other_url = served(&servers, listening(&other, authority.peers()), worker, Arc::default());
    let across = DataFusionCombiner::across(
        Nodes::declared(own, own_url, [other_url], authority.identity(&node))
            .across_above(NonZeroU64::new(1))
            .row_ceiling(FederatedRowCeiling::parse(1).expect("a nonzero ceiling")),
    )
    .expect("a combiner builds");

    let plan = sum_plan(true);
    let results = five_families(&plan);
    let refused = across
        .combine(&plan, assigned(&plan, &results), UNBOUNDED)
        .expect_err("a combine whose final stage passed the row ceiling answered");
    assert_eq!(format!("{refused:?}"), "RowsOverCeiling");
}
