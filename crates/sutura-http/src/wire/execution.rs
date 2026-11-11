//! Execution details on a certified answer. Raw statements and connection controls never enter it.

use sutura_domain::warehouse::execution::Execution;
use sutura_domain::warehouse::ParamValue;

/// The exact parameterized DuckDB statement submitted for a whole-source answer.
///
/// Bind parameters in array order using their declared types. The original table attachments and
/// execution context are still required; the SQL alone is not an independently runnable query.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ExecutedSqlBody {
    source: String,
    dialect: &'static str,
    sql: String,
    parameters: Vec<ParameterBody>,
}

/// One positional binding, with the type submitted to the driver.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum ParameterBody {
    Text(String),
    Date(String),
}

pub(super) fn sql(execution: Option<&Execution>) -> Option<ExecutedSqlBody> {
    match execution? {
        Execution::DuckDb { source, sql, parameters } => Some(ExecutedSqlBody {
            source: source.as_str().to_owned(),
            dialect: "duckdb",
            sql: sql.clone(),
            parameters: parameters.iter().map(|value| match value {
                ParamValue::Text(text) => ParameterBody::Text(text.clone()),
                ParamValue::Date(date) => ParameterBody::Date(date.to_iso()),
            }).collect(),
        }),
        Execution::DataFusion { .. } => None,
    }
}

pub(super) fn logical_plan(execution: Option<&Execution>) -> Option<String> {
    match execution? {
        Execution::DataFusion { logical_plan } => Some(logical_plan.clone()),
        Execution::DuckDb { .. } => None,
    }
}
