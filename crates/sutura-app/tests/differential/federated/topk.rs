//! Case 1's own differential cell - `github.com/telekom/sutura#777`.
//!
//! **Why this file exists rather than a `DERIVED_QUESTIONS` entry.** Case 1's precondition -
//! every answer key `LegSide::Fact`, and a LEFT join - cannot be reached through the real
//! splitter: federating at all requires a dimension that reaches a second source, and the only
//! two ways to reach one are a remote group-by key (`LegSide::Lookup` by construction) or a
//! remote filter (which forces INNER). `federated.rs`'s own module doc and
//! `corpus::DERIVED_QUESTIONS`'s comment carry the argument. So this builds the shape directly -
//! which `FederatedPlan`'s own constructor allows, since it checks each answer key against its
//! leg rather than requiring a leg's every key to be answered - and proves the mechanism the
//! type permits even though no question produces it today.
//!
//! **The differential, not a snapshot.** The fact leg is executed through TWO real adapters -
//! `DuckDB` (`sutura_sql::generate_leg`'s pushdown) and the in-process engine
//! (`sutura_exec_datafusion::leg`'s own) - and both are compared against each other AND against a
//! hand-computed top three. Five product families carry distinct revenue totals with no ties, so
//! the ranking is unambiguous: `d` (400), `a` (300), `c` (230) survive; `b` (50) and `e` (10) do
//! not.
//!
//! **Its own file with its own test, for `test-causality`'s reason.** A NEW production-adjacent
//! test file with no `#[test]` of its own is dropped by the gate's base reconstruction while the
//! parent that calls into it stays at HEAD, which orphans the coverage rather than measuring it -
//! `two_kinds.rs` beside this file carries the same note.

use std::path::Path;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::federation::Federation;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    AnswerKey, Executable, FactTop, FederatedPlan, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm,
    ResultLabel, StatementTables,
};
use sutura_domain::query::{Top, TopBy, TopDirection, TopN};
use sutura_domain::warehouse::{RowSet, Value, Warehouse};

use crate::adapters::{deadline, posture, presented};

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a test table is a table")
}

fn column(table_name: &str, name: &str) -> PlanColumn {
    PlanColumn::new(table(table_name), ColumnName::parse(name).expect("a test column is a column"))
}

fn metric() -> MetricName {
    MetricName::parse("revenue").expect("a test metric is a metric")
}

/// Writes the fact and lookup CSVs into a per-process directory, and returns both paths.
///
/// The lookup table is never read by the answer at all: no answer key is `LegSide::Lookup`, so
/// `FederatedPlan::combine` never looks a link value up in it - it exists only because
/// `FederatedPlan::new` requires a lookup leg to project the link column. Its rows are therefore
/// arbitrary, and one is enough.
fn write_fixture() -> (std::path::PathBuf, std::path::PathBuf) {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("case-1-pushdown-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("could not create {}: {e}", root.display()));
    let fact = root.join("fact.csv");
    std::fs::write(
        &fact,
        "month,product_family,link,revenue_cents\n\
         2026-06-01,a,1,30000\n\
         2026-06-01,b,2,5000\n\
         2026-06-01,c,3,23000\n\
         2026-06-01,d,4,40000\n\
         2026-06-01,e,5,1000\n",
    )
    .unwrap_or_else(|e| panic!("could not write {}: {e}", fact.display()));
    let lookup = root.join("lookup.csv");
    std::fs::write(&lookup, "link\n1\n").unwrap_or_else(|e| panic!("could not write {}: {e}", lookup.display()));
    (fact, lookup)
}

/// The case-1 plan: `product_family` on the fact leg, ranked and limited to three there; the
/// lookup leg carries only the link, and no answer key reads it.
fn case_1_plan() -> FederatedPlan {
    let bucket = PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("fact", "month"));
    let measure = Measure::Simple(Term::Aggregate(AggregatedColumn::new(
        Aggregate::Sum,
        ColumnName::parse("revenue_cents").expect("a test column is a column"),
    )));
    let federation = Federation::of(&measure);
    let top = Top::new(
        TopN::parse(3).expect("three is a row count"),
        TopBy::Metric,
        TopDirection::Desc,
    );
    let fact = LegPlan::Fact {
        source: SourceName::parse("fact_source").expect("a test source is a source"),
        metric: metric(),
        tables: StatementTables::only(table("fact")),
        bucket: bucket.clone(),
        keys: vec![
            PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse("product_family").expect("a test dimension is a dimension")),
                column("fact", "product_family"),
            ),
            PlanKey::new(
                ResultLabel::internal(sutura_domain::plan::InternalLabel::Link),
                column("fact", "link"),
            ),
        ],
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("fact", "revenue_cents"),
            },
            ResultLabel::internal(sutura_domain::plan::InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::none(),
        range: TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("a test range is a range"),
        top: Some(FactTop::new(top, federation.ranking())),
    };
    let lookup = LegPlan::Lookup {
        source: SourceName::parse("lookup_source").expect("a test source is a source"),
        table: table("lookup").into(),
        keys: vec![PlanKey::new(
            ResultLabel::internal(sutura_domain::plan::InternalLabel::Link),
            column("lookup", "link"),
        )],
        bindings: PlanBindings::none(),
    };
    FederatedPlan::new(
        metric(),
        ResultLabel::measure(&metric()),
        bucket,
        fact,
        lookup,
        true, // LEFT: no remote filter, and case 1's own precondition.
        federation,
        vec![AnswerKey::fact(ResultLabel::dimension(
            &DimensionName::parse("product_family").expect("a test dimension is a dimension"),
        ))],
    )
    .expect("a fact leg, a lookup leg, and one fact-side answer key make a valid federated plan")
}

/// Executes the fact and lookup legs through one adapter, combines them, and re-ranks - the same
/// re-rank `sutura_app::federated::answer_federated` applies, because `FederatedPlan::combine`
/// sorts its own output ascending by key cell UNCONDITIONALLY, which un-ranks the fact leg's own
/// `top.n()` order exactly as it would case 2's join.
fn combined<W>(plan: &FederatedPlan, fact_warehouse: &W, lookup_warehouse: &W) -> RowSet
where
    W: Warehouse,
    W::Error: core::fmt::Debug,
{
    let fact = fact_warehouse
        .execute(Executable::Leg(plan.fact()), &presented(), deadline())
        .expect("the fact leg executes");
    let lookup = lookup_warehouse
        .execute(Executable::Leg(plan.lookup()), &presented(), deadline())
        .expect("the lookup leg executes");
    let joined = plan
        .combine(&fact, &lookup, 1 << 30)
        .expect("a case-1 combine over three rows does not exhaust anything");
    let top = plan
        .fact()
        .fact_top()
        .map(sutura_domain::plan::FactTop::top)
        .expect("this fixture is case 1, so the fact leg carries the top");
    FederatedPlan::rank(&joined, top).expect("re-ranking three already-correct rows cannot become malformed")
}

/// The hand-computed answer, in the order case 1's own `top` asks for: descending revenue, three
/// rows. `d` (40000), `a` (30000), `c` (23000); `b` (5000) and `e` (1000) are excluded.
fn expected_top_three() -> Vec<(&'static str, i64)> {
    vec![("d", 40_000), ("a", 30_000), ("c", 23_000)]
}

fn assert_matches_expected(rows: &RowSet, adapter: &str) {
    let family_at = rows.column_index("product_family").expect("product_family is projected");
    let measure_at = rows
        .column_index(ResultLabel::measure(&metric()).as_str())
        .expect("the measure is projected");
    let got: Vec<(String, i64)> = rows
        .rows()
        .iter()
        .map(|row| {
            let family = match row.get(family_at) {
                Some(Value::Text(text)) => text.clone(),
                other => panic!("{adapter}: product_family cell is not text: {other:?}"),
            };
            let revenue = match row.get(measure_at) {
                Some(Value::Integer(n)) => *n,
                other => panic!("{adapter}: the measure cell is not an integer: {other:?}"),
            };
            (family, revenue)
        })
        .collect();
    let expected: Vec<(String, i64)> = expected_top_three().into_iter().map(|(f, r)| (String::from(f), r)).collect();
    assert_eq!(got, expected, "{adapter} did not push the top-3 down exactly");
}

/// Case 1's own differential: `DuckDB`'s rendered pushdown and the engine's logical-plan pushdown
/// answer the SAME top three, in the SAME order, and it is the hand-computed one - not a snapshot
/// of whatever either adapter happens to return.
#[test]
fn case_1_pushdown_is_exact_for_a_hand_built_plan() {
    let (fact_csv, lookup_csv) = write_fixture();
    let plan = case_1_plan();

    let duckdb_fact = sutura_exec_duckdb::DuckDbWarehouse::in_memory(plan.fact().source().clone(), posture())
        .expect("an in-memory database opens");
    duckdb_fact
        .attach_csv(&table("fact"), &fact_csv)
        .expect("the fact csv attaches to duckdb");
    let duckdb_lookup = sutura_exec_duckdb::DuckDbWarehouse::in_memory(plan.lookup().source().clone(), posture())
        .expect("an in-memory database opens");
    duckdb_lookup
        .attach_csv(&table("lookup"), &lookup_csv)
        .expect("the lookup csv attaches to duckdb");
    let from_duckdb = combined(&plan, &duckdb_fact, &duckdb_lookup);
    assert_matches_expected(&from_duckdb, "duckdb");

    let ceiling = core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive");
    let engine_fact = sutura_exec_datafusion::DataFusionWarehouse::new(
        plan.fact().source().clone(),
        posture(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("an in-process engine starts");
    engine_fact
        .attach_csv(&table("fact"), &fact_csv)
        .expect("the fact csv attaches to the engine");
    let engine_lookup = sutura_exec_datafusion::DataFusionWarehouse::new(
        plan.lookup().source().clone(),
        posture(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("an in-process engine starts");
    engine_lookup
        .attach_csv(&table("lookup"), &lookup_csv)
        .expect("the lookup csv attaches to the engine");
    let from_engine = combined(&plan, &engine_fact, &engine_lookup);
    assert_matches_expected(&from_engine, "the in-process engine");

    sutura_domain::warehouse::agreement::agree_on_content(
        &from_duckdb,
        &from_engine,
        sutura_domain::warehouse::agreement::RealTolerance::DIFFERENTIAL,
    )
    .expect("duckdb and the engine must agree on the pushed-down top three");
}
