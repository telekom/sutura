//! Shared test fixtures for the engine, in one place.
//!
//! `question()` - the revenue-by-region plan every execution test here drives - was duplicated
//! verbatim in `width_tests.rs` and `pool/ceiling_tests.rs` for so long that the two copies drifted
//! apart into a byte-identical clone and tripped the copy/paste gate. A fixture that must be kept
//! in step across two files is a fixture that belongs in one: the plan is the same shape whether a
//! test is widening the runtime or squeezing the memory ceiling, so it lives here and both files
//! call it. They still keep their own `batch()` (widening wants three rows and two regions, the
//! ceiling wants a thousand distinct keys to force a reservation) and their own constructions.

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan, ResultLabel,
    StatementTables,
};
use sutura_domain::warehouse::ParamValue;

/// A test date.
pub(crate) fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

pub(crate) fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

pub(crate) fn orders() -> TableName {
    TableName::parse("orders").expect("a test table is a table")
}

pub(crate) fn on(name: &str) -> PlanColumn {
    PlanColumn::new(orders(), ColumnName::parse(name).expect("a test column is a column"))
}

/// Revenue by region for one month, the shape every execution question here has - and a grouped
/// aggregate, which is exactly the operator that matters when the memory ceiling is the thing under
/// test, because it is the one that reserves.
pub(crate) fn question() -> QueryPlan {
    QueryPlan::new(
        source(),
        MetricName::parse("revenue").expect("a test metric is a metric"),
        StatementTables::only(orders()),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, on("order_date")),
        vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension is a dimension")),
            on("region"),
        )],
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: on("amount_cents"),
            },
        },
        ResultLabel::measure(&MetricName::parse("revenue").expect("a test metric is a metric")),
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: on("order_date"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: on("order_date"),
                    param: 1,
                },
            ),
        ],
        vec![ParamValue::Date(day("2026-06-01")), ParamValue::Date(day("2026-07-01"))],
        TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range"),
    )
}
