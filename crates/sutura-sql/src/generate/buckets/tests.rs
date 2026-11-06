//! A bucket's refusals, and the statement each dialect hashes a lookup leg's integer keys with.
//!
//! The statements are pinned as text. That a `DuckDB` bucket holds the rows it should is executed in
//! `crates/sutura-app/tests/differential/buckets.rs`; the `ClickHouse` and Oracle ones are rendered
//! here and run nowhere offline.

use arrow_schema::{DataType, Field, Schema};
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::model::{ColumnName, DimensionName, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{InternalLabel, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel};

use super::{Bucket, BucketsRefused, InvalidBucket, generate_leg_buckets};
use crate::dialect::Dialect;

const TABLE: &str = "dim_customer";

fn column(name: &str) -> PlanColumn {
    PlanColumn::new(
        TableName::parse(TABLE).expect("a test table is a table"),
        ColumnName::parse(name).expect("a test column is a column"),
    )
}

fn link(position: usize, name: &str) -> PlanKey {
    PlanKey::new(ResultLabel::internal(InternalLabel::link(position)), column(name))
}

fn dimension(name: &str) -> PlanKey {
    PlanKey::new(
        ResultLabel::dimension(&DimensionName::parse(name).expect("a test dimension is a dimension")),
        column(name),
    )
}

fn lookup(keys: Vec<PlanKey>) -> LegPlan {
    LegPlan::Lookup {
        source: SourceName::parse("crm").expect("a test source is a source"),
        metric: MetricDigest::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: TableName::parse(TABLE).expect("a test table is a table").into(),
        keys,
        bindings: PlanBindings::none(),
    }
}

fn described(columns: &[(&str, DataType)]) -> Schema {
    Schema::new(
        columns
            .iter()
            .map(|(name, data_type)| Field::new(*name, data_type.clone(), true))
            .collect::<Vec<_>>(),
    )
}

/// The link key an integer, the region beside it text.
fn customer_lookup() -> (LegPlan, Schema) {
    (
        lookup(vec![link(0, "customer_key"), dimension("region")]),
        described(&[("0_link", DataType::Int64), ("region", DataType::Utf8)]),
    )
}

fn bucket(index: u32, count: u32) -> Bucket {
    Bucket::new(index, count).expect("a test bucket is a bucket")
}

#[test]
fn a_bucket_is_one_of_two_or_more_and_never_past_the_last() {
    assert_eq!(Bucket::new(0, 0), Err(InvalidBucket::TooFew { count: 0 }));
    assert_eq!(Bucket::new(0, 1), Err(InvalidBucket::TooFew { count: 1 }));
    assert_eq!(Bucket::new(2, 2), Err(InvalidBucket::PastTheLast { index: 2, count: 2 }));
    assert_eq!(Bucket::new(7, 3), Err(InvalidBucket::PastTheLast { index: 7, count: 3 }));
    let last = Bucket::new(1, 2).expect("bucket 1 of 2 is the last one");
    assert_eq!((last.index(), last.count()), (1, 2));
}

/// Each dialect hashes the integer link key and not the text region, with a NULL key hashed as `0`.
#[test]
fn each_dialect_hashes_the_integer_keys_with_a_null_key_as_zero() {
    let (leg, described) = customer_lookup();
    let select = r#"SELECT "dim_customer"."customer_key" AS "0_link", "dim_customer"."region" AS "region" FROM "dim_customer""#;
    let group = r#"GROUP BY "dim_customer"."customer_key", "dim_customer"."region""#;
    let order = r#"ORDER BY "dim_customer"."customer_key", "dim_customer"."region""#;
    let key = r#"COALESCE("dim_customer"."customer_key", 0)"#;
    for (dialect, within, order) in [
        (Dialect::ClickHouse, format!("cityHash64({key}) % 3 = 1"), String::from(order)),
        (Dialect::Oracle, format!("ORA_HASH({key}, 2) = 1"), String::from(order)),
        (
            Dialect::DuckDb,
            format!("HASH({key}) % 3 = 1"),
            String::from(r#"ORDER BY "dim_customer"."customer_key" NULLS LAST, "dim_customer"."region" NULLS LAST"#),
        ),
    ] {
        let query = generate_leg_buckets(&leg, &described, bucket(1, 3), dialect).expect("the leg renders a bucket");
        assert_eq!(query.sql(), format!("{select} WHERE {within} {group} {order}"), "{dialect}");
        assert!(query.params().is_empty(), "{dialect}: a bucket binds no value");
    }
}

#[test]
fn clickhouse_hashes_every_integer_key_in_one_call() {
    let leg = lookup(vec![link(0, "customer_key"), link(1, "contract_key"), dimension("region")]);
    let described = described(&[
        ("0_link", DataType::Int64),
        ("1_link", DataType::UInt32),
        ("region", DataType::Utf8),
    ]);
    let query = generate_leg_buckets(&leg, &described, bucket(0, 4), Dialect::ClickHouse).expect("the leg renders a bucket");
    assert!(
        query.sql().contains(
            r#"WHERE cityHash64(COALESCE("dim_customer"."customer_key", 0), COALESCE("dim_customer"."contract_key", 0)) % 4 = 0 "#
        ),
        "{}",
        query.sql()
    );
}

#[test]
fn a_leg_with_no_integer_key_is_refused() {
    let leg = lookup(vec![link(0, "customer_key"), dimension("region")]);
    let described = described(&[("0_link", DataType::Utf8), ("region", DataType::Float64)]);
    for dialect in [Dialect::ClickHouse, Dialect::Oracle, Dialect::DuckDb] {
        let refused = generate_leg_buckets(&leg, &described, bucket(0, 2), dialect);
        assert!(matches!(refused, Err(BucketsRefused::NoIntegerKey)), "{dialect}: {refused:?}");
    }
}

#[test]
fn a_key_its_source_did_not_describe_is_refused_by_its_label() {
    let (leg, _) = customer_lookup();
    let described = described(&[("0_link", DataType::Int64)]);
    let refused = generate_leg_buckets(&leg, &described, bucket(0, 2), Dialect::ClickHouse);
    assert!(
        matches!(&refused, Err(BucketsRefused::KeyNotDescribed { label }) if label == "region"),
        "{refused:?}"
    );
}

#[test]
fn postgres_and_bigquery_are_not_split_into_hash_buckets() {
    let (leg, described) = customer_lookup();
    for dialect in [Dialect::Postgres, Dialect::BigQuery] {
        let refused = generate_leg_buckets(&leg, &described, bucket(0, 2), dialect);
        assert!(
            matches!(refused, Err(BucketsRefused::Unsupported { dialect: refused }) if refused == dialect),
            "{dialect}: {refused:?}"
        );
    }
}

#[test]
fn oracle_refuses_a_leg_with_two_integer_keys() {
    let leg = lookup(vec![link(0, "customer_key"), link(1, "contract_key")]);
    let described = described(&[("0_link", DataType::Int64), ("1_link", DataType::Int64)]);
    let refused = generate_leg_buckets(&leg, &described, bucket(0, 2), Dialect::Oracle);
    assert!(
        matches!(
            refused,
            Err(BucketsRefused::CompoundKey {
                dialect: Dialect::Oracle,
                keys: 2
            })
        ),
        "{refused:?}"
    );
}
