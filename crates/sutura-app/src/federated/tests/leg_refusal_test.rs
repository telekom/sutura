//! Three leg-level refusals, split out of `super` (the `federated::tests` module) for that file's
//! own `max-lines` reason - not for thematic tidiness, so `use super::*` reaches every fixture
//! (`federated_plan`, `federated_fact_rows`, `federated_lookup_rows`, `FEDERATED_BUDGET`,
//! `answer_federated` itself) exactly as these tests read them before the move.

use super::*;

#[test]
fn a_federated_leg_that_hits_the_volume_bound_is_refused_not_a_503() {
    // The federated half of the volume bound, and the reason `run_leg` asks the predicates at
    // all. A leg against a data system that will not return the whole result at once used to leave
    // as `ServiceError::Warehouse` - the `503` a dead data system produces - so a caller was told
    // to retry a reply that returns the same page. Both legs here answer `Err` with
    // `result_did_not_fit` true, and the fact leg runs first, so it must be refused as
    // `ResultTooLarge` carrying `Volume`.
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::PageBoundLegsWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
    ))
    .and(crate::tests_support::PageBoundLegsWarehouse::new(
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
    )
    .expect("a bound is a refusal, not an error")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    bound: ResultBound::Volume
                }
            }
        ),
        "a leg the data system will not return at once must be refused as the volume bound, not {outcome:?}"
    );
}

/// Lookup rows one byte over [`sutura_domain::query::ResponseByteLimit::DEFAULT`] - neither
/// the row cap nor the volume bound is what fires.
fn oversized_lookup_rows() -> RowSet {
    use sutura_domain::plan::InternalLabel;
    let ceiling = usize::try_from(sutura_domain::query::ResponseByteLimit::DEFAULT.bytes()).expect("fits a usize");
    let oversized = "x".repeat(ceiling + 1);
    let row = |link: &str| vec![Value::Text(link.into()), Value::Text(oversized.clone())];
    RowSet::new(vec![InternalLabel::Link.label(), "region".into()], vec![row("c1"), row("c2")])
        .expect("a well-formed test lookup result")
}

/// The federated half of the mono-source golden cell: a JOINED dimension's own value is not
/// something either leg's own bound was measuring.
#[test]
fn a_federated_answer_within_the_row_cap_but_too_wide_to_encode_is_refused() {
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("geo").expect("a test source"),
        shared,
        oversized_lookup_rows(),
    ))
    .expect("two sources, one registry");
    let outcome = answer_federated(
        &bundle(),
        &federated_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
    )
    .expect("a bound is a refusal, not an error")
    .into_outcome();
    let limit_bytes = sutura_domain::query::ResponseByteLimit::DEFAULT.bytes();
    assert_eq!(
        outcome,
        ToolOutcome::Refusal {
            reason: RefusalReason::ResultTooLarge {
                bound: ResultBound::Encoded { limit_bytes },
            },
        }
    );
}

#[test]
fn a_federated_leg_the_source_refuses_is_refused_not_a_503() {
    // The federated half of the identity/authorization refusal, and the reason `run_leg`
    // is given the `source_refused` predicate. A leg the data system refuses because the
    // identity it runs as may not ask it used to leave as `LegError::Failure` - the `503` a
    // dead data system produces - so a caller was told to retry a refusal that returns the
    // same reply. Both legs refuse at the identity/authorization level, and the fact leg runs
    // first, so the answer must be refused as `SourceRefused` carrying the source.
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::RefusingLegsWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
    ))
    .and(crate::tests_support::RefusingLegsWarehouse::new(
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
    )
    .expect("a source refusal is a refusal, not an error")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::SourceRefused { .. }
            }
        ),
        "a leg the data system refuses at the identity/authorization level must be refused, not {outcome:?}"
    );
}
