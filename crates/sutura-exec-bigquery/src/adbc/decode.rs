//! Arrow -> [`JobRows`] decode for the ADBC transport (telekom/sutura#913).
//!
//! The driver returns Arrow record batches; this pure half turns a schema +
//! batches into the adapter's own `JobRows`, reusing the same
//! [`crate::transport::FieldType`] vocabulary the wire uses, so one plan answered
//! by two transports agrees on field kinds.
//!
//! # A batch is checked against the schema it was announced under, by NAME
//!
//! **A width check is not a schema check, and the difference is a wrong answer rather than a
//! failure.** Arrow itself validates a `RecordBatch` positionally and never by name -
//! `arrow_array::RecordBatch::try_new` zips columns against fields and compares type and
//! nullability - so a driver that hands back two same-typed columns in the wrong order builds a
//! perfectly valid batch, and a decoder that only counted columns would label those values with the
//! outer schema's names. That is a transposed answer under a certified metric name, with no error
//! anywhere. The same defect class was measured in a third-party positional cast: a swapped integer
//! key and decimal measure came back transposed, and a same-typed swap came back silently empty.
//!
//! So [`Decoding::push`] refuses a batch whose field at a position is not the field the schema
//! announced there - name AND type - as [`Decode::Mislabelled`], before a single value is read.
//! Nothing in `DataFusion` would have done it for us: `SchemaAdapter`/`SchemaMapper` are deprecated
//! and their default implementation returns `not_impl_err!`, the live `PhysicalExprAdapter`
//! resolves by name only on the datasource path, and nothing validates that a custom plan's stream
//! matches its declared schema at all. For a foreign driver behind this transport the obligation is
//! ours.
//!
//! # The stream is decoded as it arrives, under a ceiling
//!
//! One pass, not two: [`Decoding`] takes one batch at a time and appends its rows, so nothing ever
//! holds a `Vec<RecordBatch>` beside the rows decoded from it. The cast to text is one vectorised
//! `arrow_cast::cast` per COLUMN per batch - it used to run inside the row loop, which cast every
//! column once per row.
//!
//! # Completeness is the full drain, and nothing reads a reported total
//!
//! The wire refused a first page by comparing the delivered count to the endpoint's `totalRows`; an
//! ADBC read streams the whole result, so completeness here is the stream draining fully -
//! `AdbcBigQuery`'s `JobTransport::run` consumes the reader to exhaustion and any error on the way is
//! an `Err`, so a truncated stream is a failure rather than a short answer.
//!
//! **[`Reported::Total`] is not wired, and the sentence that said it was conditional is gone.**
//! [`Decoding::finish`] refuses a delivered count that does not reach a total it is GIVEN, and the
//! only production call passes [`Reported::Unreported`] - so [`Decode::Incomplete`] is reachable
//! from this module's own tests and from nowhere else. The claim it replaced said the total was
//! checked *when the driver reports one*, citing schema-metadata keys measured in a provisioned leg;
//! nothing in this crate reads schema metadata, and the leg cited never ran. Reading the driver's
//! metadata keys is a change with a provisioned run behind it, not a comment.

use std::error::Error;

use arrow_array::Array as _;
use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_cast::cast;
use arrow_schema::{DataType, Field as ArrowField, Schema};

use crate::transport::{Cell, Field, FieldType, JobRows};

/// How many rows this transport will materialise from one result stream before refusing.
///
/// **A ceiling on ROWS and not on BYTES, which is the limit this sentence used to overstate.** It
/// was called a ceiling on this process's memory; a row count times an unbounded row width is not a
/// memory bound, and what decides the width is the plan's projection - a property of the plans a
/// deployment can ask, not of this constant. So what it holds is that a stream is FINITE: an
/// unending driver is refused, a million very wide rows are not.
///
/// It is not a cap on an answer either, and that distinction decides the value.
/// `sutura_domain::plan::MAX_ROWS` caps an answer and travels in the statement's own `LIMIT`; a
/// federation LEG carries no `LIMIT` at all - `sutura_domain::plan::leg`'s header says so, because a
/// leg is not an answer - so for a leg there is nothing in the statement bounding what the source
/// may stream back, and the only thing between a driver that streams without end and this process is
/// a number here. The refusal fires WHILE reading, in [`Decoding::push`], so it cannot be reached by
/// first materialising the whole stream.
///
/// Two orders of magnitude above `MAX_ROWS`, because it has to refuse only a stream no plan could
/// have asked for: a leg legitimately returns more rows than the one answer re-aggregated above it
/// keeps. **The VALUE is held rather than commented** - review measured that raising it to
/// `usize::MAX` left the whole suite green, because `delivered + n > usize::MAX` is never true and
/// the refusal test passes its own ceiling in. Both bounds of that sentence are asserted by
/// `tests::the_transports_own_ceiling_is_two_orders_of_magnitude_above_the_answer_cap`.
pub const MOST_RESULT_ROWS: usize = 1_000_000;

/// Why a result set could not be decoded.
#[derive(Debug, PartialEq, Eq)]
pub enum Decode {
    /// A column whose Arrow type this adapter does not map.
    UnmappedColumn(String),
    /// A batch that disagrees with the schema it was announced under - a stream
    /// arriving over a C ABI from a foreign driver is exactly the case to refuse
    /// rather than trust.
    Shape { fields: usize, columns: usize },
    /// A batch of the right WIDTH whose field at one position is not the field the schema announced
    /// there, so its values would have been labelled with somebody else's name.
    ///
    /// Carries both descriptors - `name type`, which is a driver's own metadata and not a value -
    /// so an operator can see which way round the two are without the refusal quoting a cell.
    Mislabelled {
        /// The position the two disagree at, counting from zero.
        at: usize,
        /// The field the schema announced there.
        announced: String,
        /// The field the driver delivered there.
        delivered: String,
    },
    /// The stream was not complete: a reported total the delivered rows do not
    /// reach.
    Incomplete { delivered: usize, reported: usize },
    /// The stream carried more rows than this transport will materialise - see [`MOST_RESULT_ROWS`].
    OverBound {
        /// The ceiling that was reached.
        most: usize,
    },
}

impl core::fmt::Display for Decode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::UnmappedColumn(ref name) => write!(f, "unmapped result column `{name}`"),
            Self::Shape { fields, columns } => {
                write!(f, "result batch has {columns} columns for a {fields}-field schema")
            }
            Self::Mislabelled {
                at,
                ref announced,
                ref delivered,
            } => write!(
                f,
                "result batch carries `{delivered}` at position {at} where the schema announced `{announced}`"
            ),
            Self::Incomplete { delivered, reported } => {
                write!(f, "incomplete answer: delivered {delivered} of {reported} reported rows")
            }
            Self::OverBound { most } => write!(f, "result stream carries more than {most} rows"),
        }
    }
}

impl Error for Decode {}

/// The row count a stream reports, when it reports one at all.
///
/// [`Reported::Unreported`] is an honest absence, never a defaulted `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reported {
    Unreported,
    Total(usize),
}

/// Maps one Arrow column type into the adapter's closed field vocabulary, the
/// same six the wire accepts: `INT64`, `FLOAT64`, `NUMERIC`, `BOOL`, `STRING`,
/// `DATE`.
fn field_type(name: &str, dt: &DataType) -> FieldType {
    match *dt {
        DataType::Int64 => FieldType::Int64,
        DataType::Float64 => FieldType::Float64,
        DataType::Decimal128(_, _) | DataType::Decimal256(_, _) => FieldType::Numeric,
        DataType::Boolean => FieldType::Bool,
        DataType::Utf8 | DataType::Utf8View | DataType::LargeUtf8 => FieldType::String,
        DataType::Date32 | DataType::Date64 => FieldType::Date,
        ref other => FieldType::Unmapped(format!("{other} ({name})")),
    }
}

/// How a field reads in a refusal: its name and its Arrow type, never a value.
fn descriptor(field: &ArrowField) -> String {
    format!("{} {}", field.name(), field.data_type())
}

/// One result stream, decoded batch by batch into the adapter's own rows.
///
/// **The accumulator exists so the bound and the schema check can fire WHILE the stream is read.**
/// The transport's `run` drives it directly off the driver's `RecordBatchReader`, so a stream that
/// will be refused is refused at the batch that crosses the line rather than after every batch has
/// been collected and then decoded a second time. It replaced a `job_rows(&schema, &batches, ..)`
/// whose only caller collected the whole stream first; nothing needs that shape now, so it is gone
/// rather than kept beside this one.
pub struct Decoding<'announced> {
    announced: &'announced Schema,
    most: usize,
    fields: Vec<Field>,
    rows: Vec<Vec<Cell>>,
    delivered: usize,
}

impl<'announced> Decoding<'announced> {
    /// Reads the announced schema's types, refusing one this adapter does not map.
    ///
    /// **The type pass is here rather than per batch on purpose:** a result with no rows at all
    /// still refuses an unmapped column, which is the answer a caller needs before it reads a field
    /// list. `most` is the row ceiling, passed rather than defaulted so the call site says which
    /// bound it is under - [`MOST_RESULT_ROWS`] is what the transport passes.
    pub fn of(announced: &'announced Schema, most: usize) -> Result<Self, Decode> {
        let mut fields = Vec::with_capacity(announced.fields().len());
        for f in announced.fields() {
            let ft = field_type(f.name(), f.data_type());
            if matches!(ft, FieldType::Unmapped(_)) {
                return Err(Decode::UnmappedColumn(f.name().to_owned()));
            }
            fields.push(Field::of(f.name().to_owned(), ft));
        }
        Ok(Self {
            announced,
            most,
            fields,
            rows: Vec::new(),
            delivered: 0,
        })
    }

    /// Checks one batch against the announced schema and appends its rows.
    ///
    /// # Errors
    ///
    /// [`Decode::Shape`] for a batch of the wrong width, [`Decode::Mislabelled`] for one whose
    /// field at a position is not the announced one, [`Decode::OverBound`] where this batch would
    /// take the stream past the ceiling, and [`Decode::UnmappedColumn`] where a column would not
    /// cast to text.
    pub fn push(&mut self, batch: &RecordBatch) -> Result<(), Decode> {
        let announced = self.announced.fields();
        let delivered = batch.schema_ref().fields();
        if delivered.len() != announced.len() {
            return Err(Decode::Shape {
                fields: announced.len(),
                columns: batch.num_columns(),
            });
        }
        // The `zip` cannot truncate, and the length refusal above is what makes that true rather
        // than a comment: a `zip` WITHOUT it silently matches the shorter prefix, which is both
        // halves of the defect this check exists for - a narrower batch reads as a match on its
        // first columns, and a same-typed swap reads as a match on nothing at all.
        for (at, (announced, delivered)) in announced.iter().zip(delivered).enumerate() {
            if announced.name() != delivered.name() || announced.data_type() != delivered.data_type() {
                return Err(Decode::Mislabelled {
                    at,
                    announced: descriptor(announced),
                    delivered: descriptor(delivered),
                });
            }
        }
        let n = batch.num_rows();
        if self.delivered.saturating_add(n) > self.most {
            return Err(Decode::OverBound { most: self.most });
        }
        // ONE cast per column, not per row. This used to sit inside the row loop, which cast every
        // column's whole array once for every row of it.
        let mut cast_columns: Vec<ArrayRef> = Vec::with_capacity(batch.num_columns());
        for col in batch.columns() {
            cast_columns
                .push(cast(&**col, &DataType::Utf8).map_err(|e| Decode::UnmappedColumn(format!("column cast failed: {e}")))?);
        }
        let mut texts: Vec<&StringArray> = Vec::with_capacity(cast_columns.len());
        for col in &cast_columns {
            texts.push(
                col.as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| Decode::UnmappedColumn("not a string after cast".to_owned()))?,
            );
        }
        for r in 0..n {
            self.rows.push(
                texts
                    .iter()
                    .map(|s| {
                        if s.is_null(r) {
                            Cell::Null
                        } else {
                            Cell::Text(s.value(r).to_owned())
                        }
                    })
                    .collect(),
            );
        }
        self.delivered = self.delivered.saturating_add(n);
        Ok(())
    }

    /// The decoded rows, refusing a stream that did not reach a reported total.
    ///
    /// # Errors
    ///
    /// [`Decode::Incomplete`] where `reported` names a total the delivered rows do not reach.
    pub fn finish(self, reported: Reported) -> Result<JobRows, Decode> {
        if let Reported::Total(total) = reported
            && self.delivered != total
        {
            return Err(Decode::Incomplete {
                delivered: self.delivered,
                reported: total,
            });
        }
        Ok(JobRows::of(self.fields, self.rows, self.delivered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::builder::{Int64Builder, StringBuilder};
    use std::sync::Arc;

    fn ints(v: &[i64]) -> Arc<arrow_array::Int64Array> {
        let mut b = Int64Builder::new();
        for x in v {
            b.append_value(*x);
        }
        Arc::new(b.finish())
    }

    fn strings(v: &[Option<&str>]) -> Arc<arrow_array::StringArray> {
        let mut b = StringBuilder::new();
        for s in v {
            match s {
                Some(s) => b.append_value(s),
                None => b.append_null(),
            }
        }
        Arc::new(b.finish())
    }

    /// The whole decode over a slice, for a test that has batches rather than a stream.
    ///
    /// In the test module because production has no such caller: `run` drives [`Decoding`] off the
    /// driver's reader one batch at a time, which is the whole point of the accumulator.
    fn decoded(schema: &Schema, batches: &[RecordBatch], reported: Reported) -> Result<JobRows, Decode> {
        let mut decoding = Decoding::of(schema, MOST_RESULT_ROWS)?;
        for batch in batches {
            decoding.push(batch)?;
        }
        decoding.finish(reported)
    }

    #[test]
    fn maps_columns_and_rows() {
        let schema = Schema::new(vec![
            arrow_schema::Field::new("id", DataType::Int64, false),
            arrow_schema::Field::new("name", DataType::Utf8, false),
        ]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![ints(&[1, 2]), strings(&[Some("a"), Some("b")])]).unwrap();
        let rows = decoded(batch.schema().as_ref(), &[batch], Reported::Total(2)).unwrap();
        assert_eq!(rows.fields().len(), 2);
        assert_eq!(rows.rows().len(), 2);
        assert_eq!(rows.total_rows(), 2);
        assert_eq!(rows.rows()[0][0], Cell::Text("1".into()));
        assert_eq!(rows.rows()[0][1], Cell::Text("a".into()));
    }

    #[test]
    fn refuses_an_unmapped_column_even_with_no_rows() {
        let schema = Schema::new(vec![arrow_schema::Field::new("geo", DataType::Binary, false)]);
        let res = decoded(&schema, &[], Reported::Unreported);
        assert!(matches!(res, Err(Decode::UnmappedColumn(_))));
    }

    #[test]
    fn refuses_a_batch_narrower_than_the_schema() {
        // Two fields announced, but the batch (its own 1-field schema, valid on
        // its own) carries one column - the public job_rows API can be handed a
        // schema that does not match the batches it is given.
        let announced = Schema::new(vec![
            arrow_schema::Field::new("id", DataType::Int64, false),
            arrow_schema::Field::new("name", DataType::Utf8, false),
        ]);
        let batch_schema = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, false)]);
        let batch = RecordBatch::try_new(Arc::new(batch_schema), vec![ints(&[1, 2])]).unwrap();
        let res = decoded(&announced, &[batch], Reported::Unreported);
        assert!(matches!(res, Err(Decode::Shape { fields: 2, columns: 1 })));
    }

    #[test]
    fn refuses_an_incomplete_stream() {
        let schema = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, false)]);
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![ints(&[1, 2, 3])]).unwrap();
        let res = decoded(&schema, &[batch], Reported::Total(5));
        assert!(matches!(
            res,
            Err(Decode::Incomplete {
                delivered: 3,
                reported: 5
            })
        ));
    }

    #[test]
    fn a_null_cell_is_null_not_a_number() {
        let schema = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, true)]);
        let mut b = Int64Builder::new();
        b.append_value(7);
        b.append_null();
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![Arc::new(b.finish())]).unwrap();
        let rows = decoded(&schema, &[batch], Reported::Unreported).unwrap();
        assert_eq!(rows.rows()[1][0], Cell::Null);
    }

    #[test]
    fn a_date32_column_renders_iso() {
        // 0 days after the epoch = 1970-01-01, ISO.
        let schema = Schema::new(vec![arrow_schema::Field::new("d", DataType::Date32, true)]);
        let arr = arrow_array::Date32Array::from(vec![Some(0)]);
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![Arc::new(arr)]).unwrap();
        let rows = decoded(&schema, &[batch], Reported::Unreported).unwrap();
        assert_eq!(*rows.fields()[0].kind(), crate::transport::FieldType::Date);
        assert_eq!(rows.rows()[0][0], Cell::Text("1970-01-01".into()));
    }

    #[test]
    fn a_decimal128_column_renders_exact_scale() {
        // 12345 with scale 2 => 123.45, exact digits.
        use arrow_array::array::Decimal128Array;
        let schema = Schema::new(vec![arrow_schema::Field::new("amount", DataType::Decimal128(9, 2), true)]);
        let arr = Decimal128Array::from(vec![Some(12345i128)])
            .with_precision_and_scale(9, 2)
            .unwrap();
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![Arc::new(arr)]).unwrap();
        let rows = decoded(&schema, &[batch], Reported::Unreported).unwrap();
        assert_eq!(*rows.fields()[0].kind(), crate::transport::FieldType::Numeric);
        assert_eq!(rows.rows()[0][0], Cell::Text("123.45".into()));
    }

    #[test]
    fn refuses_two_same_typed_columns_the_driver_delivered_in_the_wrong_order() {
        // THE FINDING A WIDTH CHECK CANNOT SEE. Two `Utf8` columns, the right count, the right
        // types - and swapped. Arrow builds this batch without complaint, because
        // `RecordBatch::try_new` compares types positionally and never names, so the only thing
        // standing between a driver's column order and a transposed answer under a certified metric
        // name is this refusal. A DIFFERENTLY-typed swap is the easy half that Arrow's own type
        // check already catches; this is the half it does not.
        let announced = Schema::new(vec![
            arrow_schema::Field::new("region", DataType::Utf8, false),
            arrow_schema::Field::new("product", DataType::Utf8, false),
        ]);
        let swapped = Schema::new(vec![
            arrow_schema::Field::new("product", DataType::Utf8, false),
            arrow_schema::Field::new("region", DataType::Utf8, false),
        ]);
        let batch = RecordBatch::try_new(Arc::new(swapped), vec![strings(&[Some("widget")]), strings(&[Some("north")])]).unwrap();
        let res = decoded(&announced, &[batch], Reported::Unreported);
        let Err(Decode::Mislabelled {
            at,
            announced,
            delivered,
        }) = res
        else {
            panic!("a swapped pair of same-typed columns was accepted: {res:?}");
        };
        assert_eq!(at, 0);
        assert_eq!(announced, "region Utf8");
        assert_eq!(delivered, "product Utf8");
    }

    #[test]
    fn refuses_a_column_the_driver_retyped_under_the_announced_name() {
        // The other half of the same check: the name agrees and the TYPE does not. Arrow never
        // compares the announced schema with the batch's own - they are two values - so a decoder
        // reading field kinds off the announced schema would report `Int64` for a column of text.
        let announced = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, false)]);
        let retyped = Schema::new(vec![arrow_schema::Field::new("id", DataType::Utf8, false)]);
        let batch = RecordBatch::try_new(Arc::new(retyped), vec![strings(&[Some("1")])]).unwrap();
        let res = decoded(&announced, &[batch], Reported::Unreported);
        let Err(Decode::Mislabelled {
            at,
            announced,
            delivered,
        }) = res
        else {
            panic!("a retyped column was accepted under the announced name: {res:?}");
        };
        assert_eq!(at, 0);
        assert_eq!(announced, "id Int64");
        assert_eq!(delivered, "id Utf8");
    }

    #[test]
    fn a_stream_past_the_ceiling_is_refused_at_the_batch_that_crosses_it() {
        // WHILE reading, not after: the first batch is accepted and the second is refused, so the
        // rows past the ceiling are never materialised. A ceiling checked after the stream had been
        // collected would have held both batches in memory first, which is the whole point of the
        // accumulator.
        let schema = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, false)]);
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![ints(&[1, 2])]).unwrap();
        let mut decoding = Decoding::of(&schema, 3).expect("the schema maps");
        decoding.push(&batch).expect("two rows are under a ceiling of three");
        assert_eq!(decoding.push(&batch), Err(Decode::OverBound { most: 3 }));
    }

    #[test]
    fn the_transports_own_ceiling_is_two_orders_of_magnitude_above_the_answer_cap() {
        // **BOTH bounds, because only one of them was held and the other is the one that matters.**
        // Review raised `MOST_RESULT_ROWS` to `usize::MAX` and the whole suite stayed green at exit
        // 0: `delivered + n > usize::MAX` is never true, `a_stream_past_the_ceiling_...` passes its
        // own ceiling of 3 in, and this cell asserted the floor alone - so the number no gate and no
        // cell read was free to become no ceiling at all.
        //
        // Read off `MAX_ROWS` rather than restated, so a test that spelled either number again
        // cannot pass with the production value changed underneath it. The factor is the constant's
        // own documented rule - two orders of magnitude above the answer cap - which is what makes
        // this an assertion about the value rather than about arithmetic.
        let cap = usize::try_from(sutura_domain::plan::MAX_ROWS).expect("the answer cap fits a usize");
        assert!(
            MOST_RESULT_ROWS > cap,
            "a ceiling at or under the answer cap refuses legitimate results"
        );
        assert!(
            MOST_RESULT_ROWS <= cap.saturating_mul(100),
            "a ceiling further than two orders of magnitude above the answer cap is not the bound this \
             constant documents - {MOST_RESULT_ROWS} rows is what a stream would have to reach to be refused"
        );
        let schema = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, false)]);
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![ints(&[1])]).unwrap();
        decoded(&schema, &[batch], Reported::Unreported).expect("an ordinary result is under the ceiling");
    }
}
