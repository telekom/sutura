//-----------------------------------------------------------------------------
// Copyright (c) 2026, Oracle and/or its affiliates.
//
// This software is dual-licensed to you under the Universal Permissive License
// (UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
// 2.0 as shown at http://www.apache.org/licenses/LICENSE-2.0. You may choose
// either license.
//
// If you elect to accept the software under the Apache License, Version 2.0,
// the following applies:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// fetch.rs
//
// Defines support for fetching query results as Arrow record batches.
//-----------------------------------------------------------------------------

use std::collections::VecDeque;
use std::sync::Arc;

use arrow_array::builder::ArrayBuilder;
use arrow_array::builder::make_builder;
use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Decimal128Builder, Float32Builder,
    Float64Builder, Int8Builder, Int16Builder, Int32Builder, Int64Builder,
    StringBuilder, StructBuilder, TimestampMicrosecondBuilder, UInt8Builder,
    UInt16Builder, UInt32Builder, UInt64Builder,
};
use arrow_schema::{DataType, Field, Fields, Schema, SchemaRef, TimeUnit};

use crate::bind_params::BindParameters;
use crate::constants;
use crate::error::Error;
use crate::metadata::Metadata;
use crate::ora_type::OracleNumber;
use crate::ora_type::OracleTimestamp;
use crate::response::Response;
use crate::row::DbRow;
use crate::row::Row;
use crate::statement::Statement;

/// Returns the default schema to use for fetching data in the Arrow format.
fn default_schema(columns: &[Metadata]) -> Result<Schema, Error> {
    let mut fields: Vec<Field> = Vec::with_capacity(columns.len());
    for col in columns {
        let data_type = default_type(col)?;
        fields.push(Field::new(col.name(), data_type, col.nullable()));
    }
    Ok(Schema::new(fields))
}

/// Returns the default type to use for fetching data in the Arrow format.
fn default_type(col: &Metadata) -> Result<DataType, Error> {
    if col.db_type().is_string_type() {
        Ok(DataType::Utf8)
    } else if col.db_type().is_binary_type() {
        Ok(DataType::Binary)
    } else if col.db_type() == crate::DB_TYPE_TIMESTAMP_TZ {
        Ok(timestamp_with_offset())
    } else if col.db_type().is_date_type() {
        Ok(DataType::Timestamp(TimeUnit::Microsecond, None))
    } else {
        match col.db_type().ora_type_num {
            constants::ORA_TYPE_NUM_BINARY_DOUBLE => Ok(DataType::Float64),
            constants::ORA_TYPE_NUM_BINARY_FLOAT => Ok(DataType::Float32),
            constants::ORA_TYPE_NUM_BOOLEAN => Ok(DataType::Boolean),
            constants::ORA_TYPE_NUM_NUMBER => {
                let precision = col.precision();
                let scale = col.scale();
                // Arrow decimal values require a fixed precision and scale.
                // Oracle uses a precision of 0 to indicate that the precision
                // and scale of the stored numbers may vary: up to 40
                // significant digits, at a decimal exponent from -130 to 125.
                // No fixed Arrow decimal holds all of these values, and a
                // float rounds most of them, so the exact decimal text is
                // used instead. The same applies when the absolute value of
                // the scale exceeds the precision.
                if precision == 0 || scale.unsigned_abs() > precision {
                    Ok(DataType::Utf8)
                } else if scale != 0 || precision > 18 {
                    Ok(DataType::Decimal128(precision, scale))
                } else if precision <= 2 {
                    Ok(DataType::Int8)
                } else if precision <= 4 {
                    Ok(DataType::Int16)
                } else if precision <= 9 {
                    Ok(DataType::Int32)
                } else {
                    Ok(DataType::Int64)
                }
            }
            _ => Err(Error::unsupported_db_type(col.db_type())),
        }
    }
}

/// Returns the type used for a TIMESTAMP WITH TIME ZONE value: the instant
/// in UTC, beside the offset of the value from UTC in minutes. An Arrow
/// timestamp holds one time zone for all of its column, but each value in a
/// TIMESTAMP WITH TIME ZONE column can have a different offset.
fn timestamp_with_offset() -> DataType {
    let instant =
        DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()));
    DataType::Struct(Fields::from(vec![
        Field::new("timestamp", instant, true),
        Field::new("offset_minutes", DataType::Int16, true),
    ]))
}

/// Performs a query with one or more sets of parameters and returns a single
/// RecordBatch containing all of the data.
pub(crate) fn query_single_batch(
    statement: Statement,
    params: BindParameters,
) -> Result<arrow_array::RecordBatch, Error> {
    let mut batches = ArrowBatches::new(statement, params)?;
    while batches.add_round_trip()? {}
    batches.creator.create_batch()
}

/// The results of a query as Arrow record batches, one batch for each round
/// trip to the database. Returned by
/// [Connection::query_arrow_batches()](`crate::Connection::query_arrow_batches`).
/// The next round trip is made only when the next batch is requested, so a
/// caller can stop reading between round trips, for example when the data
/// already received exceeds a limit. This struct implements the Iterator
/// trait.
pub struct ArrowBatches<'a> {
    statement: Statement,
    params: BindParameters<'a>,
    next_params: usize,
    column_info: Arc<Vec<Metadata>>,
    creator: RecordBatchCreator,
    response: Option<Response>,
    last_row: Option<DbRow>,
    end_of_fetch: bool,
}

impl<'a> ArrowBatches<'a> {
    /// Executes the query with the first set of parameters.
    pub(crate) fn new(
        mut statement: Statement,
        params: BindParameters<'a>,
    ) -> Result<Self, Error> {
        let response =
            statement.get_execute_response(params.slice(0, 1), false)?;
        let metadata = statement.out_metadata().to_vec();
        let creator = RecordBatchCreator::new(&metadata)?;
        Ok(Self {
            statement,
            params,
            next_params: 1,
            column_info: Arc::new(metadata),
            creator,
            response: Some(response),
            last_row: None,
            end_of_fetch: false,
        })
    }

    /// Returns the schema of every batch.
    pub fn schema(&self) -> SchemaRef {
        self.creator.schema.clone()
    }

    /// Returns the columns of the query.
    pub fn columns(&self) -> &[Metadata] {
        &self.column_info
    }

    /// Adds the rows of the next round trip to the batch being created.
    /// Returns false when no round trip is left.
    fn add_round_trip(&mut self) -> Result<bool, Error> {
        let mut response = if let Some(response) = self.response.take() {
            response
        } else if !self.end_of_fetch {
            self.statement.fetch(self.last_row.take())?
        } else if self.next_params < self.params.num_rows() {
            let params = self.params.slice(self.next_params, 1);
            self.next_params += 1;
            self.statement.get_execute_response(params, false)?
        } else {
            return Ok(false);
        };
        self.end_of_fetch = response.is_end_of_fetch();
        if let Some(rows) = response.take_rows() {
            let mut rows: VecDeque<DbRow> = rows.into();
            while let Some(row) = rows.pop_front() {
                if rows.is_empty() {
                    self.last_row = Some(row.clone());
                }
                self.creator.add_row(&Row::new(&self.column_info, row))?;
            }
        }
        Ok(true)
    }
}

impl Iterator for ArrowBatches<'_> {
    type Item = Result<arrow_array::RecordBatch, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.add_round_trip() {
            Ok(true) => Some(self.creator.create_batch()),
            Ok(false) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Structure used for creating a record batch from the results of a fetch.
struct RecordBatchCreator {
    schema: SchemaRef,
    builders: Vec<Box<dyn ArrayBuilder>>,
    number_text: Vec<bool>,
}

impl RecordBatchCreator {
    /// Adds a row to the RecordBatch is being created.
    fn add_row(&mut self, row: &Row) -> Result<(), Error> {
        let fields = self.schema.fields();
        for (column_index, field) in fields.into_iter().enumerate() {
            let builder = &mut self.builders[column_index];
            if self.number_text[column_index] {
                let value = row
                    .get::<Option<OracleNumber>>(column_index)?
                    .map(|v| v.to_string());
                builder
                    .as_any_mut()
                    .downcast_mut::<StringBuilder>()
                    .unwrap()
                    .append_option(value);
            } else {
                Self::add_value(row, field, builder, column_index)?;
            }
        }
        Ok(())
    }

    /// Adds a value from the row to the Arrow array.
    fn add_value(
        row: &Row,
        field: &Field,
        builder: &mut Box<dyn ArrayBuilder>,
        column_index: usize,
    ) -> Result<(), Error> {
        match field.data_type() {
            DataType::Binary | DataType::LargeBinary => {
                builder
                    .as_any_mut()
                    .downcast_mut::<BinaryBuilder>()
                    .unwrap()
                    .append_option(row.get::<Option<Vec<u8>>>(column_index)?);
            }
            DataType::Boolean => {
                builder
                    .as_any_mut()
                    .downcast_mut::<BooleanBuilder>()
                    .unwrap()
                    .append_option(row.get::<Option<bool>>(column_index)?);
            }
            DataType::Decimal128(precision, scale) => {
                let value = row
                    .get::<Option<OracleNumber>>(column_index)?
                    .map(|v| v.as_decimal128(*precision, *scale))
                    .transpose()?;
                builder
                    .as_any_mut()
                    .downcast_mut::<Decimal128Builder>()
                    .unwrap()
                    .append_option(value);
            }
            DataType::Float32 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<Float32Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<f32>>(column_index)?);
            }
            DataType::Float64 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<Float64Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<f64>>(column_index)?);
            }
            DataType::Int8 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<Int8Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<i8>>(column_index)?);
            }
            DataType::Int16 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<Int16Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<i16>>(column_index)?);
            }
            DataType::Int32 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<Int32Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<i32>>(column_index)?);
            }
            DataType::Int64 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<Int64Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<i64>>(column_index)?);
            }
            DataType::Timestamp(TimeUnit::Microsecond, _) => {
                let value = row
                    .get::<Option<OracleTimestamp>>(column_index)?
                    .map(|v| v.to_timestamp_micros())
                    .transpose()?;
                builder
                    .as_any_mut()
                    .downcast_mut::<TimestampMicrosecondBuilder>()
                    .unwrap()
                    .append_option(value)
            }
            DataType::Struct(_) => {
                let value =
                    row.get::<Option<OracleTimestamp>>(column_index)?;
                let micros =
                    value.map(|v| v.to_timestamp_micros()).transpose()?;
                let offset = value.map(|v| {
                    i16::from(v.tz_hour_offset()) * 60
                        + i16::from(v.tz_minute_offset())
                });
                let builder = builder
                    .as_any_mut()
                    .downcast_mut::<StructBuilder>()
                    .unwrap();
                builder
                    .field_builder::<TimestampMicrosecondBuilder>(0)
                    .unwrap()
                    .append_option(micros);
                builder
                    .field_builder::<Int16Builder>(1)
                    .unwrap()
                    .append_option(offset);
                builder.append(value.is_some());
            }
            DataType::UInt8 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<UInt8Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<u8>>(column_index)?);
            }
            DataType::UInt16 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<UInt16Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<u16>>(column_index)?);
            }
            DataType::UInt32 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<UInt32Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<u32>>(column_index)?);
            }
            DataType::UInt64 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<UInt64Builder>()
                    .unwrap()
                    .append_option(row.get::<Option<u64>>(column_index)?);
            }
            DataType::Utf8 | DataType::LargeUtf8 => {
                builder
                    .as_any_mut()
                    .downcast_mut::<StringBuilder>()
                    .unwrap()
                    .append_option(row.get::<Option<String>>(column_index)?);
            }
            _ => {
                return Err(Error::unsupported_arrow_type(
                    field.data_type().to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Creates the RecordBatch and returns it.
    fn create_batch(&mut self) -> Result<arrow_array::RecordBatch, Error> {
        let arrays: Vec<arrow_array::ArrayRef> =
            self.builders.iter_mut().map(|b| b.finish()).collect();
        Ok(arrow_array::RecordBatch::try_new(
            self.schema.clone(),
            arrays,
        )?)
    }

    /// Creates a new structure for creating a RecordBatch
    fn new(columns: &[Metadata]) -> Result<Self, Error> {
        let schema = default_schema(columns)?;
        let builders = schema
            .fields()
            .iter()
            .map(|f| make_builder(f.data_type(), 0))
            .collect();
        let number_text = columns
            .iter()
            .zip(schema.fields())
            .map(|(col, field)| {
                col.db_type().ora_type_num == constants::ORA_TYPE_NUM_NUMBER
                    && field.data_type() == &DataType::Utf8
            })
            .collect();
        Ok(Self {
            schema: schema.into(),
            builders,
            number_text,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use arrow_array::cast::AsArray;
    use arrow_array::types::{Int16Type, TimestampMicrosecondType};
    use arrow_schema::DataType;

    use super::RecordBatchCreator;
    use crate::db_value::DbValue;
    use crate::metadata::Metadata;
    use crate::ora_type::{OracleNumber, OracleTimestamp};
    use crate::row::{DbRow, Row};

    #[test]
    fn number_without_precision_is_fetched_as_exact_text() {
        let values = [
            "9007199254740993",
            "12345678901234567890123456789012345678",
            "-0.000000000000000000000000000000000001",
        ];
        let columns =
            Arc::new(vec![Metadata::new_scalar("N", crate::DB_TYPE_NUMBER, 0)]);
        let mut creator = RecordBatchCreator::new(&columns).unwrap();
        for value in values {
            let number = OracleNumber::from_str(value).unwrap();
            let row = DbRow::new(vec![Some(DbValue::Number(number))]);
            creator.add_row(&Row::new(&columns, row)).unwrap();
        }
        let batch = creator.create_batch().unwrap();
        assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
        let fetched = batch.column(0).as_string::<i32>();
        for (index, value) in values.into_iter().enumerate() {
            let exact = OracleNumber::from_str(value).unwrap().to_string();
            assert_eq!(fetched.value(index), exact);
        }
        assert_eq!(fetched.value(0), "9007199254740993");
    }

    #[test]
    fn timestamp_with_time_zone_is_fetched_with_its_offset() {
        let columns = Arc::new(vec![Metadata::new_scalar(
            "T",
            crate::DB_TYPE_TIMESTAMP_TZ,
            0,
        )]);
        let mut creator = RecordBatchCreator::new(&columns).unwrap();
        // 2025-10-31 11:22:15.456 -06:30, read as UTC fields beside the
        // offset.
        let value = OracleTimestamp::new_timestamp_tz(
            2025,
            10,
            31,
            17,
            52,
            15,
            456_000_000,
            -6,
            -30,
        );
        let row = DbRow::new(vec![Some(DbValue::Timestamp(value))]);
        creator.add_row(&Row::new(&columns, row)).unwrap();
        let batch = creator.create_batch().unwrap();
        let fetched = batch.column(0).as_struct();
        let instants =
            fetched.column(0).as_primitive::<TimestampMicrosecondType>();
        let offsets = fetched.column(1).as_primitive::<Int16Type>();
        assert_eq!(instants.value(0), 1_761_933_135_456_000);
        assert_eq!(offsets.value(0), -390);
    }
}
