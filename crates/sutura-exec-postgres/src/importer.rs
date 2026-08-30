//! Turning a committed fixture CSV into Postgres tables, for the corpus harness.
//!
//! The example models are files; a relational data system has to be *given* tables before it can
//! answer. This module infers a column type per column from the values, and renders the
//! `CREATE TABLE` and `COPY ... FROM STDIN` statements plus the copy body. Type inference runs every
//! time from the same committed bytes, so a fixture cannot drift from the CSV on disk.

use sutura_domain::model::{ColumnName, InvalidIdentifier, TableName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PgType {
    Boolean,
    BigInt,
    Double,
    Date,
    Text,
}

impl PgType {
    const fn sql(self) -> &'static str {
        match self {
            Self::Boolean => "BOOLEAN",
            Self::BigInt => "BIGINT",
            Self::Double => "DOUBLE PRECISION",
            Self::Date => "DATE",
            Self::Text => "TEXT",
        }
    }

    /// Whether this type can hold the given non-empty cell.
    fn holds(self, value: &str) -> bool {
        let value = value.trim();
        match self {
            Self::Boolean => matches!(value, "true" | "false" | "t" | "f"),
            Self::BigInt => value.parse::<i64>().is_ok(),
            Self::Double => value.parse::<f64>().is_ok(),
            Self::Date => is_date(value),
            // Text is the fallback and can hold anything; chosen only after every narrower type has
            // been ruled out for at least one value in the column.
            Self::Text => true,
        }
    }
}

/// A date-shaped value: `YYYY-MM-DD`.
fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 {
        return false;
    }
    bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

/// The inferred schema of one fixture table, plus the copy body.
pub(crate) struct Schema {
    columns: Vec<(String, PgType)>,
    /// The data rows, header excluded, each already joined into one CSV line.
    body: String,
}

/// One column being accumulated during inference: its name and the cells seen so far.
struct ColumnAccumulator {
    name: String,
    values: Vec<String>,
}

impl Schema {
    /// The `CREATE TABLE` statement, dropping any previous table of the same name first so a run is
    /// idempotent against the last one.
    pub(crate) fn create_statement(&self, table: &TableName) -> String {
        let defines: Vec<String> = self
            .columns
            .iter()
            .map(|(name, kind)| format!("\"{name}\" {}", kind.sql()))
            .collect();
        format!(
            "DROP TABLE IF EXISTS \"{}\"; CREATE TABLE \"{}\" ({})",
            table.as_str(),
            table.as_str(),
            defines.join(", ")
        )
    }

    /// The `COPY ... FROM STDIN` statement.
    pub(crate) fn copy_statement(&self, table: &TableName) -> String {
        let names: Vec<String> = self.columns.iter().map(|(name, _)| format!("\"{name}\"")).collect();
        format!(
            "COPY \"{}\" ({}) FROM STDIN WITH (FORMAT csv)",
            table.as_str(),
            names.join(", ")
        )
    }

    /// The rows to feed through `COPY`, header excluded.
    pub(crate) fn body(&self) -> &str {
        &self.body
    }
}

/// Splits one CSV line on commas. The fixtures carry no quoted commas, embedded newlines or quotes,
/// which is the ceiling this parser accepts - growing a real CSV parser here is the day a fixture
/// needs one, and that is `ponytail:` the boundary the importer is allowed to hold.
fn split_row(line: &str) -> Vec<String> {
    line.split(',').map(|cell| cell.trim().to_owned()).collect()
}

/// Infers the schema of a fixture from its text.
///
/// The column names are a CSV header split on commas and interpolated into `CREATE TABLE` and
/// `COPY` inside double quotes with no other check, so each one is parsed as a [`ColumnName`] first.
/// A repo fixture has no live hole, but a DDL renderer that trusts its input is precisely the
/// "document read off disk" shape the repository refuses to trust.
pub(crate) fn infer_schema(text: &str) -> Result<Schema, InvalidIdentifier> {
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    let names: Vec<String> = split_row(header);
    let column_count = names.len();

    for name in &names {
        ColumnName::parse(name)?;
    }

    let mut accumulators: Vec<ColumnAccumulator> = names
        .iter()
        .map(|name| ColumnAccumulator {
            name: name.clone(),
            values: Vec::new(),
        })
        .collect();
    let mut body_rows: Vec<String> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let mut cells = split_row(line);
        // Pad or truncate to the declared column count, so a ragged row cannot shift where a value
        // lands in a later column.
        cells.resize(column_count, String::new());
        for (index, cell) in cells.iter().enumerate() {
            if let Some(column) = accumulators.get_mut(index) {
                column.values.push(cell.clone());
            }
        }
        body_rows.push(cells.join(","));
    }

    let columns: Vec<(String, PgType)> = accumulators
        .into_iter()
        .map(|column| {
            let type_for = [PgType::Boolean, PgType::BigInt, PgType::Double, PgType::Date, PgType::Text]
                .into_iter()
                .find(|kind| column.values.iter().all(|cell| cell.is_empty() || kind.holds(cell)));
            let kind = match type_for {
                Some(kind) if column.values.iter().any(|cell| !cell.is_empty()) => kind,
                // A column with no data gets the neutral type; letting the Boolean arm claim it
                // would be an arbitrary win for a type nothing needed.
                _ => PgType::Text,
            };
            (column.name, kind)
        })
        .collect();

    let mut body = body_rows.join("\n");
    body.push('\n');
    Ok(Schema { columns, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: &str = "month,subscription_key,status,mrr_cents,churned_in_month,data_gb\n\
        2026-01-01,1001,active,3749,false,2.47\n\
        2026-02-01,1002,active,5000,true,3.5\n";

    #[test]
    fn infers_the_fixture_column_shapes() {
        let schema = infer_schema(ROWS).expect("the fixture headers are valid identifiers");
        let kinds: Vec<&str> = schema.columns.iter().map(|(_, kind)| kind.sql()).collect();
        assert_eq!(kinds, ["DATE", "BIGINT", "TEXT", "BIGINT", "BOOLEAN", "DOUBLE PRECISION"]);
    }

    #[test]
    fn an_empty_column_does_not_collapse_the_scan() {
        let schema = infer_schema("only\n\n\n").expect("a valid header");
        assert_eq!(schema.columns[0].1, PgType::Text);
    }

    #[test]
    fn a_number_only_column_stays_a_number() {
        let schema = infer_schema("k\n0\n1\n").expect("a valid header");
        assert_eq!(schema.columns[0].1, PgType::BigInt);
    }

    #[test]
    fn a_header_that_is_not_a_column_name_is_refused() {
        assert!(infer_schema("orders.amount\n1\n").is_err());
        assert!(infer_schema("\"quoted\"\n1\n").is_err());
    }
}
