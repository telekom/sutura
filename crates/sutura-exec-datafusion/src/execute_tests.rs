//! The adapter's own suite: attaching a file, executing a plan, and the translation helpers.
//!
//! In its own file for the reason `width_tests.rs` is: `lib.rs` is at
//! the 1000-line gate, and the gate's answer to that is to split the file rather than to shorten
//! the fix. A pure move - nothing here changed with the split.

use super::translate::{aggregate_expr, literal, measure_expression, unit};
use super::{DataFusionError, DataFusionWarehouse, column};
use datafusion::arrow::array::{ArrayRef, BooleanArray, Date32Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::catalog::streaming::StreamingTable;
use datafusion::execution::{RecordBatchStream, SendableRecordBatchStream, TaskContext};
use datafusion::prelude::col;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    Executable, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
    QueryPlan, ResultLabel, StatementTables,
};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
// `Warehouse as _`: the trait is imported for `dry_run` and `execute`, and never named.
use sutura_domain::warehouse::{ParamValue, Real, Value, Warehouse as _};

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

/// A ceiling no test in this module is meant to reach. The bound has its own suite in `pool.rs`.
fn roomy() -> super::WorkingSet {
    super::WorkingSet::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test ceiling is positive"))
}

fn real(value: f64) -> Real {
    Real::parse(value).expect("a test literal is finite")
}

fn orders() -> TableName {
    TableName::parse("orders").expect("a test table is a table")
}

fn on(name: &str) -> PlanColumn {
    PlanColumn::new(orders(), ColumnName::parse(name).expect("a test column is a column"))
}

fn agg(aggregate: Aggregate, name: &str) -> PlanTerm {
    PlanTerm::Aggregate {
        aggregate,
        column: on(name),
    }
}

fn simple(aggregate: Aggregate, name: &str) -> PlanMeasure {
    PlanMeasure::Simple {
        term: agg(aggregate, name),
    }
}

/// One month of one table, grouped by region, over a bounded range.
///
/// The two range bounds are bound as parameters at indices 0 and 1, which is what the SQL path
/// would render as placeholders, so the predicate-by-index path is exercised by every test that
/// executes anything.
fn plan(measure: PlanMeasure, label: &str, keys: Vec<PlanKey>) -> QueryPlan {
    QueryPlan::new(
        SourceName::parse("local").expect("a test source is a source"),
        MetricName::parse("revenue").expect("a test metric is a metric"),
        StatementTables::only(orders()),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, on("order_date")),
        keys,
        measure,
        ResultLabel::measure(&MetricName::parse(label).expect("a test measure label is a metric name")),
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
        .expect("a test plan binds its two range bounds in placeholder order"),
        TimeRange::new(day("2026-06-01"), day("2026-08-01")).expect("a test range is a range"),
    )
}

/// An adapter with one in-memory table called `orders`.
///
/// `register_batch` is synchronous and takes a `RecordBatch`, so a test needs no fixture file and
/// no temporary directory - which is what lets the end-to-end cases below run in the unit suite.
fn warehouse(batch: RecordBatch) -> DataFusionWarehouse {
    let adapter = DataFusionWarehouse::new(
        SourceName::parse("local").expect("a test source is a source"),
        crate::test_posture(),
        roomy(),
    )
    .expect("a current-thread runtime builds");
    drop(
        adapter
            .context
            .register_batch("orders", batch)
            .expect("an in-memory batch registers"),
    );
    adapter
}

fn batch(fields: Vec<Field>, columns: Vec<ArrayRef>) -> RecordBatch {
    RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).expect("a test batch is rectangular")
}

fn region_column() -> ArrayRef {
    Arc::new(StringArray::from(vec!["north", "north", "south"]))
}

fn date_column() -> ArrayRef {
    Arc::new(Date32Array::from(vec![
        day("2026-06-05").days_since_epoch(),
        day("2026-06-20").days_since_epoch(),
        day("2026-07-02").days_since_epoch(),
    ]))
}

fn region_key() -> Vec<PlanKey> {
    vec![PlanKey::new(
        ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension is a dimension")),
        on("region"),
    )]
}

// TWO CELLS MOVED RATHER THAN DELETED, and where they went is the point: the value mapping is
// `sutura_domain::warehouse::arrow` since `docs/adr/0039`, so the day-number-to-ISO cell and the
// unmapped-type refusal are rows of that module's table now. Asserting them here as well would be
// two copies of one mapping, which is the shape this change exists to remove.

#[test]
fn a_column_reference_keeps_the_case_the_catalog_wrote() {
    // The bug this crate is most exposed to. `col("Orders.Amount")` normalises BOTH halves to
    // lowercase, so a model whose table or column carries a capital resolves against a name that
    // does not exist - or, worse, against a different one that does. The domain preserves case
    // deliberately, so every reference here is built the case-preserving way.
    let mixed = PlanColumn::new(
        TableName::parse("Orders").expect("a test table is a table"),
        ColumnName::parse("Amount").expect("a test column is a column"),
    );
    assert_eq!(format!("{}", column(&mixed)), "Orders.Amount");
    // The negative control, and the only `col` in this crate: this is what the reference would
    // have silently been.
    assert_eq!(format!("{}", col("Orders.Amount")), "orders.amount");
}

#[test]
fn every_aggregate_the_domain_has_maps_to_a_distinct_engine_function() {
    // The bug: a copy-pasted match arm that maps Max to the minimum. Nothing about the plan
    // changes, the query runs, and the number is wrong. Distinctness is what catches it; the
    // name check is what catches a whole column of arms shifted by one.
    let kinds = [
        Aggregate::Sum,
        Aggregate::Count,
        Aggregate::CountDistinct,
        Aggregate::Avg,
        Aggregate::Min,
        Aggregate::Max,
    ];
    let mut rendered: Vec<String> = kinds
        .iter()
        .map(|kind| format!("{}", aggregate_expr(*kind, column(&on("amount")))).to_lowercase())
        .collect();
    for (kind, text) in kinds.iter().zip(rendered.iter()) {
        let wanted = kind.as_str().trim_start_matches("count_");
        assert!(text.contains(wanted), "{kind} rendered as {text}");
    }
    rendered.sort();
    rendered.dedup();
    assert_eq!(rendered.len(), kinds.len());
}

#[test]
fn every_grain_truncates_to_the_unit_the_sql_path_names() {
    // The bug: a unit spelled differently here than in `generate.rs`, which truncates to a
    // different period and makes the two adapters answer different questions from one plan.
    // Asserted against the domain's own spelling rather than against a second literal list, so
    // the two cannot drift apart quietly.
    for grain in [Grain::Day, Grain::Week, Grain::Month, Grain::Quarter, Grain::Year] {
        assert_eq!(unit(grain), grain.as_str());
    }
    assert_eq!(unit(Grain::Month), "month");
}

#[test]
fn a_parameter_becomes_a_typed_value_and_never_syntax() {
    // The claim in `literal`'s doc, asserted. A value that would be an injection in a rendered
    // statement is a `Utf8` scalar here: there is no parser and no statement for it to be part
    // of, so the engine's only option is to compare it.
    let hostile = format!("{:?}", literal(&ParamValue::Text(String::from("a' OR '1'='1"))));
    assert!(hostile.contains("Utf8"), "{hostile}");
    assert!(hostile.contains("a' OR "), "{hostile}");
    // A date is bound as the day number the column holds, not as text the engine has to parse.
    let bound = format!("{:?}", literal(&ParamValue::Date(day("2026-06-01"))));
    assert!(bound.contains("Date32"), "{bound}");
}

#[test]
fn a_ratio_casts_its_numerator_so_integer_division_cannot_truncate() {
    // The bug: `sum(cents) / count(*)` over two integer columns truncates in this engine exactly
    // as it does in SQL, so a ratio of 7 to 2 answers 3. Checked on the expression as well as
    // end to end below, because the cast is the part a refactor would drop.
    let measure = PlanMeasure::Ratio {
        numerator: agg(Aggregate::Sum, "hits"),
        denominator: agg(Aggregate::Sum, "tries"),
        zero_denominator: ZeroDenominator::Fail,
    };
    let rendered = format!("{}", measure_expression(&measure).expect("a ratio is one expression"));
    assert!(rendered.contains("Float64"), "{rendered}");
}

#[test]
fn a_grouped_sum_comes_back_labelled_and_ordered_the_way_the_plan_says() {
    // The bug: a result whose columns are in a different order than `result_labels`, which every
    // consumer reads by position. Row order is asserted too, because the differential test
    // against the SQL path compares rows in order and an unordered aggregate is not stable.
    let adapter = warehouse(batch(
        vec![
            Field::new("region", DataType::Utf8, false),
            Field::new("order_date", DataType::Date32, false),
            Field::new("amount", DataType::Int64, false),
        ],
        vec![
            region_column(),
            date_column(),
            Arc::new(Int64Array::from(vec![100_i64, 50_i64, 7_i64])),
        ],
    ));
    let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());
    let result = adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .expect("the plan runs");
    assert_eq!(result.columns(), query.result_labels().as_slice());
    assert_eq!(
        result.rows(),
        [
            vec![
                Value::Text(String::from("north")),
                Value::Text(String::from("2026-06-01")),
                Value::Integer(150),
            ],
            vec![
                Value::Text(String::from("south")),
                Value::Text(String::from("2026-07-01")),
                Value::Integer(7),
            ],
        ]
    );
}

#[test]
#[cfg(feature = "fixtures")]
fn a_large_decimal_fixture_total_stays_exact() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/datafusion-decimal-total");
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("the fixture directory is writable");
    let path = dir.join("orders.csv");
    std::fs::write(
        &path,
        "order_date,region,amount\n\
         2026-06-05,north,9000000000000000000000000000000000000\n\
         2026-06-20,north,9000000000000000000000000000000000000\n",
    )
    .expect("the fixture is writable");
    let adapter = DataFusionWarehouse::new(
        SourceName::parse("local").expect("a test source is a source"),
        crate::test_posture(),
        roomy(),
    )
    .expect("a current-thread runtime builds");
    adapter.attach_fixture_csv(&orders(), &path).expect("the fixture attaches");

    let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());
    let result = adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .expect("the plan runs");
    assert_eq!(
        result.rows(),
        [vec![
            Value::Text(String::from("north")),
            Value::Text(String::from("2026-06-01")),
            Value::Text(String::from("18000000000000000000000000000000000000")),
        ]]
    );
    drop(std::fs::remove_dir_all(dir));
}

#[test]
fn a_ratio_whose_zero_denominator_yields_null_answers_null_rather_than_failing() {
    // Two bugs at once. Integer division would answer 3 for 7 over 2, and dividing by a zero
    // denominator is a hard "Divide by zero" in this engine rather than the null that SQL's
    // NULLIF produces - which would fail the whole question because one group had no tries.
    let adapter = warehouse(batch(
        vec![
            Field::new("region", DataType::Utf8, false),
            Field::new("order_date", DataType::Date32, false),
            Field::new("hits", DataType::Int64, false),
            Field::new("tries", DataType::Int64, false),
        ],
        vec![
            region_column(),
            date_column(),
            Arc::new(Int64Array::from(vec![7_i64, 0_i64, 5_i64])),
            Arc::new(Int64Array::from(vec![2_i64, 0_i64, 0_i64])),
        ],
    ));
    let query = plan(
        PlanMeasure::Ratio {
            numerator: agg(Aggregate::Sum, "hits"),
            denominator: agg(Aggregate::Sum, "tries"),
            zero_denominator: ZeroDenominator::Null,
        },
        "hit_rate",
        region_key(),
    );
    let result = adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .expect("the plan runs");
    assert_eq!(result.cell(0, 2), Some(&Value::Real(real(3.5))));
    assert_eq!(result.cell(1, 2), Some(&Value::Null));
}

#[test]
fn a_count_if_answers_zero_for_a_group_with_no_matches_rather_than_nothing() {
    // The bug: a filtered count, or a CASE with no ELSE, leaves a group whose every row is false
    // with a null instead of a 0. `generate.rs` sums a CASE with an ELSE for exactly this
    // reason, and a differential test would see null against 0.
    let adapter = warehouse(batch(
        vec![
            Field::new("region", DataType::Utf8, false),
            Field::new("order_date", DataType::Date32, false),
            Field::new("paid", DataType::Boolean, true),
        ],
        vec![
            region_column(),
            date_column(),
            Arc::new(BooleanArray::from(vec![Some(false), None, Some(true)])),
        ],
    ));
    let query = plan(
        PlanMeasure::Simple {
            term: PlanTerm::CountIf { column: on("paid") },
        },
        "paid_orders",
        region_key(),
    );
    let result = adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .expect("the plan runs");
    assert_eq!(result.cell(0, 2), Some(&Value::Integer(0)));
    assert_eq!(result.cell(1, 2), Some(&Value::Integer(1)));
}

#[test]
fn a_conditional_count_is_usable_as_a_ratio_numerator() {
    // The metric the previous vocabulary could not express, executed. `count_if` was a sibling
    // of `ratio` rather than a term inside one, so a rate over a conditional count had every
    // ingredient present and nowhere to write it. Two groups on purpose: `north` divides 1 by 2
    // and `south` divides 0 by 1, which is the answer a `count_if` denominator would have got
    // wrong as a null.
    let adapter = warehouse(batch(
        vec![
            Field::new("region", DataType::Utf8, false),
            Field::new("order_date", DataType::Date32, false),
            Field::new("paid", DataType::Boolean, true),
            Field::new("order_id", DataType::Int64, false),
        ],
        vec![
            region_column(),
            date_column(),
            Arc::new(BooleanArray::from(vec![Some(true), Some(false), None])),
            Arc::new(Int64Array::from(vec![1_i64, 2_i64, 3_i64])),
        ],
    ));
    let query = plan(
        PlanMeasure::Ratio {
            numerator: PlanTerm::CountIf { column: on("paid") },
            denominator: agg(Aggregate::CountDistinct, "order_id"),
            zero_denominator: ZeroDenominator::Null,
        },
        "paid_share",
        region_key(),
    );
    let result = adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .expect("the plan runs");
    assert_eq!(result.cell(0, 2), Some(&Value::Real(real(0.5))));
    assert_eq!(result.cell(1, 2), Some(&Value::Real(real(0.0))));
}

#[test]
fn the_engine_asks_for_one_row_past_the_cap_exactly_as_the_sql_path_does() {
    // The row cap is enforced ABOVE this port, by comparing the rows that came back against
    // `QueryPlan::max_rows` - and that comparison can only tell "reached the cap" from "cut off
    // by the cap" if the adapter asked for `row_limit`, which is one more. So the `+1` is not an
    // implementation detail of the SQL renderer: it is half of the mechanism, and an adapter that
    // asked for `max_rows` instead would silently truncate at exactly the boundary while every
    // other test still passed.
    //
    // The SQL path has this pinned by a golden that reads `LIMIT 10001`. This path renders no SQL
    // and had nothing, which is the asymmetry that makes a fix in one renderer a differential
    // failure waiting to happen. Asserted on the logical plan rather than by counting ten
    // thousand rows through the engine, because the claim is about the number asked for.
    let adapter = warehouse(batch(
        vec![
            Field::new("region", DataType::Utf8, false),
            Field::new("order_date", DataType::Date32, false),
            Field::new("amount", DataType::Int64, false),
        ],
        vec![
            region_column(),
            date_column(),
            Arc::new(Int64Array::from(vec![100_i64, 50_i64, 7_i64])),
        ],
    ));
    let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());
    let logical = adapter
        .runtime()
        .expect("a test runtime is present")
        .block_on(adapter.logical_plan(&query))
        .expect("the plan resolves");
    let rendered = format!("{}", logical.display_indent());
    assert!(
        rendered.contains(&format!("fetch={}", query.row_limit())),
        "the engine must fetch one past the cap:\n{rendered}"
    );
    assert!(
        !rendered.contains(&format!("fetch={}", query.max_rows())),
        "fetching exactly the cap makes a full result indistinguishable from a truncated one:\n{rendered}"
    );
}

#[test]
fn a_plan_naming_a_table_that_was_never_attached_is_an_error_and_never_an_empty_answer() {
    // An unattached table must be an error and not an empty result, because an empty result
    // reads as "there was no revenue in June". The engine resolves every name during analysis,
    // so that holds on the only pass this adapter makes.
    let adapter = DataFusionWarehouse::new(
        SourceName::parse("local").expect("a test source is a source"),
        crate::test_posture(),
        roomy(),
    )
    .expect("a current-thread runtime builds");
    let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());

    // And it holds WITHOUT a pre-flight, which is the other half of the claim. This adapter
    // takes the port's defaulted `dry_run` deliberately: checking here means building the
    // logical plan and running the analyzer and the optimizer, which is most of executing it,
    // so a required pre-check bought this guarantee at the price of planning every question
    // twice. Asserted rather than assumed, because "the engine does not pre-check" is exactly
    // the kind of claim that stops being true when somebody adds an override back.
    assert!(
        adapter
            .dry_run(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
            .is_ok(),
        "the engine answers `would this work` by not asking, so a plan it cannot run still dry-runs clean"
    );

    let error = adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .expect_err("an unattached table does not resolve");
    assert!(matches!(error, DataFusionError::Analyze { .. }), "{error:?}");
    assert_eq!(adapter.source().as_str(), "local");
}

#[test]
fn credential_material_this_engine_cannot_use_is_refused_before_the_plan_is_built() {
    // The wiring defect between a credential broker and a source declaration, at the engine. This is
    // one process reading local files under one operating-system identity - which is what
    // `IMPERSONATION` declares - so there is nowhere for a subject's own credential to arrive, and
    // accepting one would report a leg as impersonated that ran as this process.
    //
    // **An `Err` and never a refusal**: nothing about the question was wrong. It is also refused
    // BEFORE the plan is built, which is what the assertion on the error variant shows - a plan
    // naming an unattached table would otherwise fail as `Analyze` first and hide this.
    let adapter = DataFusionWarehouse::new(
        SourceName::parse("local").expect("a test source is a source"),
        crate::test_posture(),
        roomy(),
    )
    .expect("a current-thread runtime builds");
    let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());
    for handed in [
        sutura_domain::identity::Presented::SubjectToken {
            material: sutura_domain::identity::Secret::new("an-exchanged-token"),
        },
        sutura_domain::identity::Presented::SubjectPrincipal {
            name: sutura_domain::identity::PrincipalName::parse("analyst_role").expect("a test name is a name"),
        },
    ] {
        let expected = handed.as_str();
        let error = adapter
            .execute(Executable::Query(&query), &handed, crate::test_deadline())
            .expect_err("this engine cannot carry a subject");
        let DataFusionError::NoPlaceForASubject { ref at, presented } = error else {
            panic!("the adapter names what it was handed, before it plans anything: {error:?}");
        };
        assert_eq!(at, "local");
        assert_eq!(presented, expected);
    }
    // And the shape it CAN execute with reaches the engine, so the assertions above are not passing
    // against an adapter that refuses everything: the same plan then fails at resolution instead.
    let error = adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .expect_err("the table is not attached");
    assert!(matches!(error, DataFusionError::Analyze { .. }), "{error:?}");
}

#[test]
fn a_shared_leg_carrying_another_acknowledgement_is_refused_rather_than_executed() {
    // THE CHECK THE SHAPE MATCH DOES NOT MAKE, and a review is what found it missing. The match on
    // `Presented` above compares what arrived against what this CODE can carry, and never reads the
    // posture the composition root handed this adapter - so a leg whose variant is right and whose
    // operator acknowledgement is somebody else's got past it and executed. Provenance is read off
    // `posture`, so the answer would then have recorded this adapter's own declaration rather than
    // the acknowledgement the broker presented: the record would describe a leg that did not happen.
    //
    // Both values are real and independent. The broker reads the settings tree; the adapter holds what
    // the root handed it. Comparing them is a comparison; reading one of them twice is not.
    let adapter = DataFusionWarehouse::new(
        SourceName::parse("local").expect("a test source is a source"),
        crate::test_posture(),
        roomy(),
    )
    .expect("a current-thread runtime builds");
    let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());

    let fabricated = sutura_domain::identity::Presented::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse("a witness no operator wrote for this source")
                .expect("a test reason is a reason"),
        ),
    };
    // It matches the variant this adapter accepts, which is exactly why the shape check cannot see it.
    assert_eq!(fabricated.as_str(), crate::test_leg().as_str());

    let error = adapter
        .execute(Executable::Query(&query), &fabricated, crate::test_deadline())
        .expect_err("a witness that is not this source's is not this source's");
    let DataFusionError::PresentedDisagreesWithPosture { ref cause } = error else {
        panic!("the adapter names the disagreement rather than executing: {error:?}");
    };
    assert_eq!(
        *cause,
        sutura_domain::identity::PresentedDisagreesWithPosture::WitnessIsNotThisSources {
            at: SourceName::parse("local").expect("a test source is a source"),
        }
    );

    // And the source's OWN witness still reaches the engine, so the assertion above is not passing
    // against an adapter that refuses every shared leg: the same plan then fails at resolution.
    let error = adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .expect_err("the table is not attached");
    assert!(matches!(error, DataFusionError::Analyze { .. }), "{error:?}");
}

/// `telekom/sutura#160` PR2: the engine stops a question at its deadline rather than answering it.
///
/// A `StreamingTable` never runs out on its own, which is the source this suite needs: it keeps
/// producing rows past any budget this test opens, so the ONLY way `execute` can return before
/// `stop_by` is that the deadline stopped it. Each batch is gated behind a real
/// `tokio::time::sleep`, which is what gives the runtime's own timer a genuine turn between
/// batches - the same reason the source cannot simply spin: a future that never returns `Pending`
/// gives the executor no point at which to notice the deadline passed either.
mod deadline_tests {
    use super::{
        Aggregate, AtomicBool, Budget, Context, DataFusionWarehouse, DataType, Date32Array, Deadline, Duration, Executable,
        Field, Instant, Int64Array, Ordering, Pin, Poll, RecordBatch, RecordBatchStream, Schema, SchemaRef,
        SendableRecordBatchStream, StreamingTable, StringArray, TaskContext, plan, region_key, roomy, simple,
    };
    use datafusion::physical_plan::streaming::PartitionStream;
    use futures_util::stream::Stream;
    use std::future::Future as _;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    // `super::source` does not exist: `source()` lives at the crate root (`lib.rs`), not in
    // `execute_tests` - `crate::source()` is the same route `crate::test_posture()` already takes.
    // `Warehouse as _`: the outer module's own import is not inherited by this one either.
    use sutura_domain::warehouse::Warehouse as _;

    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, false),
            Field::new("order_date", DataType::Date32, false),
            Field::new("amount", DataType::Int64, false),
        ]))
    }

    fn one_row(schema: &SchemaRef) -> RecordBatch {
        RecordBatch::try_new(
            Arc::clone(schema),
            vec![
                Arc::new(StringArray::from(vec!["north"])),
                Arc::new(Date32Array::from(vec![super::day("2026-06-05").days_since_epoch()])),
                Arc::new(Int64Array::from(vec![1_i64])),
            ],
        )
        .expect("a one-row batch is rectangular")
    }

    /// A row every 5ms, forever - the test's own 200ms budget is always spent first.
    ///
    /// `Pin<Box<Sleep>>` keeps this type `Unpin` regardless of `Sleep`'s own pinning (`Box<T>` is
    /// `Unpin` for every `T`), so `poll_next` needs no unsafe projection. `Drop` is the proof the
    /// test reads: this source never yields `None` inside any budget the tests below open, so the
    /// only way it stops is by being dropped.
    struct ForeverRows {
        schema: SchemaRef,
        stop_by: Instant,
        sleep: Pin<Box<tokio::time::Sleep>>,
        dropped: Arc<AtomicBool>,
    }

    impl ForeverRows {
        fn new(schema: SchemaRef, stop_by: Instant, dropped: Arc<AtomicBool>) -> Self {
            Self {
                schema,
                stop_by,
                sleep: Box::pin(tokio::time::sleep(Duration::from_millis(5))),
                dropped,
            }
        }
    }

    impl Stream for ForeverRows {
        type Item = datafusion::error::Result<RecordBatch>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let this = self.get_mut();
            if this.sleep.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            if Instant::now() >= this.stop_by {
                return Poll::Ready(None);
            }
            let batch = one_row(&this.schema);
            this.sleep = Box::pin(tokio::time::sleep(Duration::from_millis(5)));
            Poll::Ready(Some(Ok(batch)))
        }
    }

    impl RecordBatchStream for ForeverRows {
        fn schema(&self) -> SchemaRef {
            Arc::clone(&self.schema)
        }
    }

    impl Drop for ForeverRows {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[derive(Debug)]
    struct ForeverPartition {
        schema: SchemaRef,
        stop_by: Instant,
        dropped: Arc<AtomicBool>,
    }

    impl PartitionStream for ForeverPartition {
        fn schema(&self) -> &SchemaRef {
            &self.schema
        }

        fn execute(&self, _ctx: Arc<TaskContext>) -> SendableRecordBatchStream {
            Box::pin(ForeverRows::new(
                Arc::clone(&self.schema),
                self.stop_by,
                Arc::clone(&self.dropped),
            ))
        }
    }

    /// A table over [`ForeverPartition`], registered as `orders` - the plan below never sees an end
    /// of input on its own.
    fn register_never_ending_orders(adapter: &DataFusionWarehouse, stop_by: Instant) -> Arc<AtomicBool> {
        let dropped = Arc::new(AtomicBool::new(false));
        let table = StreamingTable::try_new(
            schema(),
            vec![Arc::new(ForeverPartition {
                schema: schema(),
                stop_by,
                dropped: Arc::clone(&dropped),
            })],
        )
        .expect("a streaming table with matching schema builds");
        adapter
            .context
            .register_table("orders", Arc::new(table))
            .expect("a streaming table registers");
        dropped
    }

    fn revenue_by_region() -> super::QueryPlan {
        plan(simple(Aggregate::Sum, "amount"), "revenue", region_key())
    }

    /// `abort()` on the spawned partition task `SpawnedTask::drop` calls is a REQUEST, not a
    /// synchronous fact - the task's own `Drop` runs at its next poll, which needs another turn on
    /// the runtime `execute` already returned from. This re-enters that same runtime and gives it
    /// up to 500ms of scheduling to reap the abort, rather than reading the flag the instant
    /// `execute` returns and calling a race the mechanism's own limit.
    fn wait_for_drop(adapter: &DataFusionWarehouse, dropped: &AtomicBool) {
        adapter.runtime().expect("a test runtime is present").block_on(async {
            for _ in 0..100 {
                if dropped.load(Ordering::SeqCst) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
    }

    /// The RED test: on the head before this slice, `execute` ignores the deadline and this
    /// adapter answers after `stop_by` (two seconds) with rows, failing `elapsed < ceiling` and
    /// `expect_err`. With the timeout wrapping `rows`, the call returns inside the 200ms budget
    /// plus a ceiling, names [`DataFusionError::DeadlineExceeded`], and the source was DROPPED -
    /// not merely outlived.
    ///
    /// **The ceiling is CI's tight one, or a wider one on a shared machine - never a fixed guess.**
    /// A single `elapsed < 500ms` reddened three times in one evening on branches that could not
    /// touch this code (`telekom/sutura#140`'s comment thread: load 22-34 on 15 cores, three other
    /// lanes compiling). `sutura_dev::tolerance` decides which number applies and CI keeps the
    /// original 500ms; see that module for why the nix sandbox this repo's own CI runs through
    /// lands on the strict number by construction. `Instant::now() < stop_by` stays unconditional
    /// underneath it: the source still had rows left, so the call was cancelled and not outlived,
    /// which is exact rather than a margin and needs no venue to pick a number for it.
    #[test]
    fn a_question_that_exceeds_its_budget_is_stopped_at_the_data_system() {
        let stop_by = Instant::now() + Duration::from_secs(2);
        let adapter =
            DataFusionWarehouse::new(crate::source(), crate::test_posture(), roomy()).expect("a current-thread runtime builds");
        let dropped = register_never_ending_orders(&adapter, stop_by);

        let query = revenue_by_region();
        let deadline = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_millis(200)).expect("200ms is a budget"),
        );

        let started = Instant::now();
        let error = adapter
            .execute(Executable::Query(&query), &crate::test_leg(), deadline)
            .expect_err("the engine must stop at its deadline rather than answer");
        let elapsed = started.elapsed();

        // 1.5s stays well under `stop_by`'s 2s floor - a deadline enforced so slowly it would miss
        // this ceiling is already outlived rather than cancelled, which the assertion below catches
        // on its own.
        let ceiling =
            sutura_dev::tolerance::Tolerance::from_env().ceiling(Duration::from_millis(500), Duration::from_millis(1500));
        assert!(elapsed < ceiling, "the call took {elapsed:?} against a 200ms budget");
        assert!(adapter.deadline_exceeded(&error), "{error:?}");
        wait_for_drop(&adapter, &dropped);
        assert!(
            dropped.load(Ordering::SeqCst),
            "dropping the future must cancel the source, not merely stop waiting on it"
        );
        assert!(
            Instant::now() < stop_by,
            "the source would still have rows left when the call returned, so it was cancelled and not outlived"
        );
    }

    /// The same proof on the wide runtime: `enable_time()` is on BOTH constructors, and this is the
    /// cell that would miss a fix applied only to [`DataFusionWarehouse::new`].
    #[test]
    fn a_question_that_exceeds_its_budget_is_stopped_on_the_wide_runtime_too() {
        let stop_by = Instant::now() + Duration::from_secs(2);
        let adapter = DataFusionWarehouse::with_worker_threads(
            crate::source(),
            crate::test_posture(),
            core::num::NonZeroUsize::new(2).expect("two is nonzero"),
            roomy(),
        )
        .expect("a multi-thread runtime builds");
        let dropped = register_never_ending_orders(&adapter, stop_by);

        let query = revenue_by_region();
        let deadline = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_millis(200)).expect("200ms is a budget"),
        );

        let started = Instant::now();
        let error = adapter
            .execute(Executable::Query(&query), &crate::test_leg(), deadline)
            .expect_err("the wide runtime must stop at its deadline too");
        let elapsed = started.elapsed();

        let ceiling =
            sutura_dev::tolerance::Tolerance::from_env().ceiling(Duration::from_millis(500), Duration::from_millis(1500));
        assert!(elapsed < ceiling, "the call took {elapsed:?} against a 200ms budget");
        assert!(adapter.deadline_exceeded(&error), "{error:?}");
        wait_for_drop(&adapter, &dropped);
        assert!(
            dropped.load(Ordering::SeqCst),
            "dropping must cancel the source on this runtime too"
        );
        assert!(
            Instant::now() < stop_by,
            "the source would still have rows left when the call returned, so it was cancelled and not outlived"
        );
    }

    // No `dry_run` variant: this adapter takes the port's default (`lib.rs`'s comment on the
    // omission), which never calls `rows` and is unchanged by this slice - a test pinning it would
    // pass identically before and after, which `just causality` would rightly read as no coverage.

    /// A partition whose `execute` counts how many times `DataFusion` actually started pulling from
    /// it - the proof the review of #680 (finding 2) asked for: a budget already spent must refuse
    /// before the physical plan ever reaches the source, not merely produce the right error by some
    /// other route. The stream it returns is never meant to run, so it borrows [`ForeverRows`]
    /// rather than defining a third one.
    #[derive(Debug)]
    struct CountingPartition {
        schema: SchemaRef,
        calls: Arc<AtomicUsize>,
    }

    impl PartitionStream for CountingPartition {
        fn schema(&self) -> &SchemaRef {
            &self.schema
        }

        fn execute(&self, _ctx: Arc<TaskContext>) -> SendableRecordBatchStream {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(ForeverRows::new(
                Arc::clone(&self.schema),
                Instant::now(),
                Arc::new(AtomicBool::new(false)),
            ))
        }
    }

    /// The variant doc says a spent budget is "found before a call" - this is what proves it rather
    /// than asserting it: `Deadline::opened_at(now - 1s, 200ms)` is already spent the instant
    /// `execute` reads it, so [`CountingPartition::execute`] must never run at all. Removing the
    /// pre-check (`review of #680`'s M4: `remaining_at(..).unwrap_or(Duration::ZERO)` in place of the
    /// early `return`) does not change the ERROR this test sees - `tokio::time::timeout(Duration::ZERO, rows)`
    /// still answers `DeadlineExceeded` on its first poll - so `deadline_exceeded` alone cannot tell
    /// the two shapes apart; only the call count can, and only this cell asks it.
    #[test]
    fn a_spent_deadline_never_lets_the_plan_touch_the_source() {
        let calls = Arc::new(AtomicUsize::new(0));
        let table_schema = schema();
        let table = StreamingTable::try_new(
            Arc::clone(&table_schema),
            vec![Arc::new(CountingPartition {
                schema: Arc::clone(&table_schema),
                calls: Arc::clone(&calls),
            })],
        )
        .expect("a streaming table with matching schema builds");

        let adapter =
            DataFusionWarehouse::new(crate::source(), crate::test_posture(), roomy()).expect("a current-thread runtime builds");
        adapter
            .context
            .register_table("orders", Arc::new(table))
            .expect("a streaming table registers");

        let query = revenue_by_region();
        let spent = Deadline::opened_at(
            Instant::now()
                .checked_sub(Duration::from_secs(1))
                .expect("a test instant is not near the process start"),
            Budget::parse(Duration::from_millis(200)).expect("200ms is a budget"),
        );

        let error = adapter
            .execute(Executable::Query(&query), &crate::test_leg(), spent)
            .expect_err("a deadline already spent must be refused before the plan runs");

        assert!(adapter.deadline_exceeded(&error), "{error:?}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the source's own execute must never run once the deadline is already spent"
        );
    }
}
