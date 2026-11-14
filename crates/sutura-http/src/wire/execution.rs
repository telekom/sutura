//! Submitted operations on a certified answer; disclosure, not standalone replay.

use sutura_domain::plan::remote::LegWire;
use sutura_domain::warehouse::execution::{Binding, BindingIdentity, BindingValue, Execution, PlanStage};
use sutura_domain::warehouse::schema::TypeDialect;

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionBody {
    BigqueryRead {
        source: String,
        operation: &'static str,
        descriptor_sha256: String,
    },
    Sql {
        source: String,
        dialect: &'static str,
        sql: String,
        parameters: Vec<ParameterBody>,
    },
    Datafusion {
        source: Option<String>,
        stage: &'static str,
        logical_plan: String,
    },
    Cube {
        source: String,
        query: serde_json::Value,
    },
    Node {
        source: String,
        #[schema(value_type = Object)]
        query: Box<LegWire>,
        parameters: Vec<ParameterBody>,
    },
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ParameterBody {
    binding: IdentityBody,
    value: ValueBody,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum IdentityBody {
    Position(usize),
    Named(String),
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum ValueBody {
    Utf8(String),
    OracleString(String),
    Date32(i32),
    OracleDate(String),
}

fn parameters(values: &[Binding]) -> Vec<ParameterBody> {
    values
        .iter()
        .map(|parameter| ParameterBody {
            binding: match parameter.identity() {
                BindingIdentity::Position(position) => IdentityBody::Position(*position),
                BindingIdentity::Named(name) => IdentityBody::Named(name.clone()),
            },
            value: match parameter.value() {
                BindingValue::Utf8(text) => ValueBody::Utf8(text.clone()),
                BindingValue::OracleString(text) => ValueBody::OracleString(text.clone()),
                BindingValue::Date32(days) => ValueBody::Date32(*days),
                BindingValue::OracleDate(date) => ValueBody::OracleDate(date.to_iso()),
            },
        })
        .collect()
}

pub(super) fn list(executions: &[Execution]) -> Vec<ExecutionBody> {
    executions
        .iter()
        .map(|execution| match execution {
            Execution::BigQueryRead {
                source,
                descriptor_sha256,
            } => ExecutionBody::BigqueryRead {
                source: source.as_str().to_owned(),
                operation: "adbc_read_partition",
                descriptor_sha256: core::fmt::from_fn(|f| descriptor_sha256.iter().try_for_each(|byte| write!(f, "{byte:02x}")))
                    .to_string(),
            },
            Execution::Sql {
                source,
                dialect,
                sql,
                parameters: values,
            } => ExecutionBody::Sql {
                source: source.as_str().to_owned(),
                dialect: match dialect {
                    TypeDialect::DuckDb => "duckdb",
                    TypeDialect::DataFusion => "datafusion",
                    TypeDialect::Cube => "cube",
                    TypeDialect::Postgres => "postgres",
                    TypeDialect::ClickHouse => "clickhouse",
                    TypeDialect::BigQuery => "bigquery",
                    TypeDialect::Oracle => "oracle",
                },
                sql: sql.clone(),
                parameters: parameters(values),
            },
            Execution::DataFusion {
                source,
                stage,
                logical_plan,
            } => ExecutionBody::Datafusion {
                source: source.as_ref().map(|source| source.as_str().to_owned()),
                stage: match stage {
                    PlanStage::Files => "files",
                    PlanStage::Combine => "combine",
                },
                logical_plan: logical_plan.clone(),
            },
            Execution::Cube { source, query } => ExecutionBody::Cube {
                source: source.as_str().to_owned(),
                query: query.clone(),
            },
            Execution::Node {
                source,
                query,
                parameters: values,
            } => ExecutionBody::Node {
                source: source.as_str().to_owned(),
                query: query.clone(),
                parameters: parameters(values),
            },
        })
        .collect()
}
