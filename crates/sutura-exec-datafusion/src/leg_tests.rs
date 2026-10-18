//! A fact leg whose link is truncated to a grain, executed by the engine.

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, Date32Array, Int64Array};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    Executable, InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanPredicate,
    PlanTerm, PredicateOrigin, ResultLabel, StatementTables,
};
use sutura_domain::warehouse::{ParamValue, Value, Warehouse as _};

use crate::{DataFusionWarehouse, WorkingSet};

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

fn on(name: &str) -> PlanColumn {
    PlanColumn::new(
        TableName::parse("usage").expect("a test table is a table"),
        ColumnName::parse(name).expect("a test column is a column"),
    )
}

/// The link and the bucket truncate one column to one grain: the shape a usage fact takes when
/// the relationship it crosses compares its day against a snapshot month.
fn leg() -> LegPlan {
    let bound = |predicate| PlanFilter::new(PredicateOrigin::Definition, predicate);
    LegPlan::Fact {
        source: SourceName::parse("local").expect("a test source is a source"),
        metric: MetricName::parse("minutes").expect("a test metric is a metric"),
        tables: StatementTables::only(TableName::parse("usage").expect("a test table is a table")),
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, on("usage_date")),
        keys: vec![PlanKey::truncated(
            ResultLabel::internal(InternalLabel::Link),
            on("usage_date"),
            Grain::Month,
        )],
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: on("minutes"),
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::parse(
            vec![
                bound(PlanPredicate::AtOrAfter {
                    column: on("usage_date"),
                    param: 0,
                }),
                bound(PlanPredicate::Before {
                    column: on("usage_date"),
                    param: 1,
                }),
            ],
            vec![ParamValue::Date(day("2026-06-01")), ParamValue::Date(day("2026-08-01"))],
        )
        .expect("a test leg binds its two range bounds in placeholder order"),
        range: TimeRange::new(day("2026-06-01"), day("2026-08-01")).expect("a test range is a range"),
    }
}

#[test]
fn a_truncated_link_is_a_date_at_the_start_of_its_month() {
    let adapter = DataFusionWarehouse::new(
        SourceName::parse("local").expect("a test source is a source"),
        crate::test_posture(),
        WorkingSet::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test ceiling is positive")),
    )
    .expect("a current-thread runtime builds");
    let days: ArrayRef = Arc::new(Date32Array::from(vec![
        day("2026-06-30").days_since_epoch(),
        day("2026-07-01").days_since_epoch(),
        day("2026-07-15").days_since_epoch(),
    ]));
    let batch = RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Field::new("usage_date", DataType::Date32, false),
            Field::new("minutes", DataType::Int64, false),
        ])),
        vec![days, Arc::new(Int64Array::from(vec![1_i64, 2, 3]))],
    )
    .expect("a test batch is rectangular");
    drop(
        adapter
            .context
            .register_batch("usage", batch)
            .expect("an in-memory batch registers"),
    );

    let leg = leg();
    let batches = adapter
        .execute(Executable::Leg(&leg), &crate::test_leg(), crate::test_deadline())
        .expect("the leg runs");
    let link = InternalLabel::Link.label();
    let field = batches.schema().field_with_name(&link).expect("the leg projects its link");
    assert_eq!(
        field.data_type(),
        &DataType::Date32,
        "the link is a date the combiner can join on"
    );

    let month = |iso: &str| Value::Text(String::from(iso));
    assert_eq!(
        crate::decoded(&batches).rows(),
        [
            vec![month("2026-06-01"), month("2026-06-01"), Value::Integer(1)],
            vec![month("2026-07-01"), month("2026-07-01"), Value::Integer(5)],
        ],
        "the last day of June and the first of July are two links"
    );
}
