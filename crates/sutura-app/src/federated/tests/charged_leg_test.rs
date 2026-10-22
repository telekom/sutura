//! The leg runner refuses a leg that is not among the cut's own legs
//! (`docs/adr/20261010140559-datafusion-federates-the-sql-tool-and-the-metrics.md` decision 4).
//!
//! Both fakes live in this `#[cfg(test)]` file for the reason `dry_run_count_test` states.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use sutura_domain::plan::federated::{
    Cut, ExecuteError, Executed, FederatedAnswerRefusal, FederatedPlan, FederationCombiner, LegRunner, Legs,
};
use sutura_domain::plan::{ArmLegs, DrillAcrossCombiner, DrillAcrossPlan};
use sutura_domain::warehouse::PreFlight;
use sutura_exec_datafusion::{CombineError, DataFusionCombiner};

/// A [`crate::tests_support::LegsWarehouse`] that counts how many times a leg reached `execute`.
///
/// `AtomicUsize` because the combiner may run a leg on a scoped thread, so the warehouse is `Sync`.
struct CountingLegs {
    legs: crate::tests_support::LegsWarehouse,
    executes: AtomicUsize,
}

impl CountingLegs {
    fn answering(source: SourceName, result: RowSet) -> Self {
        Self {
            legs: crate::tests_support::LegsWarehouse::answering(source, shared(), result),
            executes: AtomicUsize::new(0),
        }
    }

    fn executes(&self) -> usize {
        self.executes.load(Ordering::SeqCst)
    }
}

impl Warehouse for CountingLegs {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        self.legs.source()
    }

    fn posture(&self) -> &SourcePosture {
        self.legs.posture()
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.legs.dry_run(executable, presented, deadline)
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.executes.fetch_add(1, Ordering::SeqCst);
        self.legs.execute(executable, presented, deadline)
    }

    /// The boot path's, which this cell never takes.
    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::Statement {
            cause: crate::tests_support::DriverFailure,
        })
    }
}

/// The production combiner, except for what `execute` does with the fact leg the cut gave out.
struct Misuses(DataFusionCombiner, Runs);

/// How [`Misuses`] runs a leg that is not among the cut's own legs.
#[derive(Clone, Copy)]
enum Runs {
    /// Runs the fact leg a second time: a leg the charged cut gave out once.
    TheFactLegTwice,
    /// Runs a clone of the fact leg once and never the leg itself: a leg the cut never held, equal
    /// to one it did.
    ACloneOfTheFactLeg,
    /// Runs a clone of the fact leg at a source the registry does not have: a leg the cut never
    /// held, refused before its warehouse is looked up.
    ACloneAtAnUnregisteredSource,
}

impl FederationCombiner for Misuses {
    type Error = CombineError;

    fn combine(&self, plan: &FederatedPlan, legs: Legs<'_>, working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        self.0.combine(plan, legs, working_set_bytes)
    }

    fn working_set_exhausted(&self, error: &Self::Error) -> Option<u64> {
        self.0.working_set_exhausted(error)
    }

    fn answer_not_well_formed(&self, error: &Self::Error) -> Option<FederatedAnswerRefusal> {
        self.0.answer_not_well_formed(error)
    }

    fn execute<R>(&self, cut: Cut<'_>, runner: &R, _working_set_bytes: u64) -> Executed<R, Self>
    where
        R: LegRunner + Sync,
        R::Error: Send,
    {
        let fact = cut.legs().next().expect("a cut has a fact leg");
        match self.1 {
            Runs::TheFactLegTwice => {
                let _first = runner.run(fact).map_err(ExecuteError::Leg)?;
                runner.run(fact).map_err(ExecuteError::Leg)
            }
            Runs::ACloneOfTheFactLeg => runner.run(&fact.clone()).map_err(ExecuteError::Leg),
            Runs::ACloneAtAnUnregisteredSource => {
                let mut elsewhere = fact.clone();
                if let sutura_domain::plan::LegPlan::Fact { source, .. } = &mut elsewhere {
                    *source = unregistered();
                }
                runner.run(&elsewhere).map_err(ExecuteError::Leg)
            }
        }
    }
}

impl DrillAcrossCombiner for Misuses {
    fn merge(&self, plan: &DrillAcrossPlan, arms: &[ArmLegs<'_>], working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        self.0.merge(plan, arms, working_set_bytes)
    }
}

/// A source the registry of [`refused_as_not_given_out`] does not have.
const UNREGISTERED: &str = "nowhere";

/// [`UNREGISTERED`] parsed outside [`Misuses`]'s `execute`, which returns a `Result`.
fn unregistered() -> SourceName {
    SourceName::parse(UNREGISTERED).expect("a test source")
}

/// Answers the federated plan through a combiner that `runs` a leg without a token: it is refused
/// as `LegNotGivenOut` at `at`, after the fact warehouse executed `executes` times.
fn refused_as_not_given_out(runs: Runs, at: &str, executes: usize) {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let warehouses = Warehouses::of(CountingLegs::answering(fact_source.clone(), federated_fact_rows()))
        .and(CountingLegs::answering(
            SourceName::parse("geo").expect("a test source"),
            federated_lookup_rows(),
        ))
        .expect("two sources, one registry");
    let at = SourceName::parse(at).expect("a test source");

    let failure = answer_federated(
        (&bundle(), PinnedSchemas::default().digest()),
        &federated_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &Misuses(DataFusionCombiner::new().expect("a combiner builds"), runs),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .expect_err("a leg that is not among the cut's own legs is this workspace's own defect, not an answer");
    let crate::ServiceError::Miswired {
        cause: cause @ crate::FederationMiswired::LegNotGivenOut { at: refused },
    } = &failure
    else {
        panic!("the uncharged leg is refused as a defect: {failure:?}");
    };
    assert_eq!(*refused, at);
    assert_eq!(
        cause.to_string(),
        format!("a leg at `{at}` was run that is not among the cut's own legs")
    );
    assert_eq!(
        warehouses.get(&fact_source).expect("the fact leg is registered").executes(),
        executes,
        "the uncharged fact leg never reached its warehouse"
    );
}

#[test]
fn a_leg_the_charged_cut_did_not_give_out_is_refused() {
    refused_as_not_given_out(Runs::TheFactLegTwice, "facts", 1);
}

#[test]
fn an_equal_leg_the_charged_cut_never_held_is_refused() {
    refused_as_not_given_out(Runs::ACloneOfTheFactLeg, "facts", 0);
}

#[test]
fn a_leg_the_charged_cut_never_held_is_refused_before_its_warehouse_is_looked_up() {
    refused_as_not_given_out(Runs::ACloneAtAnUnregisteredSource, UNREGISTERED, 0);
}
