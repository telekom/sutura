//! The builders every combine assertion is written against: plans, legs and leg results.
//!
//! **Holds no `#[test]` and no assertion**, so what is true of a value is the parent's to say. It is
//! the adapter's own set rather than the domain's, because the domain's fixtures are `#[cfg(test)]`
//! in that crate and a suite about THIS engine's plan belongs beside it.

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, RecordBatch};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_domain::federation::Federation;
use sutura_domain::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::leg::{LegPlan, LegTerm};
use sutura_domain::plan::{
    AnswerKey, FederatedPlan, InternalLabel, LegResult, Legs, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm,
    ResultLabel, StatementTables, labels,
};
use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::{Accumulating, ResultBatches, RowSet, Value};

use crate::combine::{CombineError, DataFusionCombiner};

/// A ceiling wide enough that only the ceiling cell exercises the refusal.
pub(super) const UNBOUNDED: u64 = 1 << 30;

pub(super) const FACT: &str = "fct_subscription_monthly";

pub(super) fn combiner() -> DataFusionCombiner {
    DataFusionCombiner::new().expect("a combiner builds")
}

pub(super) fn metric(name: &str) -> MetricName {
    MetricName::parse(name).expect("a test metric is a metric")
}

pub(super) fn column(name: &str) -> ColumnName {
    ColumnName::parse(name).expect("a test column is a column")
}

pub(super) fn dimension(name: &str) -> DimensionName {
    DimensionName::parse(name).expect("a test dimension is a dimension")
}

pub(super) fn term(aggregate: Aggregate, name: &str) -> Term {
    Term::Aggregate(AggregatedColumn::new(aggregate, column(name)))
}

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source is a source")
}

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a test table is a table")
}

fn range() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-01-01").expect("a test start"),
        Date::parse("2026-02-01").expect("a test end"),
    )
    .expect("a test range")
}

fn key(name: &str) -> PlanKey {
    PlanKey::new(
        ResultLabel::dimension(&dimension(name)),
        PlanColumn::new(table(FACT), column(name)),
    )
}

/// The label the link column carries in either leg's result, from the type rather than spelled.
pub(super) fn link() -> String {
    InternalLabel::Link.label()
}

/// The label the carried leaf at `position` carries in the fact leg's result.
pub(super) fn leaf(position: usize) -> String {
    InternalLabel::Leaf(position).label()
}

fn link_key() -> PlanKey {
    PlanKey::new(
        ResultLabel::internal(InternalLabel::Link),
        PlanColumn::new(table(FACT), column("customer_key")),
    )
}

fn bucket() -> PlanBucket {
    PlanBucket::new(
        ResultLabel::bucket(),
        Grain::Month,
        PlanColumn::new(table(FACT), column("month")),
    )
}

/// One placeholder [`LegTerm`] per leaf, labelled exactly as [`FederatedPlan::new`]'s own check
/// requires - the same `labels(&federation)` zip the one production splitter runs. Nothing here
/// reads the term's computation; only the label.
fn terms_for(federation: &Federation) -> Vec<LegTerm> {
    labels(federation)
        .into_iter()
        .map(|label| {
            LegTerm::new(
                PlanTerm::CountIf {
                    column: PlanColumn::new(table(FACT), column("mrr_cents")),
                },
                ResultLabel::internal(label),
            )
        })
        .collect()
}

fn fact_leg(terms: Vec<LegTerm>) -> LegPlan {
    LegPlan::Fact {
        source: source("facts"),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: vec![key("product_family"), link_key()],
        terms,
        bindings: PlanBindings::none(),
        range: range(),
    }
}

fn lookup_leg() -> LegPlan {
    LegPlan::Lookup {
        source: source("geo"),
        table: table(FACT).into(),
        keys: vec![link_key(), key("region")],
        bindings: PlanBindings::none(),
    }
}

pub(super) fn plan_for(measure_name: &str, measure: &Measure, include_unmatched: bool) -> FederatedPlan {
    let name = metric(measure_name);
    let measure_label = ResultLabel::measure(&name);
    let federation = Federation::of(measure);
    FederatedPlan::new(
        name,
        measure_label,
        bucket(),
        fact_leg(terms_for(&federation)),
        None,
        lookup_leg(),
        include_unmatched,
        federation,
        vec![
            AnswerKey::fact(ResultLabel::dimension(&dimension("product_family"))),
            AnswerKey::lookup(ResultLabel::dimension(&dimension("region"))),
        ],
    )
    .expect("a test plan is a valid two-leg plan")
}

pub(super) fn sum_plan(include_unmatched: bool) -> FederatedPlan {
    plan_for(
        "revenue",
        &Measure::Simple(term(Aggregate::Sum, "mrr_cents")),
        include_unmatched,
    )
}

/// A decomposed average: the leg carries a sum and a count, and the division waits for the combine.
pub(super) fn avg_plan() -> FederatedPlan {
    plan_for(
        "mean_subscription_mrr",
        &Measure::Simple(term(Aggregate::Avg, "mrr_cents")),
        true,
    )
}

/// A ratio whose definition declared `fails` for a zero denominator.
pub(super) fn failing_ratio_plan() -> FederatedPlan {
    plan_for(
        "mean_subscription_mrr",
        &Measure::Ratio {
            numerator: term(Aggregate::Sum, "mrr_cents"),
            denominator: term(Aggregate::Count, "mrr_cents"),
            zero_denominator: ZeroDenominator::Fail,
        },
        true,
    )
}

pub(super) fn extreme_plan(aggregate: Aggregate, measure_name: &str) -> FederatedPlan {
    plan_for(measure_name, &Measure::Simple(term(aggregate, "mrr_cents")), true)
}

/// A fact leg's result: a product family, a link value, the month bucket and one leaf.
pub(super) fn fact(rows: Vec<Vec<Value>>) -> ResultBatches {
    batches_of(
        vec![
            String::from("product_family"),
            link(),
            String::from(TIME_BUCKET_LABEL),
            leaf(0),
        ],
        rows,
    )
}

/// The same with two leaves, for a decomposed measure.
pub(super) fn two_leaf_fact(rows: Vec<Vec<Value>>) -> ResultBatches {
    batches_of(
        vec![
            String::from("product_family"),
            link(),
            String::from(TIME_BUCKET_LABEL),
            leaf(0),
            leaf(1),
        ],
        rows,
    )
}

/// A lookup leg's result: the link value and the remote key.
pub(super) fn lookup(rows: Vec<Vec<Value>>) -> ResultBatches {
    batches_of(vec![link(), String::from("region")], rows)
}

/// One lookup row, so every fact row in a re-aggregation cell shares one group.
pub(super) fn one_lookup() -> ResultBatches {
    lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]])
}

/// One fact row: a family, a link value, the month and the leaf.
pub(super) fn fact_row(family: &str, link_value: Value, measure: Value) -> Vec<Value> {
    vec![Value::Text(family.into()), link_value, Value::Text("2026-06".into()), measure]
}

/// Domain rows as the port's currency, through the one builder the row-speaking adapters use.
pub(super) fn batches_of(columns: Vec<String>, rows: Vec<Vec<Value>>) -> ResultBatches {
    of_row_set(&RowSet::new(columns, rows).expect("a test leg result is well formed")).expect("a leg result converts")
}

/// A leg result built from Arrow arrays directly, for the shapes domain rows cannot express.
///
/// **The type of an EMPTY column is the case this exists for.** `arrow::of_row_set` infers a
/// column's type from its cells, so every column of a result with no rows reads as `Int64` - which
/// makes an empty leg's declared link type unstatable through that builder. A schema is what a real
/// adapter hands over, empty or not.
pub(super) fn typed(fields: Vec<(&str, DataType)>, columns: Vec<ArrayRef>, rows: usize) -> ResultBatches {
    let schema: SchemaRef = Arc::new(Schema::new(
        fields
            .into_iter()
            .map(|(name, kind)| Field::new(name, kind, true))
            .collect::<Vec<Field>>(),
    ));
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), rows, roomy());
    if rows > 0 {
        let batch = RecordBatch::try_new(schema, columns).expect("a test batch is well formed");
        accumulating.push(batch).expect("a test batch agrees with its own schema");
    }
    accumulating.finish()
}

/// One combine, as the rows a caller would read.
pub(super) fn combined(plan: &FederatedPlan, fact: &ResultBatches, lookup: &ResultBatches, ceiling: u64) -> RowSet {
    try_combine(plan, fact, lookup, ceiling)
        .expect("this combine answers")
        .to_rows()
        .expect("a combined answer decodes")
}

/// One combine, un-judged, so a cell can assert on the refusal.
///
/// The pair is built OUTSIDE the `Result`-returning body, because `clippy::unwrap_in_result` is
/// denied and `Legs::of` cannot fail here: the sides come off `plan.fact()` and `plan.lookup()`,
/// which are the two variants by construction.
pub(super) fn try_combine(
    plan: &FederatedPlan,
    fact: &ResultBatches,
    lookup: &ResultBatches,
    ceiling: u64,
) -> Result<ResultBatches, CombineError> {
    use sutura_domain::plan::FederationCombiner as _;

    let fact = LegResult::of(plan.fact(), fact.clone());
    let lookup = LegResult::of(plan.lookup(), lookup.clone());
    combiner().combine(plan, pair(&fact, &lookup), ceiling)
}

/// The two leg results as a pair, which cannot fail for a pair taken off one plan's own legs.
pub(super) fn pair<'a>(fact: &'a LegResult, lookup: &'a LegResult) -> Legs<'a> {
    Legs::of(fact, lookup).expect("one of each is a pair")
}

/// The refusal a combine carries, or a panic naming what it answered instead.
pub(super) fn refusal(
    plan: &FederatedPlan,
    fact: &ResultBatches,
    lookup: &ResultBatches,
) -> sutura_domain::plan::FederatedAnswerRefusal {
    use sutura_domain::plan::FederationCombiner as _;

    let error = try_combine(plan, fact, lookup, UNBOUNDED).expect_err("this combine is refused");
    combiner()
        .answer_not_well_formed(&error)
        .unwrap_or_else(|| panic!("expected a caller-facing refusal, got {error:?}"))
}

/// A materialisation budget no fixture in this file comes near.
///
/// The bound under test here is never the byte budget - `sutura_domain::warehouse::arrow`'s own
/// cells own that - so a fixture that refused for crossing it would be testing its own size.
const fn roomy() -> sutura_domain::warehouse::ResultBudget {
    sutura_domain::warehouse::ResultBudget::of_bytes(core::num::NonZeroUsize::MAX)
}
