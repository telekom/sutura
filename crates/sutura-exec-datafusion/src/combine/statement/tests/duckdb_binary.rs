//! `DuckDB` takes the raw condition only for a column its pin says compares byte for byte.

use std::collections::HashMap;

use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;

use super::super::text_prefilter::discover;
use super::*;

/// A `BODY` column with an acknowledged raw prefilter, pinned under `collation` as the pin writes it.
fn notes(dialect: TypeDialect, collation: Option<&str>, replacement: &str) -> StatementTable {
    let policy = serde_json::json!({
        "max_text_bytes": 128,
        "redact": [{"pattern": "secret[^ ]*", "replace": replacement}],
        "raw_prefilter": {"acknowledged_because": "accepted secret-dependent timing"},
    });
    let mut metadata = HashMap::from([(String::from("sutura:text_policy"), policy.to_string())]);
    if let Some(collation) = collation {
        drop(metadata.insert(String::from("sutura:collation"), String::from(collation)));
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

fn prefiltered(dialect: TypeDialect, collation: Option<&str>, replacement: &str) -> bool {
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%TERM%'";
    let statement = Parser::parse_sql(&GenericDialect, sql)
        .expect("generated SQL parses")
        .pop()
        .expect("one statement");
    discover(&statement, &[notes(dialect, collation, replacement)], dialect)
        .expect("discovery reads the pinned policy")
        .is_some()
}

#[test]
fn a_duckdb_column_takes_the_raw_condition_only_when_pinned_binary() {
    assert!(prefiltered(TypeDialect::DuckDb, Some("binary"), "[masked]"));
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
        assert!(!prefiltered(TypeDialect::DuckDb, pinned, "[masked]"), "{pinned:?}");
    }
}

#[test]
fn another_arm_reads_no_collation_fact() {
    assert!(prefiltered(TypeDialect::DataFusion, None, "[masked]"));
}

/// A wrong `binary` pin on a case-insensitive column must keep a row at worst, never drop one: the
/// replacement `term` is a match for `TERM` there, and which row survived would say what the secret held.
#[test]
fn a_replacement_that_differs_only_in_case_never_passes_the_proof_where_a_collation_may_apply() {
    assert!(!prefiltered(TypeDialect::DuckDb, Some("binary"), "term"));
    assert!(!prefiltered(TypeDialect::DuckDb, Some("binary"), "[tErM]"));
    assert!(prefiltered(TypeDialect::DuckDb, Some("binary"), "[masked]"));
    assert!(
        prefiltered(TypeDialect::DataFusion, None, "term"),
        "a binary-only arm keeps the exact comparison"
    );
}

/// `K` is also the Kelvin sign's lowercase, which a case fold of non-ASCII text would match: a
/// non-ASCII replacement never passes the proof, whatever the collation.
#[test]
fn a_non_ascii_replacement_never_passes_the_proof() {
    let kelvin = "[\u{212A}]";
    let sql = "SELECT count(\"BODY\") FROM \"notes\" WHERE \"BODY\" LIKE '%K%'";
    let statement = Parser::parse_sql(&GenericDialect, sql)
        .expect("generated SQL parses")
        .pop()
        .expect("one statement");
    for (dialect, collation) in [(TypeDialect::DuckDb, Some("binary")), (TypeDialect::DataFusion, None)] {
        let table = notes(dialect, collation, kelvin);
        assert!(
            discover(&statement, &[table], dialect)
                .expect("discovery reads the pinned policy")
                .is_none(),
            "{dialect:?}"
        );
    }
}
