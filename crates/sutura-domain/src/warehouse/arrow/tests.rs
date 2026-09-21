//! The schema guard, and the value mapping as a table.
//!
//! # The mapping table is still duplicated once, and the duplication is still the test
//!
//! It used to be duplicated because it had to be: the two `cell` functions lived in two adapter
//! crates, neither may depend on the other, and a shared helper would have had to live here - which
//! this crate was not allowed to name an Arrow type in. `docs/adr/0039` reverses exactly that, so
//! the Arrow half of the table is now HERE and the engine has no `cell` of its own.
//!
//! What survives is the ONE-sided duplication against a source that does not speak Arrow:
//! `every_type_this_adapter_maps_answers_what_the_engine_answers` in
//! `crates/sutura-exec-duckdb/src/lib.rs` maps a `duckdb::types::Value`, and that crate still may
//! not name an Arrow type - the `duckdb` crate declares `arrow ^58` while this major is 59
//! (`devco/arrow-majors-allow`). So the agreement between those two is asserted as the same
//! expected column written out in both places, and the two test names quote each other, so a change
//! made to one and not the other shows up as a failing assertion rather than as a disagreement
//! nobody notices until an anchor stops reproducing.
//!
//! The alternative - one test calling both adapters - already exists as `tests/differential.rs` in
//! `sutura-app`, which runs one plan through both and compares the rows. It can only see the types a
//! fixture CSV produces, which is why this table is not redundant with it: it reaches the widths a
//! Parquet file has and a CSV never will.

use std::sync::Arc;

use arrow_array::{
    ArrayRef, BooleanArray, Date32Array, Decimal128Array, Decimal256Array, Float32Array, Float64Array, Int8Array, Int16Array,
    Int32Array, Int64Array, RecordBatch, StringArray, StringViewArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use arrow_buffer::i256;
use arrow_schema::{DataType, Field, Schema, SchemaRef};

use super::{Accumulating, UnannouncedBatch, UnreadableCell, cell, of_rows};
use crate::calendar::Date;
use crate::warehouse::{Real, Value};

/// One row of the shared table: what the value is called, the array the engine would hand over, and
/// the domain value both adapters have to produce for it. Named because the tuple is over the
/// `type_complexity` threshold this workspace tightened.
type Case = (&'static str, ArrayRef, Value);

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

fn real(value: f64) -> Real {
    Real::parse(value).expect("a test literal is finite")
}

/// A `DECIMAL(9, 2)` holding 123.45, which is the scaled payload 12345.
fn decimal() -> ArrayRef {
    Arc::new(
        Decimal128Array::from(vec![12_345_i128])
            .with_precision_and_scale(9, 2)
            .expect("a test decimal has a width and a scale"),
    )
}

fn whole_decimal(value: i128) -> ArrayRef {
    Arc::new(
        Decimal128Array::from(vec![value])
            .with_precision_and_scale(38, 0)
            .expect("a test whole decimal has a width and no scale"),
    )
}

fn wide_decimal(value: i128, scale: i8) -> ArrayRef {
    Arc::new(
        Decimal256Array::from(vec![i256::from_i128(value)])
            .with_precision_and_scale(38, scale)
            .expect("a test wide decimal has a width and scale"),
    )
}

#[test]
fn every_type_the_interior_maps_answers_what_the_data_source_answers() {
    // The twin of `every_type_this_adapter_maps_answers_what_the_engine_answers` in
    // `crates/sutura-exec-duckdb/src/lib.rs`. Same logical values, same expected column, one row per
    // width - because a Parquet `INT32` column under a `min` or a `max` answered there and errored
    // here, which is one plan with two outcomes depending on which adapter ran it.
    // Boundary values rather than round ones, written in hex where the decimal form is a bit pattern
    // nobody reads: an arm that downcast to the wrong width would come back truncated or sign-flipped,
    // and 42 would survive that.
    let cases: Vec<Case> = vec![
        ("BOOLEAN true", Arc::new(BooleanArray::from(vec![true])), Value::Integer(1)),
        ("BOOLEAN false", Arc::new(BooleanArray::from(vec![false])), Value::Integer(0)),
        ("Int8", Arc::new(Int8Array::from(vec![i8::MIN])), Value::Integer(-128)),
        ("Int16", Arc::new(Int16Array::from(vec![i16::MIN])), Value::Integer(-0x8000)),
        (
            "Int32",
            Arc::new(Int32Array::from(vec![i32::MAX])),
            Value::Integer(0x7FFF_FFFF),
        ),
        ("Int64", Arc::new(Int64Array::from(vec![i64::MIN])), Value::Integer(i64::MIN)),
        ("UInt8", Arc::new(UInt8Array::from(vec![u8::MAX])), Value::Integer(255)),
        ("UInt16", Arc::new(UInt16Array::from(vec![u16::MAX])), Value::Integer(0xFFFF)),
        (
            "UInt32",
            Arc::new(UInt32Array::from(vec![u32::MAX])),
            Value::Integer(0xFFFF_FFFF),
        ),
        (
            "UInt64 that fits an i64",
            Arc::new(UInt64Array::from(vec![42_u64])),
            Value::Integer(42),
        ),
        (
            "UInt64 that does not",
            Arc::new(UInt64Array::from(vec![u64::MAX])),
            Value::Text(String::from("18446744073709551615")),
        ),
        ("Float64", Arc::new(Float64Array::from(vec![0.1_f64])), Value::Real(real(0.1))),
        // Zero is finite, and it is here because the check that refuses `inf` is a check about a
        // division BY zero: a metric that legitimately answers zero must still answer.
        (
            "Float64 zero",
            Arc::new(Float64Array::from(vec![0.0_f64])),
            Value::Real(real(0.0)),
        ),
        (
            "Decimal128 stays text so it stays exact",
            decimal(),
            Value::Text(String::from("123.45")),
        ),
        ("whole Decimal128 fitting i64", whole_decimal(42), Value::Integer(42)),
        (
            "whole Decimal128 past i64",
            whole_decimal(i128::from(i64::MAX) + 1),
            Value::Text(String::from("9223372036854775808")),
        ),
        ("whole Decimal256 fitting i64", wide_decimal(42, 0), Value::Integer(42)),
        (
            "whole Decimal256 past i64",
            wide_decimal(i128::from(i64::MAX) + 1, 0),
            Value::Text(String::from("9223372036854775808")),
        ),
        (
            "fractional Decimal256 stays text so it stays exact",
            wide_decimal(12_345, 2),
            Value::Text(String::from("123.45")),
        ),
        (
            "Utf8",
            Arc::new(StringArray::from(vec!["north"])),
            Value::Text(String::from("north")),
        ),
        (
            "Utf8View, which is what Parquet gives",
            Arc::new(StringViewArray::from(vec!["north"])),
            Value::Text(String::from("north")),
        ),
        (
            "Date32 as ISO text",
            Arc::new(Date32Array::from(vec![day("2026-06-01").days_since_epoch()])),
            Value::Text(String::from("2026-06-01")),
        ),
    ];
    for (name, array, expected) in cases {
        assert_eq!(cell(name, array.as_ref(), 0).expect(name), expected, "{name}");
    }
}

#[test]
fn a_null_of_any_mapped_type_is_null_and_not_the_zero_of_that_type() {
    // The null check is before the match, so this holds for every arm at once - and a regression
    // that moved it into the arms would show up here as `Integer(0)` rather than as `Null`.
    let arrays: Vec<(&str, ArrayRef)> = vec![
        ("Int32", Arc::new(Int32Array::from(vec![None::<i32>]))),
        ("UInt64", Arc::new(UInt64Array::from(vec![None::<u64>]))),
        ("Date32", Arc::new(Date32Array::from(vec![None::<i32>]))),
        ("Boolean", Arc::new(BooleanArray::from(vec![None::<bool>]))),
    ];
    for (name, array) in arrays {
        assert_eq!(cell(name, array.as_ref(), 0).expect(name), Value::Null, "{name}");
    }
}

#[test]
fn a_32_bit_float_is_refused_here_because_the_data_source_refuses_it() {
    // The data source widened this one to an `f64` and this engine refused it, which is one plan
    // answering 0.10000000149011612 through one adapter and erroring through the other. Refusing is
    // the half of that disagreement that can be fixed without inventing a rendering: there is no
    // `f64` that is `0.1_f32`. `a_32_bit_float_is_refused_here_because_it_is_refused_there` in
    // `crates/sutura-exec-duckdb/src/lib.rs` is the same assertion against the data source.
    let array = Float32Array::from(vec![0.1_f32]);
    let error = cell("amount", &array, 0).expect_err("a 32-bit float is not mapped");
    assert!(matches!(error, UnreadableCell::UnsupportedType { .. }), "{error:?}");
    // And the width that does answer, so this is not a test that would pass with every float
    // refused.
    let wide = Float64Array::from(vec![0.1_f64]);
    assert_eq!(
        cell("amount", &wide, 0).expect("a 64-bit float is mapped"),
        Value::Real(real(0.1))
    );
}

#[test]
fn a_non_finite_double_is_refused_here_because_the_data_source_refuses_it() {
    // THE FINDING THIS ARM EXISTS FOR, and the twin of
    // `a_non_finite_double_is_refused_here_because_it_is_refused_there` in
    // `crates/sutura-exec-duckdb/src/lib.rs`. This arm was `Value::Real(..)` on a raw `f64`, and the
    // value that reached it was real: a ratio measure declaring `zero_denominator: fails` is
    // translated as an unguarded division with the numerator cast to `Float64`, and IEEE float
    // division by zero answers `inf` rather than failing. So the metric answered the string "inf"
    // under its own certified name, the data source answered the same string, and the differential
    // test therefore agreed and passed.
    //
    // All three of the class, not just the one a zero denominator produces first: a guard on the
    // division would have left `-inf` and `NaN` on the way in.
    for (name, raw) in [
        ("positive infinity", f64::INFINITY),
        ("negative infinity", f64::NEG_INFINITY),
        ("not a number", f64::NAN),
    ] {
        let array = Float64Array::from(vec![raw]);
        let error = cell("revenue_per_refunded_order", &array, 0).expect_err(name);
        assert!(matches!(error, UnreadableCell::NotFinite { .. }), "{name}: {error:?}");
        assert_eq!(
            error.to_string(),
            "column revenue_per_refunded_order came back as a value that is not a finite number",
            "{name}"
        );
    }
    // And the values that DO answer, so this is not a test that would pass with every double
    // refused - zero included, because the check is about dividing BY zero and not about it.
    let zero = Float64Array::from(vec![0.0_f64]);
    assert_eq!(
        cell("revenue", &zero, 0).expect("zero is a finite number"),
        Value::Real(real(0.0))
    );
    let average = Float64Array::from(vec![63_335.777_777_777_78_f64]);
    assert_eq!(
        cell("average_order", &average, 0).expect("an average is a finite number"),
        Value::Real(real(63_335.777_777_777_78))
    );
}

/// A one-column Int64 schema under `label`.
fn one_int(label: &str) -> SchemaRef {
    Arc::new(Schema::new(vec![Field::new(label, DataType::Int64, true)]))
}

fn ints(values: &[i64]) -> ArrayRef {
    Arc::new(Int64Array::from(values.to_vec()))
}

/// THE DEFECT THIS GUARD EXISTS FOR: two SAME-TYPED columns delivered in the wrong order.
///
/// A differently-typed swap Arrow already refuses through `RecordBatch::try_new`, so a test that
/// swapped an integer for a string would prove nothing about this code. Two `Int64` columns build a
/// perfectly valid batch under either schema, and a consumer that only counted columns would label
/// each column with the other's name - a transposed answer under a certified metric name, with no
/// error anywhere.
#[test]
fn a_same_typed_column_swap_is_refused_before_a_value_is_read() {
    let announced: SchemaRef = Arc::new(Schema::new(vec![
        Field::new("orders", DataType::Int64, true),
        Field::new("refunds", DataType::Int64, true),
    ]));
    // Arrow accepts this: same width, same types, different NAMES.
    let swapped: SchemaRef = Arc::new(Schema::new(vec![
        Field::new("refunds", DataType::Int64, true),
        Field::new("orders", DataType::Int64, true),
    ]));
    let batch = RecordBatch::try_new(Arc::clone(&swapped), vec![ints(&[7]), ints(&[2])])
        .expect("Arrow accepts a same-typed swap, which is the whole point");

    let mut accumulating = Accumulating::announcing(announced, 10);
    let refused = accumulating.push(batch).expect_err("a swapped batch is refused");
    assert!(matches!(refused, UnannouncedBatch::Mislabelled { at: 0, .. }), "{refused:?}");
    // The refusal names both descriptors and no cell value.
    let message = refused.to_string();
    assert!(message.contains("refunds Int64"), "{message}");
    assert!(message.contains("orders Int64"), "{message}");
    assert!(!message.contains('7'), "a refusal must not quote a cell: {message}");
}

/// A narrower batch is refused, and the width check is what stops the name check truncating.
#[test]
fn a_batch_narrower_than_its_schema_is_refused_rather_than_matched_on_its_prefix() {
    let announced: SchemaRef = Arc::new(Schema::new(vec![
        Field::new("orders", DataType::Int64, true),
        Field::new("refunds", DataType::Int64, true),
    ]));
    let narrow = RecordBatch::try_new(one_int("orders"), vec![ints(&[7])]).expect("a one-column batch is valid");
    let refused = Accumulating::announcing(announced, 10)
        .push(narrow)
        .expect_err("a narrow batch is refused");
    assert_eq!(
        refused,
        UnannouncedBatch::Width {
            announced: 2,
            delivered: 1
        }
    );
}

/// The ceiling fires at the batch that crosses it, not after the stream is collected.
#[test]
fn the_row_ceiling_refuses_the_batch_that_crosses_it() {
    let schema = one_int("orders");
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), 3);
    let two = RecordBatch::try_new(Arc::clone(&schema), vec![ints(&[1, 2])]).expect("a two-row batch is valid");
    accumulating.push(two.clone()).expect("two rows are under the ceiling");
    assert_eq!(accumulating.delivered(), 2);
    let refused = accumulating.push(two).expect_err("four rows are over a ceiling of three");
    assert_eq!(refused, UnannouncedBatch::OverBound { most: 3 });
}

/// A batch that matches its announced schema by name and type is kept, and reads back as rows.
#[test]
fn an_announced_batch_reads_back_under_the_names_it_was_announced_with() {
    let schema = one_int("orders");
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), 10);
    accumulating
        .push(RecordBatch::try_new(schema, vec![ints(&[7, 2])]).expect("a valid batch"))
        .expect("an announced batch is kept");
    let result = accumulating.finish();
    assert_eq!(result.rows(), 2);
    let rows = result.to_rows().expect("Int64 maps");
    assert_eq!(rows.columns(), ["orders"]);
    assert_eq!(rows.rows(), [vec![Value::Integer(7)], vec![Value::Integer(2)]]);
}

/// Rows in, the same rows out - for the two column shapes a source actually produces.
///
/// **A MIXED column is deliberately NOT asserted to round-trip**, because it does not: `of_rows`
/// falls back to `Utf8` and renders each value, so an `Integer` in a column that also holds `Text`
/// comes back as `Text`. That is the stated limit on `arrow_column`, and no data system produces such
/// a column - a source declares a column's type.
#[test]
fn a_typed_column_round_trips_through_arrow_and_a_mixed_one_becomes_text() {
    let columns = vec![String::from("region"), String::from("orders"), String::from("rate")];
    let rows = vec![
        vec![Value::Text(String::from("north")), Value::Integer(7), Value::Real(real(0.25))],
        vec![Value::Text(String::from("south")), Value::Null, Value::Null],
    ];
    let result = of_rows(&columns, &rows).expect("a rectangular fixture builds");
    let back = result.to_rows().expect("every built column maps back");
    assert_eq!(back.columns(), columns.as_slice());
    assert_eq!(back.rows(), rows.as_slice());

    let mixed = of_rows(
        &[String::from("amount")],
        &[vec![Value::Integer(42)], vec![Value::Text(String::from("12.34"))]],
    )
    .expect("a mixed fixture builds");
    assert_eq!(
        mixed.to_rows().expect("a text column maps back").rows(),
        [
            vec![Value::Text(String::from("42"))],
            vec![Value::Text(String::from("12.34"))]
        ],
        "a mixed column is text, which is the stated limit"
    );
}

/// A ragged fixture is refused here rather than as an Arrow error nobody reads.
#[test]
fn a_ragged_fixture_is_refused_with_the_row_that_is_wrong() {
    let refused = of_rows(
        &[String::from("a"), String::from("b")],
        &[vec![Value::Integer(1), Value::Integer(2)], vec![Value::Integer(3)]],
    )
    .expect_err("a ragged fixture is refused");
    assert_eq!(
        refused,
        crate::warehouse::MalformedRowSet::RowWidth {
            row: 1,
            cells: 1,
            columns: 2
        }
    );
}
