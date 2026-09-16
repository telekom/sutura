//! `sutura_app::answer` wired to a REAL, configured `SpendLedger` - split out of `super` (`tests.rs`)
//! for that file's own `max-lines` cap, and because the local `answer` wrapper there is pinned to
//! `SpendLedger::no_budget()` for every other test - so most of these call `crate::answer` directly.
//! The last cell calls `crate::charge_subject` directly instead, to pin an exact offset into the
//! ledger's window - `answer` reads `Instant::now()` itself and cannot be handed one.
//! `use super::*` reaches every fixture this file needs: `bundle`, `source`, `shared`,
//! `asked_by_a_person`, `test_deadline`, `metric`, `june`, `certified`.

use sutura_domain::identity::{Actor, ActorChain};
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
    // The ledger's POSITION, not just the outcome: a refusal that ran `execute` anyway would still
    // read as `BudgetExhausted` above, so that assertion alone does not hold "refused, not billed
    // silently" - only a call count on the adapter itself can.
    assert_eq!(
        warehouses
            .get(&source())
            .expect("the priced source is registered")
            .executions(),
        0,
        "a question refused for spend must never reach `execute`"
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

#[test]
fn two_subjects_are_isolated_through_answer_not_only_in_the_ledger() {
    // `spend::tests::two_subjects_are_isolated_from_each_other` proves the MAP; this proves the
    // SERVICE keys on the subject `PrincipalChain::attribution()` names when it consults that map -
    // a claim about `crate::answer`'s own wiring, held by nothing until now.
    let warehouses = Warehouses::of(PricedWarehouse::pricing(source(), shared(), certified(), Some(600)));
    let validated = verify_and_validate(bundle(), &warehouses).expect("the anchors hold");
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, std::time::Duration::from_secs(60))));
    let subject_a = RequestContext::of(PrincipalChain::of(
        Subject::verified("one@example.com").expect("a test subject id parses"),
    ));
    let subject_b = RequestContext::of(PrincipalChain::of(
        Subject::verified("two@example.com").expect("a test subject id parses"),
    ));
    let ask = |context: &RequestContext| {
        crate::answer(
            &validated,
            &question,
            context,
            &FixedBroker::GrantsShared,
            &warehouses,
            1 << 30,
            test_deadline(),
            &ledger,
        )
        .expect("an Ok either way")
        .into_outcome()
    };

    assert!(
        matches!(ask(&subject_a), ToolOutcome::Answer { .. }),
        "subject A's first 600 bytes fit under the 1000-byte ceiling"
    );
    assert!(
        matches!(
            ask(&subject_a),
            ToolOutcome::Refusal {
                reason: RefusalReason::BudgetExhausted { .. }
            }
        ),
        "subject A's second 600 bytes put them over the ceiling"
    );
    assert!(
        matches!(ask(&subject_b), ToolOutcome::Answer { .. }),
        "subject B has never been charged and is unaffected by subject A's own ceiling"
    );
}

#[test]
fn an_agent_acting_for_a_subject_spends_that_subjects_own_budget() {
    // `docs/adr/0030`'s reversal of its own first draft: the key is the SUBJECT `attribution()`
    // names, never the acting chain - so an agent's charge lands on the human it acted for, and
    // that human's own next direct question is refused by what the agent already spent for them.
    let warehouses = Warehouses::of(PricedWarehouse::pricing(source(), shared(), certified(), Some(600)));
    let validated = verify_and_validate(bundle(), &warehouses).expect("the anchors hold");
    let question = Query::new(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, std::time::Duration::from_secs(60))));
    let subject = Subject::verified("one@example.com").expect("a test subject id parses");
    let acting_for_the_subject = RequestContext::of(
        PrincipalChain::of(subject.clone()).acting(ActorChain::of(Actor::parse("query_agent").expect("a test actor parses"))),
    );
    let the_subject_directly = RequestContext::of(PrincipalChain::of(subject));
    let ask = |context: &RequestContext| {
        crate::answer(
            &validated,
            &question,
            context,
            &FixedBroker::GrantsShared,
            &warehouses,
            1 << 30,
            test_deadline(),
            &ledger,
        )
        .expect("an Ok either way")
        .into_outcome()
    };

    assert!(
        matches!(ask(&acting_for_the_subject), ToolOutcome::Answer { .. }),
        "the agent's first 600 bytes fit under the 1000-byte ceiling"
    );
    assert!(
        matches!(
            ask(&the_subject_directly),
            ToolOutcome::Refusal {
                reason: RefusalReason::BudgetExhausted { .. }
            }
        ),
        "the subject's own direct question is refused by what the agent already spent on their behalf"
    );
}

#[test]
fn a_refusal_in_the_last_half_second_of_a_window_rounds_up_to_one_second() {
    // `crates/sutura-http/src/wire.rs` documents `Retry-After: 0` as "a promise the next request
    // will be answered" - `Duration::as_secs` floors, so a refusal with 0.5s left in its window
    // must round UP to 1, not down to 0, or that promise is broken for anyone refused in the
    // window's last fraction.
    let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, std::time::Duration::from_secs(60))));
    let context = asked_by_a_person();
    let now = std::time::Instant::now();
    assert_eq!(
        crate::charge_subject(&ledger, &context, 1_000, now),
        None,
        "the first charge admits and refuses nothing"
    );
    let fifty_nine_point_five_seconds_in = now + std::time::Duration::from_millis(59_500);
    assert_eq!(
        crate::charge_subject(&ledger, &context, 1, fifty_nine_point_five_seconds_in),
        Some(RefusalReason::BudgetExhausted { reset_after_seconds: 1 }),
        "0.5 seconds left in the window rounds UP to 1, not down to 0"
    );
}
