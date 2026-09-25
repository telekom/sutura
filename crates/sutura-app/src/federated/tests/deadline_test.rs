//! `docs/adr/0029` decision 3's own RED cell, split out of `super` (the `federated::tests` module)
//! for that file's own `max-lines` reason - not for thematic tidiness, so `use super::*` reaches
//! every fixture (`federated_plan`, `federated_fact_rows`, `federated_lookup_rows`,
//! `FEDERATED_BUDGET`, `answer_federated` itself) exactly as this test read them before the move.

use super::*;

#[test]
fn a_federated_leg_that_times_out_is_refused_not_a_503() {
    // `docs/adr/0029` D2's federated call site: `run_leg` must map an `execute` failure that
    // satisfies `deadline_exceeded` to `RefusalReason::DeadlineExceeded`, the same as the mono
    // path's `running_out_of_time_is_a_refusal_and_not_a_503` - never into `LegError::Failure`,
    // which the transport answers `503`, inviting a retry that spends the whole budget again.
    let shared = shared();
    let warehouses = Warehouses::of(LegDeadlineExceededWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
    ))
    .and(LegDeadlineExceededWarehouse::new(
        SourceName::parse("geo").expect("a test source"),
        shared,
    ))
    .expect("two sources, one registry");

    let plan = federated_plan();
    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a stopped deadline is a refusal, not an error")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::DeadlineExceeded { .. }
            }
        ),
        "a leg the data system reports as timed out must be refused, not {outcome:?}"
    );
}

#[test]
fn a_federated_legs_pre_flight_that_times_out_is_refused_not_a_503() {
    // The `dry_run`-failure sibling of the cell above, for `dry_run_leg`'s own pre-flight arm - an
    // adapter can report the deadline as fired while CHECKING a leg's statement, not only while
    // running it.
    let shared = shared();
    let fact = LegPreflightWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        federated_fact_rows(),
        DryRunOutcome::TimedOut,
    );
    let lookup = LegPreflightWarehouse::new(
        SourceName::parse("geo").expect("a test source"),
        shared,
        federated_lookup_rows(),
        DryRunOutcome::Accepted,
    );
    let warehouses = Warehouses::of(fact).and(lookup).expect("two sources, one registry");
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
    .expect("a stopped deadline is a refusal, not an error")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::DeadlineExceeded { .. }
            }
        ),
        "a leg pre-flight the data system reports as timed out must be refused, not {outcome:?}"
    );
}

/// `docs/adr/0029` decision 3 - ONE `Deadline`, never divided - through the dry-run overrun.
///
/// Under the concurrent path both legs now run in parallel, so "the second leg sees what the first
/// left" (an ordering over `execute` calls) no longer exists to pin. What the shared instant must
/// still mean is that a budget spent by ONE leg's pre-flight is spent for BOTH legs: `answer_federated`
/// dry-runs both legs (sequentially, before either `execute` runs) under the same `Deadline`, so a
/// fact-leg pre-flight that sleeps past the tiny budget leaves the LOOKUP leg's own pre-call check
/// finding it spent and refusing `DeadlineExceeded` - and neither leg is ever asked to execute.
///
/// The fact leg's fake takes one second to dry-run - deterministically longer than the 250ms
/// budget, rather than a race against however long two in-memory calls happen to take. The RATIO is
/// what matters (4x), not the absolute numbers: an earlier 5ms budget / 200ms delay pair left the
/// LOOKUP leg's pre-call check needing to catch the instant the fact leg's dry run crossed, a margin
/// a scheduling stall under a parallel, thousands-of-tests run could cross. 250ms leaves that same
/// preamble a wide margin.
#[test]
fn a_federated_dry_run_that_spends_the_budget_refuses_both_legs_before_either_executes() {
    let fact = SlowDryRunLegsWarehouse::answering_after(
        SourceName::parse("facts").expect("a test source"),
        shared(),
        federated_fact_rows(),
        std::time::Duration::from_secs(1),
    );
    let lookup = SlowDryRunLegsWarehouse::answering_after(
        SourceName::parse("geo").expect("a test source"),
        shared(),
        federated_lookup_rows(),
        std::time::Duration::ZERO,
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
        "a budget spent by one leg's dry run must refuse the shared answer, not {outcome:?}"
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("facts").expect("a test source"))
            .expect("facts is registered")
            .executions(),
        0,
        "the fact leg's own dry run spent the budget, so even it must never execute"
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("geo").expect("a test source"))
            .expect("geo is registered")
            .executions(),
        0,
        "the lookup leg's pre-call check finds the shared budget spent, so it never executes"
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("geo").expect("a test source"))
            .expect("geo is registered")
            .dry_runs(),
        0,
        "the lookup leg is refused by the pre-call check before it is even asked to dry-run"
    );
}
