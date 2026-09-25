//! Two more fakes for `docs/adr/0029`'s own RED cells: a data system that counts whether it was
//! asked at all, and a leg-executing one that records the `Deadline` each call was handed.
//!
//! Split out of `tests_support.rs` for that file's own reason - it was at the `max-lines` cap this
//! record needed to add two fakes past.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::Executable;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Warehouse};

use super::{AdapterFailure, DriverFailure};

/// A data system that counts how many times its `dry_run` and `execute` were reached.
///
/// **The instrument for the pre-call deadline check**, which is worth having a fake of its own for:
/// what it proves is that a budget already spent means the data system is never asked at all,
/// counted rather than inferred from an outcome that would look the same either way. Always errs
/// when called, so a test that expects zero calls fails loudly - with a cause the deadline check has
/// nothing to do with - if the check is ever skipped.
pub(crate) struct NeverAskedWarehouse {
    source: SourceName,
    posture: SourcePosture,
    dry_runs: std::sync::atomic::AtomicUsize,
    executes: std::sync::atomic::AtomicUsize,
}

impl NeverAskedWarehouse {
    pub(crate) fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self {
            source,
            posture,
            dry_runs: std::sync::atomic::AtomicUsize::new(0),
            executes: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// How many times `dry_run` and `execute` were reached, added together - either one being
    /// non-zero means the data system was asked.
    pub(crate) fn calls(&self) -> usize {
        self.dry_runs.load(std::sync::atomic::Ordering::SeqCst) + self.executes.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Warehouse for NeverAskedWarehouse {
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
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.executes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }
}

/// A data system whose `execute` fails with what it reports as the deadline having fired.
///
/// The mono-path sibling `RefusingSourceWarehouse` is modelled on: `answer` (and, when
/// `EXECUTES_LEGS` is `true`, `crate::federated::run_leg`) must turn this specific error into
/// `RefusalReason::DeadlineExceeded` and never into the retryable `503` a dead data system
/// produces, so `sutura_app::tests`'s `running_out_of_time_is_a_refusal_and_not_a_503` and its
/// federated sibling pin the mapping the same way `RefusingLegsWarehouse` pins `source_refused` on
/// both paths.
pub(crate) struct DeadlineExceededWarehouse<const EXECUTES_LEGS: bool> {
    source: SourceName,
    posture: SourcePosture,
}

impl<const EXECUTES_LEGS: bool> DeadlineExceededWarehouse<EXECUTES_LEGS> {
    pub(crate) fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self { source, posture }
    }
}

impl<const EXECUTES_LEGS: bool> Warehouse for DeadlineExceededWarehouse<EXECUTES_LEGS> {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = EXECUTES_LEGS;

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
        Err(AdapterFailure::TimedOut)
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::TimedOut)
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::TimedOut)
    }
}

/// A leg-executing fake whose `dry_run` takes a chosen time before it accepts.
///
/// **The instrument for the federated half of the pre-`execute` re-check**, on the `dry_run` side
/// rather than the `execute` side: `answer_federated` dry-runs BOTH legs (sequentially, before
/// either `execute` runs) and only then runs either leg, so a budget spent while the FACT leg's
/// pre-flight sleeps is found spent by the LOOKUP leg's own pre-call check inside `dry_run_leg` -
/// and neither leg is ever executed. `FixedWarehouse::answering_after` gives the mono path the same
/// shape; this type declares `EXECUTES_LEGS` so the federated path reaches it.
pub(crate) struct SlowDryRunLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    /// How long this fake's `dry_run` sleeps before it accepts - see the struct's own note.
    dry_run_takes: std::time::Duration,
    dry_runs: std::sync::atomic::AtomicUsize,
    executions: std::sync::atomic::AtomicUsize,
}

impl SlowDryRunLegsWarehouse {
    /// Answers `result`, its `dry_run` taking `dry_run_takes` before accepting.
    pub(crate) fn answering_after(
        source: SourceName,
        posture: SourcePosture,
        result: RowSet,
        dry_run_takes: std::time::Duration,
    ) -> Self {
        Self {
            source,
            posture,
            result,
            dry_run_takes,
            dry_runs: std::sync::atomic::AtomicUsize::new(0),
            executions: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// How many times `dry_run` was reached.
    pub(crate) fn dry_runs(&self) -> usize {
        self.dry_runs.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// How many times `execute` was reached - the count a dry-run overrun must keep at zero.
    pub(crate) fn executions(&self) -> usize {
        self.executions.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Warehouse for SlowDryRunLegsWarehouse {
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
        self.dry_runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(self.dry_run_takes);
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.executions.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(crate::tests_support::canned(&self.result))
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// The mono-path and federated-leg instances of `DeadlineExceededWarehouse`, the shape
/// `MonoPreflightWarehouse`/`LegPreflightWarehouse` already give the pre-flight sibling.
pub(crate) type MonoDeadlineExceededWarehouse = DeadlineExceededWarehouse<false>;
pub(crate) type LegDeadlineExceededWarehouse = DeadlineExceededWarehouse<true>;
