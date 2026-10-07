//! What a raw text must be before any of it runs: every statement a `SELECT`, by `DuckDB`'s own
//! parser, and every table function it calls one of [`RAW_TABLE_FUNCTIONS`].
//!
//! The parser is asked through [`SERIALIZED`], so the tree read here is the one the engine itself
//! builds from the same text - not a second grammar that could read it differently.

use serde_json::Value;

/// The table functions a raw statement may call: generators, and reads of the declared database's
/// own catalog.
///
/// **Default-closed.** A table function is code the engine runs, and not every one only reads: some
/// act on the database instance beyond the call, and some run SQL held in a string argument that
/// the tree carries as a value. So a name not listed here, or a qualified one, refuses the whole
/// text.
pub const RAW_TABLE_FUNCTIONS: [&str; 10] = [
    "range",
    "generate_series",
    "unnest",
    "duckdb_tables",
    "duckdb_columns",
    "duckdb_views",
    "duckdb_schemas",
    "duckdb_types",
    "duckdb_constraints",
    "duckdb_indexes",
];

/// The statement the screen runs: the caller's text is BOUND as its one value, never spliced in.
/// `json_serialize_sql` answers every statement's parsed tree, or that one of them is not a `SELECT`
/// or does not parse.
pub(crate) const SERIALIZED: &str = "SELECT json_serialize_sql(?::VARCHAR)::VARCHAR";

/// Why a raw text was refused before any of it ran.
#[derive(Debug, thiserror::Error)]
pub enum NotARead {
    /// `DuckDB`'s parser answered that a statement is not a `SELECT`, or that the text does not
    /// parse - its own message.
    #[error("{0}")]
    Statement(String),
    /// A table function [`RAW_TABLE_FUNCTIONS`] does not list, or a qualified one - named as the
    /// tree spells it.
    #[error("`{0}` is not a table function a raw statement may call")]
    TableFunction(String),
    /// The parser's answer was not the tree it documents: not JSON, nested deeper than `serde_json`
    /// reads, or JSON of another shape (`cause` is `None` then).
    #[error("the parsed statement could not be read")]
    Unreadable {
        #[source]
        cause: Option<serde_json::Error>,
    },
}

/// Refuses `serialized` - [`SERIALIZED`]'s answer - unless every statement parsed and every table
/// function in every statement, at any depth, is listed.
pub(crate) fn screen(serialized: &str) -> Result<(), NotARead> {
    let tree: Value = serde_json::from_str(serialized).map_err(|cause| NotARead::Unreadable { cause: Some(cause) })?;
    match tree.get("error") {
        Some(&Value::Bool(false)) => {}
        Some(&Value::Bool(true)) => {
            let message = tree.get("error_message").and_then(Value::as_str).unwrap_or_default();
            return Err(NotARead::Statement(message.to_owned()));
        }
        _ => return Err(NotARead::Unreadable { cause: None }),
    }
    tree.get("statements")
        .and_then(Value::as_array)
        .ok_or(NotARead::Unreadable { cause: None })?
        .iter()
        .try_for_each(walked)
}

/// Every node, so a table function inside a subquery, a CTE, a set operation or a join is read too.
/// Bounded: `serde_json` refuses a document nested past its recursion limit, as
/// [`NotARead::Unreadable`].
fn walked(node: &Value) -> Result<(), NotARead> {
    match *node {
        Value::Array(ref items) => items.iter().try_for_each(walked),
        Value::Object(ref fields) => {
            if fields.get("type").and_then(Value::as_str) == Some("TABLE_FUNCTION") {
                called(fields.get("function"))?;
            }
            fields.values().try_for_each(walked)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// A table function's call: unqualified, by a name that is listed. A call the tree gives no name, or
/// no empty catalog and schema, is refused with the rest.
fn called(function: Option<&Value>) -> Result<(), NotARead> {
    let field = |key: &str| function.and_then(|call| call.get(key)).and_then(Value::as_str);
    let name = field("function_name").unwrap_or_default();
    let plain = field("catalog") == Some("") && field("schema") == Some("");
    if plain && RAW_TABLE_FUNCTIONS.contains(&name) {
        Ok(())
    } else {
        Err(NotARead::TableFunction(name.to_owned()))
    }
}
