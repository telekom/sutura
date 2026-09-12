//! The builders every assertion in this suite is written against: plans, legs, row sets and the
//! parsed names they need.
//!
//! **Holds no `#[test]` and no assertion:** every builder returns a value, and what is true of that
//! value is the parent's to assert. Extracted once rather than per module, because two copies of a
//! fixture are two things to keep in step.

use crate::calendar::{Date, TimeRange};
use crate::catalog::TIME_BUCKET_LABEL;
use crate::federation::Federation;
use crate::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
use crate::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use crate::plan::leg::LegPlan;
use crate::plan::{
    AnswerKey, FederatedPlan, FederatedPlanError, InternalLabel, PlanBucket, PlanColumn, PlanKey, ResultLabel, StatementTables,
};
use crate::warehouse::{RowSet, Value};

pub(super) const FACT: &str = "fct_subscription_monthly";

pub(super) const FACT_SOURCE: &str = "facts";

pub(super) const REMOTE_SOURCE: &str = "geo";

/// No budget for the correctness tests: each passes an effectively unbounded ceiling so only the
/// budget test exercises the refusal.
pub(super) const UNBOUNDED: u64 = u64::MAX;

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

pub(super) fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source is a source")
}

pub(super) fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a test table is a table")
}

pub(super) fn range() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-01-01").expect("a test start"),
        Date::parse("2026-02-01").expect("a test end"),
    )
    .expect("a test range")
}

pub(super) fn key(name: &str, table_name: &str) -> PlanKey {
    PlanKey::new(
        ResultLabel::dimension(&dimension(name)),
        PlanColumn::new(table(table_name), column(name)),
    )
}

/// The label the link column carries in either leg's result.
///
/// Taken from the type rather than spelled, because these tests are about what the combine COMPUTES.
/// The spelling itself, and the namespace that makes it uncollidable, is pinned once by
/// [`an_internal_label_is_in_a_namespace_no_question_can_name`].
pub(super) fn link() -> String {
    InternalLabel::Link.label()
}

/// The label the carried leaf at `position` carries in the fact leg's result.
pub(super) fn leaf(position: usize) -> String {
    InternalLabel::Leaf(position).label()
}

/// The link key of a leg: the internal label over the physical join column `customer_key`.
pub(super) fn link_key(table_name: &str) -> PlanKey {
    PlanKey::new(
        ResultLabel::internal(InternalLabel::Link),
        PlanColumn::new(table(table_name), column("customer_key")),
    )
}

pub(super) fn bucket() -> PlanBucket {
    PlanBucket::new(
        ResultLabel::bucket(),
        Grain::Month,
        PlanColumn::new(table(FACT), column("month")),
    )
}

pub(super) fn fact_leg() -> LegPlan {
    LegPlan::Fact {
        source: source(FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: vec![key("product_family", FACT), link_key(FACT)],
        terms: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
        range: range(),
    }
}

pub(super) fn lookup_leg() -> LegPlan {
    LegPlan::Lookup {
        source: source(REMOTE_SOURCE),
        table: table(FACT).into(),
        keys: vec![link_key(FACT), key("region", FACT)],
        filters: Vec::new(),
        params: Vec::new(),
    }
}

pub(super) fn plan_for(measure_name: &str, measure: &Measure, include_unmatched: bool) -> FederatedPlan {
    try_plan_for(measure_name, measure, include_unmatched).expect("a test plan is a valid two-leg plan")
}

pub(super) fn try_plan_for(
    measure_name: &str,
    measure: &Measure,
    include_unmatched: bool,
) -> Result<FederatedPlan, FederatedPlanError> {
    let name = metric(measure_name);
    let measure_label = ResultLabel::measure(&name);
    let federation = Federation::of(measure);
    FederatedPlan::new(
        name,
        measure_label,
        bucket(),
        fact_leg(),
        lookup_leg(),
        include_unmatched,
        federation,
        vec![
            AnswerKey::fact(ResultLabel::dimension(&dimension("product_family"))),
            AnswerKey::lookup(ResultLabel::dimension(&dimension("region"))),
        ],
    )
}

pub(super) fn sum_plan(include_unmatched: bool) -> FederatedPlan {
    plan_for(
        "revenue",
        &Measure::Simple(term(Aggregate::Sum, "mrr_cents")),
        include_unmatched,
    )
}

pub(super) fn avg_plan() -> FederatedPlan {
    plan_for(
        "mean_subscription_mrr",
        &Measure::Simple(term(Aggregate::Avg, "mrr_cents")),
        true,
    )
}

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

pub(super) fn fact(rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(
        vec![
            String::from("product_family"),
            link(),
            String::from(TIME_BUCKET_LABEL),
            leaf(0),
        ],
        rows,
    )
    .expect("a test fact result is well formed")
}

/// One fact row carrying `measure`, in the single group [`one_lookup`] maps `c1` into.
pub(super) fn fact_row(measure: Value) -> Vec<Value> {
    vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        measure,
    ]
}

/// The one lookup row the re-aggregation tests join against, so every fact row shares one group.
pub(super) fn one_lookup() -> RowSet {
    lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]])
}

/// `2^53`: every integer below it is exactly representable as an `f64`.
pub(super) const TWO_POW_53: i64 = 1 << 53;

/// `2^53 + 1`, the first integer an `f64` cannot hold: it rounds to [`TWO_POW_53`], so a comparison
/// taken on the widened values reads the two as equal.
pub(super) const TWO_POW_53_PLUS_ONE: i64 = TWO_POW_53 + 1;

/// A plan whose measure is a re-aggregating minimum or maximum over one column.
pub(super) fn extreme_plan(aggregate: Aggregate, measure_name: &str) -> FederatedPlan {
    plan_for(measure_name, &Measure::Simple(term(aggregate, "mrr_cents")), true)
}

pub(super) fn lookup(rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(vec![link(), String::from("region")], rows).expect("a test lookup result is well formed")
}

pub(super) fn avg_fact(rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(
        vec![
            String::from("product_family"),
            link(),
            String::from(TIME_BUCKET_LABEL),
            leaf(0),
            leaf(1),
        ],
        rows,
    )
    .expect("an average fact result is well formed")
}

/// One fact row: a product family, a link value, and the sum leaf.
pub(super) fn keyed_fact_row(family: &str, link: Value, measure: i64) -> Vec<Value> {
    vec![
        Value::Text(family.into()),
        link,
        Value::Text("2026-06".into()),
        Value::Integer(measure),
    ]
}
