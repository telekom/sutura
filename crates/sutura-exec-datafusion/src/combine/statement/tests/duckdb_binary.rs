//! `DuckDB` takes the raw condition only for a column its pin says compares byte for byte.

use std::collections::HashMap;

use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;
use sutura_domain::catalog::COLLATION_METADATA_KEY;

use super::super::text_prefilter::discover;
use super::*;

/// A `BODY` column with an acknowledged raw prefilter, pinned under `collation` as the pin writes it.
fn notes(dialect: TypeDialect, collation: Option<&str>) -> StatementTable {
    let policy = serde_json::json!({
        "max_text_bytes": 128,
        "redact": [{"pattern": "secret[^ ]*", "replace": "[masked]"}],
        "raw_prefilter": {"acknowledged_because": "accepted secret-dependent timing"},
    });
    let mut metadata = HashMap::from([(String::from("sutura:text_policy"), policy.to_string())]);
    if let Some(collation) = collation {
        drop(metadata.insert(String::from(COLLATION_METADATA_KEY), String::from(collation)));
    }
    StatementTable::new(
        named("data"),
        QualifiedTable::parse("notes").expect("a table"),
        dialect,
        Arc::new(Schema::new(vec![
            Field::new("BODY", DataType::Utf8, true).with_metadata(metadata),
        ])),
    )
}

fn prefiltered(dialect: TypeDialect, collation: Option<&str>) -> bool {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let statement = Parser::parse_sql(&GenericDialect, sql)
        .expect("generated SQL parses")
        .pop()
        .expect("one statement");
    discover(&statement, &[notes(dialect, collation)], dialect)
        .expect("discovery reads the pinned policy")
        .is_some()
}

#[test]
fn a_duckdb_column_takes_the_raw_condition_only_when_pinned_binary() {
    assert!(prefiltered(TypeDialect::DuckDb, Some("binary")));
    // `NOCASE` and `NOACCENT` are consulted by `LIKE` and survive `REGEXP_REPLACE`; an unreadable
    // fact and a missing one are no evidence either.
    for pinned in [
        Some("named:NOCASE"),
        Some("named:NOACCENT"),
        Some("named:de"),
        Some("BINARY"),
        Some("unreadable"),
        None,
    ] {
        assert!(!prefiltered(TypeDialect::DuckDb, pinned), "{pinned:?}");
    }
}

#[test]
fn another_arm_reads_no_collation_fact() {
    assert!(prefiltered(TypeDialect::DataFusion, None));
}
