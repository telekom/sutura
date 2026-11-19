//! Bounded execution receipts shared by the native Flight and worker-completion protocols.
//!
//! Private wire DTOs are an internal-protocol exception: two adapters need this pure codec without
//! depending on one another. HTTP/MCP own their public DTOs. This validates shape and identifiers,
//! not the honesty of a peer; byte/count/depth bounds do not bound total process memory.

use std::io::Write as _;

use crate::calendar::Date;
use crate::model::SourceName;
use crate::plan::LegWire;
use crate::warehouse::execution::{Binding, BindingIdentity, BindingValue, Execution, PlanStage};
use crate::warehouse::schema::TypeDialect;

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
#[error("the execution envelope is missing, invalid or outside its bound")]
pub struct InvalidCapture;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    executions: Vec<Record>,
}

type WireSource = Option<String>;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    BigqueryRead {
        source: String,
        descriptor_sha256: [u8; 32],
    },
    Sql {
        source: String,
        dialect: Dialect,
        sql: String,
        parameters: Vec<Parameter>,
    },
    Datafusion {
        #[serde(deserialize_with = "required_source")]
        source: WireSource,
        stage: Stage,
        logical_plan: String,
    },
    Cube {
        source: String,
        query: serde_json::Value,
    },
    Node {
        source: String,
        query: Box<LegWire>,
        parameters: Vec<Parameter>,
    },
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Dialect {
    Duckdb,
    Datafusion,
    Cube,
    Postgres,
    Clickhouse,
    Bigquery,
    Oracle,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Files,
    Combine,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameter {
    binding: Identity,
    value: Value,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case", deny_unknown_fields)]
enum Identity {
    Position(usize),
    Named(String),
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case", deny_unknown_fields)]
enum Value {
    Utf8(String),
    OracleString(String),
    Date32(i32),
    OracleDate(String),
}

fn required_source<'de, D>(deserializer: D) -> Result<WireSource, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer)
}

fn valid_bindings(parameters: &[Binding], dialect: TypeDialect) -> bool {
    parameters.iter().enumerate().all(|(position, parameter)| {
        let identity = match (dialect, parameter.identity()) {
            (TypeDialect::ClickHouse, BindingIdentity::Named(name)) => name == &format!("p{position}"),
            (TypeDialect::ClickHouse, BindingIdentity::Position(_)) | (_, BindingIdentity::Named(_)) => false,
            (_, BindingIdentity::Position(actual)) => *actual == position,
        };
        let value = match (dialect, parameter.value()) {
            (TypeDialect::Oracle, BindingValue::OracleString(_) | BindingValue::OracleDate(_)) => true,
            (TypeDialect::Oracle, BindingValue::Utf8(_) | BindingValue::Date32(_))
            | (_, BindingValue::OracleString(_) | BindingValue::OracleDate(_)) => false,
            (_, BindingValue::Utf8(_) | BindingValue::Date32(_)) => true,
        };
        identity && value
    })
}

impl From<TypeDialect> for Dialect {
    fn from(value: TypeDialect) -> Self {
        match value {
            TypeDialect::DuckDb => Self::Duckdb,
            TypeDialect::DataFusion => Self::Datafusion,
            TypeDialect::Cube => Self::Cube,
            TypeDialect::Postgres => Self::Postgres,
            TypeDialect::ClickHouse => Self::Clickhouse,
            TypeDialect::BigQuery => Self::Bigquery,
            TypeDialect::Oracle => Self::Oracle,
        }
    }
}
impl From<Dialect> for TypeDialect {
    fn from(value: Dialect) -> Self {
        match value {
            Dialect::Duckdb => Self::DuckDb,
            Dialect::Datafusion => Self::DataFusion,
            Dialect::Cube => Self::Cube,
            Dialect::Postgres => Self::Postgres,
            Dialect::Clickhouse => Self::ClickHouse,
            Dialect::Bigquery => Self::BigQuery,
            Dialect::Oracle => Self::Oracle,
        }
    }
}

fn parameters(values: &[Binding]) -> Vec<Parameter> {
    values
        .iter()
        .map(|parameter| Parameter {
            binding: match parameter.identity() {
                BindingIdentity::Position(position) => Identity::Position(*position),
                BindingIdentity::Named(name) => Identity::Named(name.clone()),
            },
            value: match parameter.value() {
                BindingValue::Utf8(text) => Value::Utf8(text.clone()),
                BindingValue::OracleString(text) => Value::OracleString(text.clone()),
                BindingValue::Date32(days) => Value::Date32(*days),
                BindingValue::OracleDate(date) => Value::OracleDate(date.to_iso()),
            },
        })
        .collect()
}

fn bindings(values: Vec<Parameter>) -> Result<Vec<Binding>, InvalidCapture> {
    values
        .into_iter()
        .map(|parameter| {
            let identity = match parameter.binding {
                Identity::Position(position) => BindingIdentity::Position(position),
                Identity::Named(name) => BindingIdentity::Named(name),
            };
            let value = match parameter.value {
                Value::Utf8(text) => BindingValue::Utf8(text),
                Value::OracleString(text) => BindingValue::OracleString(text),
                Value::Date32(days) => BindingValue::Date32(days),
                Value::OracleDate(text) => BindingValue::OracleDate(Date::parse(&text).map_err(|_cause| InvalidCapture)?),
            };
            Ok(Binding::new(identity, value))
        })
        .collect()
}

impl From<&Execution> for Record {
    fn from(value: &Execution) -> Self {
        match value {
            Execution::BigQueryRead {
                source,
                descriptor_sha256,
            } => Self::BigqueryRead {
                source: source.as_str().to_owned(),
                descriptor_sha256: *descriptor_sha256,
            },
            Execution::Sql {
                source,
                dialect,
                sql,
                parameters: values,
            } => Self::Sql {
                source: source.as_str().to_owned(),
                dialect: (*dialect).into(),
                sql: sql.clone(),
                parameters: parameters(values),
            },
            Execution::DataFusion {
                source,
                stage,
                logical_plan,
            } => Self::Datafusion {
                source: source.as_ref().map(|source| source.as_str().to_owned()),
                stage: match stage {
                    PlanStage::Files => Stage::Files,
                    PlanStage::Combine => Stage::Combine,
                },
                logical_plan: logical_plan.clone(),
            },
            Execution::Cube { source, query } => Self::Cube {
                source: source.as_str().to_owned(),
                query: query.clone(),
            },
            Execution::Node {
                source,
                query,
                parameters: values,
            } => Self::Node {
                source: source.as_str().to_owned(),
                query: query.clone(),
                parameters: parameters(values),
            },
        }
    }
}

impl TryFrom<Record> for Execution {
    type Error = InvalidCapture;
    fn try_from(value: Record) -> Result<Self, Self::Error> {
        let source = |name: &str| SourceName::parse(name).map_err(|_cause| InvalidCapture);
        Ok(match value {
            Record::BigqueryRead {
                source: name,
                descriptor_sha256,
            } => Self::BigQueryRead {
                source: source(&name)?,
                descriptor_sha256,
            },
            Record::Sql {
                source: name,
                dialect,
                sql,
                parameters,
            } => {
                let dialect = dialect.into();
                let parameters = bindings(parameters)?;
                if sql.is_empty() || !valid_bindings(&parameters, dialect) {
                    return Err(InvalidCapture);
                }
                Self::Sql {
                    source: source(&name)?,
                    dialect,
                    sql,
                    parameters,
                }
            }
            Record::Datafusion {
                source: name,
                stage,
                logical_plan,
            } => {
                if logical_plan.is_empty() || matches!(stage, Stage::Files) != name.is_some() {
                    return Err(InvalidCapture);
                }
                Self::DataFusion {
                    source: name.as_deref().map(source).transpose()?,
                    stage: match stage {
                        Stage::Files => PlanStage::Files,
                        Stage::Combine => PlanStage::Combine,
                    },
                    logical_plan,
                }
            }
            Record::Cube { source: name, query } => Self::Cube {
                source: source(&name)?,
                query,
            },
            Record::Node {
                source: name,
                query,
                parameters,
            } => {
                let parameters = bindings(parameters)?;
                if !valid_bindings(&parameters, TypeDialect::DuckDb) {
                    return Err(InvalidCapture);
                }
                Self::Node {
                    source: source(&name)?,
                    query,
                    parameters,
                }
            }
        })
    }
}

/// Maximum metadata retained alongside one result, also charged against that result's budget.
#[must_use]
pub fn byte_limit(result_bytes: usize) -> usize {
    result_bytes.min(usize::try_from(crate::query::ResponseByteLimit::DEFAULT.bytes()).unwrap_or(usize::MAX))
}

fn preflight(bytes: &[u8], limit: usize) -> Result<(), InvalidCapture> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(InvalidCapture);
    }
    let (mut depth, mut records, mut quoted, mut escaped) = (0_usize, 0_usize, false, false);
    for &byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    if depth == 2 && byte == b'{' {
                        records = records.saturating_add(1);
                    }
                    depth = depth.saturating_add(1);
                    if depth > 64 || records > (limit >> 6) {
                        return Err(InvalidCapture);
                    }
                }
                b'}' | b']' => depth = depth.checked_sub(1).ok_or(InvalidCapture)?,
                _ => {}
            }
        }
    }
    if depth != 0 || quoted {
        return Err(InvalidCapture);
    }
    Ok(())
}

struct Capped {
    bytes: Vec<u8>,
    limit: usize,
}
impl std::io::Write for Capped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("execution metadata bound"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Encodes a list, including an explicit empty compute completion. Node boundaries require nonempty facts.
///
/// # Errors
/// Invalid or excessive capture. The bound covers encoded metadata, not total process memory.
pub fn encode(executions: &[Execution], limit: usize) -> Result<Vec<u8>, InvalidCapture> {
    if executions.len() > (limit >> 6)
        || executions
            .iter()
            .fold(0_u64, |bytes, execution| bytes.saturating_add(execution.rendered_byte_len()))
            > limit as u64
    {
        return Err(InvalidCapture);
    }
    let envelope = Envelope {
        version: 1,
        executions: executions.iter().map(Record::from).collect(),
    };
    let mut output = Capped {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, &envelope).map_err(|_cause| InvalidCapture)?;
    output.flush().map_err(|_cause| InvalidCapture)?;
    preflight(&output.bytes, limit)?;
    Ok(output.bytes)
}

/// Decodes after checking byte, depth and record bounds, then parses domain identifiers/dates.
///
/// # Errors
/// Missing, malformed, excessive or unknown-version metadata fails; a valid empty completion is explicit.
pub fn decode(bytes: &[u8], limit: usize) -> Result<Vec<Execution>, InvalidCapture> {
    preflight(bytes, limit)?;
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_cause| InvalidCapture)?;
    if envelope.version != 1 || envelope.executions.len() > (limit >> 6) {
        return Err(InvalidCapture);
    }
    envelope.executions.into_iter().map(Execution::try_from).collect()
}
