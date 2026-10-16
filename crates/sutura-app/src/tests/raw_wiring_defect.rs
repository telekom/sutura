//! `run_sql` refuses an unserved deployment shape as an answer the caller reads. What stays a
//! wiring defect is an adapter that declares `ACCEPTS_RAW_STATEMENTS` and then answers no
//! statement, which no caller can cause.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable, RefusingCombiner};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, Warehouse};

use crate::surface::{LocalService, Surface as _, SurfaceFailure};
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{AdapterFailure, DiscardingAuditSink, FixedBroker, FixedCatalog, bundle_over};
use crate::{RunSqlError, Warehouses, run_sql};

struct DeclaresRawAndAnswersNone {
    at: SourceName,
    posture: SourcePosture,
}

impl Warehouse for DeclaresRawAndAnswersNone {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const ACCEPTS_RAW_STATEMENTS: bool = true;

    fn source(&self) -> &SourceName {
        &self.at
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
    }
}

fn silent() -> DeclaresRawAndAnswersNone {
    DeclaresRawAndAnswersNone {
        at: source(),
        posture: shared(),
    }
}

#[test]
fn an_adapter_declaring_raw_and_answering_none_stays_a_wiring_defect() {
    let statement = sutura_domain::raw::RawStatement::parse("select 1").expect("a test statement is a statement");
    let failure = run_sql(
        &asked_by_a_person(),
        &statement,
        &FixedBroker::GrantsShared,
        &Warehouses::of(silent()),
        test_deadline(),
    )
    .expect_err("an adapter that declares raw and answers none is not a refusal the caller can read");
    assert!(matches!(failure, RunSqlError::NoAcceptingSource), "{failure:?}");

    let surfaced = LocalService::start(
        &FixedCatalog::of(bundle_over(&[])),
        Warehouses::of(silent()),
        DiscardingAuditSink,
        FixedBroker::GrantsShared,
        RefusingCombiner,
        1 << 30,
    )
    .expect("an empty catalog with no anchors boots against any warehouse")
    .run_sql(&asked_by_a_person(), &statement, test_deadline())
    .expect_err("the surface reports it as a failure, not an answer");
    assert!(matches!(surfaced, SurfaceFailure::Miswired { .. }), "{surfaced:?}");
}
