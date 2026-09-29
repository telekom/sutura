//! Fakes for `docs/adr/0029`'s own cells: a data system whose failure IS the deadline, and one that
//! reports what was left of the deadline a raw statement arrived with.
//!
//! Split out of `testing.rs` for that file's own `max-lines` reason.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, RawExecution, RawRows, ResultBatches, Value, Warehouse};

use super::{ConnectionRefused, HELD_AT_MOST, StatementRejected, shared_posture};

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

/// A data system whose raw path holds `"hold me"` while armed and records what was left of the
/// deadline any other statement arrived with - so a statement queued behind the held one shows
/// whether `/v1/sql/run`'s deadline opened before admission or after it.
pub(crate) struct RawDeadlineProbe {
    source: SourceName,
    posture: SourcePosture,
    probed: Arc<Probed>,
}

/// The switch that holds a [`RawDeadlineProbe`], and what it observed.
#[derive(Default)]
pub(crate) struct Probed {
    held: AtomicBool,
    holding: AtomicBool,
    remaining: OnceLock<Duration>,
}

impl Probed {
    /// From here on, `"hold me"` does not come back.
    pub(crate) fn arm(&self) {
        self.held.store(true, Ordering::SeqCst);
    }

    /// Let `"hold me"` finish, so the blocking pool drains with the test.
    pub(crate) fn release(&self) {
        self.held.store(false, Ordering::SeqCst);
    }

    /// Whether `"hold me"` is inside the port.
    pub(crate) fn holding(&self) -> bool {
        self.holding.load(Ordering::SeqCst)
    }

    /// What was left of the first other statement's deadline, zero if it arrived spent; `None` if
    /// none arrived.
    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.remaining.get().copied()
    }
}

impl RawDeadlineProbe {
    /// One probe reporting into `probed` - the same `Arc` the test reads.
    pub(crate) fn new(source: SourceName, probed: &Arc<Probed>) -> sutura_app::Warehouses<Self> {
        sutura_app::Warehouses::of(Self {
            source,
            posture: shared_posture(),
            probed: Arc::clone(probed),
        })
    }
}

impl Warehouse for RawDeadlineProbe {
    type Error = StatementRejected;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const ACCEPTS_RAW_STATEMENTS: bool = true;

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
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }

    fn execute_raw(
        &self,
        statement: &sutura_domain::raw::RawStatement,
        _presented: &Presented,
        deadline: Deadline,
    ) -> RawExecution<Self::Error> {
        if statement.as_str() == "hold me" {
            self.probed.holding.store(true, Ordering::SeqCst);
            let cap = Instant::now() + HELD_AT_MOST;
            while self.probed.held.load(Ordering::SeqCst) && Instant::now() < cap {
                std::thread::sleep(Duration::from_millis(5));
            }
            self.probed.holding.store(false, Ordering::SeqCst);
        } else {
            self.probed
                .remaining
                .get_or_init(|| deadline.remaining_at(Instant::now()).unwrap_or_default());
        }
        Some(Ok(RawRows::of(vec![String::from("n")], vec![vec![Value::Integer(1)]])))
    }
}
