//! The working-set ceiling, biting on a real operator.
//!
//! Its own file for the reason `width_tests.rs` is: `lib.rs` is at the 1000-line gate and needs the
//! room more than this does.
//!
//! **What is asserted here, and what deliberately is not.** The pool's own boundary - a reservation
//! one byte over the ceiling refused, one at it granted - is in `pool.rs`, against a
//! `MemoryConsumer` directly, because that is where the number is. What this file adds is the thing
//! that could not be asserted there: that a question travelling the port reaches the pool at all, and
//! that a refused reservation comes back through this adapter as something the domain can recognise
//! rather than as an abort.
//!
//! **It cannot assert an oversized leg, and the name of the test says so.** The pool counts operator
//! reservations and nothing else, so a result set large enough to end the process while a driver is
//! materialising it ends it, before any operator has reserved anything. `docs/adr/0009` puts the bound
//! that reaches that - a byte budget applied as rows are converted - with the execution boundary, and
//! says plainly that this branch can assert a reservation refused and cannot assert a leg refused.

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, Date32Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    Executable, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
};
use sutura_domain::warehouse::{ParamValue, Warehouse as _};

use crate::{DataFusionError, DataFusionWarehouse, WorkingSet};

/// A ceiling small enough that any real operator reservation is over it.
///
/// One byte, and not a plausible-looking small number: the assertion is that the configured value is
/// what refuses, and a number chosen to sit near an allocator's block size would make this test
/// depend on the allocator instead.
const TINY: usize = 1;

/// A ceiling nothing in this corpus comes near, for the control.
const ROOMY: usize = 64 * 1024 * 1024;

fn ceiling(bytes: usize) -> WorkingSet {
    WorkingSet::of_bytes(core::num::NonZeroUsize::new(bytes).expect("a test ceiling is positive"))
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

/// Enough rows in enough groups that a grouped aggregate builds a hash table worth reserving for.
///
/// A thousand distinct keys rather than three, because a three-group aggregate over three rows may
/// reserve nothing at all - and a test that passed because the operator never asked for memory would
/// be asserting the absence of a reservation while claiming to assert a refused one.
fn batch() -> RecordBatch {
    let rows = 1000_i64;
    let columns: Vec<ArrayRef> = vec![
        // One date for every row: the bucket is not what this fixture varies, the group key is.
        Arc::new(Date32Array::from(vec![
            day("2026-06-05").days_since_epoch();
            usize::try_from(rows).expect("a thousand fits a usize")
        ])),
        Arc::new(StringArray::from(
            (0..rows).map(|row| format!("region-{row}")).collect::<Vec<String>>(),
        )),
        Arc::new(Int64Array::from((0..rows).collect::<Vec<i64>>())),
    ];
    let schema = Schema::new(vec![
        Field::new("order_date", DataType::Date32, false),
        Field::new("region", DataType::Utf8, false),
        Field::new("amount_cents", DataType::Int64, false),
    ]);
    RecordBatch::try_new(Arc::new(schema), columns).expect("a test batch is rectangular")
}

/// Revenue by region for one month: a grouped aggregate, which is an operator that reserves.
fn question() -> QueryPlan {
    QueryPlan::new(
        source(),
        MetricName::parse("revenue").expect("a test metric is a metric"),
        orders(),
        Vec::new(),
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

fn engine(bytes: usize) -> DataFusionWarehouse {
    let adapter = DataFusionWarehouse::new(source(), ceiling(bytes)).expect("a bounded engine builds");
    drop(
        adapter
            .context
            .register_batch("orders", batch())
            .expect("an in-memory batch registers"),
    );
    adapter
}

#[test]
fn an_operator_reservation_over_the_ceiling_is_refused_rather_than_aborting_the_process() {
    // THE assertion this step exists for, and the name is deliberately narrow: what is refused is an
    // operator RESERVATION. Not a driver's buffer, not `collect()` materialising batches, not the row
    // set built while a result is converted - the pool sees none of those, and a test named for
    // "an oversized intermediate" would be claiming a mechanism nobody has built.
    //
    // The process surviving is the whole point: without a pool the engine's allocation is unbounded,
    // and shipped profiles compile `panic = "abort"`, so this test reaching its own assertions at all
    // is part of what it asserts.
    let bounded = engine(TINY);
    let failure = bounded
        .execute(Executable::Query(&question()))
        .expect_err("a grouped aggregate cannot run inside one byte");

    // It arrives as `Execute`, which is the running stage: the plan built and resolved, and the
    // refusal happened where memory is reserved.
    assert!(matches!(failure, DataFusionError::Execute { .. }), "{failure:?}");

    // And the port's own question, which is what makes it a REFUSAL upstream rather than a `503`.
    // The number that comes back is the configured ceiling, because that is what an operator can act
    // on and what is the same for every caller.
    assert_eq!(bounded.working_set_exhausted(&failure), Some(1));
}

#[test]
fn the_same_question_under_a_roomy_ceiling_is_answered() {
    // The control, and it is not decoration: without it the test above passes just as well against an
    // engine that cannot answer anything - a plan built wrong, a column misnamed - and would be
    // asserting a broken fixture rather than a bound.
    let roomy = engine(ROOMY);
    let rows = roomy
        .execute(Executable::Query(&question()))
        .expect("a thousand groups fit in 64 mebibytes");
    // A thousand distinct keys in one month: three columns - the bucket, the key and the measure.
    assert_eq!((rows.columns().len(), rows.rows().len()), (3, 1000));
    // Nothing is left reserved once the answer is collected, which is what makes the ceiling a bound
    // on concurrent work rather than a budget the process spends down over its lifetime.
    assert_eq!(roomy.memory_pool().reserved(), 0);
}

#[test]
fn a_failure_that_is_not_the_ceiling_is_not_reported_as_one() {
    // The other direction, and the more dangerous mistake. A caller told "resources exhausted" is
    // told not to retry; a data system that is briefly unwell is exactly the case where retrying is
    // right. So a plan naming a table nothing attached must answer `None` here, even though it is a
    // failure from the same adapter carrying the same engine error type.
    let bare = DataFusionWarehouse::new(source(), ceiling(ROOMY)).expect("a bounded engine builds");
    let failure = bare
        .execute(Executable::Query(&question()))
        .expect_err("a plan naming an unattached table does not run");
    assert!(matches!(failure, DataFusionError::Analyze { .. }), "{failure:?}");
    assert_eq!(bare.working_set_exhausted(&failure), None);
}

#[test]
fn the_ceiling_is_read_from_the_configured_value_and_not_from_the_pool() {
    // `docs/adr/0015` decides this and gives the reason: `MemoryPool::memory_limit` defaults to
    // `Unknown`, so a pool implementation that does not override it reports no ceiling and the
    // reserved-against-ceiling ratio an operator wants cannot be computed. The configured number is
    // always knowable, so it is what the adapter keeps.
    let bounded = DataFusionWarehouse::new(source(), ceiling(4096)).expect("a bounded engine builds");
    assert_eq!(bounded.working_set().bytes(), 4096);
}

#[test]
fn the_debug_of_a_bounded_warehouse_still_shows_only_the_source() {
    // The property the pool must not have cost. A `Debug` of a warehouse reaches a log by accident,
    // and a live reservation figure is an observation about whatever question is in flight - so the
    // hand-written `Debug` stayed as it was, and `working_set()` is the accessor for the half that is
    // a configuration fact.
    let rendered = format!("{:?}", DataFusionWarehouse::new(source(), ceiling(4096)).expect("builds"));
    assert!(rendered.contains("local"), "{rendered}");
    assert!(
        !rendered.contains("4096"),
        "the ceiling reached a Debug rendering: {rendered}"
    );
    assert!(!rendered.contains("Greedy"), "the pool reached a Debug rendering: {rendered}");
}
