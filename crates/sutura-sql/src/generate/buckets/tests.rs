//! A bucket's refusals, and the statement each dialect hashes a lookup leg's integer keys with.
//!
//! The statements are pinned as text. That a `DuckDB` and a `ClickHouse` bucket hold the rows they
//! should is executed in `crates/sutura-app/tests/differential/buckets.rs`; the Oracle one is rendered
//! here and runs nowhere offline.

use std::collections::HashMap;

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

/// Oracle's hash takes one value, and any subset of the keys is exact, so it hashes the first.
#[test]
fn oracle_hashes_the_first_of_two_integer_keys() {
    let leg = lookup(vec![link(0, "customer_key"), link(1, "contract_key")]);
    let described = described(&[("0_link", DataType::Int64), ("1_link", DataType::Int64)]);
    let query = generate_leg_buckets(&leg, &described, bucket(1, 4), Dialect::Oracle).expect("the leg renders a bucket");
    assert!(
        query
            .sql()
            .contains(r#"WHERE ORA_HASH(COALESCE("dim_customer"."customer_key", 0), 3) = 1 "#),
        "{}",
        query.sql()
    );
}

/// A field as the vendored `ClickHouse` driver describes it: an Arrow type and the column's own.
fn from_clickhouse(name: &str, arrow: DataType, declared: &str) -> Field {
    Field::new(name, arrow, true).with_metadata(HashMap::from([(String::from("CLICKHOUSE:type"), String::from(declared))]))
}

/// The driver describes a `Date` as `UInt16`, a `DateTime` or `IPv4` as `UInt32` and an `Enum` as an
/// integer too, and `COALESCE(k, 0)` over a `Date` has no common type in `ClickHouse` (predicted, not run).
#[test]
fn a_clickhouse_key_the_driver_calls_an_integer_but_its_source_does_not_is_not_hashed() {
    let leg = lookup(vec![link(0, "customer_key")]);
    for (arrow, declared) in [
        (DataType::UInt16, "Date"),
        (DataType::UInt32, "DateTime('UTC')"),
        (DataType::UInt32, "Nullable(IPv4)"),
        (DataType::Int8, "Enum8('a' = 1, 'b' = 2)"),
        (DataType::UInt16, "LowCardinality(Nullable(Date))"),
    ] {
        let described = Schema::new(vec![from_clickhouse("0_link", arrow.clone(), declared)]);
        let refused = generate_leg_buckets(&leg, &described, bucket(0, 2), Dialect::ClickHouse);
        assert!(
            matches!(refused, Err(BucketsRefused::NoIntegerKey)),
            "{declared} as {arrow}: {refused:?}"
        );
    }
}

/// A date key beside an integer one is grouped and not hashed, so its bucket still renders.
#[test]
fn a_date_key_beside_an_integer_key_is_grouped_and_not_hashed() {
    let leg = lookup(vec![link(0, "customer_key"), dimension("signed_on")]);
    let described = Schema::new(vec![
        from_clickhouse("0_link", DataType::Int64, "Int64"),
        from_clickhouse("signed_on", DataType::UInt16, "Date"),
    ]);
    let query = generate_leg_buckets(&leg, &described, bucket(0, 2), Dialect::ClickHouse).expect("the leg renders a bucket");
    assert!(
        query
            .sql()
            .contains(r#"WHERE cityHash64(COALESCE("dim_customer"."customer_key", 0)) % 2 = 0 "#),
        "{}",
        query.sql()
    );
}

#[test]
fn a_clickhouse_integer_is_one_under_any_nullable_or_low_cardinality() {
    let leg = lookup(vec![link(0, "customer_key")]);
    for declared in [
        "Int8",
        "UInt64",
        "Nullable(Int32)",
        "LowCardinality(UInt16)",
        "LowCardinality(Nullable(UInt32))",
    ] {
        let described = Schema::new(vec![from_clickhouse("0_link", DataType::Int64, declared)]);
        let query = generate_leg_buckets(&leg, &described, bucket(0, 2), Dialect::ClickHouse);
        assert!(query.is_ok(), "{declared}: {query:?}");
    }
}
