//! The federated answer orchestration: the two-source answer path exercised above fake leg-executing
//! adapters, in the crate that owns `answer`.
//!
//! Split out of `tests.rs` for the repository's `max-lines` cap. The fakes here - `LegsWarehouse`,
//! which declares `EXECUTES_LEGS`, and the shared posture they are opened with - sit between the
//! orchestrator and the domain's own combiner suite: what these pin is that `answer_federated` mints
//! once, runs both legs, records both identities, and refuses the working-set ceiling, while the
//! arithmetic of combining is proven in `sutura_domain::plan::federated`.

use crate::Warehouses;
use crate::federated::answer_federated;
use crate::tests::{asked_by_a_person, bundle, june, metric, shared};
use crate::tests_support::FixedBroker;
use sutura_domain::model::{Grain, SourceName};
use sutura_domain::query::{RefusalReason, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Value};

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
    use sutura_domain::catalog::TIME_BUCKET_LABEL;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::Aggregate;
    use sutura_domain::model::{ColumnName, TableName};
    use sutura_domain::plan::{AnswerKey, LegPlan, PlanBucket, PlanColumn, PlanKey, StatementTables};

    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let table = TableName::parse("fct_subscription_monthly").expect("a test table");
    let column = |n: &str| ColumnName::parse(n).expect("a test column");
    let tablecol = |n: &str| PlanColumn::new(table.clone(), column(n));
    let key = |n: &str| PlanKey::new(String::from(n), tablecol(n));
    let bucket = |c: &str| {
        PlanBucket::new(
            String::from(TIME_BUCKET_LABEL),
            Grain::Month,
            PlanColumn::new(table.clone(), column(c)),
        )
    };

    let fact = LegPlan::Fact {
        source: fact_source,
        metric: metric(),
        tables: StatementTables::only(table.clone()),
        bucket: bucket("month"),
        keys: vec![key("product_family"), key("customer_key")],
        terms: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
        range: june(),
    };
    let lookup = LegPlan::Lookup {
        source: lookup_source,
        table: table.clone().into(),
        keys: vec![key("customer_key"), key("region")],
        filters: Vec::new(),
        params: Vec::new(),
    };
    let sum = Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))));
    sutura_domain::plan::FederatedPlan::new(
        metric(),
        String::from("revenue"),
        bucket("month"),
        fact,
        lookup,
        String::from("customer_key"),
        String::from("customer_key"),
        true,
        sutura_domain::federation::Federation::of(&sum),
        vec![
            AnswerKey::fact(String::from("product_family")),
            AnswerKey::lookup(String::from("region")),
        ],
    )
    .expect("a valid two-leg plan")
}

fn federated_fact_rows() -> RowSet {
    RowSet::new(
        vec![
            String::from("product_family"),
            String::from("customer_key"),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            String::from("revenue"),
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
    RowSet::new(
        vec![String::from("customer_key"), String::from("region")],
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
    let outcome = answer_federated(&bundle(), &plan, &asked_by_a_person(), &broker, &warehouses, FEDERATED_BUDGET)
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
        1,
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
fn a_federated_answer_is_refused_when_no_adapter_executes_a_leg() {
    // The shipped binary's adapters declare `EXECUTES_LEGS = false`, and this is what `answer` does
    // on that build: it refuses cleanly BEFORE minting or running a leg, rather than surfacing a
    // typed leg refusal as a retryable 503.
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
        FEDERATED_BUDGET,
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
