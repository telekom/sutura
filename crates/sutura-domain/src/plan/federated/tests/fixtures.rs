//! The builders every assertion in this suite is written against: plans, legs, row sets and the
//! parsed names they need.
//!
//! **Holds no `#[test]` and no assertion:** every builder returns a value, and what is true of that
//! value is the parent's to assert. Extracted once rather than per module, because two copies of a
//! fixture are two things to keep in step.

use crate::calendar::{Date, TimeRange};
use crate::federation::Federation;
use crate::measure::{AggregatedColumn, Measure, Term};
use crate::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use crate::plan::leg::{LegPlan, LegTerm};
use crate::plan::{
    AnswerKey, FederatedPlan, FederatedPlanError, InternalLabel, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm,
    ResultLabel, StatementTables, labels,
};

pub(super) const FACT: &str = "fct_subscription_monthly";

pub(super) const FACT_SOURCE: &str = "facts";

pub(super) const REMOTE_SOURCE: &str = "geo";

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

pub(super) fn fact_leg(terms: Vec<LegTerm>) -> LegPlan {
    LegPlan::Fact {
        source: source(FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: vec![key("product_family", FACT), link_key(FACT)],
        terms,
        bindings: PlanBindings::none(),
        range: range(),
    }
}

/// One placeholder [`LegTerm`] per leaf `federation` carries, labelled exactly as
/// [`FederatedPlan::new`]'s own D9 check requires - the same `labels(&federation)` zip the one
/// production splitter runs. The term's computation is never read by anything this suite asserts
/// on; only the label is.
pub(super) fn terms_for(federation: &Federation) -> Vec<LegTerm> {
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

pub(super) fn lookup_leg() -> LegPlan {
    LegPlan::Lookup {
        source: source(REMOTE_SOURCE),
        table: table(FACT).into(),
        keys: vec![link_key(FACT), key("region", FACT)],
        bindings: PlanBindings::none(),
    }
}

pub(super) const SECOND_FACT_SOURCE: &str = "second";

/// A second fact leg over a different source, sharing the link key with the first fact leg so the
/// chasm guard passes. It carries no terms because a simple measure names no second model, so the
/// constructor's term check expects none on it.
pub(super) fn second_fact_leg() -> LegPlan {
    LegPlan::Fact {
        source: source(SECOND_FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table("dim_orders")),
        bucket: bucket(),
        keys: vec![key("product_family", "dim_orders"), link_key("dim_orders")],
        terms: Vec::new(),
        bindings: PlanBindings::none(),
        range: range(),
    }
}

/// A second fact leg over a different source that shares NO key with the first fact leg - the
/// chasm-trap guard: no join is possible.
pub(super) fn second_fact_leg_with_no_shared_key() -> LegPlan {
    LegPlan::Fact {
        source: source(SECOND_FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table("dim_orders")),
        bucket: bucket(),
        keys: vec![key("unrelated", "dim_orders")],
        terms: Vec::new(),
        bindings: PlanBindings::none(),
        range: range(),
    }
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
}
