//! The evidence that a federated answer's two legs run CONCURRENTLY, and that concurrency does
//! not disturb the invariants the sequential path held: a shared `Deadline` and a deterministic
//! provenance order. Split out of `super` (the `federated::tests` module) for that file's own
//! `max-lines` reason - not for thematic tidiness, so `use super::*` reaches every fixture exactly
//! as the other split-out modules read them.

use super::*;
#[expect(
    clippy::disallowed_types,
    reason = "a synchronous test fake's recorder and barrier: blocking scoped threads with no async awaiting the lock, so the executor-deadlock this ban exists for cannot arise; std Mutex (not tokio) because the fakes are synchronous"
)]
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// A two-party rendezvous whose wait resolves within a timeout instead of deadlocking, so a
/// SEQUENTIAL caller (one party finishing before the other starts) fails red in finite time rather
/// than hanging the suite.
///
/// `std::sync::Barrier(2)` cannot express the timeout half - it has no timed wait - so this
/// reproduces its contract over a `Mutex`-and-`Condvar`, which is the std primitive that does:
/// both parties must arrive for either `wait` to return `true`; a party that waits alone past the
/// timeout returns `false`. That is the whole of what the concurrency cell needs.
#[expect(
    clippy::disallowed_types,
    reason = "a synchronous test-barrier: the Mutex guard is taken and dropped inside one timed wait call on a blocking thread with no async awaiting the lock, so the executor-deadlock this ban exists for cannot arise; std Mutex (not tokio) because the wait is synchronous"
)]
struct TimedBarrier {
    arrived: Mutex<u8>,
    release: Condvar,
}

impl TimedBarrier {
    /// A two-party barrier, both sides absent.
    #[expect(
        clippy::disallowed_types,
        reason = "a synchronous test-barrier's counter: see the struct field's own license - std Mutex because the wait is synchronous"
    )]
    fn new() -> Self {
        Self {
            arrived: Mutex::new(0),
            release: Condvar::new(),
        }
    }

    /// Blocks until the second party arrives, or `timeout` elapses.
    ///
    /// `true` = both parties reached the barrier (they ran in parallel - a sequential caller
    /// cannot, because the second party never starts until the first returns). `false` = the
    /// partner never arrived inside the timeout.
    ///
    /// Uses [`Condvar::wait_timeout_while`] rather than a single [`Condvar::wait_timeout`]: the
    /// `while` re-checks the predicate after every spurious wakeup, so only the timeout - or the
    /// second arrival - ends the wait. A bare `wait_timeout` trusts a spurious wakeup to mean
    /// "the other party arrived", which a scheduler can deliver early.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "the guard must stay alive across the whole wait, including the Condvar wait_timeout_while it is handed, so the barrier cannot be modelled as a short-lived temporary"
    )]
    fn wait(&self, timeout: Duration) -> bool {
        let mut arrived = self.arrived.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *arrived += 1;
        if *arrived == 2 {
            self.release.notify_all();
            return true;
        }
        let (guard, result) = self
            .release
            .wait_timeout_while(arrived, timeout, |count| *count < 2)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard == 2 && !result.timed_out()
    }
}

/// A leg-executing fake whose `execute` waits for a partner before answering.
///
/// Two of these over ONE shared [`TimedBarrier`] are the instrument for "the two legs run
/// concurrently": both `execute` calls can only pass their barrier if they overlap in time. A caller
/// that runs the fact leg to completion before starting the lookup leg - the sequential path this
/// change replaces - makes the fact leg's `execute` wait on a barrier the lookup leg never reaches,
/// and the wait times out into a warehouse failure rather than an answer.
struct BarrieredLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    barrier: Arc<TimedBarrier>,
}

impl BarrieredLegsWarehouse {
    fn answering(source: SourceName, posture: SourcePosture, result: RowSet, barrier: Arc<TimedBarrier>) -> Self {
        Self {
            source,
            posture,
            result,
            barrier,
        }
    }
}

impl Warehouse for BarrieredLegsWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

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
        // Ten seconds is far past anything a sequential partner could be waiting on (the partner
        // would never start), so a timeout here proves the legs did NOT overlap - the sequential
        // path - and is a genuine warehouse failure rather than a deadline the question opened.
        if self.barrier.wait(Duration::from_secs(10)) {
            Ok(crate::tests_support::canned(&self.result))
        } else {
            Err(AdapterFailure::Statement {
                cause: crate::tests_support::DriverFailure,
            })
        }
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// The two legs of one answer run CONCURRENTLY.
///
/// Two [`BarrieredLegsWarehouse`] over one barrier: the answer can only be produced if both
/// `execute` calls overlap in time, which is exactly what two concurrent tasks do and a sequential
/// caller cannot. On the sequential path this cell is RED - the fact leg's wait times out and
/// `answer_federated` leaves as a `ServiceError::Warehouse` the `.expect` here panics on.
#[test]
fn two_legs_that_each_wait_for_the_other_run_concurrently() {
    let shared = shared();
    let barrier = Arc::new(TimedBarrier::new());
    let warehouses = Warehouses::of(BarrieredLegsWarehouse::answering(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        federated_fact_rows(),
        Arc::clone(&barrier),
    ))
    .and(BarrieredLegsWarehouse::answering(
        SourceName::parse("geo").expect("a test source"),
        shared,
        federated_lookup_rows(),
        Arc::clone(&barrier),
    ))
    .expect("two sources, one registry");

    let outcome = answer_federated(
        &bundle(),
        &federated_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a concurrent answer whose two legs wait on each other completes")
    .into_outcome();
    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "both legs passed their shared barrier, so they ran concurrently: {outcome:?}"
    );
}

/// The pre-`execute` deadline re-check (`run_leg`'s own guard, leg.rs) still refuses BOTH legs when
/// the budget is spent DURING the dry-run phase - even though, unlike the dry-run cell above, both
/// dry runs are actually reached this time.
///
/// Here it is the LOOKUP leg's dry run that is slow, not the fact leg's: `dry_run_leg`'s own
/// pre-call guard is asked before EACH leg's own `dry_run`, using the deadline as it stands at that
/// moment - so the fact leg's dry run (instant) passes its guard, and the lookup leg's dry run
/// (which then sleeps past the budget) passes ITS guard too, because the sleep is inside the call
/// the guard only gates the START of. Both dry runs are therefore reached (unlike the fact-leg-slow
/// cell, where the lookup leg's dry-run guard already finds the budget spent and its `dry_run` is
/// never called at all) - so this cell exercises a DIFFERENT guard: `run_leg`'s own re-check,
/// asked of each leg just before its `execute`, after the dry-run phase has already spent the
/// shared deadline.
#[test]
fn a_federated_lookup_dry_run_that_spends_the_budget_refuses_both_legs_before_either_executes() {
    let fact = SlowDryRunLegsWarehouse::answering_after(
        SourceName::parse("facts").expect("a test source"),
        shared(),
        federated_fact_rows(),
        std::time::Duration::ZERO,
    );
    let lookup = SlowDryRunLegsWarehouse::answering_after(
        SourceName::parse("geo").expect("a test source"),
        shared(),
        federated_lookup_rows(),
        std::time::Duration::from_secs(1),
    );
    let warehouses = Warehouses::of(fact).and(lookup).expect("two sources");
    let almost_spent = Deadline::opened_at(
        Instant::now(),
        Budget::parse(std::time::Duration::from_millis(250)).expect("250ms"),
    );
    let outcome = answer_federated(
        &bundle(),
        &federated_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        almost_spent,
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a refusal is an Ok")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::DeadlineExceeded { .. }
            }
        ),
        "a budget spent during the dry-run phase must refuse both legs at their pre-execute check, not {outcome:?}"
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("facts").expect("a test source"))
            .expect("facts is registered")
            .executions(),
        0,
        "the fact leg's own pre-execute check must find the shared budget spent"
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("geo").expect("a test source"))
            .expect("geo is registered")
            .executions(),
        0,
        "the lookup leg's own pre-execute check must find the shared budget spent"
    );
}
