//! A catalog's column type, read as an Arrow type in the spelling of one data system.

use arrow_schema::{DataType, TimeUnit};

use crate::catalog::ColumnType;

/// The spelling of a column type: which data system the source is.
///
/// The same word means different types across systems (`INTEGER` is 32 bits in Postgres and 64 in
/// `BigQuery`, `DATE` carries a time in Oracle), so the mapping is per dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypeDialect {
    DuckDb,
    Postgres,
    ClickHouse,
    BigQuery,
    Oracle,
}

impl TypeDialect {
    /// The Arrow type a declared type names here, or `None` for a spelling this list does not carry.
    ///
    /// **A closed list, and `None` is the answer for the rest** - a nested, geographic, interval or
    /// JSON type, a decimal wider than 38 digits, an unbounded Oracle `NUMBER`. The caller makes
    /// that column `Utf8` and records it. It is read case-folded, with the parameter list read only
    /// for a decimal's precision and scale.
    #[must_use]
    pub fn arrow(self, declared: &ColumnType) -> Option<DataType> {
        self.read(&declared.as_str().to_ascii_lowercase())
    }

    fn read(self, text: &str) -> Option<DataType> {
        let (name, parameters) = match text.split_once('(') {
            Some((name, rest)) => (name.trim_end(), Some(rest.strip_suffix(')')?)),
            None => (text, None),
        };
        match name {
            "boolean" | "bool" => Some(DataType::Boolean),
            "text" | "string" | "varchar" | "char" | "character" | "character varying" | "nvarchar" | "nchar" => {
                Some(DataType::Utf8)
            }
            _ => match self {
                Self::ClickHouse => clickhouse(name, parameters),
                Self::BigQuery => bigquery(name, parameters),
                Self::Oracle => oracle(name, parameters),
                Self::DuckDb | Self::Postgres => postgres(name, parameters),
            },
        }
    }
}

fn micros(zone: Option<&str>) -> DataType {
    DataType::Timestamp(TimeUnit::Microsecond, zone.map(Into::into))
}

fn clickhouse(name: &str, parameters: Option<&str>) -> Option<DataType> {
    match name {
        "nullable" | "lowcardinality" => TypeDialect::ClickHouse.read(parameters?),
        "int8" => Some(DataType::Int8),
        "int16" => Some(DataType::Int16),
        "int32" => Some(DataType::Int32),
        "int64" => Some(DataType::Int64),
        "uint8" => Some(DataType::UInt8),
        "uint16" => Some(DataType::UInt16),
        "uint32" => Some(DataType::UInt32),
        "uint64" => Some(DataType::UInt64),
        "float32" => Some(DataType::Float32),
        "float64" => Some(DataType::Float64),
        "date" | "date32" => Some(DataType::Date32),
        "datetime" => Some(DataType::Timestamp(TimeUnit::Second, None)),
        "decimal" => parameters.and_then(decimal),
        _ => None,
    }
}

fn bigquery(name: &str, parameters: Option<&str>) -> Option<DataType> {
    match name {
        "int64" | "int" | "integer" | "smallint" | "bigint" | "tinyint" | "byteint" => Some(DataType::Int64),
        "float64" => Some(DataType::Float64),
        "numeric" | "decimal" => parameters.map_or(Some(DataType::Decimal128(38, 9)), decimal),
        "bytes" => Some(DataType::Binary),
        "date" => Some(DataType::Date32),
        "timestamp" => Some(micros(Some("UTC"))),
        "datetime" => Some(micros(None)),
        _ => None,
    }
}

fn oracle(name: &str, parameters: Option<&str>) -> Option<DataType> {
    match name {
        "varchar2" | "nvarchar2" | "clob" => Some(DataType::Utf8),
        "number" => parameters.and_then(decimal),
        "integer" | "int" | "smallint" => Some(DataType::Decimal128(38, 0)),
        "binary_float" => Some(DataType::Float32),
        "float" | "binary_double" => Some(DataType::Float64),
        "date" => Some(DataType::Timestamp(TimeUnit::Second, None)),
        "timestamp" => Some(micros(None)),
        "raw" | "blob" => Some(DataType::Binary),
        _ => None,
    }
}

fn postgres(name: &str, parameters: Option<&str>) -> Option<DataType> {
    match name {
        "smallint" | "int2" => Some(DataType::Int16),
        "int" | "integer" | "int4" => Some(DataType::Int32),
        "bigint" | "int8" => Some(DataType::Int64),
        "real" | "float4" => Some(DataType::Float32),
        "double precision" | "float8" | "double" => Some(DataType::Float64),
        "numeric" | "decimal" => parameters.and_then(decimal),
        "date" => Some(DataType::Date32),
        "timestamp" | "timestamp without time zone" => Some(micros(None)),
        "timestamptz" | "timestamp with time zone" => Some(micros(Some("UTC"))),
        "bytea" | "blob" => Some(DataType::Binary),
        _ => None,
    }
}

/// A decimal from its `precision` or `precision, scale` list, `None` past `Decimal128`'s 38 digits.
fn decimal(parameters: &str) -> Option<DataType> {
    let (precision, scale) = parameters.split_once(',').map_or((parameters, "0"), |(p, s)| (p, s));
    let (precision, scale) = (precision.trim().parse::<u8>().ok()?, scale.trim().parse::<i8>().ok()?);
    ((1..=38).contains(&precision) && scale >= 0 && scale.unsigned_abs() <= precision)
        .then_some(DataType::Decimal128(precision, scale))
}
