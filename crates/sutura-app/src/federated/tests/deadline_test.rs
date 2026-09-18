//! `docs/adr/0029` decision 3's own RED cell, split out of `super` (the `federated::tests` module)
//! for that file's own `max-lines` reason - not for thematic tidiness, so `use super::*` reaches
//! every fixture (`federated_plan`, `federated_fact_rows`, `federated_lookup_rows`,
//! `FEDERATED_BUDGET`, `answer_federated` itself) exactly as this test read them before the move.

use super::*;

#[test]
fn a_federated_answer_shares_one_instant_and_the_second_leg_sees_what_is_left() {
    // `docs/adr/0029` decision 3: ONE `Deadline`, never divided. The fact leg's own fake takes one
    // second to answer - deterministically longer than the 250ms budget, rather than a race against
    // however long two in-memory calls happen to take - so the lookup leg's pre-call check finds it
    // spent and never asks its adapter at all. The RATIO is what matters (4x), not the absolute
    // numbers: an earlier 5ms budget / 200ms delay pair left the FIRST leg's own pre-call check (run
    // after broker agreement, `Warehouses::get`, `presented` and `agrees_with`, all before any
    // sleep) needing to complete inside 5ms of `Instant::now()` - a margin a scheduling stall under
    // a parallel, thousands-of-tests run could cross for a reason that has nothing to do with this
    // change. 250ms leaves that same preamble a wide margin.
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let warehouses = Warehouses::of(RecordingLegsWarehouse::answering_after(
        SourceName::parse("facts").expect("a test source"),
        shared(),
        federated_fact_rows(),
        std::rc::Rc::clone(&seen),
        std::time::Duration::from_secs(1),
    ))
    .and(RecordingLegsWarehouse::answering(
        SourceName::parse("geo").expect("a test source"),
        shared(),
        federated_lookup_rows(),
        std::rc::Rc::clone(&seen),
    ))
    .expect("two sources");
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
        "a spent budget must refuse the second leg, not {outcome:?}"
    );
    assert_eq!(
        *seen.borrow(),
        vec![almost_spent],
        "only the fact leg should run, seeing the SAME deadline"
    );
}

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
