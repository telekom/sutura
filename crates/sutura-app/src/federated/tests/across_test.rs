//! The drill-across answer path's own cells: one mint, a pre-flight of every leg and one charge over every leg of
//! every arm, and the refusals that stop it before a leg runs. `use super::*` reaches the fixtures
//! the two-source cells read.

use std::collections::BTreeSet;

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Definitions, Description, Metric, Model};
use sutura_domain::federation::Federation;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, MetricName, ModelName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::{
    Arm, DrillAcrossPlan, InternalLabel, LegPlan, LegTerm, LocalArm, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm,
    ResultLabel, StatementTables, labels,
};

use super::*;
use crate::federated::answer_drill_across;
use crate::tests_support::{CountingBroker, LegsWarehouse, PricedWarehouse, RefusingLegsWarehouse};

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source")
}

/// A bundle defining both metrics the plan measures, `revenue` and `usage`, so an answer over both
/// arms can be certified.
fn both_metrics() -> PinnedDefinitions {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column");
    let model = |name: &str, on: &str, columns: &[&str]| {
        Model::new(
            ModelName::parse(name).expect("a test model"),
            source(on),
            TableName::parse(name).expect("a test table"),
            columns.iter().map(|raw| column(raw)),
            Description::default(),
        )
    };
    let metric = |name: &str, on: &str, measured: &str, time: &str| {
        Metric::new(
            MetricName::parse(name).expect("a test metric"),
            ModelName::parse(on).expect("a test model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column(measured)))),
            Vec::new(),
            column(time),
            BTreeSet::from([Grain::Month]),
            Vec::new(),
            None,
            Description::default(),
            Audience::Open,
        )
        .expect("no dimensions to duplicate")
    };
    let definitions = Definitions::assemble(
        vec![
            model("orders", "facts", &["amount_cents", "order_date"]),
            model("fct_usage", "orders", &["minutes", "month"]),
        ],
        vec![],
        vec![
            metric("revenue", "orders", "amount_cents", "order_date"),
            metric("usage", "fct_usage", "minutes", "month"),
        ],
    )
    .expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(source("local"), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the test definitions hash")
}

/// The metric `federated_plan` answers beside, as one fact leg on `orders` grouped by the same keys.
fn usage_arm() -> Arm {
    let table = TableName::parse("fct_usage").expect("a test table");
    let column = |name: &str| ColumnName::parse(name).expect("a test column");
    let dimension = |name: &str| ResultLabel::dimension(&DimensionName::parse(name).expect("a test dimension"));
    let key = |name: &str| PlanKey::new(dimension(name), PlanColumn::new(table.clone(), column(name)));
    let federation = Federation::of(&Measure::Simple(Term::Aggregate(AggregatedColumn::new(
        Aggregate::Sum,
        column("minutes"),
    ))));
    let terms = labels(&federation)
        .into_iter()
        .map(|label| {
            LegTerm::new(
                PlanTerm::CountIf {
                    column: PlanColumn::new(table.clone(), column("minutes")),
                },
                ResultLabel::internal(label),
            )
        })
        .collect();
    let fact = LegPlan::Fact {
        source: source("orders"),
        metric: MetricName::parse("usage").expect("a test metric"),
        tables: StatementTables::only(table.clone()),
        bucket: PlanBucket::new(
            ResultLabel::bucket(),
            Grain::Month,
            PlanColumn::new(table.clone(), column("month")),
        ),
        keys: vec![key("product_family"), key("region")],
        terms,
        bindings: PlanBindings::none(),
        range: june(),
    };
    let keys = vec![dimension("product_family"), dimension("region")];
    Arm::Local(Box::new(
        LocalArm::new(fact, federation, keys).expect("a test local arm is well formed"),
    ))
}

/// One joined arm over `facts` and `geo`, and one local arm over `orders`: three legs, three sources.
fn across_plan() -> DrillAcrossPlan {
    DrillAcrossPlan::new(Arm::Joined(Box::new(federated_plan())), usage_arm(), Vec::new()).expect("two arms over one key set")
}

fn usage_rows() -> RowSet {
    RowSet::new(
        vec![
            String::from("product_family"),
            String::from("region"),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(0).label(),
        ],
        vec![
            vec![
                Value::Text("A".into()),
                Value::Text("north".into()),
                Value::Text("2026-06".into()),
                Value::Integer(7),
            ],
            vec![
                Value::Text("B".into()),
                Value::Text("north".into()),
                Value::Text("2026-06".into()),
                Value::Integer(3),
            ],
        ],
    )
    .expect("a well-formed test usage result")
}

fn answer<W, B>(plan: &DrillAcrossPlan, broker: &B, warehouses: &Warehouses<W>, ledger: &SpendLedger) -> ToolOutcome
where
    W: Warehouse + Sync,
    W::Error: Send,
    B: sutura_domain::identity::CredentialBroker,
    B::Error: Send,
{
    answer_drill_across(
        (&both_metrics(), PinnedSchemas::default().digest()),
        plan,
        &asked_by_a_person(),
        broker,
        warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        ledger,
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect("a drill-across answer is not an error")
    .into_outcome()
}

#[test]
fn a_drill_across_answer_mints_once_over_every_source_and_joins_the_arms() {
    let shared = shared();
    let warehouses = Warehouses::of(LegsWarehouse::answering(
        source("facts"),
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(LegsWarehouse::answering(
        source("geo"),
        shared.clone(),
        federated_lookup_rows(),
    ))
    .and_then(|registry| registry.and(LegsWarehouse::answering(source("orders"), shared, usage_rows())))
    .expect("three sources, one registry");
    let broker = CountingBroker::default();
    let outcome = answer(&across_plan(), &broker, &warehouses, &SpendLedger::no_budget());
    let ToolOutcome::Answer { provenance, rows } = outcome else {
        panic!("three leg-executing sources answer the question, not {outcome:?}");
    };
    assert_eq!(broker.asked(), 1, "one mint over three sources, not one per arm or per leg");
    let recorded: Vec<&str> = provenance.executed_as().legs().map(|(source, _)| source.as_str()).collect();
    assert_eq!(recorded, ["facts", "geo", "orders"]);
    let certified: Vec<&str> = provenance
        .metric_digests()
        .iter()
        .map(|each| each.metric().as_str())
        .collect();
    assert_eq!(
        certified,
        ["revenue", "usage"],
        "every arm's metric is certified, not the first's"
    );
    assert_eq!(rows.columns().len(), 5, "two keys, the bucket and one measure per arm");
    assert_eq!(rows.rows().len(), 2, "the two groups both arms answer");
}

#[test]
fn a_drill_across_answer_charges_the_summed_estimate_once_before_any_leg_runs() {
    // 400 + 400 + 400 is over the 1000 ceiling; the first arm's 800 is under it. An answer that
    // charged per arm would admit the first arm, and the third leg would then be what is refused.
    let shared = shared();
    let priced = |name: &str, rows: RowSet| PricedWarehouse::pricing(source(name), shared.clone(), rows, Some(400));
    let warehouses = Warehouses::of(priced("facts", federated_fact_rows()))
        .and(priced("geo", federated_lookup_rows()))
        .and_then(|registry| registry.and(priced("orders", usage_rows())))
        .expect("three sources, one registry");
    let ledger = SpendLedger::new(Some(crate::spend::SpendBudget::new(
        1_000,
        std::time::Duration::from_secs(60),
    )));
    let outcome = answer(&across_plan(), &FixedBroker::GrantsShared, &warehouses, &ledger);
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::BudgetExhausted { .. }
            }
        ),
        "the summed estimate (1200) is over the ceiling (1000): {outcome:?}"
    );
    for name in ["facts", "geo", "orders"] {
        let leg = warehouses.get(&source(name)).expect("every source is registered");
        assert_eq!(leg.executions(), 0, "a question refused for spend reaches no leg's `execute`");
    }
    assert_eq!(ledger.spent_bytes_total(), Some(0), "a refused charge leaves nothing spent");
}

#[test]
fn a_drill_across_answer_the_ledger_admits_is_charged_the_sum_once() {
    let shared = shared();
    let priced = |name: &str, rows: RowSet| PricedWarehouse::pricing(source(name), shared.clone(), rows, Some(400));
    let warehouses = Warehouses::of(priced("facts", federated_fact_rows()))
        .and(priced("geo", federated_lookup_rows()))
        .and_then(|registry| registry.and(priced("orders", usage_rows())))
        .expect("three sources, one registry");
    let ledger = SpendLedger::new(Some(crate::spend::SpendBudget::new(
        2_000,
        std::time::Duration::from_secs(60),
    )));
    let outcome = answer(&across_plan(), &FixedBroker::GrantsShared, &warehouses, &ledger);
    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "1200 fits under 2000: {outcome:?}"
    );
    assert_eq!(
        ledger.spent_bytes_total(),
        Some(1_200),
        "the three legs' estimates are charged once, as their sum"
    );
}

#[test]
fn an_adapter_that_runs_no_leg_declines_the_drill_across_answer_before_minting() {
    let shared = shared();
    let fixed = |name: &str, rows: RowSet| crate::tests_support::FixedWarehouse::answering(source(name), shared.clone(), rows);
    let warehouses = Warehouses::of(fixed("facts", federated_fact_rows()))
        .and(fixed("geo", federated_lookup_rows()))
        .and_then(|registry| registry.and(fixed("orders", usage_rows())))
        .expect("three sources, one registry");
    let broker = CountingBroker::default();
    let outcome = answer(&across_plan(), &broker, &warehouses, &SpendLedger::no_budget());
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal {
                reason: RefusalReason::FederationNotExecutable
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(broker.asked(), 0, "nothing is minted for an adapter that cannot run a leg");
}

#[test]
fn a_source_nobody_opened_declines_the_drill_across_answer_before_minting() {
    let shared = shared();
    let warehouses = Warehouses::of(LegsWarehouse::answering(
        source("facts"),
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(LegsWarehouse::answering(source("geo"), shared, federated_lookup_rows()))
    .expect("two sources, one registry");
    let broker = CountingBroker::default();
    let outcome = answer(&across_plan(), &broker, &warehouses, &SpendLedger::no_budget());
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal { reason: RefusalReason::SourceUnavailable { ref source } } if source.as_str() == "orders"
        ),
        "{outcome:?}"
    );
    assert_eq!(broker.asked(), 0, "nothing is minted for a question that cannot run");
}

#[test]
fn a_drill_across_leg_the_source_refuses_is_a_refusal_naming_it_not_a_failure() {
    let shared = shared();
    let refusing = |name: &str| RefusingLegsWarehouse::new(source(name), shared.clone());
    let warehouses = Warehouses::of(refusing("facts"))
        .and(refusing("geo"))
        .and_then(|registry| registry.and(refusing("orders")))
        .expect("three sources, one registry");
    let outcome = answer(
        &across_plan(),
        &FixedBroker::GrantsShared,
        &warehouses,
        &SpendLedger::no_budget(),
    );
    assert!(
        matches!(
            outcome,
            ToolOutcome::Refusal { reason: RefusalReason::SourceRefused { ref source } } if source.as_str() == "facts"
        ),
        "the first leg to run is the first arm's fact, and it is refused: {outcome:?}"
    );
}
