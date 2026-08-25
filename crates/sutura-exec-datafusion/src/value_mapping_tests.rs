//! The value mapping, as a table, held against the other implementor of the [`Warehouse`] port.
//!
//! **The table is duplicated on purpose, and the duplication is the test.** The two `cell` functions
//! live in different crates - this one takes an Arrow array and the data source's takes a
//! `duckdb::types::Value` - and neither crate may depend on the other: they are two implementors of
//! one port, and a shared helper would have to live in `sutura-domain`, which is not allowed to know
//! that either of them exists. So the agreement is asserted as the same expected column written out
//! in both places: here, and in `every_type_this_adapter_maps_answers_what_the_engine_answers` in
//! `crates/sutura-exec-duckdb/src/lib.rs`. The two test names quote each other, so a change made to
//! one and not the other shows up as a failing assertion rather than as a disagreement nobody
//! notices until an anchor stops reproducing.
//!
//! The alternative - one test calling both adapters - already exists as `tests/differential.rs` in
//! `sutura-app`, which runs one plan through both and compares the rows. It can only see the types a
//! fixture CSV produces, which is why this table is not redundant with it: it reaches the widths a
//! Parquet file has and a CSV never will.
//!
//! [`Warehouse`]: sutura_domain::warehouse::Warehouse

use datafusion::arrow::array::{
    ArrayRef, BooleanArray, Date32Array, Decimal128Array, Float32Array, Float64Array, Int8Array, Int16Array, Int32Array,
    Int64Array, StringArray, StringViewArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use std::sync::Arc;
use sutura_domain::calendar::Date;
use sutura_domain::warehouse::Value;

use super::{DataFusionError, cell};

/// One row of the shared table: what the value is called, the array the engine would hand over, and
/// the domain value both adapters have to produce for it. Named because the tuple is over the
/// `type_complexity` threshold this workspace tightened.
type Case = (&'static str, ArrayRef, Value);

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

/// A `DECIMAL(9, 2)` holding 123.45, which is the scaled payload 12345.
fn decimal() -> ArrayRef {
    Arc::new(
        Decimal128Array::from(vec![12_345_i128])
            .with_precision_and_scale(9, 2)
            .expect("a test decimal has a width and a scale"),
    )
}

#[test]
fn every_type_the_engine_maps_answers_what_the_data_source_answers() {
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
        ("Float64", Arc::new(Float64Array::from(vec![0.1_f64])), Value::Real(0.1)),
        (
            "Decimal128 stays text so it stays exact",
            decimal(),
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
fn a_32_bit_float_is_refused_on_both_sides_of_the_port() {
    // The data source widened this one to an `f64` and this engine refused it, which is one plan
    // answering 0.10000000149011612 through one adapter and erroring through the other. Refusing is
    // the half of that disagreement that can be fixed without inventing a rendering: there is no
    // `f64` that is `0.1_f32`. `a_32_bit_float_is_refused_here_because_it_is_refused_there` in
    // `crates/sutura-exec-duckdb/src/lib.rs` is the same assertion against the data source.
    let array = Float32Array::from(vec![0.1_f32]);
    let error = cell("amount", &array, 0).expect_err("a 32-bit float is not mapped");
    assert!(matches!(error, DataFusionError::UnsupportedType { .. }), "{error:?}");
    // And the width that does answer, so this is not a test that would pass with every float
    // refused.
    let wide = Float64Array::from(vec![0.1_f64]);
    assert_eq!(cell("amount", &wide, 0).expect("a 64-bit float is mapped"), Value::Real(0.1));
}
