//! An Arrow result at the interior: the schema guard a foreign driver needs, and the one place an
//! Arrow array becomes a domain [`Value`].
//!
//! # Why the hexagon's interior names an Arrow array type
//!
//! `docs/adr/0039` decides it, reversing `docs/adr/0007`'s *the port's currency stays `RowSet`*
//! and the unmerged 0037's refusal. The argument is the one `ALLOWED_IN_DOMAIN`'s own line draws -
//! *no runtime, no client, no engine*: Arrow is a data FORMAT, and the engine, the ADBC driver
//! manager and every future Arrow Flight leg already speak it. **The cost is in that allowlist and
//! nowhere else**, which is where it can be argued in a diff.
//!
//! What it buys is one decode instead of one per adapter. Before this module there were three, and
//! the `BigQuery` one went through TEXT: Arrow arrays cast to `Utf8`, a text cell per value, then a
//! `parse::<i64>()` back to a number. A total that was exact in the data system and exact in Arrow
//! had two chances to stop being exact on the way out.
//!
//! # `RecordBatch::try_new` is not a schema check, and that is the whole reason [`Accumulating`]
//! exists
//!
//! Arrow validates a batch **positionally and by type only** - it zips columns against fields and
//! never reads a field NAME. So a driver that hands back two same-typed columns in the wrong order
//! builds a perfectly valid `RecordBatch`, and a consumer that only counted columns would label
//! those values with the announced schema's names: a transposed answer under a certified metric
//! name, with no error anywhere. A differently-typed swap Arrow already refuses, which is why the
//! swap that matters - and the mutation worth running against this module - is of two **same-typed**
//! columns.
//!
//! Nothing in `DataFusion` closes it either: `SchemaAdapter`/`SchemaMapper` are deprecated,
//! `PhysicalExprAdapter` resolves by name on the DATASOURCE path and is opt-in, and nothing
//! validates that a custom `ExecutionPlan`'s stream matches its declared schema at all. For a
//! foreign driver the obligation is ours, so it is held here once rather than per adapter.
//!
//! # Where this module's checks stop
//!
//! [`Accumulating`] refuses a width, a mislabelled position and a row ceiling **before a value is
//! read**. It does not check nullability - Arrow does, positionally - and it does not check that the
//! announced schema is the one the plan asked for; that is the caller's, and
//! `sutura_domain::plan::QueryPlan::result_labels` is what it compares against.

use arrow_array::cast::AsArray as _;
use arrow_array::{Array, RecordBatch};
use arrow_schema::{DataType, SchemaRef};

use crate::calendar::Date;
use crate::warehouse::cell::{Real, Value};
use crate::warehouse::rows::{MalformedRowSet, RowSet};

/// A result, as Arrow: the schema every batch was checked against, and the batches.
///
/// **A newtype whose invariant is the schema agreement**, so a value of this type is one whose every
/// batch carried the announced fields by name and type. There is no public constructor taking
/// batches directly - [`Accumulating`] is the only way in - because a constructor that took a
/// `Vec<RecordBatch>` and checked afterwards would let a caller hold an unchecked one for a line,
/// and the check has to happen while the stream is read or a bound on it is not a bound.
#[derive(Debug, Clone)]
pub struct ResultBatches {
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
    rows: usize,
}

/// Why a result stream was refused, before any value in it was read.
///
/// Each variant carries field DESCRIPTORS - a name and an Arrow type, which is a driver's own
/// metadata - and never a cell, so a refusal an operator reads discloses no data.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UnannouncedBatch {
    /// A batch of a different width than the schema it arrived under.
    #[error("a result batch has {delivered} columns for a {announced}-field schema")]
    Width { announced: usize, delivered: usize },
    /// A batch of the right width whose field at one position is not the announced one, so its
    /// values would have been labelled with another column's name.
    #[error("a result batch carries `{delivered}` at position {at}, where the schema announced `{announced}`")]
    Mislabelled {
        at: usize,
        announced: String,
        delivered: String,
    },
    /// The stream carried more rows than the caller's ceiling allows.
    ///
    /// The ceiling is passed in rather than fixed here: what is a sane bound depends on whether the
    /// caller is reading an answer or a federation leg, and only the caller knows which.
    #[error("a result stream carries more than {most} rows")]
    OverBound { most: usize },
}

/// Why an Arrow array could not become a domain value.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum UnreadableCell {
    /// A column whose Arrow type this domain does not map.
    ///
    /// An error rather than a `Debug` rendering, because this is the last place a value can be wrong
    /// without anybody noticing: a rendered nested type would flow into an answer looking like data,
    /// and an anchor comparison against it would pass or fail for reasons nobody could read.
    #[error("column {column} came back as {arrow_type}, which this domain does not map")]
    UnsupportedType { column: String, arrow_type: String },
    /// An array whose runtime type is not the one its own schema declares - an Arrow invariant
    /// violation, so a driver defect rather than anything a caller asked for.
    #[error("column {column} did not downcast to {arrow_type}, though its schema says that is its type")]
    Downcast { column: String, arrow_type: &'static str },
    /// A 64-bit float that is not finite.
    ///
    /// CHECKED rather than taken: `inf` is what an unguarded division answers, and rendering it puts
    /// the string `"inf"` in an answer under a metric's own certified name.
    #[error("column {column} came back as a value that is not a finite number")]
    NotFinite {
        column: String,
        #[source]
        cause: crate::warehouse::cell::NotFinite,
    },
    /// A day count that is not a date this calendar can express.
    #[error("column {column} came back as a day number that is not a date")]
    NotADate {
        column: String,
        #[source]
        cause: crate::calendar::InvalidDate,
    },
    /// The decoded rows did not form a rectangle - unreachable by construction, propagated rather
    /// than swallowed so an edit that breaks the construction fails loudly.
    #[error("the decoded rows are not rectangular")]
    Ragged(#[source] MalformedRowSet),
}

/// How a field reads in a refusal: its name and its Arrow type, never a value.
fn descriptor(field: &arrow_schema::Field) -> String {
    let (name, kind) = (field.name(), field.data_type());
    format!("{name} {kind}")
}

/// One result stream, checked against its announced schema batch by batch.
///
/// **The accumulator exists so the schema check and the row ceiling fire WHILE the stream is read.**
/// A driver's reader is driven straight into [`Self::push`], so a stream that will be refused is
/// refused at the batch that crosses the line - not after every batch has been collected, which is
/// the point at which the memory a ceiling protects has already been spent.
#[derive(Debug)]
pub struct Accumulating {
    announced: SchemaRef,
    most: usize,
    batches: Vec<RecordBatch>,
    rows: usize,
}

impl Accumulating {
    /// Starts reading a stream announced under `schema`, refusing past `most` rows.
    #[must_use]
    pub const fn announcing(schema: SchemaRef, most: usize) -> Self {
        Self {
            announced: schema,
            most,
            batches: Vec::new(),
            rows: 0,
        }
    }

    /// Checks one batch against the announced schema and keeps it.
    ///
    /// # Errors
    ///
    /// [`UnannouncedBatch::Width`] for a batch of the wrong width, [`UnannouncedBatch::Mislabelled`]
    /// for one whose field at a position is not the announced one, and
    /// [`UnannouncedBatch::OverBound`] where this batch would take the stream past the ceiling.
    pub fn push(&mut self, batch: RecordBatch) -> Result<(), UnannouncedBatch> {
        let announced = self.announced.fields();
        let delivered = batch.schema_ref().fields();
        if delivered.len() != announced.len() {
            return Err(UnannouncedBatch::Width {
                announced: announced.len(),
                delivered: delivered.len(),
            });
        }
        // The `zip` cannot truncate, and the width refusal above is what makes that true rather than
        // a comment: a `zip` WITHOUT it silently matches the shorter prefix, which is both halves of
        // the defect this check exists for - a narrower batch reads as a match on its first columns,
        // and a same-typed swap reads as a match on nothing at all.
        for (at, (announced, delivered)) in announced.iter().zip(delivered).enumerate() {
            if announced.name() != delivered.name() || announced.data_type() != delivered.data_type() {
                return Err(UnannouncedBatch::Mislabelled {
                    at,
                    announced: descriptor(announced),
                    delivered: descriptor(delivered),
                });
            }
        }
        let arriving = batch.num_rows();
        if self.rows.saturating_add(arriving) > self.most {
            return Err(UnannouncedBatch::OverBound { most: self.most });
        }
        self.rows = self.rows.saturating_add(arriving);
        self.batches.push(batch);
        Ok(())
    }

    /// How many rows have been accepted so far, which a completeness check compares against.
    #[inline]
    #[must_use]
    pub const fn delivered(&self) -> usize {
        self.rows
    }

    /// The checked result.
    #[must_use]
    pub fn finish(self) -> ResultBatches {
        ResultBatches {
            schema: self.announced,
            batches: self.batches,
            rows: self.rows,
        }
    }
}

impl ResultBatches {
    /// A result with no rows, under a schema - what an adapter answers for an empty stream.
    #[must_use]
    pub fn none_under(schema: SchemaRef) -> Self {
        Accumulating::announcing(schema, 0).finish()
    }

    /// The schema every batch was checked against.
    #[inline]
    #[must_use]
    pub const fn schema(&self) -> &SchemaRef {
        &self.schema
    }

    /// The batches, in the order the stream delivered them.
    #[inline]
    #[must_use]
    pub fn batches(&self) -> &[RecordBatch] {
        &self.batches
    }

    /// How many rows the stream delivered.
    #[inline]
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// The result as domain rows.
    ///
    /// **The one Arrow-to-[`Value`] decode in this workspace.** It used to be three - the engine's
    /// own, the `DuckDB` adapter's, and `BigQuery`'s via text - and one plan answered by two adapters
    /// has to produce one number or an anchor certified against one stops reproducing against the
    /// other. Agreement is a correctness property here, not tidiness, which is why the mapping is a
    /// single function with a single test table rather than a convention.
    ///
    /// # Errors
    ///
    /// [`UnreadableCell`], naming the column and the Arrow type.
    pub fn to_rows(&self) -> Result<RowSet, UnreadableCell> {
        let columns: Vec<String> = self
            .schema
            .fields()
            .iter()
            .map(|field| String::from(field.name().as_str()))
            .collect();
        // THE TYPE PASS, BEFORE A ROW IS READ, and the hole it closes was measured rather than
        // imagined. `cell` answers a null before it looks at the column's type, so a result with NO
        // rows never reaches it at all and a result whose unmapped column happens to be entirely
        // null reaches it and is answered: a `TIMESTAMP` column came back as a successful EMPTY
        // result, and whether this workspace maps a type depended on what the data happened to be.
        // `sutura-exec-bigquery`'s own decoder had this pass and the engine did not; it is here now,
        // so every adapter gets it.
        for (label, field) in columns.iter().zip(self.schema.fields()) {
            mapped(label, field.data_type())?;
        }
        let mut rows: Vec<Vec<Value>> = Vec::with_capacity(self.rows);
        for batch in &self.batches {
            for row in 0..batch.num_rows() {
                let mut cells = Vec::with_capacity(columns.len());
                for (index, label) in columns.iter().enumerate() {
                    // `index` walks the announced schema's own width, which `Accumulating::push`
                    // refused a batch for not matching - so the column is there by construction.
                    // Propagated rather than defaulted, because a `Value::Null` here would be a
                    // wrong number dressed as a missing one.
                    let array = batch.columns().get(index).ok_or_else(|| UnreadableCell::Downcast {
                        column: label.clone(),
                        arrow_type: "a column at the announced position",
                    })?;
                    cells.push(cell(label, array.as_ref(), row)?);
                }
                rows.push(cells);
            }
        }
        RowSet::new(columns, rows).map_err(UnreadableCell::Ragged)
    }
}

/// Whether this domain maps a column of this Arrow type at all, asked of the TYPE and no value.
///
/// **A second match over the same set as [`cell`], and both directions of a drift fail closed.** A
/// type [`cell`] reads but this does not name is refused at the schema pass, which is a column
/// refused that could have been read; a type this names but [`cell`] does not is refused per cell as
/// [`UnreadableCell::UnsupportedType`] instead, one step later. Neither direction answers a value.
/// `every_mapped_type_passes_the_schema_pass_and_float32_does_not` is what holds the two together,
/// and it reads the mapping table rather than a list of its own.
fn mapped(label: &str, kind: &DataType) -> Result<(), UnreadableCell> {
    match *kind {
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Float64
        | DataType::Utf8
        | DataType::LargeUtf8
        | DataType::Utf8View
        | DataType::Date32
        | DataType::Boolean
        | DataType::Decimal128(..)
        | DataType::Decimal256(..) => Ok(()),
        ref other => Err(UnreadableCell::UnsupportedType {
            column: String::from(label),
            arrow_type: format!("{other:?}"),
        }),
    }
}

/// One cell, as a domain value.
///
/// Every integer width answers, because every one of them fits an `i64` losslessly - a Parquet
/// `INT32` column under a `min` or a `max` used to answer through one adapter and error in another.
/// A 64-bit unsigned value that does not fit is rendered as text rather than wrapped: a silently
/// truncated total is a wrong number.
///
/// **Widening a 32-bit float to an `f64` is the tempting arm and is exactly wrong**, so it is absent:
/// `0.1_f32` as an `f64` prints as `0.10000000149011612`, and two adapters would then disagree about
/// a number neither of them got wrong.
///
/// An exact decimal stays TEXT unless its scale is zero, for the same reason: turning it into an
/// `f64` is how a total that was correct in the engine stops being correct in an answer. A
/// zero-scale decimal that fits an `i64` widens, and one that does not stays text.
///
/// `Date64` is deliberately absent. Nothing on this path produces one - `date_trunc` over a `Date32`
/// stays a `Date32`, a Parquet `DATE` logical type reads as `Date32`, and `DuckDB`'s `DATE` is a day
/// count - so mapping it would mean choosing what a millisecond count that is not a whole number of
/// days means, in an arm no question can reach. An unreachable arm holding a semantic choice nobody
/// reviewed is worse than an error naming the type.
fn cell(label: &str, array: &dyn Array, row: usize) -> Result<Value, UnreadableCell> {
    if array.is_null(row) {
        return Ok(Value::Null);
    }
    match *array.data_type() {
        DataType::Int64 => Ok(Value::Integer(
            array.as_primitive::<arrow_array::types::Int64Type>().value(row),
        )),
        // Every narrower width, because `i64::from` is lossless for all of them. Written out rather
        // than reached through a cast so that the conversion is the compiler's business.
        DataType::Int8 => Ok(Value::Integer(i64::from(
            array.as_primitive::<arrow_array::types::Int8Type>().value(row),
        ))),
        DataType::Int16 => Ok(Value::Integer(i64::from(
            array.as_primitive::<arrow_array::types::Int16Type>().value(row),
        ))),
        DataType::Int32 => Ok(Value::Integer(i64::from(
            array.as_primitive::<arrow_array::types::Int32Type>().value(row),
        ))),
        DataType::UInt8 => Ok(Value::Integer(i64::from(
            array.as_primitive::<arrow_array::types::UInt8Type>().value(row),
        ))),
        DataType::UInt16 => Ok(Value::Integer(i64::from(
            array.as_primitive::<arrow_array::types::UInt16Type>().value(row),
        ))),
        DataType::UInt32 => Ok(Value::Integer(i64::from(
            array.as_primitive::<arrow_array::types::UInt32Type>().value(row),
        ))),
        // The one width that does not fit. Text when it overflows rather than wrapped: a total that
        // came back correct must not become a negative number on the way into an answer.
        DataType::UInt64 => {
            let value = array.as_primitive::<arrow_array::types::UInt64Type>().value(row);
            Ok(i64::try_from(value).map_or_else(|_| Value::Text(value.to_string()), Value::Integer))
        }
        DataType::Float64 => {
            let value = array.as_primitive::<arrow_array::types::Float64Type>().value(row);
            Real::parse(value)
                .map(Value::Real)
                .map_err(|cause| UnreadableCell::NotFinite {
                    column: String::from(label),
                    cause,
                })
        }
        DataType::Utf8 => Ok(Value::Text(String::from(array.as_string::<i32>().value(row)))),
        DataType::LargeUtf8 => Ok(Value::Text(String::from(array.as_string::<i64>().value(row)))),
        // The Parquet default for a string column in this Arrow version, so an affordance that reads
        // one is not broken on arrival. CSV inference gives the owned form above.
        DataType::Utf8View => Ok(Value::Text(String::from(array.as_string_view().value(row)))),
        // ISO text, so the domain's calendar stays the one definition of what a day number means -
        // and so two adapters' time buckets can be compared at all.
        DataType::Date32 => {
            let days = array.as_primitive::<arrow_array::types::Date32Type>().value(row);
            Date::from_days_since_epoch(days)
                .map(|date| Value::Text(date.to_iso()))
                .map_err(|cause| UnreadableCell::NotADate {
                    column: String::from(label),
                    cause,
                })
        }
        // `0` or `1`: this domain's `Value` has no boolean, and a text `"true"` would compare unequal
        // to another adapter's `1`.
        DataType::Boolean => Ok(Value::Integer(i64::from(array.as_boolean().value(row)))),
        DataType::Decimal128(_, 0) => {
            let text = array
                .as_primitive::<arrow_array::types::Decimal128Type>()
                .value_as_string(row);
            Ok(text.parse::<i64>().map_or_else(|_| Value::Text(text), Value::Integer))
        }
        DataType::Decimal128(..) => Ok(Value::Text(
            array
                .as_primitive::<arrow_array::types::Decimal128Type>()
                .value_as_string(row),
        )),
        DataType::Decimal256(_, 0) => {
            let text = array
                .as_primitive::<arrow_array::types::Decimal256Type>()
                .value_as_string(row);
            Ok(text.parse::<i64>().map_or_else(|_| Value::Text(text), Value::Integer))
        }
        DataType::Decimal256(..) => Ok(Value::Text(
            array
                .as_primitive::<arrow_array::types::Decimal256Type>()
                .value_as_string(row),
        )),
        ref other => Err(UnreadableCell::UnsupportedType {
            column: String::from(label),
            arrow_type: format!("{other:?}"),
        }),
    }
}

/// One column's Arrow array, built from domain values.
///
/// Behind the `fixtures` feature, because only a fake and an adapter whose source speaks rows need
/// it: an adapter reading Arrow from its driver has nothing to build.
///
/// **The inference is deliberately narrow and stated where it is made.** All-`Integer` is `Int64`, all-`Real` is `Float64`, anything else is `Utf8` with each value
/// rendered - so a MIXED column round-trips as text rather than as the types it went in as. That is
/// the honest limit of a per-cell union meeting a per-column format, and no source produces a mixed
/// column: a data system declares a column's type.
#[cfg(feature = "fixtures")]
#[must_use]
pub fn arrow_column(values: &[Value]) -> (DataType, arrow_array::ArrayRef) {
    use std::sync::Arc;

    let present = || values.iter().filter(|value| !matches!(**value, Value::Null));
    if present().all(|value| matches!(*value, Value::Integer(_))) {
        let integers: Vec<Option<i64>> = values
            .iter()
            .map(|value| match *value {
                Value::Integer(number) => Some(number),
                _ => None,
            })
            .collect();
        return (DataType::Int64, Arc::new(arrow_array::Int64Array::from(integers)));
    }
    if present().all(|value| matches!(*value, Value::Real(_))) {
        let reals: Vec<Option<f64>> = values
            .iter()
            .map(|value| match *value {
                Value::Real(number) => Some(number.get()),
                _ => None,
            })
            .collect();
        return (DataType::Float64, Arc::new(arrow_array::Float64Array::from(reals)));
    }
    let texts: Vec<Option<String>> = values
        .iter()
        .map(|value| match *value {
            Value::Null => None,
            ref other => Some(other.render()),
        })
        .collect();
    (DataType::Utf8, Arc::new(arrow_array::StringArray::from(texts)))
}

/// A result built from domain rows, for a fake and for an adapter whose source speaks rows.
///
/// Behind `fixtures` for [`arrow_column`]'s reason, and it carries that function's inference limit.
///
/// # Errors
///
/// [`MalformedRowSet::RowWidth`] for a ragged input, refused here rather than at the Arrow layer -
/// `RecordBatch::try_new` would answer a different error for the same defect, and one of the two
/// would be the one nobody had read.
#[cfg(feature = "fixtures")]
pub fn of_rows(columns: &[String], rows: &[Vec<Value>]) -> Result<ResultBatches, MalformedRowSet> {
    use std::sync::Arc;

    use arrow_schema::{Field, Schema};

    for (index, row) in rows.iter().enumerate() {
        if row.len() != columns.len() {
            return Err(MalformedRowSet::RowWidth {
                row: index,
                cells: row.len(),
                columns: columns.len(),
            });
        }
    }
    let mut fields = Vec::with_capacity(columns.len());
    let mut arrays = Vec::with_capacity(columns.len());
    for (index, label) in columns.iter().enumerate() {
        let column: Vec<Value> = rows
            .iter()
            .map(|row| row.get(index).cloned().unwrap_or(Value::Null))
            .collect();
        let (kind, array) = arrow_column(&column);
        fields.push(Field::new(label.as_str(), kind, true));
        arrays.push(array);
    }
    let schema: SchemaRef = Arc::new(Schema::new(fields));
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), rows.len());
    if rows.is_empty() {
        return Ok(accumulating.finish());
    }
    // `try_new` cannot fail here: every array was built from the same `rows.len()` and its field
    // carries the type the builder chose. Mapped rather than unwrapped, because this workspace
    // allows neither `unwrap` nor `expect`.
    let batch = RecordBatch::try_new(Arc::clone(&schema), arrays).map_err(|_arrow| MalformedRowSet::RowWidth {
        row: 0,
        cells: 0,
        columns: columns.len(),
    })?;
    accumulating.push(batch).map_err(|_unannounced| MalformedRowSet::RowWidth {
        row: 0,
        cells: 0,
        columns: columns.len(),
    })?;
    Ok(accumulating.finish())
}

#[cfg(test)]
mod tests;
