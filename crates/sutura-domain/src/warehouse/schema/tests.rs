use std::collections::BTreeMap;

use arrow_schema::{DataType, TimeUnit};

use super::{PinnedSchemas, TypeDialect};
use crate::catalog::{Collation, Column, ColumnType, Definitions, Description, JoinKey, JoinKeys, Model, Relationship};
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
fn operator_markers_change_the_schema_digest_without_changing_column_types() {
    use std::collections::HashMap;
    use std::sync::Arc;

    use arrow_schema::{Field, Schema};

    let name = ModelName::parse("orders").unwrap();
    let field = Field::new("amount", DataType::Utf8, true);
    let plain = BTreeMap::from([(name.clone(), Arc::new(Schema::new(vec![field.clone()])))]);
    let marked = field.with_metadata(HashMap::from([(
        String::from("sutura:unbounded_number"),
        String::from("utf8"),
    )]));
    let marked = BTreeMap::from([(name, Arc::new(Schema::new(vec![marked])))]);
    assert_ne!(
        super::digest_of(&plain, &BTreeMap::new()),
        super::digest_of(&marked, &BTreeMap::new())
    );
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
    let customers = ModelName::parse("customers").expect("a test model is a model");
    let orders = ModelName::parse("orders").expect("a test model is a model");
    assert!(
        local.of(&orders).is_some(),
        "a local model on a source with a dialect is pinned"
    );
    assert_eq!(
        local.fell_back(&customers).map(ColumnName::as_str).collect::<Vec<_>>(),
        ["id"]
    );
}

#[test]
fn each_dialect_reads_a_type_in_its_own_spelling_and_leaves_the_rest_unread() {
    use TypeDialect::{BigQuery, ClickHouse, Cube, DataFusion, DuckDb, Oracle, Postgres};
    let micros = |zone: Option<&str>| DataType::Timestamp(TimeUnit::Microsecond, zone.map(Into::into));
    let seconds = DataType::Timestamp(TimeUnit::Second, None);
    let cases = [
        (DataFusion, "BIGINT", Some(DataType::Int64)),
        (DataFusion, "integer", Some(DataType::Int32)),
        (DataFusion, "date", Some(DataType::Date32)),
        (DataFusion, "timestamp", Some(micros(None))),
        (Cube, "number", Some(DataType::Float64)),
        (Cube, "time", Some(micros(None))),
        (Cube, "string", Some(DataType::Utf8)),
        (Cube, "geography", None),
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
        (Oracle, "TIMESTAMP WITH TIME ZONE", Some(micros(Some("UTC")))),
        (Oracle, "timestamp(3) with time zone", Some(micros(Some("UTC")))),
        (Oracle, "TIMESTAMP(6) WITH LOCAL TIME ZONE", Some(micros(Some("UTC")))),
        (Oracle, "TIMESTAMP WITH LOCAL TIME ZONE", Some(micros(Some("UTC")))),
        (Oracle, "TIMESTAMP(9)", None),
        (Oracle, "TIMESTAMP(7) WITH TIME ZONE", None),
        (Oracle, "TIMESTAMP(9) WITH LOCAL TIME ZONE", None),
        (Oracle, "TIMESTAMP(6) WITH TIME ZONE EXTRA", None),
        (Oracle, "TIMESTAMP(bad) WITH TIME ZONE", None),
        (Oracle, "RAW(16)", Some(DataType::Binary)),
        (Oracle, "BLOB", Some(DataType::Binary)),
        (Oracle, "XMLTYPE", None),
    ];
    for (dialect, spelling, expected) in cases {
        let declared = ColumnType::parse(spelling).expect("a test type is a type");
        assert_eq!(dialect.arrow(&declared), expected, "{dialect:?} reads {spelling}");
    }
}

#[test]
fn oracle_text_policies_pin_lob_kinds_for_full_search_and_distinct_scalar_capacities() {
    use crate::catalog::TextPolicy;
    let policy: TextPolicy = serde_json::from_value(serde_json::json!({"max_text_bytes":4000})).unwrap();
    let definitions = Definitions::assemble(
        vec![model(
            "notes",
            "lake",
            vec![
                column("body", Some("CLOB")).with_text_policy(policy.clone()),
                column("national", Some("NCLOB")).with_text_policy(policy.clone()),
                column("title", Some("VARCHAR2(80)")).with_text_policy(policy),
            ],
        )],
        vec![],
        vec![],
    )
    .unwrap();
    let pinned = PinnedSchemas::pin(
        &definitions,
        &BTreeMap::from([(SourceName::parse("lake").unwrap(), TypeDialect::Oracle)]),
    )
    .unwrap();
    let schema = pinned.of(&ModelName::parse("notes").unwrap()).unwrap();
    for (name, kind) in [("body", Some("clob")), ("national", Some("nclob")), ("title", None)] {
        assert_eq!(
            schema
                .field_with_name(name)
                .unwrap()
                .metadata()
                .get("sutura:oracle_lob")
                .map(String::as_str),
            kind
        );
    }
}

#[test]
fn text_policies_coexist_with_disjoint_certified_columns_on_every_source_dialect() {
    use crate::catalog::{Audience, Metric, TextPolicy};
    use crate::measure::{AggregatedColumn, Measure, Term};
    use crate::model::{Aggregate, Grain, MetricName};
    use std::collections::BTreeSet;

    for dialect in [
        TypeDialect::DataFusion,
        TypeDialect::DuckDb,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
        TypeDialect::Cube,
    ] {
        for placement in ["same_table", "other_table", "other_source"] {
            let policy: TextPolicy = serde_json::from_value(serde_json::json!({"max_text_bytes": 128})).unwrap();
            let metric = Metric::new(
                MetricName::parse("total").unwrap(),
                ModelName::parse("facts").unwrap(),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(
                    Aggregate::Count,
                    ColumnName::parse("id").unwrap(),
                ))),
                vec![],
                ColumnName::parse("day").unwrap(),
                BTreeSet::from([Grain::Day]),
                vec![],
                None,
                Description::default(),
                Audience::Open,
            )
            .unwrap();
            let mut facts = vec![column("id", Some("BIGINT")), column("day", Some("DATE"))];
            if placement == "same_table" {
                facts.push(column("body", Some("TEXT")).with_text_policy(policy.clone()));
            }
            let mut models = vec![model("facts", "lake", facts)];
            if placement != "same_table" {
                models.push(model(
                    "notes",
                    if placement == "other_source" { "crm" } else { "lake" },
                    vec![column("body", Some("TEXT")).with_text_policy(policy)],
                ));
            }
            let definitions = Definitions::assemble(models, vec![], vec![metric]).unwrap();
            let dialects = ["lake", "crm"]
                .into_iter()
                .map(|name| (SourceName::parse(name).unwrap(), dialect))
                .collect();
            let pinned = PinnedSchemas::pin(&definitions, &dialects);
            assert!(pinned.is_ok(), "{dialect:?} {placement}: {pinned:?}");
        }
    }
}

#[test]
fn certified_column_reads_refuse_text_policies_including_physical_aliases_and_join_keys() {
    use crate::catalog::{Audience, Dimension, DimensionValue, Metric, TextPolicy, ViaChain};
    use crate::measure::{AggregatedColumn, Measure, RequiredFilter, Term};
    use crate::model::{Aggregate, DimensionName, Grain, MetricName, QualifiedTable};
    use std::collections::BTreeSet;

    let name = |raw: &str| ModelName::parse(raw).unwrap();
    let col = |raw: &str| ColumnName::parse(raw).unwrap();
    for read in [
        "disjoint",
        "measure",
        "filter",
        "dimension",
        "time",
        "origin",
        "target",
        "alias",
    ] {
        let policy: TextPolicy = serde_json::from_value(serde_json::json!({"max_text_bytes": 128})).unwrap();
        let mut facts = vec![
            column("id", Some("TEXT")),
            column("day", Some("TEXT")),
            column("body", Some("TEXT")),
            column("lookup_key", Some("TEXT")),
        ];
        let protected = match read {
            "time" => "day",
            "origin" => "lookup_key",
            _ => "body",
        };
        let mut lookup = vec![column("id", Some("TEXT")), column("body", Some("TEXT"))];
        if read == "target" {
            lookup[0] = column("id", Some("TEXT")).with_text_policy(policy);
        } else {
            let target = facts.iter().position(|c| c.name().as_str() == protected).unwrap();
            facts[target] = column(protected, Some("TEXT")).with_text_policy(policy);
        }
        let mut models = vec![model("facts", "lake", facts), model("lookup", "crm", lookup)];
        let metric_model = if read == "alias" {
            models.push(Model::new(
                name("alias"),
                SourceName::parse("lake").unwrap(),
                QualifiedTable::parse("schema.FACTS").unwrap(),
                vec![column("BODY", Some("TEXT")), column("day", Some("TEXT"))],
                Description::default(),
            ));
            name("alias")
        } else {
            name("facts")
        };
        let relationship = Relationship::new(
            RelationshipName::parse("to_lookup").unwrap(),
            name("facts"),
            name("lookup"),
            JoinType::ManyToOne,
            JoinKeys::single(JoinKey::Equal {
                origin: col("lookup_key"),
                target: col("id"),
            }),
        );
        let dimensions = if matches!(read, "dimension" | "origin" | "target") {
            vec![Dimension::new(
                DimensionName::parse("label").unwrap(),
                col("body"),
                (read != "dimension").then(|| ViaChain::of(vec![relationship.name().clone()]).unwrap()),
                None,
                Description::default(),
            )]
        } else {
            vec![]
        };
        let filters = if read == "filter" {
            vec![RequiredFilter::Equals {
                column: col("body"),
                value: DimensionValue::parse("ok").unwrap(),
            }]
        } else {
            vec![]
        };
        let measured = match read {
            "measure" => "body",
            "alias" => "BODY",
            _ => "id",
        };
        let metric = Metric::new(
            MetricName::parse("total").unwrap(),
            metric_model,
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Count, col(measured)))),
            filters,
            col("day"),
            BTreeSet::from([Grain::Day]),
            dimensions,
            None,
            Description::default(),
            Audience::Open,
        )
        .unwrap();
        let definitions = Definitions::assemble(models, vec![relationship], vec![metric]).unwrap();
        let pinned = PinnedSchemas::pin(&definitions, &dialects());
        if read == "disjoint" {
            assert!(pinned.is_ok(), "disjoint columns must pin: {pinned:?}");
        } else {
            let refused = pinned.expect_err("a certified read cannot bypass a text policy");
            if read == "measure" {
                assert_eq!(
                    format!("{refused:?}"),
                    r#"MetricReadsProtectedText { metric: MetricName("total"), model: ModelName("facts"), column: ColumnName("body") }"#
                );
            }
            let refused = refused.to_string();
            let protected_model = if read == "target" { "lookup" } else { "facts" };
            let protected_column = if read == "target" { "id" } else { protected };
            assert_eq!(
                refused,
                format!(
                    "metric total reads text-policy column {protected_model}.{protected_column}; certified execution cannot apply that policy"
                ),
                "{read}"
            );
        }
    }
}

#[test]
fn cross_fact_text_policies_cover_shared_calendar_and_second_fact_dimension_links() {
    use crate::catalog::{Audience, Dimension, Metric, TextPolicy, ViaChain};
    use crate::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
    use crate::model::{Aggregate, DimensionName, Grain, MetricName};
    use std::collections::BTreeSet;

    let name = |raw: &str| ModelName::parse(raw).unwrap();
    let col = |raw: &str| ColumnName::parse(raw).unwrap();
    for (protected_model, protected_column) in [
        ("calendar", "unused"),
        ("calendar", "day"),
        ("calendar", "key"),
        ("second", "calendar_key"),
        ("second", "lookup_key"),
        ("second", "value"),
    ] {
        let policy: TextPolicy = serde_json::from_value(serde_json::json!({"max_text_bytes":128})).unwrap();
        let declared = |model_name: &str, columns: &[&str]| {
            model(
                model_name,
                "lake",
                columns
                    .iter()
                    .map(|column_name| {
                        let declared = column(column_name, Some("TEXT"));
                        if model_name == protected_model && *column_name == protected_column {
                            declared.with_text_policy(policy.clone())
                        } else {
                            declared
                        }
                    })
                    .collect(),
            )
        };
        let models = vec![
            declared("first", &["value", "calendar_key", "lookup_key", "day"]),
            declared("second", &["value", "calendar_key", "lookup_key"]),
            declared("calendar", &["key", "day", "unused"]),
            declared("lookup", &["key", "label"]),
        ];
        let link = |link_name: &str, origin: &str, target: &str, origin_key: &str| {
            Relationship::new(
                RelationshipName::parse(link_name).unwrap(),
                name(origin),
                name(target),
                JoinType::ManyToOne,
                JoinKeys::single(JoinKey::Equal {
                    origin: col(origin_key),
                    target: col("key"),
                }),
            )
        };
        let links = vec![
            link("first_calendar", "first", "calendar", "calendar_key"),
            link("second_calendar", "second", "calendar", "calendar_key"),
            link("first_lookup", "first", "lookup", "lookup_key"),
            link("second_lookup", "second", "lookup", "lookup_key"),
        ];
        let metric = Metric::new(
            MetricName::parse("ratio").unwrap(),
            name("first"),
            Measure::Ratio {
                numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Count, col("value"))),
                denominator: Term::Aggregate(AggregatedColumn::on_model(Aggregate::Count, col("value"), name("second"))),
                zero_denominator: ZeroDenominator::Null,
            },
            vec![],
            col("day"),
            BTreeSet::from([Grain::Day]),
            vec![Dimension::new(
                DimensionName::parse("label").unwrap(),
                col("label"),
                Some(ViaChain::of(vec![RelationshipName::parse("first_lookup").unwrap()]).unwrap()),
                None,
                Description::default(),
            )],
            None,
            Description::default(),
            Audience::Open,
        )
        .unwrap()
        .with_shared_calendar(name("calendar"));
        let definitions = Definitions::assemble(models, links, vec![metric]).unwrap();
        let pinned = PinnedSchemas::pin(&definitions, &dialects());
        if protected_column == "unused" {
            assert!(pinned.is_ok(), "an unused shared-calendar column must coexist: {pinned:?}");
        } else {
            assert_eq!(
                pinned.unwrap_err().to_string(),
                format!(
                    "metric ratio reads text-policy column {protected_model}.{protected_column}; certified execution cannot apply that policy"
                )
            );
        }
    }
}

fn collated(name: &str, declared: &str, collation: Collation) -> Column {
    column(name, Some(declared)).with_collation(collation)
}

fn pin_notes(dialect: TypeDialect, columns: Vec<Column>) -> (PinnedSchemas, ModelName) {
    let source = SourceName::parse("lake").expect("a test source is a source");
    let definitions = Definitions::assemble(vec![model("notes", "lake", columns)], vec![], vec![]).expect("a test bundle holds");
    let pinned = PinnedSchemas::pin(&definitions, &BTreeMap::from([(source, dialect)])).expect("a typed bundle pins");
    (pinned, ModelName::parse("notes").expect("a test model is a model"))
}

fn pinned_collation(pinned: &PinnedSchemas, model: &ModelName, column: &str) -> Collation {
    let schema = pinned.of(model).expect("the model is pinned");
    Collation::of_field(schema.field_with_name(column).expect("the column is pinned"))
}

#[test]
fn a_collation_change_moves_the_schema_digest_and_unknown_changes_nothing() {
    let digest = |collation| pin_notes(TypeDialect::Postgres, vec![collated("body", "text", collation)]).0;
    let (unknown, binary, ci, other) = (
        digest(Collation::Unknown),
        digest(Collation::Binary),
        digest(Collation::named("und:ci")),
        digest(Collation::named("und:ai")),
    );
    let digests = [unknown.digest(), binary.digest(), ci.digest(), other.digest()];
    for (index, left) in digests.iter().enumerate() {
        for right in &digests[index + 1..] {
            assert_ne!(left, right, "each collation re-pins");
        }
    }
    let bare = pin_notes(TypeDialect::Postgres, vec![column("body", Some("text"))]).0;
    assert_eq!(bare.digest(), unknown.digest(), "no collation fact hashes as it always did");
}

#[test]
fn a_pinned_text_column_carries_what_was_recorded_and_unknown_when_nothing_was() {
    let columns = vec![
        collated("exact", "text", Collation::Binary),
        collated("folded", "text", Collation::named("und:ci")),
        column("silent", Some("text")),
        collated("count", "bigint", Collation::Binary),
    ];
    let (pinned, notes) = pin_notes(TypeDialect::Postgres, columns);
    assert_eq!(pinned_collation(&pinned, &notes, "exact"), Collation::Binary);
    assert_eq!(pinned_collation(&pinned, &notes, "folded"), Collation::named("und:ci"));
    assert_eq!(pinned_collation(&pinned, &notes, "silent"), Collation::Unknown);
    assert!(!pinned_collation(&pinned, &notes, "silent").is_binary());
    assert_eq!(
        pinned_collation(&pinned, &notes, "count"),
        Collation::Unknown,
        "a collation on a column that is not text is dropped"
    );
}

#[test]
fn an_engine_with_no_column_collation_pins_binary_over_whatever_a_catalog_declares() {
    for dialect in [TypeDialect::DataFusion, TypeDialect::ClickHouse] {
        let (pinned, notes) = pin_notes(
            dialect,
            vec![
                collated("folded", "text", Collation::named("und:ci")),
                column("silent", Some("text")),
            ],
        );
        assert_eq!(pinned_collation(&pinned, &notes, "folded"), Collation::Binary, "{dialect:?}");
        assert_eq!(pinned_collation(&pinned, &notes, "silent"), Collation::Binary, "{dialect:?}");
    }
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let (pinned, notes) = pin_notes(dialect, vec![column("silent", Some("text"))]);
        assert_eq!(pinned_collation(&pinned, &notes, "silent"), Collation::Unknown, "{dialect:?}");
    }
}
