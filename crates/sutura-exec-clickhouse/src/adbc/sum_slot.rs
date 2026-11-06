//! How a generated sum's tuple reads once the driver has answered: the batch `normalize_batch` hands
//! `Accumulating`, built here as the driver types it, so no server is needed.

use std::sync::Arc;

use arrow_array::{ArrayRef, Decimal256Array, Float64Array, RecordBatch, StringArray, StructArray};
use arrow_buffer::i256;
use arrow_schema::{DataType, Field};
use sutura_domain::warehouse::{Accumulating, ResultBudget, Value};

use super::{AdbcError, normalize_batch};

/// `total` at `scale` fractional digits, widened to the decimal slot's 38 as the server does.
fn widened(total: i128, scale: u32) -> i256 {
    i256::from_i128(10)
        .checked_pow(38 - scale)
        .and_then(|factor| i256::from_i128(total).checked_mul(factor))
        .expect("a test total fits the slot")
}

/// One `total` column as the generated sum arrives under `type_name`: its totals in the
/// `Decimal(76, 38)` slot, the integer and float slots `NULL`.
fn decimal_sum(type_name: &str, totals: Vec<Option<i256>>) -> RecordBatch {
    let rows = totals.len();
    let slot = |values: Vec<Option<i256>>, scale| -> ArrayRef {
        Arc::new(
            Decimal256Array::from(values)
                .with_precision_and_scale(76, scale)
                .expect("a slot type is a Decimal256"),
        )
    };
    let tuple = StructArray::from(vec![
        (
            Arc::new(Field::new("name", DataType::Utf8, false)),
            Arc::new(StringArray::from(vec![type_name; rows])) as ArrayRef,
        ),
        (
            Arc::new(Field::new("integer", DataType::Decimal256(76, 0), true)),
            slot(vec![None; rows], 0),
        ),
        (
            Arc::new(Field::new("decimal", DataType::Decimal256(76, 38), true)),
            slot(totals, 38),
        ),
        (
            Arc::new(Field::new("float", DataType::Float64, true)),
            Arc::new(Float64Array::from(vec![None; rows])) as ArrayRef,
        ),
    ]);
    RecordBatch::try_from_iter([("total", Arc::new(tuple) as ArrayRef)]).expect("one column is a batch")
}

/// The domain values `batch` answers, read the way `execute_to_completion` reads it.
fn answered(batch: &RecordBatch) -> Vec<Value> {
    let batch = normalize_batch(batch).expect("the tuple reads");
    let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(1 << 20).expect("a test budget is positive"));
    let mut accumulating = Accumulating::announcing(batch.schema(), 10, budget);
    accumulating.push(batch).expect("the batch is the announced one");
    let rows = accumulating.finish().to_rows().expect("the cells read");
    rows.rows().iter().map(|row| row[0].clone()).collect()
}

#[test]
fn a_decimal_total_answers_at_its_columns_scale() {
    let at_two = decimal_sum(
        "Nullable(Decimal(38, 2))",
        vec![Some(widened(1150, 2)), Some(widened(-7, 2)), None],
    );
    assert_eq!(
        answered(&at_two),
        [
            Value::Text(String::from("11.50")),
            Value::Text(String::from("-0.07")),
            Value::Null
        ]
    );
    let whole = decimal_sum("Decimal(38, 0)", vec![Some(widened(3, 0))]);
    assert_eq!(answered(&whole), [Value::Integer(3)]);
}

#[test]
fn a_decimal_total_with_a_digit_past_its_columns_scale_is_refused() {
    let past = widened(1150, 2)
        .checked_add(i256::from_i128(1))
        .expect("one more fits the slot");
    let refused = normalize_batch(&decimal_sum("Decimal(38, 2)", vec![Some(past)])).expect_err("a digit would be dropped");
    assert!(
        matches!(&refused, AdbcError::DecimalTotalPastScale { column, scale: 2 } if column == "total"),
        "{refused}"
    );
}
