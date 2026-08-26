//! The result side of this adapter: what the engine hands back, and what a domain row is.
//!
//! # The seam
//!
//! [`crate::translate`] is the half that never reads a result - a plan becomes expressions there,
//! and nothing in it has seen a row. This is the mirror half: an Arrow schema is checked against the
//! plan's labels, and Arrow arrays become `sutura_domain::warehouse::Value`. Between the two,
//! `lib.rs` keeps what neither of them is about - the session, the runtime, attaching a file, and
//! executing.
//!
//! It is its own file for the reason `translate.rs` is: `lib.rs` is at the 1000-line gate, and the
//! gate's answer to that is to split the file rather than to shorten the change. The seam is the one
//! `lib.rs` already named in prose before there was a file on either side of it.
//!
//! # Why an unmapped type is an error here rather than a fallback
//!
//! Because this is the last place a value can be wrong without anybody noticing. A `Debug` rendering
//! of a nested type would flow into an answer looking like data, and an anchor comparison against it
//! would pass or fail for reasons nobody could read. The `DuckDB` adapter refuses the same set for
//! the same reason, and `value_mapping_tests.rs` is the table both are held to.

use datafusion::arrow::array::{
    Array, BooleanArray, Date32Array, Decimal128Array, Float64Array, Int8Array, Int16Array, Int32Array, Int64Array, StringArray,
    StringViewArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, DFSchema};
use datafusion::logical_expr::Expr;
use sutura_domain::calendar::Date;
use sutura_domain::warehouse::{Real, Value};

use crate::DataFusionError;

/// The aliased projection, and the expressions to order the result by.
///
/// Named rather than written out, because the two lists are produced together and consumed one line
/// apart: separating them into two passes over the same schema is how they would come to disagree
/// about which position is a grouped one.
pub(crate) type Projected = (Vec<Expr>, Vec<Expr>);

/// The final projection, and the expressions to order by.
///
/// The projection references the aggregate's own output fields rather than re-stating the grouped
/// expressions, because after aggregating there is no `orders.order_date` left to truncate - the
/// truncated value *is* a field. Each is aliased so the result labels are exactly
/// `QueryPlan::result_labels` in order: the keys, then the time bucket, then the measure.
///
/// Ordering is by the projected label columns, unaliased, for the grouped positions only. That is
/// the SQL path's "order by what it grouped by", and it is what makes two runs of one question
/// return rows in one order - which a differential test over row order depends on.
pub(crate) fn outputs(schema: &DFSchema, labels: &[String], group_count: usize) -> Result<Projected, DataFusionError> {
    if schema.fields().len() != labels.len() {
        return Err(DataFusionError::SchemaMismatch {
            expected: labels.to_vec(),
            actual: schema.fields().iter().map(|f| String::from(f.name().as_str())).collect(),
        });
    }
    let mut projection = Vec::with_capacity(labels.len());
    let mut ordering = Vec::with_capacity(group_count);
    for (index, ((qualifier, field), label)) in schema.iter().zip(labels.iter()).enumerate() {
        let reference = Expr::Column(Column::new(qualifier.cloned(), field.name().as_str()));
        projection.push(reference.alias(label.as_str()));
        if index < group_count {
            // Unqualified: an aliased projection field carries no qualifier, so this is the name the
            // sort resolves against.
            ordering.push(Expr::Column(Column::new_unqualified(label.as_str())));
        }
    }
    Ok((projection, ordering))
}

/// One array, downcast to the type its schema declares.
fn typed<'array, A>(label: &str, array: &'array dyn Array) -> Result<&'array A, DataFusionError>
where
    A: 'static,
{
    array.as_any().downcast_ref::<A>().ok_or_else(|| DataFusionError::Downcast {
        column: String::from(label),
        arrow_type: core::any::type_name::<A>(),
    })
}

/// One cell, as a domain value.
///
/// **One mapping, two adapters, and the set is decided once.** This engine and the `DuckDB` data
/// source are the two implementors of the [`Warehouse`] port, so a type that answers there and
/// errors here means an anchor certified against one adapter does not reproduce against the other.
/// `cell` in `crates/sutura-exec-duckdb/src/lib.rs` is the other half, and
/// `value_mapping_tests.rs` beside this file is the table both are held to.
///
/// Every integer width answers, because every one of them fits an `i64` losslessly - a Parquet
/// `INT32` column under a `min` or a `max` used to answer through the data source and error here. A
/// 64-bit unsigned value that does not fit is rendered as text rather than wrapped: a silently
/// truncated total is a wrong number.
///
/// An unmapped type is [`DataFusionError::UnsupportedType`] naming the column and the Arrow type
/// rather than a `Debug` rendering. Widening a 32-bit float to an `f64` would be the tempting one
/// and is exactly wrong: `0.1_f32` as an `f64` prints as `0.10000000149011612`, and the two adapters
/// would then disagree about a number neither of them got wrong.
///
/// A 64-bit float that is not finite is [`DataFusionError::NotFinite`], for the same reason a
/// nested type is an error: `inf` is what an unguarded division answers, and rendering it puts the
/// string `"inf"` in an answer under a metric's own certified name.
///
/// `Date64` is deliberately NOT here, and not for symmetry's sake either: the data source has no
/// counterpart to be symmetric with - `DuckDB`'s `DATE` is a day count - and nothing on this path
/// produces one, because `date_trunc` over a `Date32` stays a `Date32` and a Parquet `DATE` logical
/// type reads as `Date32`. Mapping it would mean choosing what a millisecond count that is not a
/// whole number of days means, in an arm no question can reach. An unreachable arm holding a
/// semantic choice nobody reviewed is worse than an error naming the type.
pub(crate) fn cell(label: &str, array: &dyn Array, row: usize) -> Result<Value, DataFusionError> {
    if array.is_null(row) {
        return Ok(Value::Null);
    }
    match *array.data_type() {
        DataType::Int64 => Ok(Value::Integer(typed::<Int64Array>(label, array)?.value(row))),
        // Every narrower width, because `i64::from` is lossless for all of them. Written out rather
        // than reached through a cast so that the conversion is the compiler's business.
        DataType::Int8 => Ok(Value::Integer(i64::from(typed::<Int8Array>(label, array)?.value(row)))),
        DataType::Int16 => Ok(Value::Integer(i64::from(typed::<Int16Array>(label, array)?.value(row)))),
        DataType::Int32 => Ok(Value::Integer(i64::from(typed::<Int32Array>(label, array)?.value(row)))),
        DataType::UInt8 => Ok(Value::Integer(i64::from(typed::<UInt8Array>(label, array)?.value(row)))),
        DataType::UInt16 => Ok(Value::Integer(i64::from(typed::<UInt16Array>(label, array)?.value(row)))),
        DataType::UInt32 => Ok(Value::Integer(i64::from(typed::<UInt32Array>(label, array)?.value(row)))),
        // The one width that does not fit. Text when it overflows rather than wrapped, which is what
        // the data source does with its own `UBIGINT`: a total that came back correct must not
        // become a negative number on the way into an answer.
        DataType::UInt64 => {
            let value = typed::<UInt64Array>(label, array)?.value(row);
            Ok(i64::try_from(value).map_or_else(|_| Value::Text(value.to_string()), Value::Integer))
        }
        // Checked, not taken. A `Float64` column is where an unguarded division lands, and a division
        // by zero in IEEE arithmetic answers `inf` rather than failing - so this is the arm that
        // decides whether `zero_denominator: fails` means what its word says. The data source's
        // `cell` has the same arm for the same reason.
        DataType::Float64 => {
            let value = typed::<Float64Array>(label, array)?.value(row);
            Real::parse(value)
                .map(Value::Real)
                .map_err(|cause| DataFusionError::NotFinite {
                    column: String::from(label),
                    cause,
                })
        }
        DataType::Utf8 => Ok(Value::Text(String::from(typed::<StringArray>(label, array)?.value(row)))),
        // The Parquet default for a string column in this version, so the affordance that reads one
        // is not broken on arrival. CSV inference gives the owned form above.
        DataType::Utf8View => Ok(Value::Text(String::from(typed::<StringViewArray>(label, array)?.value(row)))),
        // ISO text, exactly as the `DuckDB` adapter converts its own day numbers, which is what lets
        // a differential test compare the time bucket of the two at all. Converted here rather than
        // cast to text inside the plan, so the domain's calendar stays the one definition of what a
        // day number means.
        DataType::Date32 => {
            let days = typed::<Date32Array>(label, array)?.value(row);
            Date::from_days_since_epoch(days)
                .map(|date| Value::Text(date.to_iso()))
                .map_err(|cause| DataFusionError::NotADate {
                    column: String::from(label),
                    cause,
                })
        }
        // 0 or 1, matching the `DuckDB` adapter, which maps a boolean to an integer for the same
        // reason: the domain's `Value` has no boolean, and a text "true" would compare unequal to
        // the other adapter's 1.
        DataType::Boolean => Ok(Value::Integer(i64::from(typed::<BooleanArray>(label, array)?.value(row)))),
        // Text, so an exact decimal stays exact. Turning it into an `f64` here is how a total that
        // was correct in the engine stops being correct in an answer.
        DataType::Decimal128(..) => Ok(Value::Text(typed::<Decimal128Array>(label, array)?.value_as_string(row))),
        ref other => Err(DataFusionError::UnsupportedType {
            column: String::from(label),
            arrow_type: format!("{other:?}"),
        }),
    }
}
