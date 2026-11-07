//! Which declared types fix a column's Arrow type, and a partition's rows typed by them.

use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, SchemaRef};
use sutura_domain::warehouse::{ResultBudget, RowSet, Value};

use super::{declared, fixed, typed};
use crate::OracleError;

fn budget() -> ResultBudget {
    ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024 * 1024).expect("a mebibyte is positive"))
}

fn key_and_region() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("customer_key", DataType::Int64, true),
        Field::new("region", DataType::Utf8, true),
    ]))
}

fn answer(rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(vec![String::from("customer_key"), String::from("region")], rows).expect("a rectangular answer")
}

/// A `NUMBER` fixes `Int64` only where every value it can hold fits one; `BINARY_DOUBLE` and the
/// four character types fix theirs; a `DATE` fixes none.
#[test]
fn only_a_declaration_whose_every_value_reads_one_way_fixes_its_column_type() {
    assert_eq!(fixed(oracledb::DB_TYPE_NUMBER, 10, 0), Some(DataType::Int64));
    assert_eq!(fixed(oracledb::DB_TYPE_NUMBER, 18, 0), Some(DataType::Int64));
    assert_eq!(fixed(oracledb::DB_TYPE_NUMBER, 19, 0), None, "19 digits pass an i64");
    assert_eq!(fixed(oracledb::DB_TYPE_NUMBER, 10, 2), None, "a scale reads as text");
    assert_eq!(fixed(oracledb::DB_TYPE_NUMBER, 0, -127), None, "an unconstrained NUMBER");
    assert_eq!(fixed(oracledb::DB_TYPE_BINARY_DOUBLE, 0, 0), Some(DataType::Float64));
    for text in [
        oracledb::DB_TYPE_VARCHAR,
        oracledb::DB_TYPE_NVARCHAR,
        oracledb::DB_TYPE_CHAR,
        oracledb::DB_TYPE_NCHAR,
    ] {
        assert_eq!(fixed(text, 0, 0), Some(DataType::Utf8), "{text:?}");
    }
    assert_eq!(fixed(oracledb::DB_TYPE_DATE, 0, 0), None);
}

/// One column no declaration fixes leaves the whole split with no one schema.
#[test]
fn a_split_with_one_unfixed_column_describes_no_schema() {
    let fixed_columns = [
        ("customer_key", oracledb::DB_TYPE_NUMBER, 10, 0),
        ("region", oracledb::DB_TYPE_VARCHAR, 0, 0),
    ];
    assert_eq!(declared(fixed_columns), Some(key_and_region()));
    let decimal = [
        ("customer_key", oracledb::DB_TYPE_NUMBER, 10, 0),
        ("amount", oracledb::DB_TYPE_NUMBER, 10, 2),
    ];
    assert_eq!(declared(decimal), None);
}

/// An empty partition and one whose text column is all `NULL` answer the declared schema, which
/// reading by the cells would type `Int64`; the cells themselves are unchanged.
#[test]
fn a_partition_answers_its_declared_schema_whatever_its_cells_hold() {
    let empty = typed(&answer(Vec::new()), &key_and_region(), budget()).expect("an empty partition types");
    assert_eq!(empty.schema(), &key_and_region());
    let rows = vec![vec![Value::Integer(7), Value::Null], vec![Value::Null, Value::Null]];
    let nulls = typed(&answer(rows.clone()), &key_and_region(), budget()).expect("a partition types");
    assert_eq!(nulls.schema(), &key_and_region());
    assert_eq!(nulls.to_rows().expect("the batches decode").rows(), rows.as_slice());
}

/// A cell its column's type cannot hold is refused naming the column, never read as text.
#[test]
fn a_cell_its_declared_type_cannot_hold_is_refused_naming_its_column() {
    let refused = typed(
        &answer(vec![vec![Value::Text(String::from("seven")), Value::Null]]),
        &key_and_region(),
        budget(),
    );
    assert!(
        matches!(refused, Err(OracleError::NotOfItsColumnType { ref column }) if column == "customer_key"),
        "{refused:?}"
    );
}

/// A schema of another width than the rows is refused as a shape, never answered.
#[test]
fn a_schema_of_another_width_than_its_rows_is_refused() {
    let one = Arc::new(Schema::new(vec![Field::new("customer_key", DataType::Int64, true)]));
    let refused = typed(&answer(vec![vec![Value::Integer(7), Value::Null]]), &one, budget());
    assert!(matches!(refused, Err(OracleError::Shape { .. })), "{refused:?}");
}
