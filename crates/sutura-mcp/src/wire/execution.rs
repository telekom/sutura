//! Submitted operations on a certified answer; disclosure, not standalone replay.

use sutura_domain::plan::remote::LegWire;
use sutura_domain::warehouse::execution::{Binding, BindingIdentity, BindingValue, Execution, PlanStage};
use sutura_domain::warehouse::schema::TypeDialect;

#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionContent {
    BigqueryRead {
        source: String,
        operation: &'static str,
        descriptor_sha256: String,
    },
    Sql {
        source: String,
        dialect: &'static str,
        sql: String,
        parameters: Vec<ParameterContent>,
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
        query: Box<LegWire>,
        parameters: Vec<ParameterContent>,
    },
}

#[derive(Debug, serde::Serialize)]
pub struct ParameterContent {
    binding: IdentityContent,
    value: ValueContent,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum IdentityContent {
    Position(usize),
    Named(String),
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum ValueContent {
    Utf8(String),
    OracleString(String),
    Date32(i32),
    OracleDate(String),
}

fn parameters(values: &[Binding]) -> Vec<ParameterContent> {
    values
        .iter()
        .map(|parameter| ParameterContent {
            binding: match parameter.identity() {
                BindingIdentity::Position(position) => IdentityContent::Position(*position),
                BindingIdentity::Named(name) => IdentityContent::Named(name.clone()),
            },
            value: match parameter.value() {
                BindingValue::Utf8(text) => ValueContent::Utf8(text.clone()),
                BindingValue::OracleString(text) => ValueContent::OracleString(text.clone()),
                BindingValue::Date32(days) => ValueContent::Date32(*days),
                BindingValue::OracleDate(date) => ValueContent::OracleDate(date.to_iso()),
            },
        })
        .collect()
}

pub(super) fn list(executions: &[Execution]) -> Vec<ExecutionContent> {
    executions
        .iter()
        .map(|execution| match execution {
            Execution::BigQueryRead {
                source,
                descriptor_sha256,
            } => ExecutionContent::BigqueryRead {
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
            } => ExecutionContent::Sql {
                source: source.as_str().to_owned(),
                dialect: match dialect {
                    TypeDialect::DuckDb => "duckdb",
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
            } => ExecutionContent::Datafusion {
                source: source.as_ref().map(|source| source.as_str().to_owned()),
                stage: match stage {
                    PlanStage::Files => "files",
                    PlanStage::Combine => "combine",
                },
                logical_plan: logical_plan.clone(),
            },
            Execution::Cube { source, query } => ExecutionContent::Cube {
                source: source.as_str().to_owned(),
                query: query.clone(),
            },
            Execution::Node {
                source,
                query,
                parameters: values,
            } => ExecutionContent::Node {
                source: source.as_str().to_owned(),
                query: query.clone(),
                parameters: parameters(values),
            },
        })
        .collect()
}

impl ExecutionContent {
    pub(super) fn append_text(&self, out: &mut String) {
        match self {
            Self::BigqueryRead {
                source,
                descriptor_sha256,
                ..
            } => {
                out.push_str("\nnative adbc_read_partition, source: ");
                out.push_str(&super::escaped(source));
                out.push_str(", descriptor SHA-256: ");
                out.push_str(descriptor_sha256);
            }
            Self::Sql {
                source,
                dialect,
                sql,
                parameters,
            } => {
                out.push_str("\nexecuted SQL (");
                out.push_str(dialect);
                out.push_str("), source: ");
                out.push_str(&super::escaped(source));
                out.push_str("\n  ");
                out.push_str(&super::escaped(sql));
                for parameter in parameters {
                    out.push_str("\n  bind ");
                    out.push_str(&super::escaped(&format!("{:?}: {:?}", parameter.binding, parameter.value)));
                }
            }
            Self::Datafusion { logical_plan, stage, .. } => {
                out.push_str("\nDataFusion logical plan (");
                out.push_str(stage);
                out.push_str("):\n  ");
                out.push_str(&super::escaped(logical_plan));
            }
            Self::Cube { query, .. } => {
                out.push_str("\nCube query: ");
                out.push_str(&super::escaped(&query.to_string()));
            }
            Self::Node { source, .. } => {
                out.push_str("\nNode submission (native query and bindings in structured content), source: ");
                out.push_str(&super::escaped(source));
            }
        }
    }
}
