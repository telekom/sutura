use std::collections::BTreeMap;

use arrow_schema::DataType;

use super::{PinnedSchemas, TypeDialect};
use crate::catalog::{Column, Definitions, Description, JoinKey, JoinKeys, Model, Relationship};
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
