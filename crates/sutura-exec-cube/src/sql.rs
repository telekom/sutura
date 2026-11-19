//! Cube 1.8's SQL response: one schema line followed by rectangular data batches as JSON lines.
//!
//! Signed integer strings retain their precision here. The wire schema also labels unsigned
//! arrays that Cube narrows to signed values before serializing; this decoder cannot recover that
//! origin or repair a value already narrowed upstream.

use sutura_domain::warehouse::{RawRows, Real, Value};

use crate::{CubeError, CubeResult};

#[derive(serde::Deserialize)]
struct Column {
    name: String,
    column_type: ColumnType,
}

#[derive(serde::Deserialize)]
enum ColumnType {
    String,
    VarStr,
    Double,
    Boolean,
    Int8,
    Int32,
    Int64,
    Timestamp,
    Date(bool),
}

const fn malformed<E>() -> CubeError<E>
where
    E: core::error::Error + 'static,
{
    CubeError::MalformedAnswer {
        expected: "SQL schema followed by rectangular JSON data lines",
    }
}

#[expect(
    clippy::map_err_ignore,
    reason = "Cube's JSON error may repeat source text; only the response shape is exposed"
)]
pub(crate) fn read<E>(body: &[u8], limit: u32) -> CubeResult<RawRows, E>
where
    E: core::error::Error + 'static,
{
    let mut schema: Option<Vec<Column>> = None;
    let mut rows = Vec::new();
    let mut saw_data = false;
    for line in body.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
        let message: serde_json::Value = serde_json::from_slice(line).map_err(|_| malformed())?;
        let serde_json::Value::Object(mut object) = message else {
            return Err(malformed());
        };
        if object.contains_key("error") {
            return Err(CubeError::StatementFailed);
        }
        if let Some(columns) = object.remove("schema") {
            if schema.is_some() || saw_data || object.contains_key("data") {
                return Err(malformed());
            }
            schema = Some(serde_json::from_value(columns).map_err(|_| malformed())?);
            continue;
        }
        let columns = schema.as_ref().ok_or_else(malformed)?;
        let data = object
            .get("data")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(malformed)?;
        saw_data = true;
        for row in data {
            if rows.len() as u64 >= u64::from(limit) {
                return Err(CubeError::TooManyRows { limit });
            }
            let values = row
                .as_array()
                .filter(|values| values.len() == columns.len())
                .ok_or_else(malformed)?;
            rows.push(
                columns
                    .iter()
                    .zip(values)
                    .map(|(column, value)| cell(column, value))
                    .collect::<Result<_, _>>()?,
            );
        }
    }
    let columns = schema.filter(|_| saw_data).ok_or_else(malformed)?;
    Ok(RawRows::of(columns.into_iter().map(|column| column.name).collect(), rows))
}

fn cell<E>(column: &Column, value: &serde_json::Value) -> CubeResult<Value, E>
where
    E: core::error::Error + 'static,
{
    if value.is_null() {
        return Ok(Value::Null);
    }
    if matches!(column.column_type, ColumnType::Boolean) {
        return value
            .as_bool()
            .map(|flag| Value::Integer(i64::from(flag)))
            .ok_or_else(malformed);
    }
    let text = value.as_str().ok_or_else(malformed)?;
    match column.column_type {
        ColumnType::Int8 | ColumnType::Int32 | ColumnType::Int64 => text
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|cause| CubeError::NotAnInteger { cause }),
        ColumnType::Double => {
            let number = text.parse::<f64>().map_err(|cause| CubeError::NotANumber {
                member: String::from("SQL result column"),
                cause,
            })?;
            let real = Real::parse(number).map_err(|cause| CubeError::NotFinite {
                member: String::from("SQL result column"),
                cause,
            })?;
            if crate::rows::beyond_exact(text) || number.abs() > 9_007_199_254_740_992.0 {
                return Err(CubeError::InexactInteger {
                    member: String::from("SQL result column"),
                });
            }
            Ok(Value::Real(real))
        }
        ColumnType::String | ColumnType::VarStr | ColumnType::Timestamp | ColumnType::Date(true | false) => {
            Ok(Value::Text(String::from(text)))
        }
        ColumnType::Boolean => Err(malformed()),
    }
}
