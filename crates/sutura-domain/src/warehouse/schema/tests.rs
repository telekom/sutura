use std::collections::BTreeMap;

use arrow_schema::{DataType, TimeUnit};

use super::{PinnedSchemas, TypeDialect};
use crate::catalog::{Column, ColumnType, Definitions, Description, JoinKey, JoinKeys, Model, Relationship};
use crate::model::{ColumnName, JoinType, ModelName, RelationshipName, SourceName, TableName};
use crate::pinned::NotValidated;

fn column(name: &str, declared: Option<&str>) -> Column {
    let name = ColumnName::parse(name).expect("a test column is a column");
    Column::from_metadata(name, declared, None, None).expect("a test column has no description to refuse")
}

fn model(name: &str, source: &str, columns: Vec<Column>) -> Model {
    Model::new(
        ModelName::parse(name).expect("a test model is a model"),
        SourceName::parse(source).expect("a test source is a source"),
        TableName::parse(name).expect("a test table is a table"),
        columns,
        Description::default(),
    )
}

/// `orders` on one source joined to `customers` on `customers_source`.
fn bundle(orders: Vec<Column>, customers: Vec<Column>, customers_source: &str) -> Definitions {
    let key = JoinKey::Equal {
        origin: ColumnName::parse("customer_id").expect("a test column is a column"),
        target: ColumnName::parse("id").expect("a test column is a column"),
    };
    let relationship = Relationship::new(
        RelationshipName::parse("orders_customer").expect("a test relationship is a relationship"),
        ModelName::parse("orders").expect("a test model is a model"),
        ModelName::parse("customers").expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![key]).expect("a test relationship declares one key"),
    );
    Definitions::assemble(
        vec![
            model("orders", "lake", orders),
            model("customers", customers_source, customers),
        ],
        vec![relationship],
        vec![],
    )
    .expect("a test bundle holds together")
}

fn dialects() -> BTreeMap<SourceName, TypeDialect> {
    ["lake", "crm"]
        .into_iter()
        .map(|source| {
            (
                SourceName::parse(source).expect("a test source is a source"),
                TypeDialect::Postgres,
            )
        })
        .collect()
}

fn typed_customers() -> Vec<Column> {
    vec![column("id", Some("bigint"))]
}

#[test]
fn a_type_the_dialect_does_not_list_is_a_recorded_utf8_column_and_never_dropped() {
    let orders = vec![
        column("customer_id", Some("bigint")),
        column("where_at", Some("GEOGRAPHY")),
        column("note", None),
    ];
    let pinned = PinnedSchemas::pin(&bundle(orders, typed_customers(), "crm"), &dialects()).expect("a typed bundle pins");
    let orders = ModelName::parse("orders").expect("a test model is a model");
    let schema = pinned.of(&orders).expect("a federated model is pinned");
    let types: Vec<_> = schema
        .fields()
        .iter()
        .map(|field| (field.name().as_str(), field.data_type().clone()))
        .collect();
    assert_eq!(
        types,
        [
            ("customer_id", DataType::Int64),
            ("note", DataType::Utf8),
            ("where_at", DataType::Utf8)
        ]
    );
    let fell_back: Vec<_> = pinned.fell_back(&orders).map(ColumnName::as_str).collect();
    assert_eq!(fell_back, ["note", "where_at"]);
}

#[test]
fn two_pins_that_differ_only_in_a_fallback_have_different_digests() {
    let pin = |declared| {
        let orders = vec![column("customer_id", Some("bigint")), column("note", Some(declared))];
        PinnedSchemas::pin(&bundle(orders, typed_customers(), "crm"), &dialects()).expect("a typed bundle pins")
    };
    let (listed, unlisted) = (pin("text"), pin("GEOGRAPHY"));
    let orders = ModelName::parse("orders").expect("a test model is a model");
    assert_eq!(listed.of(&orders), unlisted.of(&orders), "both pin as Utf8");
    assert_eq!(listed.fell_back(&orders).count(), 0);
    assert_eq!(unlisted.fell_back(&orders).count(), 1);
    assert_ne!(listed.digest(), unlisted.digest());
}

#[test]
fn a_federated_model_with_no_column_type_is_refused_by_name_and_a_local_one_is_not() {
    let untyped = || vec![column("id", None)];
    let orders = || vec![column("customer_id", Some("bigint"))];
    let refused = PinnedSchemas::pin(&bundle(orders(), untyped(), "crm"), &dialects()).expect_err("an untyped federated model");
    let NotValidated::FederatedModelUntyped { ref model } = refused else {
        panic!("a federated model with no type is refused as untyped, not {refused:?}");
    };
    assert_eq!(model.as_str(), "customers");
    let local = PinnedSchemas::pin(&bundle(orders(), untyped(), "lake"), &dialects()).expect("both models share a source");
    assert_eq!(local, PinnedSchemas::default());
}

#[test]
fn each_dialect_reads_a_type_in_its_own_spelling_and_leaves_the_rest_unread() {
    use TypeDialect::{BigQuery, ClickHouse, DuckDb, Oracle, Postgres};
    let micros = |zone: Option<&str>| DataType::Timestamp(TimeUnit::Microsecond, zone.map(Into::into));
    let seconds = DataType::Timestamp(TimeUnit::Second, None);
    let cases = [
        (Postgres, "BIGINT", Some(DataType::Int64)),
        (Postgres, "integer", Some(DataType::Int32)),
        (DuckDb, "int2", Some(DataType::Int16)),
        (Postgres, "float4", Some(DataType::Float32)),
        (Postgres, "double precision", Some(DataType::Float64)),
        (Postgres, "numeric(12, 2)", Some(DataType::Decimal128(12, 2))),
        (Postgres, "numeric", None),
        (Postgres, "numeric(39, 0)", None),
        (Postgres, "numeric(5, 9)", None),
        (Postgres, "date", Some(DataType::Date32)),
        (Postgres, "timestamp", Some(micros(None))),
        (Postgres, "timestamptz", Some(micros(Some("UTC")))),
        (Postgres, "bytea", Some(DataType::Binary)),
        (Postgres, "varchar(20)", Some(DataType::Utf8)),
        (Postgres, "bool", Some(DataType::Boolean)),
        (Postgres, "varchar(20", None),
        (Postgres, "geography", None),
        (ClickHouse, "Int8", Some(DataType::Int8)),
        (ClickHouse, "Int16", Some(DataType::Int16)),
        (ClickHouse, "Int32", Some(DataType::Int32)),
        (ClickHouse, "Int64", Some(DataType::Int64)),
        (ClickHouse, "UInt8", Some(DataType::UInt8)),
        (ClickHouse, "UInt16", Some(DataType::UInt16)),
        (ClickHouse, "UInt32", Some(DataType::UInt32)),
        (ClickHouse, "UInt64", Some(DataType::UInt64)),
        (ClickHouse, "Float32", Some(DataType::Float32)),
        (ClickHouse, "Float64", Some(DataType::Float64)),
        (ClickHouse, "Date32", Some(DataType::Date32)),
        (ClickHouse, "DateTime", Some(seconds.clone())),
        (ClickHouse, "Decimal(10, 3)", Some(DataType::Decimal128(10, 3))),
        (ClickHouse, "Nullable(Int64)", Some(DataType::Int64)),
        (ClickHouse, "LowCardinality(Nullable(String))", Some(DataType::Utf8)),
        (ClickHouse, "Array(Int64)", None),
        (BigQuery, "INTEGER", Some(DataType::Int64)),
        (BigQuery, "FLOAT64", Some(DataType::Float64)),
        (BigQuery, "NUMERIC", Some(DataType::Decimal128(38, 9))),
        (BigQuery, "NUMERIC(10, 2)", Some(DataType::Decimal128(10, 2))),
        (BigQuery, "BYTES", Some(DataType::Binary)),
        (BigQuery, "DATE", Some(DataType::Date32)),
        (BigQuery, "TIMESTAMP", Some(micros(Some("UTC")))),
        (BigQuery, "DATETIME", Some(micros(None))),
        (BigQuery, "STRING", Some(DataType::Utf8)),
        (BigQuery, "GEOGRAPHY", None),
        (Oracle, "VARCHAR2(40)", Some(DataType::Utf8)),
        (Oracle, "NUMBER(10, 2)", Some(DataType::Decimal128(10, 2))),
        (Oracle, "NUMBER", None),
        (Oracle, "INTEGER", Some(DataType::Decimal128(38, 0))),
        (Oracle, "BINARY_FLOAT", Some(DataType::Float32)),
        (Oracle, "BINARY_DOUBLE", Some(DataType::Float64)),
        (Oracle, "DATE", Some(seconds)),
        (Oracle, "TIMESTAMP(6)", Some(micros(None))),
        (Oracle, "TIMESTAMP", Some(micros(None))),
        (Oracle, "RAW(16)", Some(DataType::Binary)),
        (Oracle, "BLOB", Some(DataType::Binary)),
        (Oracle, "XMLTYPE", None),
    ];
    for (dialect, spelling, expected) in cases {
        let declared = ColumnType::parse(spelling).expect("a test type is a type");
        assert_eq!(dialect.arrow(&declared), expected, "{dialect:?} reads {spelling}");
    }
}
