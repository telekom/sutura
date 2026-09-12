//! Turning a committed fixture CSV into Postgres tables, for the corpus harness.
//!
//! The example models are files; a relational data system has to be *given* tables before it can
//! answer. The existing importer keeps its historical permissive inference; the conformance path
//! maps the shared exact fixture types without changing that API.

use sutura_domain::model::{ColumnName, InvalidIdentifier, TableName};
#[cfg(feature = "fixtures")]
use sutura_domain::warehouse::csv::{self, FixtureType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PgType {
    Boolean,
    BigInt,
    Numeric { scale: u8 },
    Double,
    Date,
    Text,
}

impl PgType {
    fn sql(self) -> String {
        match self {
            Self::Boolean => String::from("BOOLEAN"),
            Self::BigInt => String::from("BIGINT"),
            Self::Numeric { scale } => format!("NUMERIC(38,{scale})"),
            Self::Double => String::from("DOUBLE PRECISION"),
            Self::Date => String::from("DATE"),
            Self::Text => String::from("TEXT"),
        }
    }

    fn holds(self, value: &str) -> bool {
        let value = value.trim();
        match self {
            Self::Boolean => matches!(value, "true" | "false" | "t" | "f"),
            Self::BigInt => value.parse::<i64>().is_ok(),
            Self::Numeric { .. } => false,
            Self::Double => value.parse::<f64>().is_ok(),
            Self::Date => is_date(value),
            Self::Text => true,
        }
    }
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

/// The inferred schema of one fixture table, plus the copy body.
pub(crate) struct Schema {
    columns: Vec<PgColumn>,
    /// The data rows, header excluded, each already joined into one CSV line.
    body: String,
}

/// One inferred column: its name, its type, and - for a decimal - the observed scale.
struct PgColumn {
    name: String,
    kind: PgType,
}

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
            .map(|column| format!("\"{}\" {}", column.name, column.kind.sql()))
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
        let names: Vec<String> = self.columns.iter().map(|column| format!("\"{}\"", column.name)).collect();
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
/// needs one, and that is the boundary the importer is allowed to hold.
fn split_row(line: &str) -> Vec<String> {
    line.split(',').map(|cell| cell.trim().to_owned()).collect()
}

pub(crate) fn infer_schema(text: &str) -> Result<Schema, InvalidIdentifier> {
    let mut lines = text.lines();
    let names = split_row(lines.next().unwrap_or_default());
    let column_count = names.len();
    for name in &names {
        ColumnName::parse(name)?;
    }
    let mut columns: Vec<ColumnAccumulator> = names
        .into_iter()
        .map(|name| ColumnAccumulator {
            name,
            values: Vec::new(),
        })
        .collect();
    let mut body_rows: Vec<String> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let mut cells = split_row(line);
        cells.resize(column_count, String::new());
        for (index, cell) in cells.iter().enumerate() {
            if let Some(column) = columns.get_mut(index) {
                column.values.push(cell.clone());
            }
        }
        body_rows.push(cells.join(","));
    }
    let mut body = body_rows.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let columns = columns
        .into_iter()
        .map(|column| PgColumn {
            name: column.name,
            kind: [PgType::Boolean, PgType::BigInt, PgType::Double, PgType::Date, PgType::Text]
                .into_iter()
                .find(|kind| column.values.iter().all(|cell| cell.is_empty() || kind.holds(cell)))
                .filter(|_| column.values.iter().any(|cell| !cell.is_empty()))
                .unwrap_or(PgType::Text),
        })
        .collect();
    Ok(Schema { columns, body })
}

#[cfg(feature = "fixtures")]
pub(crate) fn infer_fixture_schema(text: &str) -> Result<Schema, csv::InferenceError> {
    let columns = csv::infer(text)?;
    let mut body = text
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let columns = columns
        .into_iter()
        .map(|column| PgColumn {
            name: String::from(column.name().as_str()),
            kind: match column.kind() {
                FixtureType::Boolean => PgType::Boolean,
                FixtureType::Integer => PgType::BigInt,
                FixtureType::WideInteger => PgType::Numeric { scale: 0 },
                FixtureType::Decimal { scale } => PgType::Numeric { scale },
                FixtureType::Real => PgType::Double,
                FixtureType::Date => PgType::Date,
                FixtureType::Text => PgType::Text,
            },
        })
        .collect();
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
        let kinds: Vec<String> = schema.columns.iter().map(|column| column.kind.sql()).collect();
        assert_eq!(kinds, ["DATE", "BIGINT", "TEXT", "BIGINT", "BOOLEAN", "DOUBLE PRECISION"]);
    }

    #[test]
    fn an_empty_column_does_not_collapse_the_scan() {
        let schema = infer_schema("only\n\n\n").expect("a valid header");
        assert_eq!(schema.columns[0].kind, PgType::Text);
        assert_eq!(schema.body(), "");
    }

    #[test]
    fn a_number_only_column_stays_a_number() {
        let schema = infer_schema("k\n0\n1\n").expect("a valid header");
        assert_eq!(schema.columns[0].kind, PgType::BigInt);
    }

    #[test]
    fn legacy_inference_keeps_its_boolean_and_floating_spellings() {
        let schema = infer_schema("flag,amount\nt,2.4\nf,3.57\n").expect("a valid legacy fixture");
        let kinds: Vec<String> = schema.columns.iter().map(|column| column.kind.sql()).collect();
        assert_eq!(kinds, ["BOOLEAN", "DOUBLE PRECISION"]);
    }

    #[test]
    fn legacy_rows_are_trimmed_padded_and_truncated() {
        let schema = infer_schema("a,b\n 1 \n 2 , 3 , 4 \n").expect("a valid legacy fixture");
        assert_eq!(schema.body(), "1,\n2,3\n");
    }

    #[test]
    #[cfg(feature = "fixtures")]
    fn a_decimal_column_is_exact_not_a_double() {
        // A fixed-point decimal column is typed NUMERIC with the widest observed scale, rather than
        // widened to a double - the whole reason a decimal is its own shared type.
        let schema = infer_fixture_schema("amount\n2.4\n3.57\n").expect("a valid header");
        assert!(
            schema
                .create_statement(&TableName::parse("t").unwrap())
                .contains("NUMERIC(38,2)")
        );
    }

    #[test]
    #[cfg(feature = "fixtures")]
    fn a_header_only_fixture_has_no_phantom_row() {
        let schema = infer_fixture_schema("only\n").expect("a valid header");
        assert_eq!(schema.body(), "");
    }

    #[test]
    #[cfg(feature = "fixtures")]
    fn scientific_notation_uses_the_shared_real_type() {
        let schema = infer_fixture_schema("amount\n1e2\n2e2\n").expect("a valid header");
        assert!(
            schema
                .create_statement(&TableName::parse("t").unwrap())
                .contains("DOUBLE PRECISION")
        );
    }

    #[test]
    fn a_header_that_is_not_a_column_name_is_refused() {
        assert!(infer_schema("orders.amount\n1\n").is_err());
        assert!(infer_schema("\"quoted\"\n1\n").is_err());
    }
}
