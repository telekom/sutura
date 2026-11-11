//! Execution details on a certified answer. Raw statements and connection controls never enter it.

use sutura_domain::warehouse::execution::Execution;
use sutura_domain::warehouse::ParamValue;

/// The exact parameterized DuckDB statement submitted for a whole-source answer.
///
/// Bind parameters in array order using their declared types. The original table attachments and
/// execution context are still required; the SQL alone is not an independently runnable query.
#[derive(Debug, serde::Serialize)]
pub struct ExecutedSqlContent {
    source: String,
    dialect: &'static str,
    sql: String,
    parameters: Vec<ParameterContent>,
}

/// One positional binding, with the type submitted to the driver.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum ParameterContent {
    Text(String),
    Date(String),
}

pub(super) fn sql(execution: Option<&Execution>) -> Option<ExecutedSqlContent> {
    match execution? {
        Execution::DuckDb { source, sql, parameters } => Some(ExecutedSqlContent {
            source: source.as_str().to_owned(),
            dialect: "duckdb",
            sql: sql.clone(),
            parameters: parameters.iter().map(|value| match value {
                ParamValue::Text(text) => ParameterContent::Text(text.clone()),
                ParamValue::Date(date) => ParameterContent::Date(date.to_iso()),
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

impl ExecutedSqlContent {
    pub(super) fn append_text(&self, out: &mut String) {
        out.push_str("\nexecuted SQL (duckdb), source: ");
        out.push_str(&super::escaped(&self.source));
        out.push_str("\n  ");
        out.push_str(&super::escaped(&self.sql));
        for parameter in &self.parameters {
            let (kind, value) = match parameter {
                ParameterContent::Text(value) => ("text", value),
                ParameterContent::Date(value) => ("date", value),
            };
            out.push_str("\n  bind ");
            out.push_str(kind);
            out.push_str(": ");
            out.push_str(&super::escaped(value));
        }
    }
}
