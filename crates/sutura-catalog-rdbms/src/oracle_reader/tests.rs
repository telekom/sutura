use sutura_domain::identity::Secret;

use super::{OracleLogin, OracleReader, flag, statement};
use crate::postgres_reader::{InvalidReaderConfig, RowPredicate};

fn login() -> OracleLogin {
    OracleLogin::new(
        String::from("127.0.0.1"),
        1521,
        String::from("FREEPDB1"),
        String::from("reader"),
        Secret::new("a-test-password"),
    )
}

fn equals(column: &str) -> RowPredicate {
    RowPredicate::Equals {
        column: String::from(column),
        value: String::from("live'; DROP TABLE t; --"),
    }
}

#[test]
fn the_oracle_reader_refuses_a_schema_or_predicate_column_that_is_not_an_identifier() {
    let reader = |schema: &str, predicate: RowPredicate| {
        OracleReader::new(login(), schema, String::from("prod"), predicate, None, None).map(drop)
    };
    assert_eq!(
        reader("dictionary\"; DROP USER app; --", RowPredicate::None),
        Err(InvalidReaderConfig::Schema)
    );
    assert_eq!(
        reader("dictionary", equals("state\" OR 1=1 --")),
        Err(InvalidReaderConfig::PredicateColumn)
    );
    assert_eq!(reader("dictionary", equals("state")), Ok(()));
}

#[test]
fn the_oracle_statement_quotes_folded_identifiers_and_binds_every_value() {
    let sql = statement("sutura_dictionary", &equals("state"));
    assert!(sql.contains("FROM \"SUTURA_DICTIONARY\".\"COLUMNS\""), "{sql}");
    assert!(
        sql.contains("WHERE is_deleted = 0 AND environment = :1 AND \"STATE\" = :2"),
        "{sql}"
    );
    assert!(
        !sql.contains("DROP"),
        "the equals value is bound, never written into the statement: {sql}"
    );
    assert!(
        statement("d", &RowPredicate::IsNull(String::from("gone"))).contains("= :1 AND \"GONE\" IS NULL ORDER BY"),
        "an is-null predicate binds nothing"
    );
}

#[test]
fn a_primary_key_flag_is_evidence_only_as_zero_or_one() {
    assert_eq!(flag(Some(1)), Some(true));
    assert_eq!(flag(Some(0)), Some(false));
    assert_eq!(flag(Some(2)), None);
    assert_eq!(flag(None), None);
}
