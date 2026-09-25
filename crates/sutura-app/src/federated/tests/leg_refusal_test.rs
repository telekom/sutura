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
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
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
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
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
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
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

/// [`federated_fact_rows`]'s shape, with its link column carrying an integer rather than a text -
/// `telekom/sutura#138`'s own shape: paired with [`federated_lookup_rows`]'s text link, no row on
/// either leg can ever match the other.
///
/// **Local to this file on purpose.** `xtask test-causality` reverts a changed file that adds no
/// `#[test]` of its own to prove red-before-green, and `super::federated_fact_rows` lives in
/// `tests.rs` beside no new test - so a fixture this suite needed stayed OUT of that file, exactly
/// as `super::federated_plan_inner_join` below does for the same reason.
fn mismatched_fact_rows() -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![
            String::from("product_family"),
            InternalLabel::Link.label(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(0).label(),
        ],
        vec![vec![
            Value::Text("A".into()),
            Value::Integer(1),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]],
    )
    .expect("a well-formed test fact result")
}

/// [`federated_plan`]'s own shape with `include_unmatched: false` - the INNER half of the pair
/// below. Duplicated rather than reached through a shared parameterised fixture for the same
/// `test-causality` reason [`mismatched_fact_rows`] documents: `federated_plan` lives in `tests.rs`,
/// which this diff must not touch, since a helper edited there but exercised only from here would
/// be reverted out from under this file's tests when the gate proves red-before-green.
fn federated_plan_inner_join() -> sutura_domain::plan::FederatedPlan {
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::Aggregate;
    use sutura_domain::model::{ColumnName, DimensionName, TableName};
    use sutura_domain::plan::{
        AnswerKey, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel,
        StatementTables, labels,
    };

    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let table = TableName::parse("fct_subscription_monthly").expect("a test table");
    let column = |n: &str| ColumnName::parse(n).expect("a test column");
    let tablecol = |n: &str| PlanColumn::new(table.clone(), column(n));
    let dimension = |n: &str| DimensionName::parse(n).expect("a test dimension");
    let key = |n: &str| PlanKey::new(ResultLabel::dimension(&dimension(n)), tablecol(n));
    let link = || PlanKey::new(ResultLabel::internal(InternalLabel::Link), tablecol("customer_key"));
    let bucket = |c: &str| PlanBucket::new(ResultLabel::bucket(), Grain::Month, PlanColumn::new(table.clone(), column(c)));

    let sum = Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))));
    let federation = sutura_domain::federation::Federation::of(&sum);
    let terms: Vec<LegTerm> = labels(&federation)
        .into_iter()
        .map(|label| {
            LegTerm::new(
                PlanTerm::CountIf {
                    column: tablecol("amount_cents"),
                },
                ResultLabel::internal(label),
            )
        })
        .collect();
    let fact = LegPlan::Fact {
        source: fact_source,
        metric: metric(),
        tables: StatementTables::only(table.clone()),
        bucket: bucket("month"),
        keys: vec![key("product_family"), link()],
        terms,
        bindings: PlanBindings::none(),
        range: june(),
    };
    let lookup = LegPlan::Lookup {
        source: lookup_source,
        table: table.clone().into(),
        keys: vec![link(), key("region")],
        bindings: PlanBindings::none(),
    };
    sutura_domain::plan::FederatedPlan::new(
        metric(),
        ResultLabel::measure(&metric()),
        bucket("month"),
        fact,
        lookup,
        false,
        federation,
        vec![
            AnswerKey::fact(ResultLabel::dimension(&dimension("product_family"))),
            AnswerKey::lookup(ResultLabel::dimension(&dimension("region"))),
        ],
    )
    .expect("a valid two-leg plan")
}

/// Two leg-executing fakes whose link columns carry different scalar kinds: `facts` an integer,
/// `geo` a text - `telekom/sutura#138`'s own shape, run through both legs rather than asserted
/// directly against [`sutura_domain::plan::FederatedPlan::combine`].
fn mismatched_link_warehouses() -> Warehouses<crate::tests_support::LegsWarehouse> {
    let shared = shared();
    Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        mismatched_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        SourceName::parse("geo").expect("a test source"),
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry")
}

#[test]
fn a_federated_answer_whose_legs_disagree_on_link_column_type_is_refused_under_a_left_join() {
    // LEFT is the more misleading of the two flavours: before `LinkTypeMismatch`, every fact row
    // here would have survived with a null remote side, reading as "no match for this key" rather
    // than "these two link columns can never agree" - #138's own correction that one flavour
    // alone passes vacuously, so this is paired with the INNER cell below.
    let outcome = answer_federated(
        &bundle(),
        &federated_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &mismatched_link_warehouses(),
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a link type mismatch is a refusal, not an error")
    .into_outcome();
    assert_eq!(
        outcome,
        ToolOutcome::Refusal {
            reason: RefusalReason::FederatedAnswerNotWellFormed {
                federated: sutura_domain::plan::FederatedAnswerRefusal::LinkTypeMismatch,
            },
        },
        "two legs whose link columns can never match must be refused, not {outcome:?}"
    );
}

#[test]
fn a_federated_answer_whose_legs_disagree_on_link_column_type_is_refused_under_an_inner_join() {
    let outcome = answer_federated(
        &bundle(),
        &federated_plan_inner_join(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &mismatched_link_warehouses(),
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a link type mismatch is a refusal, not an error")
    .into_outcome();
    assert_eq!(
        outcome,
        ToolOutcome::Refusal {
            reason: RefusalReason::FederatedAnswerNotWellFormed {
                federated: sutura_domain::plan::FederatedAnswerRefusal::LinkTypeMismatch,
            },
        },
        "two legs whose link columns can never match must be refused, not {outcome:?}"
    );
}

/// When BOTH legs fail transiently, the FACT leg's warehouse error wins.
///
/// Both legs run in the concurrent scope and both produce a `LegError::Failure`; `answer_federated`
/// inspects them in plan order - the fact leg first - so its `ServiceError::Warehouse` is the one
/// surfaced, never the lookup leg's and never whichever happened to be scheduled later. Each fake's
/// error names its own source, so which leg won is distinguishable rather than assumed. This pins
/// the same fact-first precedence the sequential path held and the concurrent path must preserve.
#[test]
fn when_both_legs_fail_the_fact_legs_error_is_the_one_surfaced() {
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::TransientlyFailingLegsWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
    ))
    .and(crate::tests_support::TransientlyFailingLegsWarehouse::new(
        SourceName::parse("geo").expect("a test source"),
        shared,
    ))
    .expect("two sources, one registry");

    let failure = answer_federated(
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
    .expect_err("two transient failures leave as a warehouse error, not a refusal");
    assert!(
        matches!(
            failure,
            crate::ServiceError::Warehouse {
                cause: AdapterFailure::NoPlaceForASubject { ref at, .. }
            } if at == "facts"
        ),
        "the FACT leg's transient failure wins, not {failure:?}"
    );
}
