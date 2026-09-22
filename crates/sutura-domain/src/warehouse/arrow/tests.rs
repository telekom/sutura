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

use super::{Accumulating, ResultBatches, ResultBudget, UnannouncedBatch, UnreadableCell, cell, materialised, of_rows};
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

/// The shared table, so `every_mapped_type_passes_the_schema_pass_and_float32_does_not` reads the
/// same rows rather than a second list of its own.
fn mapping_table() -> Vec<Case> {
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
    cases
}

/// Every row of the table, mapped.
#[test]
fn every_type_the_interior_maps_answers_what_the_data_source_answers() {
    for (name, array, expected) in mapping_table() {
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

    let mut accumulating = Accumulating::announcing(announced, 10, roomy());
    let refused = accumulating.push(batch).expect_err("a swapped batch is refused");
    assert!(matches!(refused, UnannouncedBatch::Mislabelled { at: 0, .. }), "{refused:?}");
    // The refusal names both descriptors and no cell value.
    let message = refused.to_string();
    assert!(message.contains("refunds Int64"), "{message}");
    assert!(message.contains("orders Int64"), "{message}");
    assert!(!message.contains('7'), "a refusal must not quote a cell: {message}");
}

/// The OTHER half of the same predicate, and it was pinned by nothing.
///
/// **A measured gap, not a symmetry added for tidiness.** Replacing the whole condition with
/// `announced.data_type() != delivered.data_type()` - dropping the name half - reddens
/// `a_same_typed_column_swap_is_refused_before_a_value_is_read`; replacing it with
/// `announced.name() != delivered.name()` - dropping the TYPE half - left all 447 cells in this
/// crate green. So the name comparison was held and the type comparison was decoration.
///
/// Why it matters more than a swap does, which is the reason the announced type here is a decimal
/// and not simply a second integer: Arrow never sees the ANNOUNCED schema at all -
/// `RecordBatch::try_new` checks a batch's arrays against the schema handed to it, so a batch that
/// declares `Int64` and carries `Int64` is valid however the stream was announced. And
/// [`ResultBatches::to_rows`] type-passes the ANNOUNCED field while `cell` reads the DELIVERED
/// array, so an unrefused substitution of `Int64` for `Decimal128(38, 9)` is answered - as
/// `Value::Integer(1)` under a column the stream promised was a decimal, which is the same number
/// out by a factor of a billion rather than an error anywhere.
#[test]
fn a_column_delivered_under_its_announced_name_with_another_type_is_refused() {
    let announced: SchemaRef = Arc::new(Schema::new(vec![Field::new(
        "average_order",
        DataType::Decimal128(38, 9),
        true,
    )]));
    // Arrow accepts this: the array matches the schema this batch was built under. Same width, same
    // NAME, different type - the mirror of the swap cell above.
    let substituted = RecordBatch::try_new(one_int("average_order"), vec![ints(&[1])])
        .expect("Arrow accepts a batch that matches its own schema, whatever was announced");

    let mut accumulating = Accumulating::announcing(announced, 10, roomy());
    let refused = accumulating
        .push(substituted)
        .expect_err("a column delivered under another type is refused");
    assert!(matches!(refused, UnannouncedBatch::Mislabelled { at: 0, .. }), "{refused:?}");
    // Both descriptors, so an operator can see WHICH half disagreed - and no cell value.
    let message = refused.to_string();
    assert!(message.contains("average_order Decimal128(38, 9)"), "{message}");
    assert!(message.contains("average_order Int64"), "{message}");
    assert!(!message.contains(": 1"), "a refusal must not quote a cell: {message}");
}

/// A narrower batch is refused, and the width check is what stops the name check truncating.
#[test]
fn a_batch_narrower_than_its_schema_is_refused_rather_than_matched_on_its_prefix() {
    let announced: SchemaRef = Arc::new(Schema::new(vec![
        Field::new("orders", DataType::Int64, true),
        Field::new("refunds", DataType::Int64, true),
    ]));
    let narrow = RecordBatch::try_new(one_int("orders"), vec![ints(&[7])]).expect("a one-column batch is valid");
    let refused = Accumulating::announcing(announced, 10, roomy())
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
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), 3, roomy());
    let two = RecordBatch::try_new(Arc::clone(&schema), vec![ints(&[1, 2])]).expect("a two-row batch is valid");
    accumulating.push(two.clone()).expect("two rows are under the ceiling");
    assert_eq!(accumulating.delivered(), 2);
    let refused = accumulating.push(two).expect_err("four rows are over a ceiling of three");
    assert_eq!(refused, UnannouncedBatch::OverBound { most: 3 });
}

/// The byte budget fires at the batch that crosses it, not after the stream is collected.
///
/// **Sized from the fixture's own measured cost rather than from a literal**, so the assertion is
/// that the configured number is what refuses. A magic constant here would pass or fail on Arrow's
/// buffer padding, and the cell would be measuring the allocator.
#[test]
fn the_byte_budget_refuses_the_batch_that_crosses_it() {
    let schema = one_int("orders");
    let two = RecordBatch::try_new(Arc::clone(&schema), vec![ints(&[1, 2])]).expect("a two-row batch is valid");
    // Room for one of these batches and not for two.
    let one_batch = materialised(&two);
    let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(one_batch).expect("a batch costs something"));

    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), usize::MAX, budget);
    accumulating.push(two.clone()).expect("one batch fits its own budget");
    assert_eq!(accumulating.spent_bytes(), one_batch);

    let refused = accumulating.push(two).expect_err("a second batch does not fit");
    assert_eq!(refused, UnannouncedBatch::OverBudget { most_bytes: one_batch });
    // The BUDGET and not the demand: what the question wanted is an observation about one caller's
    // data, and the number an operator can act on is the one they configured.
    assert!(refused.to_string().contains(&one_batch.to_string()), "{refused}");
    // And nothing about the refused batch was retained - the row count is the first one's alone.
    assert_eq!(accumulating.delivered(), 2);
    assert_eq!(accumulating.spent_bytes(), one_batch);
}

/// The control for the cell above: the same two batches under a budget that fits both are kept.
///
/// Without it, that test passes just as well against a guard that refuses every batch.
#[test]
fn two_batches_inside_the_byte_budget_are_both_kept() {
    let schema = one_int("orders");
    let two = RecordBatch::try_new(Arc::clone(&schema), vec![ints(&[1, 2])]).expect("a two-row batch is valid");
    let budget = ResultBudget::of_bytes(
        core::num::NonZeroUsize::new(materialised(&two).saturating_mul(2)).expect("two batches cost something"),
    );
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), usize::MAX, budget);
    accumulating.push(two.clone()).expect("the first batch fits");
    accumulating.push(two).expect("the second batch fits too");
    assert_eq!(accumulating.delivered(), 4);
}

/// The budget charges what `to_rows` will allocate, not only what the batches cost to hold.
///
/// **This is the half round 7 of `telekom/sutura#929`'s review named**, and it is the one a budget
/// is most likely to be written without: `ResultBatches::to_rows` builds a whole second copy as
/// `Vec<Vec<Value>>`, so a bound charged for `get_array_memory_size` alone understates the peak by
/// roughly a factor of two and the process dies inside the conversion of a result the guard had
/// already accepted.
///
/// So the budget here is set to exactly the batch's ARROW size - which
/// [`RecordBatch::get_array_memory_size`] reports and the guard is not allowed to accept - and the
/// assertion is a refusal. A guard charging only the Arrow buffers would accept it and this cell
/// would go green.
#[test]
fn a_batch_whose_arrow_buffers_fit_is_refused_when_its_row_conversion_would_not() {
    let schema = one_int("orders");
    let batch = RecordBatch::try_new(Arc::clone(&schema), vec![ints(&[1, 2, 3, 4])]).expect("a four-row batch is valid");
    let arrow_only = batch.get_array_memory_size();
    assert!(
        materialised(&batch) > arrow_only,
        "the conversion has to cost something, or this cell asserts nothing"
    );
    let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(arrow_only).expect("a batch's buffers cost something"));

    let refused = Accumulating::announcing(schema, usize::MAX, budget)
        .push(batch)
        .expect_err("a batch is refused when holding it AND converting it will not fit");
    assert_eq!(refused, UnannouncedBatch::OverBudget { most_bytes: arrow_only });
}

/// The budget charges the batch's buffers TWICE, and a budget for one copy is still a refusal.
///
/// **Aimed at the factor of two, with an oracle that is not the measure under test.** The cell above
/// refuses on the ARROW size alone, which the per-row term already exceeds, so it stays green against
/// a guard that dropped the doubling - measured by mutation (`arrays.saturating_mul(2)` reduced to
/// `arrays`: 3998 tests, all passed). A first attempt at this cell sized its budget from
/// `materialised` itself and was green under the same mutation for the obvious reason: a cell whose
/// expectation is computed by the function it is testing cannot see that function change.
///
/// So the budget is written out here instead - what a guard charging the buffers ONCE would compute:
/// hold the batch, plus one `Vec` per row and one [`Value`] per cell for the conversion. The property
/// that makes it a refusal is that `ResultBatches::to_rows` clones an owned value out of every array,
/// so the buffers are paid for twice, and a budget covering them once is not a budget for both.
#[test]
fn a_budget_that_charges_the_buffers_only_once_is_refused() {
    let schema = one_int("orders");
    let values: Vec<i64> = (0..64).collect();
    let batch = RecordBatch::try_new(Arc::clone(&schema), vec![ints(&values)]).expect("a sixty-four-row batch is valid");
    let charged_once = batch.get_array_memory_size()
        + batch.num_rows() * (core::mem::size_of::<Vec<Value>>() + batch.num_columns() * core::mem::size_of::<Value>());
    let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(charged_once).expect("the cost is positive"));

    let refused = Accumulating::announcing(schema, usize::MAX, budget)
        .push(batch)
        .expect_err("a budget that pays for the buffers once does not cover both copies");
    assert_eq!(
        refused,
        UnannouncedBatch::OverBudget {
            most_bytes: charged_once
        }
    );
}

/// The budget charges the row structure, which is the term a TALL result is mostly made of.
///
/// **The third term, aimed separately from the doubling above.** `Vec<Vec<Value>>` is one `Vec` per
/// row plus one [`Value`] per cell whatever the cells hold, so a thousand narrow rows cost far more
/// to convert than their Arrow buffers do to hold - which is the same *a row count is not a width*
/// argument `docs/adr/0009` retired the per-leg row cap over, seen from the memory side.
///
/// So the budget here is twice the batch's Arrow size: enough for the batch and for an owned copy of
/// every value in it, and not enough for the row vectors. A guard charging only `arrays * 2` would
/// accept it.
#[test]
fn a_tall_narrow_batch_is_refused_when_its_row_structure_would_not_fit() {
    let schema = one_int("orders");
    let values: Vec<i64> = (0..1000).collect();
    let tall = RecordBatch::try_new(Arc::clone(&schema), vec![ints(&values)]).expect("a thousand-row batch is valid");
    let both_copies = tall.get_array_memory_size().saturating_mul(2);
    assert!(
        materialised(&tall) > both_copies,
        "the row structure has to cost something, or this cell asserts nothing"
    );
    let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(both_copies).expect("a batch's buffers cost something"));

    let refused = Accumulating::announcing(schema, usize::MAX, budget)
        .push(tall)
        .expect_err("a thousand narrow rows do not fit twice their own buffers");
    assert_eq!(refused, UnannouncedBatch::OverBudget { most_bytes: both_copies });
}

/// A batch that matches its announced schema by name and type is kept, and reads back as rows.
#[test]
fn an_announced_batch_reads_back_under_the_names_it_was_announced_with() {
    let schema = one_int("orders");
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), 10, roomy());
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

/// THE HOLE THE TYPE PASS CLOSES, in both shapes it was reachable in.
///
/// `cell` answers a null before it reads the column's type. So a result with NO rows never reaches
/// it, and a column that is entirely null reaches it and is answered - which made whether this
/// workspace maps a type depend on what the data happened to be. A `TIMESTAMP` column came back as
/// a successful empty result. Both shapes are asserted because they were reachable for two
/// different reasons.
#[test]
fn an_unmapped_column_is_refused_whatever_the_data_happened_to_be() {
    let unmapped: SchemaRef = Arc::new(Schema::new(vec![Field::new("at", DataType::Float32, true)]));

    let no_rows = ResultBatches::none_under(Arc::clone(&unmapped));
    match no_rows.to_rows().expect_err("a zero-row unmapped schema is refused") {
        UnreadableCell::UnsupportedType {
            ref column,
            ref arrow_type,
        } => {
            assert_eq!(column, "at");
            assert!(arrow_type.contains("Float32"), "{arrow_type}");
        }
        other => panic!("a zero-row unmapped schema was mapped to {other:?}"),
    }

    let mut accumulating = Accumulating::announcing(Arc::clone(&unmapped), 10, roomy());
    let all_null: ArrayRef = Arc::new(Float32Array::from(vec![None::<f32>, None::<f32>]));
    accumulating
        .push(RecordBatch::try_new(unmapped, vec![all_null]).expect("a null column is a valid batch"))
        .expect("the batch carries the announced field");
    assert!(
        matches!(accumulating.finish().to_rows(), Err(UnreadableCell::UnsupportedType { .. })),
        "an all-null unmapped column was mapped"
    );
}

/// The schema pass and the value mapping agree, read off the mapping table above rather than a
/// second list.
///
/// Two matches over one set of Arrow types is a drift risk, and this is what holds them together.
/// Both drift directions fail CLOSED - a type only `cell` reads is refused at the pass, a type only
/// the pass names is refused per cell - so the cost of a drift is a refused column and never a
/// value.
#[test]
fn every_mapped_type_passes_the_schema_pass_and_float32_does_not() {
    for (name, array, _expected) in mapping_table() {
        let schema: SchemaRef = Arc::new(Schema::new(vec![Field::new(name, array.data_type().clone(), true)]));
        let mut accumulating = Accumulating::announcing(Arc::clone(&schema), 10, roomy());
        accumulating
            .push(RecordBatch::try_new(schema, vec![Arc::clone(&array)]).expect("a one-column batch"))
            .expect("the batch carries the announced field");
        assert!(
            accumulating.finish().to_rows().is_ok(),
            "{name} is in the mapping table, so the schema pass must accept it"
        );
    }
    let refused: SchemaRef = Arc::new(Schema::new(vec![Field::new("amount", DataType::Float32, true)]));
    assert!(
        ResultBatches::none_under(refused).to_rows().is_err(),
        "Float32 is not mapped, so the schema pass must refuse it"
    );
}

/// A materialisation budget no fixture in this file comes near, so a cell aimed at the schema guard
/// or at the mapping table cannot be refused by the byte budget instead.
///
/// The budget's own cells build their own, at the size the assertion is about.
const fn roomy() -> ResultBudget {
    ResultBudget::of_bytes(core::num::NonZeroUsize::MAX)
}
