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
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Warehouse};

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

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented, _deadline: Deadline) -> Result<RowSet, Self::Error> {
        self.executes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }
}

/// A leg-executing fake that records the [`Deadline`] each `execute` call was handed, in order.
///
/// **The instrument for `docs/adr/0029`'s decision 3** - one shared instant across a federated
/// answer's legs, never divided - which a value equality on two recorded deadlines proves and a row
/// count cannot. `Rc<RefCell<..>>` rather than a field this type owns, so ONE recorder is shared by
/// the two instances a federated test opens (one per source).
pub(crate) struct RecordingLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    seen: std::rc::Rc<std::cell::RefCell<Vec<Deadline>>>,
    /// How long this fake's `execute` takes before it answers.
    ///
    /// **For the one test that needs a budget to be spent BETWEEN two legs without a real clock
    /// deciding it by chance.** `tests_support::FixedWarehouse::pre_flight_takes` is the precedent:
    /// a fake that returns instantly cannot make a real-time deadline check fail deterministically,
    /// so the leg that must still run sleeps well past the tiny budget the test opens, and the leg
    /// that must never run is handed [`Duration::ZERO`] - it is never reached at all.
    delay: std::time::Duration,
}

impl RecordingLegsWarehouse {
    pub(crate) fn answering(
        source: SourceName,
        posture: SourcePosture,
        result: RowSet,
        seen: std::rc::Rc<std::cell::RefCell<Vec<Deadline>>>,
    ) -> Self {
        Self {
            source,
            posture,
            result,
            seen,
            delay: std::time::Duration::ZERO,
        }
    }

    /// The same, taking `delay` before it answers - see the field's own documentation.
    pub(crate) fn answering_after(
        source: SourceName,
        posture: SourcePosture,
        result: RowSet,
        seen: std::rc::Rc<std::cell::RefCell<Vec<Deadline>>>,
        delay: std::time::Duration,
    ) -> Self {
        Self {
            source,
            posture,
            result,
            seen,
            delay,
        }
    }
}

impl Warehouse for RecordingLegsWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented, deadline: Deadline) -> Result<RowSet, Self::Error> {
        std::thread::sleep(self.delay);
        self.seen.borrow_mut().push(deadline);
        Ok(self.result.clone())
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}
