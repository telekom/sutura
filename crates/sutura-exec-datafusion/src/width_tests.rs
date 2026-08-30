//! How wide the engine runs, and that a wide one is still callable the way the port is called.
//!
//! Its own file for the reason `value_mapping_tests.rs` is: `lib.rs` is at the 1000-line gate, and
//! the gate's answer to that is to split the file rather than to shorten the change.
//!
//! **What is asserted here, and what deliberately is not.** The numbers that justified widening the
//! runtime are on [`DataFusionWarehouse::with_worker_threads`], measured with a scaffold that was
//! deleted with the change: a throughput assertion in this suite would be flaky on a shared runner
//! and would prove less than the two structural facts below. Those two are what a later edit could
//! plausibly break - a `worker_threads` call dropped, or a `SessionContext::new` reinstated, either
//! of which turns the key back into a number nothing reads.
//!
//! **Both constructors now carry a second thing that a `SessionContext::new` would silently lose**,
//! and it is worth knowing before editing either of them: the bounded memory pool. `new` and
//! `new_with_config` both install the engine's unbounded one, so reinstating either would give the
//! width key back its bug *and* remove the working-set ceiling. The ceiling's own assertions are in
//! `pool.rs`; here it is a constructor argument, deliberately roomy, so nothing in this file is
//! measuring memory.

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, Date32Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    Executable, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    StatementTables,
};
use sutura_domain::warehouse::{ParamValue, Warehouse as _};

use super::{DataFusionWarehouse, WorkingSet};

/// How many callers ask at once, and how many workers the engine is given.
const CALLERS: usize = 4;

fn width(count: usize) -> core::num::NonZeroUsize {
    core::num::NonZeroUsize::new(count).expect("a test width is positive")
}

/// A ceiling nothing in this file is meant to reach. The working-set bound has its own suite in
/// `pool.rs`; here it is a constructor argument and nothing more.
fn roomy() -> WorkingSet {
    WorkingSet::of_bytes(width(64 * 1024 * 1024))
}

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

fn orders() -> TableName {
    TableName::parse("orders").expect("a test table is a table")
}

fn on(name: &str) -> PlanColumn {
    PlanColumn::new(orders(), ColumnName::parse(name).expect("a test column is a column"))
}

/// Three rows in two regions, in memory. Enough that a result has a shape to assert on.
fn batch() -> RecordBatch {
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Date32Array::from(vec![
            day("2026-06-05").days_since_epoch(),
            day("2026-06-20").days_since_epoch(),
            day("2026-06-21").days_since_epoch(),
        ])),
        Arc::new(StringArray::from(vec!["north", "north", "south"])),
        Arc::new(Int64Array::from(vec![7_i64, 11, 13])),
    ];
    let schema = Schema::new(vec![
        Field::new("order_date", DataType::Date32, false),
        Field::new("region", DataType::Utf8, false),
        Field::new("amount_cents", DataType::Int64, false),
    ]);
    RecordBatch::try_new(Arc::new(schema), columns).expect("a test batch is rectangular")
}

/// Revenue by region for one month, which is the shape every question here has.
fn question() -> QueryPlan {
    QueryPlan::new(
        source(),
        MetricName::parse("revenue").expect("a test metric is a metric"),
        StatementTables::only(orders()),
        PlanBucket::new(String::from("period"), Grain::Month, on("order_date")),
        vec![PlanKey::new(String::from("region"), on("region"))],
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: on("amount_cents"),
            },
        },
        String::from("revenue"),
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

fn attached(adapter: DataFusionWarehouse) -> DataFusionWarehouse {
    drop(
        adapter
            .context
            .register_batch("orders", batch())
            .expect("an in-memory batch registers"),
    );
    adapter
}

#[test]
fn the_configured_width_is_what_the_engine_gets_and_the_plan_is_partitioned_to_match() {
    // Both halves of `with_worker_threads`. The partition half is the one that is easy to lose:
    // `DataFusion` defaults `target_partitions` to `available_parallelism`, so a two-worker runtime
    // silently builds sixteen-way plans on a sixteen-way host - which is exactly the case this key
    // exists for, because a CPU quota does not change what `available_parallelism` reports.
    let wide = DataFusionWarehouse::with_worker_threads(source(), crate::test_posture(), width(3), roomy())
        .expect("a wide runtime builds");
    assert_eq!(wide.runtime.handle().metrics().num_workers(), 3);
    assert_eq!(wide.context.copied_config().target_partitions(), 3);
}

#[test]
fn the_command_line_constructor_is_still_one_thread() {
    // The path that must not change. `sutura-cli` answers one question and exits, links this crate
    // and nothing else, and has no settings to read a width from - so a wide runtime there would be
    // sixteen threads spawned to answer one question, in all four cross-built artifacts.
    let narrow = DataFusionWarehouse::new(source(), crate::test_posture(), roomy()).expect("a current-thread runtime builds");
    assert_eq!(narrow.runtime.handle().metrics().num_workers(), 1);
}

#[test]
fn a_wide_engine_answers_from_several_threads_at_once_with_no_runtime_entered() {
    // The shape the server calls this port in: several blocking-pool threads, none of them inside a
    // runtime, every one of them `block_on`-ing the adapter's own. "No runtime entered" is not a
    // detail - `Runtime::block_on` panics when the calling thread is already inside one, which is
    // why the composition root does its startup with no runtime built and why the query handler
    // moves the call onto the blocking pool.
    let adapter = attached(
        DataFusionWarehouse::with_worker_threads(source(), crate::test_posture(), width(CALLERS), roomy())
            .expect("a wide runtime builds"),
    );
    let asked = question();
    std::thread::scope(|scope| {
        let callers: Vec<_> = core::iter::repeat_with(|| {
            scope.spawn(|| {
                let rows = adapter
                    .execute(Executable::Query(&asked), &crate::test_leg())
                    .expect("a concurrent question is answered");
                (rows.columns().len(), rows.rows().len())
            })
        })
        .take(CALLERS)
        .collect();
        for caller in callers {
            // Two regions in one month, three columns: the bucket, the key and the measure. Every
            // caller gets the whole answer - a shared session serving several `block_on`s at once
            // must not hand one of them a partial result.
            assert_eq!(caller.join().expect("the calling thread did not panic"), (3, 2));
        }
    });
}

#[test]
fn one_thread_answers_the_same_question_the_same_way_a_wide_one_does() {
    // The width is a scheduling decision and must not be a semantic one. Same plan, same data, two
    // runtimes: if the answers differed, the number an anchor certified would depend on how the
    // process was configured.
    let narrow =
        attached(DataFusionWarehouse::new(source(), crate::test_posture(), roomy()).expect("a current-thread runtime builds"));
    let wide = attached(
        DataFusionWarehouse::with_worker_threads(source(), crate::test_posture(), width(CALLERS), roomy())
            .expect("a wide runtime builds"),
    );
    let asked = question();
    let from_one = narrow
        .execute(Executable::Query(&asked), &crate::test_leg())
        .expect("one thread answers");
    let from_many = wide
        .execute(Executable::Query(&asked), &crate::test_leg())
        .expect("four threads answer");
    assert_eq!(from_one.columns(), from_many.columns());
    assert_eq!(from_one.rows(), from_many.rows());
}
