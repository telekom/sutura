use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, Date32Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use sutura_domain::calendar::TimeRange;
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, TableName};
use sutura_domain::plan::{
    Executable, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
    QueryPlan, ResultLabel, StackedPlan, StatementTables,
};
use sutura_domain::warehouse::{ParamValue, Value, Warehouse as _};

use crate::{DataFusionWarehouse, WorkingSet, day, decoded, source};

/// A region, an order date and an amount.
type Row = (&'static str, &'static str, i64);

/// `rows` as a table of a region, an order date and an amount.
fn table(rows: &[Row]) -> RecordBatch {
    let columns: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(rows.iter().map(|row| row.0).collect::<Vec<_>>())),
        Arc::new(Date32Array::from(
            rows.iter().map(|row| day(row.1).days_since_epoch()).collect::<Vec<_>>(),
        )),
        Arc::new(Int64Array::from(rows.iter().map(|row| row.2).collect::<Vec<_>>())),
    ];
    let schema = Schema::new(vec![
        Field::new("region", DataType::Utf8, false),
        Field::new("order_date", DataType::Date32, false),
        Field::new("amount", DataType::Int64, false),
    ]);
    RecordBatch::try_new(Arc::new(schema), columns).expect("a test batch is rectangular")
}

/// `metric`'s whole answer alone: its `table`'s amount summed by region and month, over June and
/// July.
fn arm(metric: &str, table: &str) -> QueryPlan {
    let on = |name: &str| {
        PlanColumn::new(
            TableName::parse(table).expect("a test table is a table"),
            ColumnName::parse(name).expect("a test column is a column"),
        )
    };
    let metric = MetricName::parse(metric).expect("a test metric is a metric");
    QueryPlan::new(
        source(),
        metric.clone(),
        StatementTables::only(TableName::parse(table).expect("a test table is a table")),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, on("order_date")),
        vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension is a dimension")),
            on("region"),
        )],
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: on("amount"),
            },
        },
        ResultLabel::measure(&metric),
        PlanBindings::parse(
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
            vec![ParamValue::Date(day("2026-06-01")), ParamValue::Date(day("2026-08-01"))],
        )
        .expect("two range bounds bind in placeholder order"),
        TimeRange::new(day("2026-06-01"), day("2026-08-01")).expect("a test range is a range"),
    )
}

/// Each metric answers its own groups, a group only one metric has answers null for the other,
/// and one group both have is one row.
#[test]
fn stacked_metrics_join_on_their_keys_and_bucket_with_a_null_where_one_has_no_rows() {
    let adapter = DataFusionWarehouse::new(
        source(),
        crate::test_posture(),
        WorkingSet::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test ceiling is positive")),
    )
    .expect("a current-thread runtime builds");
    for (name, rows) in [
        (
            "orders",
            table(&[
                ("north", "2026-06-05", 100),
                ("north", "2026-06-20", 50),
                ("south", "2026-07-02", 7),
            ]),
        ),
        ("returns", table(&[("north", "2026-06-11", 20), ("east", "2026-06-03", 5)])),
    ] {
        drop(
            adapter
                .context
                .register_batch(name, rows)
                .expect("an in-memory batch registers"),
        );
    }
    let plan = StackedPlan::new(arm("revenue", "orders"), arm("refunds", "returns"), Vec::new())
        .expect("two arms on one source with one bucket and range stack");
    let result = adapter
        .execute(Executable::Stacked(&plan), &crate::test_leg(), crate::test_deadline())
        .map(|batches| decoded(&batches))
        .expect("the stacked plan runs");
    assert_eq!(result.columns(), plan.result_labels().as_slice());
    let text = |raw: &str| Value::Text(String::from(raw));
    assert_eq!(
        result.rows(),
        [
            vec![text("east"), text("2026-06-01"), Value::Null, Value::Integer(5)],
            vec![text("north"), text("2026-06-01"), Value::Integer(150), Value::Integer(20)],
            vec![text("south"), text("2026-07-01"), Value::Integer(7), Value::Null],
        ]
    );
}
