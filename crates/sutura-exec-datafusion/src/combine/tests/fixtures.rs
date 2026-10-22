//! The builders every combine assertion is written against: plans, legs and leg results.
//!
//! **Holds no `#[test]` and no assertion**, so what is true of a value is the parent's to say. It is
//! the adapter's own set rather than the domain's, because the domain's fixtures are `#[cfg(test)]`
//! in that crate and a suite about THIS engine's plan belongs beside it.

use std::sync::Arc;

use datafusion::arrow::array::{Array as _, ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_domain::federation::Federation;
use sutura_domain::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::plan::leg::{LegPlan, LegTerm};
use sutura_domain::plan::{
    AnswerKey, FederatedPlan, InternalLabel, LegResult, Legs, LookupLeg, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm,
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
    InternalLabel::Link(0).label()
}

/// The label the carried leaf at `position` carries in the fact leg's result.
pub(super) fn leaf(position: usize) -> String {
    InternalLabel::Leaf(position).label()
}

fn link_key() -> PlanKey {
    PlanKey::new(
        ResultLabel::internal(InternalLabel::Link(0)),
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

/// One placeholder [`LegTerm`] per leaf the leg aggregates, labelled exactly as
/// [`FederatedPlan::new`]'s own check requires - the same `labels(&federation)` zip the one
/// production splitter runs. Nothing here reads the term's computation; only the label.
fn terms_for(federation: &Federation) -> Vec<LegTerm> {
    labels(federation)
        .into_iter()
        .zip(federation.carried())
        .filter(|&(_, leaf)| !leaf.is_pulled_up())
        .map(|(label, _)| {
            LegTerm::new(
                PlanTerm::CountIf {
                    column: PlanColumn::new(table(FACT), column("mrr_cents")),
                },
                ResultLabel::internal(label),
            )
        })
        .collect()
}

/// The fact leg's keys for every pulled-up leaf, each under its own label: what the splitter adds
/// beside the link for a distinct count.
fn pulled_keys(federation: &Federation) -> Vec<PlanKey> {
    labels(federation)
        .into_iter()
        .zip(federation.carried())
        .filter(|&(_, leaf)| leaf.is_pulled_up())
        .map(|(label, leaf)| {
            PlanKey::new(
                ResultLabel::internal(label),
                PlanColumn::new(table(FACT), leaf.column().clone()),
            )
        })
        .collect()
}

fn fact_leg(federation: &Federation) -> LegPlan {
    let mut keys = vec![key("product_family"), link_key()];
    keys.extend(pulled_keys(federation));
    LegPlan::Fact {
        source: source("facts"),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys,
        terms: terms_for(federation),
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
        fact_leg(&federation),
        None,
        sutura_domain::plan::LookupLegs::one(lookup_leg(), include_unmatched),
        federation,
        vec![
            AnswerKey::fact(ResultLabel::dimension(&dimension("product_family"))),
            AnswerKey::lookup(0, ResultLabel::dimension(&dimension("region"))),
        ],
    )
    .expect("a test plan is a valid two-leg plan")
}

/// A key under `label` over `name`, for a fan-in leg.
fn keyed(label: ResultLabel, name: &str) -> PlanKey {
    PlanKey::new(label, PlanColumn::new(table(FACT), column(name)))
}

fn linked(leg: usize, name: &str) -> PlanKey {
    keyed(ResultLabel::internal(InternalLabel::Link(leg)), name)
}

/// Three lookup legs: `geo` onto the fact, `crm` onto `geo` (INNER), `ops` onto the fact (LEFT).
///
/// The answer groups by `region` from `geo`, `area` from `crm` and `tier` from `ops`. `crm` joins
/// on `region_code`, which only `geo` projects, so a combine that joined it onto the fact would
/// find no such column there.
pub(super) fn fan_in_plan() -> FederatedPlan {
    let federation = Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents")));
    let fact = LegPlan::Fact {
        source: source("facts"),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: vec![linked(0, "customer_key"), linked(2, "customer_key")],
        terms: terms_for(&federation),
        bindings: PlanBindings::none(),
        range: range(),
    };
    let lookup = |on: &str, keys: Vec<PlanKey>| LegPlan::Lookup {
        source: source(on),
        table: table(FACT).into(),
        keys,
        bindings: PlanBindings::none(),
    };
    let label = |name: &str| ResultLabel::dimension(&dimension(name));
    let lookups = sutura_domain::plan::LookupLegs::parse(vec![
        LookupLeg::onto_fact(
            lookup(
                "geo",
                vec![
                    linked(0, "customer_key"),
                    linked(1, "region_code"),
                    keyed(label("region"), "region"),
                ],
            ),
            true,
        ),
        LookupLeg::onto_lookup(
            lookup("crm", vec![linked(1, "region_code"), keyed(label("area"), "area")]),
            0,
            false,
        ),
        LookupLeg::onto_fact(
            lookup("ops", vec![linked(2, "customer_key"), keyed(label("tier"), "tier")]),
            true,
        ),
    ])
    .expect("three legs, one onto an earlier one, are a tree");
    FederatedPlan::new(
        metric("revenue"),
        ResultLabel::measure(&metric("revenue")),
        bucket(),
        fact,
        None,
        lookups,
        federation,
        vec![
            AnswerKey::lookup(0, label("region")),
            AnswerKey::lookup(1, label("area")),
            AnswerKey::lookup(2, label("tier")),
        ],
    )
    .expect("a fan-in plan is a plan")
}

pub(super) fn sum_plan(include_unmatched: bool) -> FederatedPlan {
    plan_for(
        "revenue",
        &Measure::Simple(term(Aggregate::Sum, "mrr_cents")),
        include_unmatched,
    )
}

/// Distinct products, a distinct count over one column: the leg carries the column as a key.
pub(super) fn distinct_plan() -> FederatedPlan {
    plan_for(
        "products_in_use",
        &Measure::Simple(term(Aggregate::CountDistinct, "product_key")),
        true,
    )
}

/// Revenue per distinct product: a sum the leg aggregates over a distinct count it only keys.
pub(super) fn revenue_per_product_plan() -> FederatedPlan {
    plan_for(
        "revenue_per_product",
        &Measure::Ratio {
            numerator: term(Aggregate::Sum, "mrr_cents"),
            denominator: term(Aggregate::CountDistinct, "product_key"),
            zero_denominator: ZeroDenominator::Null,
        },
        true,
    )
}

/// A ratio of two distinct counts over different columns: both are pulled-up keys of the fact leg,
/// so the combine puts both in its one aggregate.
pub(super) fn distinct_ratio_plan(numerator: &str, denominator: &str) -> FederatedPlan {
    plan_for(
        "distinct_ratio",
        &Measure::Ratio {
            numerator: term(Aggregate::CountDistinct, numerator),
            denominator: term(Aggregate::CountDistinct, denominator),
            zero_denominator: ZeroDenominator::Null,
        },
        true,
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

/// A cross-model ratio's plan: the fact leg's revenue over a second fact leg's count, and one lookup
/// leg for the region. Both fact legs group by the link alone, the one key a second fact admits.
pub(super) fn two_fact_plan() -> FederatedPlan {
    let measure = Measure::Ratio {
        numerator: term(Aggregate::Sum, "mrr_cents"),
        denominator: Term::Aggregate(AggregatedColumn::on_model(
            Aggregate::Count,
            column("customer_key"),
            ModelName::parse("visits").expect("a test model is a model"),
        )),
        zero_denominator: ZeroDenominator::Null,
    };
    let name = metric("revenue_per_visit");
    let federation = Federation::of(&measure);
    let fact_leg = |second: bool| LegPlan::Fact {
        source: source(if second { "visits" } else { "facts" }),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: vec![link_key()],
        terms: labels(&federation)
            .into_iter()
            .zip(federation.carried())
            .filter(|&(_, leaf)| leaf.model().is_some() == second)
            .map(|(label, _)| {
                LegTerm::new(
                    PlanTerm::CountIf {
                        column: PlanColumn::new(table(FACT), column("mrr_cents")),
                    },
                    ResultLabel::internal(label),
                )
            })
            .collect(),
        bindings: PlanBindings::none(),
        range: range(),
    };
    FederatedPlan::new(
        name.clone(),
        ResultLabel::measure(&name),
        bucket(),
        fact_leg(false),
        Some(fact_leg(true)),
        sutura_domain::plan::LookupLegs::one(lookup_leg(), true),
        federation,
        vec![AnswerKey::lookup(0, ResultLabel::dimension(&dimension("region")))],
    )
    .expect("a test plan is a valid two-fact plan")
}

/// A fact leg's result with one row per link value: `A`, the link, the month and 100.
pub(super) fn fact_linked(kind: DataType, links: ArrayRef) -> ResultBatches {
    let rows = links.len();
    typed(
        vec![
            ("product_family", DataType::Utf8),
            (link().as_str(), kind),
            (TIME_BUCKET_LABEL, DataType::Utf8),
            (leaf(0).as_str(), DataType::Int64),
        ],
        vec![
            Arc::new(StringArray::from(vec!["A"; rows])),
            links,
            Arc::new(StringArray::from(vec!["2026-06"; rows])),
            Arc::new(Int64Array::from(vec![100_i64; rows])),
        ],
        rows,
    )
}

/// A lookup leg's result with one row per link value, all in the `north`.
pub(super) fn lookup_linked(kind: DataType, links: ArrayRef) -> ResultBatches {
    let rows = links.len();
    typed(
        vec![(link().as_str(), kind), ("region", DataType::Utf8)],
        vec![links, Arc::new(StringArray::from(vec!["north"; rows]))],
        rows,
    )
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

/// A link value and the float it carries, `None` for a NULL.
type FloatRow<'a> = (&'a str, Option<f64>);

/// A fact leg's result whose one leaf is a `Float64` key column, one row per `(link, value)`.
///
/// Built from Arrow arrays because a domain `Real` is finite by construction, so neither a NaN nor
/// two NaNs of different bit patterns can be stated through [`fact`].
pub(super) fn float_keyed_fact(rows: &[FloatRow<'_>]) -> ResultBatches {
    let count = rows.len();
    let links: Vec<&str> = rows.iter().map(|&(link, _)| link).collect();
    let values: Vec<Option<f64>> = rows.iter().map(|&(_, value)| value).collect();
    let link = link();
    let leaf = leaf(0);
    typed(
        vec![
            ("product_family", DataType::Utf8),
            (link.as_str(), DataType::Utf8),
            (TIME_BUCKET_LABEL, DataType::Utf8),
            (leaf.as_str(), DataType::Float64),
        ],
        vec![
            Arc::new(StringArray::from(vec!["A"; count])),
            Arc::new(StringArray::from(links)),
            Arc::new(StringArray::from(vec!["2026-06"; count])),
            Arc::new(Float64Array::from(values)),
        ],
        count,
    )
}

/// A fact leg with two float key leaves, one row under `c1` and one under `c2`.
pub(super) fn float_pair_fact(first: [f64; 2], second: [f64; 2]) -> ResultBatches {
    let link = link();
    let (first_leaf, second_leaf) = (leaf(0), leaf(1));
    typed(
        vec![
            ("product_family", DataType::Utf8),
            (link.as_str(), DataType::Utf8),
            (TIME_BUCKET_LABEL, DataType::Utf8),
            (first_leaf.as_str(), DataType::Float64),
            (second_leaf.as_str(), DataType::Float64),
        ],
        vec![
            Arc::new(StringArray::from(vec!["A"; 2])),
            Arc::new(StringArray::from(vec!["c1", "c2"])),
            Arc::new(StringArray::from(vec!["2026-06"; 2])),
            Arc::new(Float64Array::from(first.to_vec())),
            Arc::new(Float64Array::from(second.to_vec())),
        ],
        2,
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

/// One combine of a fact and one lookup result, un-judged, so a cell can assert on the refusal.
pub(super) fn try_combine(
    plan: &FederatedPlan,
    fact: &ResultBatches,
    lookup: &ResultBatches,
    ceiling: u64,
) -> Result<ResultBatches, CombineError> {
    try_combine_every(plan, &[fact.clone(), lookup.clone()], ceiling)
}

/// One combine of one result per [`FederatedPlan::legs`] entry, in that order, un-judged.
///
/// The legs are assigned OUTSIDE the `Result`-returning body, because `clippy::unwrap_in_result`
/// is denied and `Legs::assign` cannot fail here: each result is tagged off the plan's own leg.
pub(super) fn try_combine_every(
    plan: &FederatedPlan,
    every: &[ResultBatches],
    ceiling: u64,
) -> Result<ResultBatches, CombineError> {
    use sutura_domain::plan::FederationCombiner as _;

    let results: Vec<LegResult> = plan
        .legs()
        .into_iter()
        .zip(every)
        .map(|(leg, batches)| LegResult::of(leg, batches.clone()))
        .collect();
    combiner().combine(plan, assigned(plan, &results), ceiling)
}

/// The results assigned to the plan's legs, which cannot fail for results taken off them in order.
pub(super) fn assigned<'a>(plan: &'a FederatedPlan, results: &'a [LegResult]) -> Legs<'a> {
    Legs::assign(plan, results).expect("one result per leg of the plan, in its order")
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
