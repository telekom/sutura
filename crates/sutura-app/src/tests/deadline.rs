//! `docs/adr/0029` at the query path: the mapping from an adapter's own `deadline_exceeded` to
//! `RefusalReason::DeadlineExceeded`, and the pre-call check that refuses a spent budget before the
//! data system is ever asked.

use sutura_domain::model::Grain;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use super::{answer, asked_by_a_person, bundle, certified, june, metric, shared, source, test_deadline};
use crate::tests_support::{DeadlineExceededWarehouse, FixedBroker, FixedWarehouse, NeverAskedWarehouse};
use crate::{Warehouses, verify_and_validate};

#[test]
fn running_out_of_time_is_a_refusal_and_not_a_503() {
    // `docs/adr/0029`'s decision 2, at this call site. An adapter's failure that satisfies
    // `Warehouse::deadline_exceeded` must turn into `RefusalReason::DeadlineExceeded` - a governed
    // `Ok`, carrying the configured budget - and never into `ServiceError::Warehouse`, which is what
    // a dead data system produces and what invites a retry that spends the whole budget again.
    let working = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &working).expect("the anchor reproduces its number");
    let timed_out = Warehouses::of(DeadlineExceededWarehouse::new(source(), shared()));
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let outcome = answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &timed_out,
    )
    .expect("a refusal is an Ok, so a client cannot retry it into an answer")
    .into_outcome();
    let ToolOutcome::Refusal {
        reason: RefusalReason::DeadlineExceeded { budget_seconds },
    } = outcome
    else {
        panic!("a stopped deadline must come back as a refusal, not {outcome:?}");
    };
    // The budget this test's own `test_deadline()` opened with, read back rather than a hard-coded
    // number: the value under test is that ONE number travels from the deadline to the refusal, and
    // pinning the value redundantly would still pass if that link broke on both ends the same way.
    assert_eq!(budget_seconds, test_deadline().budget().seconds());
}

#[test]
fn a_budget_spent_before_the_leg_starts_is_refused_and_the_data_system_is_never_asked() {
    // `docs/adr/0029`: a budget already spent is refused BEFORE `dry_run` - the data system is
    // never asked at all, which is stronger than an adapter's own refusal and is what this fake's
    // call count proves rather than merely its outcome.
    let working = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &working).expect("the anchor reproduces its number");
    let warehouses = Warehouses::of(NeverAskedWarehouse::new(source(), shared()));
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    // Opened ten seconds in the past with a one-second budget: spent long before this call.
    let spent = Deadline::opened_at(
        std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(10))
            .expect("ten seconds before now does not underflow the monotonic clock"),
        Budget::parse(std::time::Duration::from_secs(1)).expect("one second is a budget"),
    );
    let outcome = crate::answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        1 << 30,
        spent,
    )
    .expect("a refusal is an Ok, so a client cannot retry it into an answer")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::DeadlineExceeded { .. }
            }
        ),
        "a budget spent before the call must refuse, not {outcome:?}"
    );
    assert_eq!(
        warehouses.get(&source()).expect("the registry holds it").calls(),
        0,
        "the data system must never be asked once its budget is already spent"
    );
}
