//! The cell that pins `docs/adr/0008`'s federated row for an ACCEPTED two-leg answer: each leg's
//! own warehouse sees exactly one `dry_run`, because `answer_federated` calls `dry_run_leg` for
//! the fact leg and again for the lookup leg even when the preflight accepts. Split out of `super`
//! (the `federated::tests` module) for that file's own `max-lines` reason - not for thematic
//! tidiness, so `use super::*` reaches every fixture (`federated_plan`, `federated_fact_rows`,
//! `federated_lookup_rows`, `FEDERATED_BUDGET`, `answer_federated` itself) exactly as the other
//! split-out modules read them.
//!
//! **The fake lives HERE, in a `#[cfg(test)]` file, and not in `tests_support`.** The one
//! existing federated `dry_runs()` assertion pins 0 for a budget-refused lookup leg
//! (`a_federated_dry_run_that_spends_the_budget_refuses_both_legs_before_either_executes`), and
//! the fake behind it - `SlowDryRunLegsWarehouse` - already counts. What this cell needs is a
//! counter on the ACCEPTED path, and adding it to an existing shared fake in a non-test file is
//! reverted on base by `just causality` and breaks the base build, so the counter is a new fake
//! in this test-only file.

use super::*;
use sutura_domain::warehouse::PreFlight;

/// A leg-executing fake that counts how many times its `dry_run` was reached, and answers a
/// fixed result on `execute`.
///
/// The shape of [`crate::tests_support::LegsWarehouse`] - declares `EXECUTES_LEGS`, answers one
/// canned [`RowSet`] - with one addition: a `dry_runs` counter, so a test can prove an accepted
/// two-leg answer dry-ran each leg exactly once. `AtomicUsize` rather than `Cell` because
/// `answer_federated` runs the lookup leg's `execute` on a scoped OS thread, so the warehouse
/// must be `Sync`; the dry-run itself is sequential (both legs before either `execute`), but the
/// counter is read after the scope joins, so the ordering is the same either way.
struct CountingDryRunLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    dry_runs: std::sync::atomic::AtomicUsize,
}

impl CountingDryRunLegsWarehouse {
    fn answering(source: SourceName, posture: SourcePosture, result: RowSet) -> Self {
        Self {
            source,
            posture,
            result,
            dry_runs: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn dry_runs(&self) -> usize {
        self.dry_runs.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Warehouse for CountingDryRunLegsWarehouse {
    type Error = crate::tests_support::AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        self.dry_runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Ok(crate::tests_support::canned(&self.result))
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// An accepted two-leg answer dry-runs EACH leg exactly once: `answer_federated` calls
/// `dry_run_leg` for the fact leg and again for the lookup leg, even when the preflight accepts,
/// so both legs' own warehouses see one `dry_run` before either `execute` runs.
#[test]
fn a_federated_answer_dry_runs_each_leg_exactly_once() {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(CountingDryRunLegsWarehouse::answering(
        fact_source.clone(),
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(CountingDryRunLegsWarehouse::answering(
        lookup_source.clone(),
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");

    let outcome = answer_federated(
        &bundle(),
        &federated_plan(),
        &asked_by_a_person(),
        &crate::tests_support::CountingBroker::default(),
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a federated answer is not an error")
    .into_outcome();

    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "the preflight accepts, so a two-leg answer is answered, not {outcome:?}"
    );
    assert_eq!(
        warehouses.get(&fact_source).expect("the fact leg is registered").dry_runs(),
        1,
        "an accepted two-leg answer dry-runs the fact leg exactly once"
    );
    assert_eq!(
        warehouses
            .get(&lookup_source)
            .expect("the lookup leg is registered")
            .dry_runs(),
        1,
        "an accepted two-leg answer dry-runs the lookup leg exactly once, even when the preflight accepts"
    );
}
