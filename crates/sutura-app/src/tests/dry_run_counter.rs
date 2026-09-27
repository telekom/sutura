//! A mono fake that counts its own `dry_run` and `execute` calls, for the two `docs/adr/0008` cells
//! in `super`: `answer` calls `dry_run` even when the pre-flight accepts, and a dry-run refusal
//! stops the answer before `execute`.
//!
//! Its own type in a test-only file rather than a counter on the shared `PreflightWarehouse`: a
//! counter added to `tests_support.rs` would be reverted on the causality gate's base tree while
//! the cells calling it stay at HEAD, a compile break rather than an assertion.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::Executable;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Warehouse};

use crate::tests_support::{AdapterFailure, DriverFailure, DryRunOutcome, canned};

/// `tests_support`'s `MonoPreflightWarehouse` shape - a source, a canned `RowSet`, a scripted
/// `DryRunOutcome` - plus a `dry_runs()` counter, so a cell proves the call was made rather than
/// inferring it from the outcome.
pub(crate) struct DryRunCountingWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    dry_run: DryRunOutcome,
    dry_runs: std::sync::atomic::AtomicUsize,
    executions: std::sync::atomic::AtomicUsize,
}

impl DryRunCountingWarehouse {
    pub(crate) fn new(source: SourceName, posture: SourcePosture, result: RowSet, dry_run: DryRunOutcome) -> Self {
        Self {
            source,
            posture,
            result,
            dry_run,
            dry_runs: std::sync::atomic::AtomicUsize::new(0),
            executions: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(crate) fn dry_runs(&self) -> usize {
        self.dry_runs.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn executions(&self) -> usize {
        self.executions.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Warehouse for DryRunCountingWarehouse {
    type Error = AdapterFailure;

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
        self.dry_runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match self.dry_run {
            DryRunOutcome::Accepted => Ok(PreFlight::Accepted { estimated_bytes: None }),
            DryRunOutcome::SourceRefused => Err(AdapterFailure::RefusedBySource),
            DryRunOutcome::TransientFailure => Err(AdapterFailure::Statement { cause: DriverFailure }),
            DryRunOutcome::TimedOut => Err(AdapterFailure::TimedOut),
        }
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.executions.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(canned(&self.result))
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::RefusedBySource)
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::TimedOut)
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}
