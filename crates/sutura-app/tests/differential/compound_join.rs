//! The compound join, proven against a real engine: a key-only join over-counts a fact whose
//! subscription spans several snapshot months, and the compound join does not.
//!
//! **The "correct" side of this comparison is the served answer, not a second oracle written
//! here.** Every subscription in `examples/single-player` appears in up to six snapshot months
//! (`fct_subscription_monthly.csv`), so a join on `subscription_key` alone matches every month a
//! subscription existed and multiplies each day of usage by that count - the same shape
//! `sutura_domain::warehouse::cardinality`'s own header measures for a duplicated dimension key,
//! produced here by dropping a key TERM rather than by bad data. Two hand-built [`QueryPlan`]s,
//! differing only in the relationship's key list, are executed directly against `DuckDB` over the
//! real corpus; the compound one is asserted equal to what `sutura_app::answer` certifies for the
//! same metric and range, and the key-only one is asserted to answer something else.

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::Presented;
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::{Aggregate, ColumnName, Grain, JoinType, MetricName, RelationshipName, SourceName, TableName};
use sutura_domain::plan::{
    Executable, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanJoin, PlanJoinKey, PlanMeasure, PlanPredicate, PlanTerm,
    PredicateOrigin, QueryPlan, ResultLabel, StatementTables,
};
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::{ParamValue, Value, Warehouse as _};
use sutura_exec_duckdb::DuckDbWarehouse;

use crate::adapters::{ReferenceCatalog, a_caller, data_root, deadline, load, posture, shared_credential, source};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn table(raw: &str) -> TableName {
    TableName::parse(raw).expect("a test table is a table")
}

fn june_2026() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("a calendar month is a range")
}

/// The two range-bound predicates every plan below carries, over `daily_usage`'s own time column.
fn bindings() -> PlanBindings {
    let usage_date = PlanColumn::new(table("fct_usage_daily"), column("usage_date"));
    PlanBindings::parse(
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: usage_date.clone(),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: usage_date,
                    param: 1,
                },
            ),
        ],
        vec![
            ParamValue::Date(Date::parse("2026-06-01").expect("a test date is a date")),
            ParamValue::Date(Date::parse("2026-07-01").expect("a test date is a date")),
        ],
    )
    .expect("two range bounds bind their two params")
}

/// `data_per_subscription`'s own shape - a ratio, no dimension - joined to the snapshot on
/// whichever `keys` the caller hands it. The only thing that differs between the two plans this
/// file compares is that list.
fn plan_with_keys(keys: Vec<PlanJoinKey>) -> QueryPlan {
    let fact = table("fct_usage_daily");
    let metric = MetricName::parse("data_per_subscription").expect("a test metric is a metric");
    let join = PlanJoin::new(
        RelationshipName::parse("daily_usage_subscription").expect("a test relationship is one"),
        table("fct_subscription_monthly"),
        JoinType::ManyToOne,
        keys,
    );
    QueryPlan::new(
        SourceName::parse("local").expect("a test source is a source"),
        metric.clone(),
        StatementTables::parse(fact.clone(), vec![join]).expect("two differently named tables are distinguishable"),
        PlanBucket::new(
            ResultLabel::bucket(),
            Grain::Month,
            PlanColumn::new(fact.clone(), column("usage_date")),
        ),
        Vec::new(),
        PlanMeasure::Ratio {
            numerator: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: PlanColumn::new(fact.clone(), column("data_gb")),
            },
            denominator: PlanTerm::Aggregate {
                aggregate: Aggregate::CountDistinct,
                column: PlanColumn::new(fact, column("subscription_key")),
            },
            zero_denominator: ZeroDenominator::Null,
        },
        ResultLabel::measure(&metric),
        bindings(),
        june_2026(),
    )
}

/// The relationship exactly as `daily_usage_subscription.md` declares it: the subscription key,
/// AND the usage date truncated to the month it names.
fn compound_keys() -> Vec<PlanJoinKey> {
    vec![
        PlanJoinKey::Equal {
            origin: PlanColumn::new(table("fct_usage_daily"), column("subscription_key")),
            target: PlanColumn::new(table("fct_subscription_monthly"), column("subscription_key")),
        },
        PlanJoinKey::TruncatedEqual {
            origin: PlanColumn::new(table("fct_usage_daily"), column("usage_date")),
            grain: Grain::Month,
            target: PlanColumn::new(table("fct_subscription_monthly"), column("month")),
        },
    ]
}

/// The relationship a document could ALSO have declared before `JoinKey` existed - one column
/// pair, no month constraint - which is precisely the shape the compound join was added to close.
fn key_only_keys() -> Vec<PlanJoinKey> {
    vec![PlanJoinKey::Equal {
        origin: PlanColumn::new(table("fct_usage_daily"), column("subscription_key")),
        target: PlanColumn::new(table("fct_subscription_monthly"), column("subscription_key")),
    }]
}

/// The identity every corpus source in this suite presents with. Matches
/// `sutura-exec-duckdb`'s own `shared_leg()` fixture: read off the fixed posture rather than
/// fabricated, so a change to that posture cannot make this pass under one this adapter no longer
/// accepts.
fn presented() -> Presented {
    match posture() {
        SourcePosture::SharedServiceUser { declared } => Presented::SharedServiceUser { declared },
        SourcePosture::ImpersonationAtSource => panic!("the corpus's own posture is shared, not impersonating"),
    }
}

fn engine() -> DuckDbWarehouse {
    let warehouse = DuckDbWarehouse::in_memory(source(), posture()).expect("an in-memory database opens");
    for name in ["fct_usage_daily", "fct_subscription_monthly"] {
        let csv = data_root().join(format!("{name}.csv"));
        warehouse
            .attach_csv(&table(name), &csv)
            .unwrap_or_else(|e| panic!("could not attach {}: {e}", csv.display()));
    }
    warehouse
}

/// The full corpus's own engine - one CSV per model in the bundle, [`crate::adapters`]'s own
/// `fixture_tables` shape - for the served side, which `verify_and_validate` runs its boot check
/// against every relationship in, not only the two this file is about.
fn full_engine(pinned: &sutura_domain::pinned::PinnedDefinitions) -> DuckDbWarehouse {
    let warehouse = DuckDbWarehouse::in_memory(source(), posture()).expect("an in-memory database opens");
    for model in pinned.definitions().models().values() {
        let name = model.table_name();
        let csv = data_root().join(format!("{name}.csv"));
        warehouse
            .attach_csv(name, &csv)
            .unwrap_or_else(|e| panic!("could not attach {}: {e}", csv.display()));
    }
    warehouse
}

/// Runs one hand-built plan directly against the port, bypassing every governance ceremony above
/// it - the same layer `sutura-exec-duckdb`'s own unit tests execute at, and the layer this file's
/// comparison is actually about.
fn ratio_of(warehouse: &DuckDbWarehouse, plan: &QueryPlan) -> f64 {
    let batches = warehouse
        .execute(Executable::Query(plan), &presented(), deadline())
        .unwrap_or_else(|e| panic!("the plan did not execute: {e}"));
    let rows = batches.to_rows().expect("a ratio is a readable cell");
    assert_eq!(rows.rows().len(), 1, "one month of data is one row: {rows:?}");
    let index = rows
        .column_index("data_per_subscription")
        .expect("the measure is projected under the metric's own name");
    match rows.cell(0, index) {
        Some(&Value::Real(value)) => value.get(),
        other => panic!("the ratio came back as {other:?} rather than a real number"),
    }
}

/// The served answer for the same metric and range - `sutura_app::answer` over the REGISTERED
/// catalog, which already declares `daily_usage_subscription` with the compound key. This is the
/// oracle the compound plan below is checked against, so the "correct" number in this file is
/// certified rather than computed twice. Opens its own engine: `Warehouses::of` takes ownership,
/// so this cannot share the caller's.
fn served_ratio() -> f64 {
    let pinned = load::<ReferenceCatalog>();
    let registry = sutura_app::Warehouses::of(full_engine(&pinned));
    let validated = sutura_app::verify_and_validate(pinned, &registry).expect("the anchors hold");
    let query = Query::new(
        MetricName::parse("data_per_subscription").expect("a test metric is a metric"),
        Grain::Month,
        june_2026(),
        Vec::new(),
        Vec::new(),
    );
    let answered = sutura_app::answer(
        &validated,
        &query,
        &a_caller(),
        &shared_credential(),
        &registry,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        1 << 30,
        deadline(),
        &sutura_app::SpendLedger::no_budget(),
        sutura_domain::plan::RowCeiling::DEFAULT,
    )
    .expect("the served plan executes")
    .into_outcome();
    let ToolOutcome::Answer { rows, .. } = answered else {
        panic!("data_per_subscription over June is not refused: {answered:?}");
    };
    let index = rows
        .column_index("data_per_subscription")
        .expect("the measure is projected under the metric's own name");
    match rows.cell(0, index) {
        Some(&Value::Real(value)) => value.get(),
        other => panic!("the served ratio came back as {other:?} rather than a real number"),
    }
}

#[test]
fn a_key_only_join_over_counts_and_the_compound_join_does_not() {
    let warehouse = engine();
    let compound = ratio_of(&warehouse, &plan_with_keys(compound_keys()));
    let key_only = ratio_of(&warehouse, &plan_with_keys(key_only_keys()));
    let served = served_ratio();

    // The compound plan renders and executes the same join `daily_usage_subscription.md` declares,
    // so it must reproduce the number the catalog already certifies.
    assert!(
        (compound - served).abs() < 1e-9,
        "the compound join answered {compound}, and the served catalog certifies {served}"
    );
    // The key-only plan drops the second term and joins on `subscription_key` alone, which matches
    // every snapshot month a subscription held - multiplying the numerator by however many months
    // that is, while `COUNT(DISTINCT subscription_key)` is unaffected. It must NOT reproduce the
    // certified number, and it must be the larger one: over-counting, never under.
    assert!(
        key_only > compound * 1.5,
        "a key-only join was expected to over-count well past the compound join's {compound}, and answered {key_only}"
    );
}
