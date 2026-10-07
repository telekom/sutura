//! What a raw text must be before any of it runs: every statement a `SELECT`, by `DuckDB`'s own
//! parser, every table function it calls one of [`RAW_TABLE_FUNCTIONS`], and its queries nested no
//! deeper than [`MAX_NESTING`] and no more than [`MAX_QUERIES`] of them.
//!
//! The parser is asked through [`SERIALIZED`], so the tree read here is `DuckDB`'s own serialised
//! parse of the same text the driver then runs. Whether the driver's split of that text into
//! statements always reads it as this parse does is not measured.

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

/// The deepest the queries of a raw text may nest, a reference to a CTE counted as the query it
/// names, at the depth it is referenced from.
///
/// Binding a nest of subqueries takes time that grows faster than its depth, and the plan nests a
/// referenced CTE where the reference is. Every shape measured at this bound on the pinned engine
/// binds in well under a second; not every shape is measured.
pub const MAX_NESTING: u64 = 12;

/// The most queries a raw text may hold, across all its statements and counted the same way: a CTE
/// once where it is defined and once more for every reference to it.
///
/// Queries side by side add up, and so does a CTE referenced more than once. Measured beside
/// [`MAX_NESTING`], with the same limit.
pub const MAX_QUERIES: u64 = 1024;

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
    /// The queries nest deeper than [`MAX_NESTING`].
    #[error("the queries in this text nest {depth} deep, past the {max} a raw text may", max = MAX_NESTING)]
    TooDeep { depth: u64 },
    /// The text holds more queries than [`MAX_QUERIES`].
    #[error("this text holds {queries} queries, past the {max} a raw text may", max = MAX_QUERIES)]
    TooMany { queries: u64 },
    /// The parser's answer was not the tree it documents: not JSON, nested deeper than `serde_json`
    /// reads, or JSON of another shape (`cause` is `None` then).
    #[error("the parsed statement could not be read")]
    Unreadable {
        #[source]
        cause: Option<serde_json::Error>,
    },
}

/// Refuses `serialized` - [`SERIALIZED`]'s answer - unless every statement parsed, every table
/// function in every statement, at any depth, is listed, and the queries stay within
/// [`MAX_NESTING`] and [`MAX_QUERIES`].
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
    let statements = tree.get("statements").filter(|statements| statements.is_array());
    let load = walked(statements.ok_or(NotARead::Unreadable { cause: None })?, &mut Vec::new())?;
    if load.depth > MAX_NESTING {
        return Err(NotARead::TooDeep { depth: load.depth });
    }
    if load.queries > MAX_QUERIES {
        return Err(NotARead::TooMany { queries: load.queries });
    }
    Ok(())
}

/// How deep the queries under a node nest and how many there are, a reference to a CTE in scope
/// counted as that CTE.
#[derive(Clone, Copy, Default)]
struct Load {
    depth: u64,
    queries: u64,
}

impl Load {
    fn beside(self, other: Self) -> Self {
        Self {
            depth: self.depth.max(other.depth),
            queries: self.queries.saturating_add(other.queries),
        }
    }
}

/// Every node, so a table function inside a subquery, a CTE, a set operation or a join is read too,
/// and every query is counted where the engine binds it: a CTE's load is added again wherever a
/// table name matches it, qualified or not, ignoring ASCII case as the engine does. `ctes` holds the
/// CTEs in scope; a CTE sees only those defined before it, and a `cte_map` of any other shape is
/// [`NotARead::Unreadable`], so no part of it goes unwalked. Bounded: `serde_json` refuses a document
/// nested past its recursion limit, as [`NotARead::Unreadable`].
fn walked<'tree>(node: &'tree Value, ctes: &mut Vec<(&'tree str, Load)>) -> Result<Load, NotARead> {
    let fields = match *node {
        Value::Object(ref fields) => fields,
        Value::Array(ref items) => {
            return items
                .iter()
                .try_fold(Load::default(), |load, item| Ok(load.beside(walked(item, ctes)?)));
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => return Ok(Load::default()),
    };
    let outer = ctes.len();
    let mut load = Load::default();
    if let Some(map) = fields.get("cte_map") {
        let shape = map
            .as_object()
            .map(|map| (map.len(), map.get("map").and_then(Value::as_array)));
        let Some((1, Some(defined))) = shape else {
            return Err(NotARead::Unreadable { cause: None });
        };
        for cte in defined {
            let named = walked(cte, ctes)?;
            load = load.beside(named);
            ctes.push((cte.get("key").and_then(Value::as_str).unwrap_or_default(), named));
        }
    }
    for (key, value) in fields {
        if key != "cte_map" {
            load = load.beside(walked(value, ctes)?);
        }
    }
    match fields.get("type").and_then(Value::as_str) {
        Some("TABLE_FUNCTION") => called(fields.get("function"))?,
        Some("BASE_TABLE") => {
            let table = fields.get("table_name").and_then(Value::as_str).unwrap_or_default();
            for &(name, named) in &*ctes {
                if name.eq_ignore_ascii_case(table) {
                    load = load.beside(named);
                }
            }
        }
        Some("SELECT_NODE") => {
            load = Load {
                depth: load.depth.saturating_add(1),
                queries: load.queries.saturating_add(1),
            };
        }
        _ => {}
    }
    ctes.truncate(outer);
    Ok(load)
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
