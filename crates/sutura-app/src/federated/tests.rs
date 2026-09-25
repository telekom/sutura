use std::time::Instant;

use super::answer_federated;
use crate::Warehouses;
use crate::spend::SpendLedger;
use crate::tests::{asked_by_a_person, bundle, june, metric, shared, test_deadline};
use crate::tests_support::{
    AdapterFailure, DryRunOutcome, FixedBroker, LegDeadlineExceededWarehouse, LegPreflightWarehouse, SlowDryRunLegsWarehouse,
};
use sutura_domain::identity::Presented;
use sutura_domain::model::{Grain, SourceName};
use sutura_domain::plan::Executable;
use sutura_domain::query::{RefusalReason, ResultBound, ToolOutcome};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{AnchorRows, ResultBatches, RowSet, Value, Warehouse};

// ---------------------------------------------------------------------------
// The federated answer orchestration
// ---------------------------------------------------------------------------

/// An amount of time only the budget test would cross: every correctness path below hands the
/// combiner this effectively unbounded ceiling so only the memory-ceiling test reaches the refusal.
const FEDERATED_BUDGET: u64 = 1 << 30;

/// A fact leg and a lookup leg on two sources, the shape the splitter emits for a two-source
/// question. The rows the two fake warehouses return are the same fixture the domain's combiner
/// tests feed it, so this test is about the ORCHESTRATOR (mint once, run both, record both) and
/// leans on the combiner suite for the arithmetic.
fn federated_plan() -> sutura_domain::plan::FederatedPlan {
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
    // The link column, under the reserved label both legs project it as. The splitter names it
    // from `InternalLabel` and this fake does too, so the shape stays the shape it emits.
    let link = || PlanKey::new(ResultLabel::internal(InternalLabel::Link), tablecol("customer_key"));
    let bucket = |c: &str| PlanBucket::new(ResultLabel::bucket(), Grain::Month, PlanColumn::new(table.clone(), column(c)));

    let sum = Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))));
    let federation = sutura_domain::federation::Federation::of(&sum);
    // D9: the fact leg's terms must name the labels `federation` expects, in order - the same
    // pairing the one production splitter builds. The term itself is a placeholder; only the
    // label is read by anything this suite asserts on.
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
        None,
        lookup,
        true,
        federation,
        vec![
            AnswerKey::fact(ResultLabel::dimension(&dimension("product_family"))),
            AnswerKey::lookup(ResultLabel::dimension(&dimension("region"))),
        ],
    )
    .expect("a valid two-leg plan")
}

fn federated_fact_rows() -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![
            String::from("product_family"),
            InternalLabel::Link.label(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(0).label(),
        ],
        vec![
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
            vec![
                Value::Text("A".into()),
                Value::Text("c2".into()),
                Value::Text("2026-06".into()),
                Value::Integer(200),
            ],
            vec![
                Value::Text("B".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(50),
            ],
        ],
    )
    .expect("a well-formed test fact result")
}

fn federated_lookup_rows() -> RowSet {
    use sutura_domain::plan::InternalLabel;
    RowSet::new(
        vec![InternalLabel::Link.label(), String::from("region")],
        vec![
            vec![Value::Text("c1".into()), Value::Text("north".into())],
            vec![Value::Text("c2".into()), Value::Text("north".into())],
        ],
    )
    .expect("a well-formed test lookup result")
}

#[test]
fn a_federated_answer_mints_once_runs_both_legs_and_records_both_identities() {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        lookup_source,
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");

    let broker = crate::tests_support::CountingBroker::default();
    let plan = federated_plan();
    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a federated answer is not an error")
    .into_outcome();

    let ToolOutcome::Answer { provenance, .. } = outcome else {
        panic!("a two-source question whose adapters execute legs is answered, not {outcome:?}");
    };
    let legs: Vec<&str> = provenance.executed_as().legs().map(|(source, _)| source.as_str()).collect();
    assert_eq!(
        legs,
        vec!["facts", "geo"],
        "provenance records BOTH identities a federated answer ran as"
    );
    assert_eq!(
        broker.asked(),
        1,
        "a federated answer mints once over both sources, not once per leg"
    );
}

#[test]
fn a_federated_answer_sums_both_legs_estimates_before_charging_the_ledger_once() {
    // `docs/adr/0030`'s "all-or-nothing": neither leg's own price is under the ceiling here, and
    // the answer must still refuse, because the CEILING is over the SUM (600 + 600 = 1200) and
    // not over either leg alone (600 < 1000). A mutation that summed only the first leg's
    // estimate would see 600, admit it, and answer instead of refusing - which is exactly the
    // substitute this cell exists to catch where `test-causality` cannot separate it from base.
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::PricedWarehouse::pricing(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
        Some(600),
    ))
    .and(crate::tests_support::PricedWarehouse::pricing(
        lookup_source,
        shared,
        federated_lookup_rows(),
        Some(600),
    ))
    .expect("two sources, one registry");
    let ledger = SpendLedger::new(Some(crate::spend::SpendBudget::new(
        1_000,
        std::time::Duration::from_secs(60),
    )));
    let outcome = answer_federated(
        &bundle(),
        &federated_plan(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &ledger,
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a refusal is an Ok")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::BudgetExhausted { .. }
            }
        ),
        "the summed estimate (1200) is over the ceiling (1000), even though neither leg alone is: {outcome:?}"
    );
    // The ledger's POSITION, not just the outcome, and on BOTH legs: a mutation that charged
    // after `run_leg` (rather than after both `dry_run_leg`s and before either `run_leg`) would
    // still refuse - the estimates are unchanged - so the outcome assertion above cannot see it.
    for leg in ["facts", "geo"] {
        let source = SourceName::parse(leg).expect("a test source");
        assert_eq!(
            warehouses.get(&source).expect("both legs are registered").executions(),
            0,
            "a federated answer refused for spend must never reach either leg's `execute`"
        );
    }
}

/// `docs/adr/0029` decision 3's own RED cell - split out so this file stays under the
/// `max-lines` cap it was already at before this record.
mod deadline_test;

/// The concurrency evidence - two legs waiting on one barrier, and provenance order under a
/// lookup-first finish - split out for the same `max-lines` reason `deadline_test` was. Its own
/// header states why `#[cfg(test)]` is redundant under this file's gate and kept anyway.
#[cfg(test)]
mod concurrent_test;

/// Three leg-level refusals, split out for the same `max-lines` reason `deadline_test` was.
///
/// `#[cfg(test)]` here is redundant under this file's own gate and present anyway - the same
/// reason `telekom/sutura#657` states at its own `mod refresh;`: `xtask test-causality` reverts
/// a file that adds no `#[test]` of its own, and a bare `mod leg_refusal_test;` declares
/// nothing the scan reads as one, so a later diff dropping only that line back to base would
/// orphan this module rather than fail loud. The attribute makes the declaration itself read as
/// `TestModule`, which the gate holds.
#[cfg(test)]
mod leg_refusal_test;

/// Case 2's own cells, split out for the same `max-lines` reason - see the module's own header.
#[cfg(test)]
mod top_test;
/// The cross-model ratio's own cells (`telekom/sutura#780`), split out for the same `max-lines`
/// reason `top_test` was. `#[cfg(test)]` is redundant under this file's own gate and present for
/// the same `test-causality` reason the submodules above state.
#[cfg(test)]
mod two_fact_test;

#[test]
fn a_federated_fact_preflight_refusal_is_not_a_partial_answer() {
    let shared = shared();
    let fact = LegPreflightWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        federated_fact_rows(),
        DryRunOutcome::SourceRefused,
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
    .expect("a pre-flight refusal is a governed answer")
    .into_outcome();
    assert!(matches!(
        outcome,
        ToolOutcome::Refusal { reason: RefusalReason::SourceRefused { ref source } }
            if source.as_str() == "facts"
    ));
    assert_eq!(
        warehouses
            .get(&SourceName::parse("facts").expect("a test source"))
            .expect("facts is registered")
            .executions(),
        0
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("geo").expect("a test source"))
            .expect("geo is registered")
            .executions(),
        0
    );
}

#[test]
fn a_federated_lookup_preflight_refusal_means_neither_leg_ever_executes() {
    // **Renamed, and the number this test pins CHANGED with it.** Before the spend ledger, the
    // fact leg's own `dry_run` and `execute` ran as one step, so a fact leg that dry-ran clean
    // executed before the lookup leg's `dry_run` was even asked - a refusal on the lookup side
    // then discarded a fact leg that had ALREADY run. Now every leg is dry-run before either one
    // executes (`docs/adr/0030`'s "all-or-nothing", needed so the two legs' estimates can be
    // summed and charged once before any leg spends anything real) - so a lookup pre-flight
    // refusal is caught before the fact leg's own `execute` is ever reached, and the fact data
    // system is never asked at all.
    let shared = shared();
    let fact = LegPreflightWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        federated_fact_rows(),
        DryRunOutcome::Accepted,
    );
    let lookup = LegPreflightWarehouse::new(
        SourceName::parse("geo").expect("a test source"),
        shared,
        federated_lookup_rows(),
        DryRunOutcome::SourceRefused,
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
    .expect("a pre-flight refusal is a governed answer")
    .into_outcome();
    assert!(matches!(
        outcome,
        ToolOutcome::Refusal { reason: RefusalReason::SourceRefused { ref source } }
            if source.as_str() == "geo"
    ));
    assert_eq!(
        warehouses
            .get(&SourceName::parse("facts").expect("a test source"))
            .expect("facts is registered")
            .executions(),
        0,
        "the fact leg's own dry run succeeded, but its execute must never be reached once the \
         lookup leg's pre-flight refuses"
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("geo").expect("a test source"))
            .expect("geo is registered")
            .executions(),
        0
    );
}

#[test]
fn a_federated_fact_preflight_failure_keeps_its_warehouse_cause_and_no_partial_answer() {
    let shared = shared();
    let fact = LegPreflightWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        federated_fact_rows(),
        DryRunOutcome::TransientFailure,
    );
    let lookup = LegPreflightWarehouse::new(
        SourceName::parse("geo").expect("a test source"),
        shared,
        federated_lookup_rows(),
        DryRunOutcome::Accepted,
    );
    let warehouses = Warehouses::of(fact).and(lookup).expect("two sources, one registry");
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
    .expect_err("a transient pre-flight failure remains a warehouse error");
    assert!(matches!(
        failure,
        super::ServiceError::Warehouse {
            cause: AdapterFailure::Statement {
                cause: super::super::tests_support::DriverFailure
            }
        }
    ));
    assert_eq!(
        warehouses
            .get(&SourceName::parse("facts").expect("a test source"))
            .expect("facts is registered")
            .executions(),
        0
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("geo").expect("a test source"))
            .expect("geo is registered")
            .executions(),
        0,
        "the failed fact leg cannot leave a partial answer or execute the lookup"
    );
}

#[test]
fn a_federated_lookup_preflight_failure_means_the_fact_leg_never_executes() {
    // Renamed for the same reason and with the same pinned number changed as
    // `a_federated_lookup_preflight_refusal_means_neither_leg_ever_executes` above: every leg is
    // now dry-run before either one executes, so a failure discovered while pre-flighting the
    // lookup leg is caught before the fact leg's own `execute` ever runs.
    let shared = shared();
    let fact = LegPreflightWarehouse::new(
        SourceName::parse("facts").expect("a test source"),
        shared.clone(),
        federated_fact_rows(),
        DryRunOutcome::Accepted,
    );
    let lookup = LegPreflightWarehouse::new(
        SourceName::parse("geo").expect("a test source"),
        shared,
        federated_lookup_rows(),
        DryRunOutcome::TransientFailure,
    );
    let warehouses = Warehouses::of(fact).and(lookup).expect("two sources, one registry");
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
    .expect_err("a transient pre-flight failure remains a warehouse error");
    assert!(matches!(
        failure,
        super::ServiceError::Warehouse {
            cause: AdapterFailure::Statement {
                cause: super::super::tests_support::DriverFailure
            }
        }
    ));
    assert_eq!(
        warehouses
            .get(&SourceName::parse("facts").expect("a test source"))
            .expect("facts is registered")
            .executions(),
        0,
        "the fact leg's own dry run succeeded, but its execute must never be reached once the \
         lookup leg's pre-flight fails"
    );
    assert_eq!(
        warehouses
            .get(&SourceName::parse("geo").expect("a test source"))
            .expect("geo is registered")
            .executions(),
        0
    );
}

#[test]
fn a_federated_answer_that_crosses_the_working_set_is_refused_not_error() {
    let fact_source = SourceName::parse("facts").expect("facts");
    let lookup_source = SourceName::parse("geo").expect("geo");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        lookup_source,
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources");

    let plan = federated_plan();
    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        1,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a refusal is an Ok")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResourcesExhausted { .. }
            }
        ),
        "a combined answer over the ceiling is a governance refusal, not {outcome:?}"
    );
}

#[test]
fn a_deterministic_combine_failure_is_a_refusal_not_a_service_error() {
    // D19 + A4: an ambiguous join key used to leave `answer_federated` as an `Err` -
    // `ServiceError::Federated` - and reach a transport as a retryable `503`. The same plan
    // against the same rows fails again, so it is a `ToolOutcome::Refusal` now.
    use sutura_domain::plan::InternalLabel;

    let fact_source = SourceName::parse("facts").expect("facts");
    let lookup_source = SourceName::parse("geo").expect("geo");
    let shared = shared();
    let ambiguous_lookup = RowSet::new(
        vec![InternalLabel::Link.label(), String::from("region")],
        vec![
            vec![Value::Text("c1".into()), Value::Text("north".into())],
            vec![Value::Text("c1".into()), Value::Text("south".into())],
        ],
    )
    .expect("a well-formed test lookup result");
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        lookup_source,
        shared,
        ambiguous_lookup,
    ))
    .expect("two sources");

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
    .expect("a deterministic combine failure is a refusal, not a `ServiceError`")
    .into_outcome();
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::FederatedAnswerNotWellFormed {
                    federated: sutura_domain::plan::FederatedAnswerRefusal::AmbiguousLink
                }
            }
        ),
        "an ambiguous join key is a governance refusal, not {outcome:?}"
    );
}

#[test]
fn an_answer_whose_legs_run_under_two_postures_is_answered_and_records_both() {
    // **`docs/adr/0040`, at the orchestrator: this used to be a `409` above the mint.** Two
    // leg-executing adapters, one `shared-service-user` and one `impersonation-at-source`. BigQuery
    // is the only impersonating adapter, so refusing this shape prevented BigQuery from
    // federating with a shared-posture adapter - which is why the disclosure replaced the refusal.
    //
    // **The reasoning the refusal carried is not softened.** Rows a shared identity was permitted to
    // see, added to rows the asking subject was permitted to see, make a total no identity is
    // entitled to. What answers for it is that a mixed answer does span two authorization domains,
    // an operator declared each one in writing on its own entry before this process started, and the
    // answer names which leg came from which. `executed_as` arrives in the same body as the rows, so
    // it is that boot-time acknowledgement and never this record that is the control.
    //
    // A registration rather than a new fake: `LegsWarehouse::answering` already takes a posture per
    // instance, which is the whole of what a mixed deployment is.
    let facts = SourceName::parse("facts").expect("a test source");
    let geo = SourceName::parse("geo").expect("a test source");
    let postures = [
        (facts.clone(), shared()),
        (geo.clone(), sutura_domain::source::SourcePosture::ImpersonationAtSource),
    ];
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        facts,
        postures[0].1.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        geo,
        postures[1].1.clone(),
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");

    // Per-posture legs: the shared source gets its own witness and the impersonating one gets the
    // asker's own credential, which is what `Presented::agrees_with` accepts for each - asked PER
    // LEG, and cross-posture is what makes it the check that each leg ran as what the record claims.
    let broker = crate::tests_support::AcknowledgingBroker::over(&postures);
    let plan = federated_plan();
    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("a cross-posture answer is an Ok")
    .into_outcome();
    let ToolOutcome::Answer { provenance, .. } = outcome else {
        panic!("two postures in one answer are disclosed per leg, not refused: {outcome:?}");
    };
    assert_eq!(
        provenance
            .executed_as()
            .legs()
            .map(|(source, posture)| (source.as_str(), posture.as_str()))
            .collect::<Vec<(&str, &str)>>(),
        vec![("facts", "shared-service-user"), ("geo", "impersonation-at-source")],
        "the answer names which leg came from which authorization domain, in source order"
    );
}

#[test]
fn two_shared_sources_with_different_acknowledgements_are_still_answered() {
    // **The strand guard, at the orchestrator.** `SourcePosture` derives `PartialEq` and the
    // acknowledgement is resolved per source, so a predicate comparing VALUES would refuse this
    // even though the two witnesses differ. The domain's own cell asserts the
    // same property one layer down; this one asserts that the answer path still answers.
    let acknowledged = |text: &str| sutura_domain::source::SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse(text).expect("a test reason is a reason"),
        ),
    };
    let facts = SourceName::parse("facts").expect("a test source");
    let geo = SourceName::parse("geo").expect("a test source");
    let postures = [
        (facts.clone(), acknowledged("a directory of CSVs this deployment owns")),
        (geo.clone(), acknowledged("a reference dataset every team reads")),
    ];
    let warehouses = Warehouses::of(crate::tests_support::LegsWarehouse::answering(
        facts,
        postures[0].1.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::LegsWarehouse::answering(
        geo,
        postures[1].1.clone(),
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");

    // Per-source witnesses, because `Presented::agrees_with` compares the acknowledgement prose
    // by equality - so a broker minting ONE witness for both sources is refused here by that
    // guard rather than by the one under test, which is exactly the confusion this fake removes.
    let broker = crate::tests_support::AcknowledgingBroker::over(&postures);
    let plan = federated_plan();
    let outcome = answer_federated(
        &bundle(),
        &plan,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("two shared legs are answered")
    .into_outcome();
    let ToolOutcome::Answer { provenance, .. } = outcome else {
        panic!("two shared legs are one posture, so this is answered, not {outcome:?}");
    };
    assert_eq!(
        provenance
            .executed_as()
            .legs()
            .map(|(_, posture)| posture.as_str())
            .collect::<Vec<&str>>(),
        vec!["shared-service-user", "shared-service-user"],
        "both legs record the same posture, with two different acknowledgements behind them"
    );
}

#[test]
fn a_federated_answer_is_refused_when_no_adapter_executes_a_leg() {
    // What `answer` does on a build whose adapter declares `EXECUTES_LEGS = false`: it refuses
    // cleanly BEFORE minting or running a leg, rather than surfacing a typed leg refusal as a
    // retryable 503. `FixedWarehouse` below takes the port's default, which is what puts this
    // test on that branch.
    //
    // **Not the shipped binary any more, and the correction matters here of all places.**
    // `sutura-exec-datafusion` declares the constant and is non-optional in both published
    // binaries, so a release ANSWERS a two-source question - see
    // `crates/sutura-cli/tests/served.rs`. This cell is about the gate, not about the shipped
    // set: what still reaches it is `sutura-exec-bigquery`, any adapter taking the default, and
    // this fake.
    let fact_source = SourceName::parse("facts").expect("facts");
    let lookup_source = SourceName::parse("geo").expect("geo");
    let shared = shared();
    let warehouses = Warehouses::of(crate::tests_support::FixedWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(crate::tests_support::FixedWarehouse::answering(
        lookup_source,
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources");

    let plan = federated_plan();
    let refused = answer_federated(
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
    .expect("a refusal is an Ok")
    .into_outcome();
    assert!(
        matches!(
            refused,
            ToolOutcome::Refusal {
                reason: RefusalReason::FederationNotExecutable
            }
        ),
        "{refused:?}"
    );
}

/// A fake whose `Warehouse::executes_legs` answer is a FIELD rather than the type-level constant -
/// the one shape a heterogeneous registry needs and a `const` cannot give it. Inline here (not
/// `tests_support`, this file's usual fakes module) so `xtask test-causality`'s base
/// reconstruction - which keeps THIS file at HEAD and reverts `tests_support.rs` - does not orphan
/// it.
///
/// **`EXECUTES_LEGS` is `true` here on purpose, and the discriminating case is exactly this
/// choice.** Before `telekom/sutura#112`, `answer_federated` read `W::EXECUTES_LEGS` once for the
/// whole build - so a registry of two instances of THIS type, whichever way their fields were set,
/// would have read `true` and tried to run both legs. The per-leg gate this type exists to prove
/// reads `Warehouse::executes_legs()`, the instance method, instead - so one instance built `false`
/// still refuses the whole answer even though the type it declines as says it can federate.
struct AsymmetricLegWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    can_execute_legs: bool,
}

impl AsymmetricLegWarehouse {
    fn answering(source: SourceName, posture: SourcePosture, result: RowSet, can_execute_legs: bool) -> Self {
        Self {
            source,
            posture,
            result,
            can_execute_legs,
        }
    }
}

impl Warehouse for AsymmetricLegWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn executes_legs(&self) -> bool {
        self.can_execute_legs
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Ok(crate::tests_support::canned(&self.result))
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

#[test]
fn a_federated_answer_is_refused_when_only_one_leg_can_execute() {
    // **`telekom/sutura#112`'s per-leg gate, isolated from the two-KINDS shape it exists for.**
    // `AsymmetricLegWarehouse` is one type, so this registry needs no closed enum at all - and
    // its `EXECUTES_LEGS` is `true`, exactly what a shipped leg-capable adapter declares. Before
    // this gate moved to the instance, `W::EXECUTES_LEGS` read that `true` once for the whole
    // build and both legs would have RUN. What proves the gate actually moved: one instance built
    // with `can_execute_legs: false` still refuses the whole answer, because the check now reads
    // `Warehouse::executes_legs()` per leg rather than the type's constant.
    let fact_source = SourceName::parse("facts").expect("facts");
    let lookup_source = SourceName::parse("geo").expect("geo");
    let shared = shared();
    let warehouses = Warehouses::of(AsymmetricLegWarehouse::answering(
        fact_source,
        shared.clone(),
        federated_fact_rows(),
        true,
    ))
    .and(AsymmetricLegWarehouse::answering(
        lookup_source,
        shared,
        federated_lookup_rows(),
        false,
    ))
    .expect("two sources");

    let plan = federated_plan();
    let refused = answer_federated(
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
    .expect("a refusal is an Ok")
    .into_outcome();
    assert!(
        matches!(
            refused,
            ToolOutcome::Refusal {
                reason: RefusalReason::FederationNotExecutable
            }
        ),
        "one leg cannot run, so the whole answer refuses rather than half-answering: {refused:?}"
    );
}
