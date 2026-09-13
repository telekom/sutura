//! `sutura_app::answer` wired to a REAL, configured `SpendLedger` - split out of `super` (`tests.rs`)
//! for that file's own `max-lines` cap, and because the local `answer` wrapper there is pinned to
//! `SpendLedger::no_budget()` for every other test - so these two call `crate::answer` directly.
//! `use super::*` reaches every fixture this file needs: `bundle`, `source`, `shared`,
//! `asked_by_a_person`, `test_deadline`, `metric`, `june`, `certified`.

use sutura_domain::model::Grain;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};

use super::*;
use crate::spend::{SpendBudget, SpendLedger};
use crate::tests_support::{FixedBroker, PricedWarehouse};

#[test]
fn a_configured_ceiling_refuses_when_the_dry_runs_own_price_exceeds_it() {
    let warehouses = Warehouses::of(PricedWarehouse::pricing(source(), shared(), certified(), Some(2_000)));
    let validated = verify_and_validate(bundle(), &warehouses).expect("the anchors hold");
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, std::time::Duration::from_secs(60))));
    let outcome = crate::answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        1 << 30,
        test_deadline(),
        &ledger,
    )
    .expect("a refusal is an Ok, so a client cannot retry it into an answer")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::BudgetExhausted { .. }
            }
        ),
        "a dry run priced over the configured ceiling must refuse, not {outcome:?}"
    );
}

#[test]
fn an_adapter_that_did_not_price_is_never_refused_for_spend() {
    // `docs/adr/0030`'s "not counted, never free": an estimate of `None` must not reach the ledger
    // as any number at all - not zero, and not a stand-in for "unknown, so assume the worst". The
    // ceiling here is the smallest `SpendBudget` accepts, so ANY charge this call made - zero or
    // otherwise - would still be visible if the mutation substitute for this cell mapped `None` to
    // something non-zero; a bug that mapped it to zero instead is inert by construction, because
    // charging zero against any valid (non-zero) ceiling can never itself cause a refusal.
    let warehouses = Warehouses::of(PricedWarehouse::pricing(source(), shared(), certified(), None));
    let validated = verify_and_validate(bundle(), &warehouses).expect("the anchors hold");
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let ledger = SpendLedger::new(Some(SpendBudget::new(1, std::time::Duration::from_secs(60))));
    let outcome = crate::answer(
        &validated,
        &question,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        1 << 30,
        test_deadline(),
        &ledger,
    )
    .expect("an answer is an Ok")
    .into_outcome();
    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "an adapter that did not price its dry run must never be refused for spend: {outcome:?}"
    );
}
