//! `docs/adr/0029` decision 3's own RED cell, split out of `super` (the `federated::tests` module)
//! for that file's own `max-lines` reason - not for thematic tidiness, so `use super::*` reaches
//! every fixture (`federated_plan`, `federated_fact_rows`, `federated_lookup_rows`,
//! `FEDERATED_BUDGET`, `answer_federated` itself) exactly as this test read them before the move.

use super::*;

#[test]
fn a_federated_answer_shares_one_instant_and_the_second_leg_sees_what_is_left() {
    // `docs/adr/0029` decision 3: ONE `Deadline`, never divided. The fact leg's own fake takes 200ms
    // to answer - deterministically longer than the 5ms budget, rather than a race against however
    // long two in-memory calls happen to take - so the lookup leg's pre-call check finds it spent
    // and never asks its adapter at all.
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let warehouses = Warehouses::of(RecordingLegsWarehouse::answering_after(
        SourceName::parse("facts").expect("a test source"),
        shared(),
        federated_fact_rows(),
        std::rc::Rc::clone(&seen),
        std::time::Duration::from_millis(200),
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
        Budget::parse(std::time::Duration::from_millis(5)).expect("5ms"),
    );
    let outcome = answer_federated(
        &bundle(),
        &federated_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        FEDERATED_BUDGET,
        almost_spent,
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
