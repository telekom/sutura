//! A fake [`AnyWarehouse`] variant that counts the labels each billed call hands it, so a test sees
//! what the enum forwards to the adapter it holds - the adapters it really holds load a driver
//! before any label is observable.

use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use sutura_domain::identity::{BillTo, Presented, PrincipalChain, RequestContext, Subject};
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::billed::BilledResult;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Warehouse};

use super::AnyWarehouse;
use super::tests::billed::{deadline, presented, two_projects};

/// The correlation id the cell here draws.
const DRAWN: [u8; 16] = [0x44; 16];

/// Answers every call with no rows, and counts the billed calls that carried [`DRAWN`] as their
/// `sutura_correlation` label.
pub(crate) struct Recorder {
    source: SourceName,
    posture: SourcePosture,
    carried: Arc<AtomicUsize>,
}

impl Recorder {
    fn nothing() -> RowSet {
        RowSet::new(Vec::new(), Vec::new()).expect("no columns and no rows is a result set")
    }

    fn no_rows() -> ResultBatches {
        sutura_domain::warehouse::arrow::of_row_set(&Self::nothing()).expect("an empty result set converts")
    }

    fn keep(&self, labels: &JobLabels) {
        let [.., (_, correlation)] = labels.pairs();
        if correlation == "44".repeat(DRAWN.len()) {
            self.carried.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Warehouse for Recorder {
    type Error = core::convert::Infallible;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

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
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Ok(Self::no_rows())
    }

    fn dry_run_billed(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _bill_to: &BillTo,
        labels: &JobLabels,
        _deadline: Deadline,
    ) -> BilledResult<PreFlight, Self::Error> {
        self.keep(labels);
        Ok(PreFlight::NotAsked)
    }

    fn execute_billed(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _bill_to: &BillTo,
        labels: &JobLabels,
        _deadline: Deadline,
    ) -> BilledResult<ResultBatches, Self::Error> {
        self.keep(labels);
        Ok(Self::no_rows())
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(Self::nothing()))
    }
}

#[test]
fn a_mixed_deployment_forwards_the_requests_labels_to_the_adapter_it_holds() {
    let carried = Arc::default();
    let warehouse = AnyWarehouse::Recorded(Box::new(Recorder {
        source: SourceName::parse("ledger").expect("a test source is a source"),
        posture: SourcePosture::ImpersonationAtSource,
        carried: Arc::clone(&carried),
    }));
    let labels = JobLabels::of(&RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself)).correlated(DRAWN));
    let leg = two_projects();
    drop(warehouse.dry_run_billed(Executable::Leg(&leg), &presented(), &BillTo::Unstated, &labels, deadline()));
    drop(warehouse.execute_billed(Executable::Leg(&leg), &presented(), &BillTo::Unstated, &labels, deadline()));
    assert_eq!(
        carried.load(Ordering::SeqCst),
        2,
        "the dry run and the job both carry the drawn id"
    );
}
