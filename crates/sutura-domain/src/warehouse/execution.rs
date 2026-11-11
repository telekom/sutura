//! Request-local submissions, captured by the adapter that submitted them.

pub mod wire;

use sha2::Digest as _;

use crate::calendar::Date;
use crate::model::SourceName;
use crate::plan::remote::LegWire;
use crate::warehouse::ParamValue;
use crate::warehouse::schema::TypeDialect;

/// Actual engine submissions in logical execution order. These disclose execution context;
/// they neither attest a remote engine cryptographically nor promise standalone replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Execution {
    /// A native stream read identified by its descriptor's digest; not SQL or a replay token.
    BigQueryRead { source: SourceName, descriptor_sha256: [u8; 32] },
    Sql { source: SourceName, dialect: TypeDialect, sql: String, parameters: Vec<Binding> },
    /// Diagnostic logical plan text, never physical-plan debug output.
    DataFusion { source: Option<SourceName>, stage: PlanStage, logical_plan: String },
    Cube { source: SourceName, query: serde_json::Value },
    Node { source: SourceName, query: LegWire, parameters: Vec<Binding> },
}

/// The operation at which a DataFusion logical plan was submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStage { Files, Combine }

/// A binding's identity and driver representation, in submission order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding { identity: BindingIdentity, value: BindingValue }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingIdentity { Position(usize), Named(String) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingValue { Utf8(String), OracleString(String), Date32(i32), OracleDate(Date) }

impl Binding {
    #[must_use]
    pub const fn new(identity: BindingIdentity, value: BindingValue) -> Self { Self { identity, value } }
    #[must_use]
    pub const fn identity(&self) -> &BindingIdentity { &self.identity }
    #[must_use]
    pub const fn value(&self) -> &BindingValue { &self.value }
}

impl Execution {
    /// Identifies the actual native ADBC argument without disclosing the opaque descriptor.
    #[must_use]
    pub fn bigquery_read(source: SourceName, descriptor: &[u8]) -> Self {
        Self::BigQueryRead { source, descriptor_sha256: sha2::Sha256::digest(descriptor).into() }
    }

    /// Records the final submitted SQL with the adapter's binding convention.
    #[must_use]
    pub fn sql(source: SourceName, dialect: TypeDialect, sql: String, params: &[ParamValue]) -> Self {
        let parameters = Self::bindings(params, dialect);
        Self::Sql { source, dialect, sql, parameters }
    }

    #[must_use]
    pub fn bindings(params: &[ParamValue], dialect: TypeDialect) -> Vec<Binding> {
        params.iter().enumerate().map(|(position, parameter)| {
            let identity = if dialect == TypeDialect::ClickHouse {
                BindingIdentity::Named(format!("p{position}"))
            } else { BindingIdentity::Position(position) };
            let value = match parameter {
                ParamValue::Text(text) if dialect == TypeDialect::Oracle => BindingValue::OracleString(text.clone()),
                ParamValue::Text(text) => BindingValue::Utf8(text.clone()),
                ParamValue::Date(date) if dialect == TypeDialect::Oracle => BindingValue::OracleDate(*date),
                ParamValue::Date(date) => BindingValue::Date32(date.days_since_epoch()),
            };
            Binding::new(identity, value)
        }).collect()
    }

    /// Text beside the rows. JSON escaping and structural overhead are not counted.
    #[must_use]
    pub fn rendered_byte_len(&self) -> u64 {
        fn parameters_len(parameters: &[Binding]) -> u64 {
            parameters.iter().fold(0_u64, |bytes, parameter| {
                let identity = match parameter.identity() {
                    BindingIdentity::Position(_) => 20,
                    BindingIdentity::Named(name) => name.len() as u64,
                };
                let value = match parameter.value() {
                    BindingValue::Utf8(text) | BindingValue::OracleString(text) => text.len() as u64,
                    BindingValue::Date32(_) => 11,
                    BindingValue::OracleDate(_) => 10,
                };
                bytes.saturating_add(identity).saturating_add(value)
            })
        }
        match self {
            Self::BigQueryRead { source, .. } => (source.as_str().len() as u64).saturating_add(64),
            Self::Sql { source, sql, parameters, .. } => (source.as_str().len() as u64)
                .saturating_add(sql.len() as u64).saturating_add(parameters_len(parameters)),
            Self::DataFusion { source, logical_plan, .. } => (logical_plan.len() as u64)
                .saturating_add(source.as_ref().map_or(0, |name| name.as_str().len() as u64)),
            Self::Cube { source, query } => (source.as_str().len() as u64).saturating_add(query.to_string().len() as u64),
            Self::Node { source, query, parameters } => (source.as_str().len() as u64)
                .saturating_add(serde_json::to_value(query).map_or(u64::MAX, |value| value.to_string().len() as u64))
                .saturating_add(parameters_len(parameters)),
        }
    }
}

/// A successful production result without the native submissions that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the successful result carried no execution submissions")]
pub struct MissingExecution;
