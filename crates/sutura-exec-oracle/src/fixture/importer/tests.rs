use sutura_domain::model::{ColumnName, TableName};
use sutura_domain::warehouse::csv::InferenceError;

use super::{Cell, OracleType, Table, Unloadable, rows};

/// One column of every classification, one row of values, one of the other spellings, one empty.
const EVERY_TYPE: &str = "id,flag,small,wide,ratio,real,day,label\n\
    1,true,10,18446744073709551615,2.47,1e2,2026-01-01,alpha\n\
    2,FALSE,-20,0,0.5,-0.25,2026-02-28,beta\n\
    3,,,,,,,\n";

fn table(text: &str) -> Table {
    Table::parse(&TableName::parse("t").expect("a table name"), text).expect("the fixture loads")
}

#[test]
fn each_classification_is_declared_as_the_oracle_type_the_header_names() {
    assert_eq!(
        table(EVERY_TYPE).create_statement(),
        "CREATE TABLE \"t\" (\"id\" NUMBER(19,0), \"flag\" BOOLEAN, \"small\" NUMBER(19,0), \"wide\" NUMBER(38,0), \
         \"ratio\" BINARY_DOUBLE, \"real\" BINARY_DOUBLE, \"day\" DATE, \"label\" VARCHAR2(4000))"
    );
    assert_eq!(table(EVERY_TYPE).drop_statement(), "DROP TABLE IF EXISTS \"t\" PURGE");
}

#[test]
fn the_insert_binds_by_position_and_parses_only_a_date_under_an_explicit_format() {
    assert_eq!(
        table(EVERY_TYPE).insert_statement().as_deref(),
        Some(
            "INSERT INTO \"t\" (\"id\", \"flag\", \"small\", \"wide\", \"ratio\", \"real\", \"day\", \"label\") \
             VALUES (:1, :2, :3, :4, :5, :6, TO_DATE(:7, 'YYYY-MM-DD'), :8)"
        )
    );
}

#[test]
fn a_cell_is_bound_as_its_columns_type_in_any_spelling_of_it() {
    let loaded = table(EVERY_TYPE);
    assert_eq!(
        loaded.rows.first(),
        Some(&vec![
            Cell::Integer(Some(1)),
            Cell::Boolean(Some(true)),
            Cell::Integer(Some(10)),
            Cell::Wide(Some(u64::MAX)),
            Cell::Double(Some(2.47)),
            Cell::Double(Some(100.0)),
            Cell::Text(Some(String::from("2026-01-01"))),
            Cell::Text(Some(String::from("alpha"))),
        ])
    );
    assert_eq!(
        loaded.rows.get(1),
        Some(&vec![
            Cell::Integer(Some(2)),
            Cell::Boolean(Some(false)),
            Cell::Integer(Some(-20)),
            Cell::Wide(Some(0)),
            Cell::Double(Some(0.5)),
            Cell::Double(Some(-0.25)),
            Cell::Text(Some(String::from("2026-02-28"))),
            Cell::Text(Some(String::from("beta"))),
        ])
    );
}

#[test]
fn an_empty_cell_is_null_of_its_columns_type() {
    assert_eq!(
        table(EVERY_TYPE).rows.get(2),
        Some(&vec![
            Cell::Integer(Some(3)),
            Cell::Boolean(None),
            Cell::Integer(None),
            Cell::Wide(None),
            Cell::Double(None),
            Cell::Double(None),
            Cell::Text(None),
            Cell::Text(None),
        ])
    );
}

#[test]
fn a_cell_that_is_not_its_columns_type_is_not_a_cell() {
    assert_eq!(Cell::parse(OracleType::Boolean, "maybe"), None);
    assert_eq!(Cell::parse(OracleType::Double, "abc"), None);
    assert_eq!(Cell::parse(OracleType::WideInteger, "-1"), None);
}

#[test]
fn an_integer_column_holds_every_i64_and_a_wider_integer_is_the_wide_classification() {
    let name = TableName::parse("t").expect("a table name");
    let extremes = Table::parse(&name, "n\n9223372036854775807\n-9223372036854775808\n").expect("every i64 fits");
    assert_eq!(extremes.create_statement(), "CREATE TABLE \"t\" (\"n\" NUMBER(19,0))");
    assert_eq!(
        extremes.rows,
        vec![vec![Cell::Integer(Some(i64::MAX))], vec![Cell::Integer(Some(i64::MIN))]]
    );
    let wider = Table::parse(&name, "n\n9223372036854775808\n").expect("a u64 fits");
    assert_eq!(wider.create_statement(), "CREATE TABLE \"t\" (\"n\" NUMBER(38,0))");
}

#[test]
fn an_integer_wider_than_its_column_is_refused_by_line_and_column() {
    let columns = [(ColumnName::parse("n").expect("a column name"), OracleType::Integer)];
    let refused = rows("n\n1\n9223372036854775808\n", &columns);
    let Err(Unloadable::Cell {
        line,
        column,
        cell,
        declared,
    }) = refused
    else {
        panic!("an integer beyond i64 does not bind as an Integer column: {refused:?}");
    };
    assert_eq!(
        (line, column.as_str(), cell.as_str(), declared),
        (3, "n", "9223372036854775808", "NUMBER(19,0)")
    );
}

#[test]
fn quoted_csv_syntax_is_the_shared_classifications_refusal_not_a_load() {
    let refused = Table::parse(&TableName::parse("t").expect("a table name"), "a\n\"x\"\n");
    assert!(
        matches!(refused, Err(Unloadable::Schema(InferenceError::UnsupportedQuotedSyntax))),
        "{refused:?}"
    );
}

#[test]
fn a_header_only_fixture_is_a_table_with_nothing_to_insert() {
    let loaded = table("only\n");
    assert_eq!(loaded.create_statement(), "CREATE TABLE \"t\" (\"only\" VARCHAR2(4000))");
    assert_eq!(loaded.insert_statement(), None);
}
