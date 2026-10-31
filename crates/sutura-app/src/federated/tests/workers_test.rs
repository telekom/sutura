//! A combine that wanted other nodes and found none ready is a refusal worth a retry, not a
//! service error.
//!
//! The fake lives in this `#[cfg(test)]` file for the reason `dry_run_count_test` states.

use super::*;
use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs};
use sutura_domain::plan::{Ceilings, WorkersShort};
use sutura_exec_datafusion::CombineError;

const SHORT: WorkersShort = WorkersShort::new(2, 0);

/// A combiner whose every combine ends with no other node ready.
struct NoWorkersReady;

impl FederationCombiner for NoWorkersReady {
    type Error = CombineError;

    fn combine(&self, _plan: &FederatedPlan, _legs: Legs<'_>, _working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        Err(CombineError::WorkersNotReady {
            wanted: SHORT.wanted(),
            ready: SHORT.ready(),
        })
    }

    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }

    fn answer_not_well_formed(&self, _error: &Self::Error) -> Option<FederatedAnswerRefusal> {
        None
    }

    fn workers_not_ready(&self, _error: &Self::Error) -> Option<WorkersShort> {
        Some(SHORT)
    }
}

#[test]
fn a_combine_no_other_node_was_ready_for_is_refused_as_workers_not_ready() {
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("facts").expect("facts"),
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("geo").expect("geo"),
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources");

    let outcome = answer_federated(
        (&bundle(), PinnedSchemas::default().digest()),
        &federated_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &NoWorkersReady,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        Ceilings::DEFAULT,
    )
    .expect("no node ready is a refusal, not a `ServiceError`")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::WorkersNotReady { wanted: 2, ready: 0 }
            }
        ),
        "no other node ready in time is a refusal naming both counts, not {outcome:?}"
    );
}
