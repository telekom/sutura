//! Each leg of a federated answer is labelled with the request's job labels: the dry run and the
//! execution of BOTH legs carry the correlation id the asking context drew. Split out of `super` for
//! its `max-lines` reason; `use super::*` reaches the two-leg fixtures.

use core::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::tests_support::LegsWarehouse;
use sutura_domain::identity::BillTo;
use sutura_domain::warehouse::PreFlight;
use sutura_domain::warehouse::billed::BilledResult;
use sutura_domain::warehouse::labels::JobLabels;

/// The correlation id every cell here draws.
const DRAWN: [u8; 16] = [0x33; 16];

/// [`LegsWarehouse`], counting its billed calls and those that carried [`DRAWN`] as their
/// `sutura_correlation` label.
struct Labelled(LegsWarehouse, AtomicUsize, AtomicUsize);

impl Labelled {
    fn keep(&self, labels: &JobLabels) {
        let [.., (_, correlation)] = labels.pairs();
        self.1.fetch_add(1, Ordering::SeqCst);
        if correlation == "33".repeat(DRAWN.len()) {
            self.2.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Warehouse for Labelled {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        self.0.source()
    }

    fn posture(&self) -> &SourcePosture {
        self.0.posture()
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.0.dry_run(executable, presented, deadline)
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.0.execute(executable, presented, deadline)
    }

    fn dry_run_billed(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        bill_to: &BillTo,
        labels: &JobLabels,
        deadline: Deadline,
    ) -> BilledResult<PreFlight, Self::Error> {
        self.keep(labels);
        self.0.dry_run_billed(executable, presented, bill_to, labels, deadline)
    }

    fn execute_billed(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        bill_to: &BillTo,
        labels: &JobLabels,
        deadline: Deadline,
    ) -> BilledResult<ResultBatches, Self::Error> {
        self.keep(labels);
        self.0.execute_billed(executable, presented, bill_to, labels, deadline)
    }

    fn verify_anchor(&self, plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        #[expect(clippy::disallowed_methods, reason = "a fake delegating the boot path to the fake it wraps")]
        let rows = self.0.verify_anchor(plan);
        rows
    }
}

#[test]
fn both_legs_of_a_federated_answer_carry_the_requests_correlation_label() {
    let fact = SourceName::parse("facts").expect("a test source");
    let lookup = SourceName::parse("geo").expect("a test source");
    let labelled = |source: &SourceName, rows| {
        Labelled(
            LegsWarehouse::answering(source.clone(), shared(), rows),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        )
    };
    let warehouses = Warehouses::of(labelled(&fact, federated_fact_rows()))
        .and(labelled(&lookup, federated_lookup_rows()))
        .expect("two sources, one registry");
    let outcome = answer_federated(
        (&bundle(), PinnedSchemas::default().digest()),
        &federated_plan(),
        &asked_by_a_person().correlated(DRAWN),
        &crate::tests_support::CountingBroker::default(),
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect("a federated answer is not an error")
    .into_outcome();
    assert!(matches!(outcome, ToolOutcome::Answer { .. }), "{outcome:?}");
    for source in [&fact, &lookup] {
        let leg = warehouses.get(source).expect("the leg is registered");
        assert_eq!(
            leg.1.load(Ordering::SeqCst),
            2,
            "{source}: its dry run and its execution are billed"
        );
        assert_eq!(leg.2.load(Ordering::SeqCst), 2, "{source}: both carry the drawn id");
    }
}
