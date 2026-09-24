//! Fakes for `docs/adr/0029`'s own mapping cell: a data system whose failure IS the deadline.
//!
//! Split out of `testing.rs` for that file's own `max-lines` reason.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, Warehouse};

use super::shared_posture;

/// What this fake's own stand-in for "the deadline fired" says. Never actually timed - nothing here
/// reads a clock while executing - because what is under test is the MAPPING from
/// [`Warehouse::deadline_exceeded`] answering `true` to `RefusalReason::DeadlineExceeded`, audited,
/// and not a real clock racing a real statement (`docs/adr/0029`'s engine slice).
#[derive(Debug, thiserror::Error)]
#[error("the deadline fired at the data system")]
pub(crate) struct DeadlineFired;

/// A data system whose `execute` fails, reporting the deadline as the cause.
///
/// The sibling `WarehouseThatWillNotPage` is modelled on, for `Warehouse::deadline_exceeded` rather
/// than `Warehouse::result_did_not_fit`: `harness`'s `a_deadline_exceeded_answer_is_422_and_not_a_503`
/// pins that the mapping reaches a caller as `422 deadline_exceeded`, rather than the retryable `503`
/// a dead data system produces.
pub(crate) struct WarehouseThatOutranItsDeadline {
    source: SourceName,
    posture: SourcePosture,
}

impl WarehouseThatOutranItsDeadline {
    pub(crate) fn new(source: SourceName) -> sutura_app::Warehouses<Self> {
        sutura_app::Warehouses::of(Self {
            source,
            posture: shared_posture(),
        })
    }
}

impl Warehouse for WarehouseThatOutranItsDeadline {
    type Error = DeadlineFired;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
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
        Err(DeadlineFired)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(DeadlineFired)
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        true
    }
}
