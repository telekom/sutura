//! Request-local execution facts supplied by an adapter, not a reconstruction of a question.

use crate::model::SourceName;
use crate::warehouse::ParamValue;

/// What a whole answer submitted to its engine.
///
/// SQL remains paired with its ordered bindings. A logical plan is diagnostic text, not SQL or a
/// stable interchange format. Neither promises a replay without the original execution context.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Execution {
    /// The exact parameterized statement and bindings submitted to DuckDB.
    DuckDb {
        source: SourceName,
        sql: String,
        parameters: Vec<ParamValue>,
    },
    /// The DataFusion logical plan handed to the engine, before analysis and optimization.
    DataFusion { logical_plan: String },
}

impl Execution {
    /// Text carried beside the rows. JSON escaping and structural overhead are not counted.
    #[must_use]
    pub fn rendered_byte_len(&self) -> u64 {
        match self {
            Self::DuckDb { source, sql, parameters } => parameters.iter().fold(
                (source.as_str().len() as u64).saturating_add(sql.len() as u64),
                |bytes, parameter| {
                    bytes.saturating_add(match parameter {
                        ParamValue::Text(value) => value.len() as u64,
                        ParamValue::Date(_) => 10,
                    })
                },
            ),
            Self::DataFusion { logical_plan } => logical_plan.len() as u64,
        }
    }
}
