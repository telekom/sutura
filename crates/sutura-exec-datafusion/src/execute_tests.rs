//! The adapter's own suite: attaching a file, executing a plan, and the translation helpers.
//!
//! In its own file for the reason `value_mapping_tests.rs` and `width_tests.rs` are: `lib.rs` is at
//! the 1000-line gate, and the gate's answer to that is to split the file rather than to shorten
//! the fix. A pure move - nothing here changed with the split.

use super::collect::cell;
use super::translate::{aggregate_expr, literal, measure_expression, unit};
use super::{DataFusionError, DataFusionWarehouse, column};
use datafusion::arrow::array::{ArrayRef, BooleanArray, Date32Array, Float32Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::prelude::col;
use std::sync::Arc;
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    Executable, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    StatementTables,
};
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
        PlanBucket::new(String::from("period"), Grain::Month, on("order_date")),
        keys,
        measure,
        String::from(label),
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
    vec![PlanKey::new(String::from("region"), on("region"))]
}

#[test]
fn a_day_number_comes_back_as_iso_text_so_the_two_adapters_agree() {
    // The bug: leaving a `Date32` as its day count, or rendering it with `Debug`. The `DuckDB`
    // adapter converts its own day numbers to ISO text through the same calendar, and a
    // differential test compares the two textually - so a bucket that came back as `20605` here
    // and as `2026-06-01` there would fail for a formatting reason and look like a data one.
    let array = Date32Array::from(vec![day("2026-06-01").days_since_epoch()]);
    assert_eq!(
        cell("period", &array, 0).expect("a day number is a date"),
        Value::Text(String::from("2026-06-01"))
    );
    // And a null stays a null rather than becoming the epoch.
    let absent = Date32Array::from(vec![None::<i32>]);
    assert_eq!(cell("period", &absent, 0).expect("a null is a null"), Value::Null);
}

#[test]
fn a_column_type_this_adapter_does_not_map_is_an_error_naming_it() {
    // The bug: a `Debug` fallback. A 32-bit float widened to an `f64` prints as
    // 0.10000000149011612, which would flow into an answer looking like data and make the two
    // adapters disagree about a number neither of them got wrong.
    let array = Float32Array::from(vec![0.1_f32]);
    let error = cell("amount", &array, 0).expect_err("a 32-bit float is not mapped");
    assert!(matches!(error, DataFusionError::UnsupportedType { .. }), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("amount"), "{message}");
    assert!(message.contains("Float32"), "{message}");
}

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
        .execute(Executable::Query(&query), &crate::test_leg())
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
        .execute(Executable::Query(&query), &crate::test_leg())
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
        .execute(Executable::Query(&query), &crate::test_leg())
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
        .execute(Executable::Query(&query), &crate::test_leg())
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
        .runtime
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
        adapter.dry_run(Executable::Query(&query), &crate::test_leg()).is_ok(),
        "the engine answers `would this work` by not asking, so a plan it cannot run still dry-runs clean"
    );

    let error = adapter
        .execute(Executable::Query(&query), &crate::test_leg())
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
            .execute(Executable::Query(&query), &handed)
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
        .execute(Executable::Query(&query), &crate::test_leg())
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
        .execute(Executable::Query(&query), &fabricated)
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
        .execute(Executable::Query(&query), &crate::test_leg())
        .expect_err("the table is not attached");
    assert!(matches!(error, DataFusionError::Analyze { .. }), "{error:?}");
}
