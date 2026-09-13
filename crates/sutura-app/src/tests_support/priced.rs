//! [`PricedWarehouse`], split out of `super` for that file's own `max-lines` cap.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::Executable;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Warehouse};

use super::AdapterFailure;

/// A fake whose `dry_run` reports a chosen byte estimate - every other fake here answers
/// `NotAsked`/`Accepted { estimated_bytes: None }`, so nothing else can provoke a ledger charge.
pub(crate) struct PricedWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    estimated_bytes: Option<u64>,
}

impl PricedWarehouse {
    /// Answers `result`, pricing the dry run at `estimated_bytes`, or reporting no price if `None`.
    pub(crate) const fn pricing(
        source: SourceName,
        posture: SourcePosture,
        result: RowSet,
        estimated_bytes: Option<u64>,
    ) -> Self {
        Self {
            source,
            posture,
            result,
            estimated_bytes,
        }
    }
}

impl Warehouse for PricedWarehouse {
    type Error = AdapterFailure;

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
        Ok(PreFlight::Accepted {
            estimated_bytes: self
                .estimated_bytes
                .map(sutura_domain::warehouse::estimate::EstimatedBytes::parse),
        })
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented, _deadline: Deadline) -> Result<RowSet, Self::Error> {
        Ok(self.result.clone())
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}
