//! A lookup leg split into hash buckets, executed: the buckets together are the leg read whole.
//!
//! Over one embedded `DuckDB` database, and over the `ClickHouse` tier where one is provisioned (a
//! cell skips it by its named exemption elsewhere), the two dialects of `sutura_sql::generate_leg_buckets`
//! that run here. The columns it hashes by are the ones the data system itself described for the whole
//! leg. The Oracle bucket is rendered and pinned in `sutura-sql`'s own cells and executed nowhere.
//!
//! **A limit of the `ClickHouse` cells:** they reach the tier over HTTP, whose rows carry no
//! `CLICKHOUSE:type` metadata and describe a `Date` as text. The vendored ADBC driver, which
//! `sutura serve` composes, describes a `Date` as `UInt16` with that metadata; `sutura-sql`'s own
//! cells pin that shape, and no cell here runs a bucket over it.

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::model::{ColumnName, DimensionName, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{Executable, InternalLabel, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::{ResultBatches, Value, Warehouse};
use sutura_exec_duckdb::DuckDbWarehouse;
use sutura_sql::{Bucket, BucketsRefused, Dialect, GeneratedQuery, generate_leg, generate_leg_buckets};

use crate::adapters::{ClickHouse, clickhouse_on, deadline, posture, presented, result_budget, runs_here};

const TABLE: &str = "dim_customer";

fn source() -> SourceName {
    SourceName::parse("crm").expect("a test source is a source")
}

fn column(name: &str) -> PlanColumn {
    PlanColumn::new(
        TableName::parse(TABLE).expect("a test table is a table"),
        ColumnName::parse(name).expect("a test column is a column"),
    )
}

/// Customers keyed by an integer, two of them with no key and one with no region, a signing date
/// each, and a repeated row the leg's grouping folds into one.
fn fixture(tag: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("leg-buckets-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("could not create {}: {e}", dir.display()));
    let csv = dir.join("customers.csv");
    std::fs::write(
        &csv,
        "customer_key,region,signed_on\n1,north,2024-01-05\n2,south,2024-01-06\n3,north,2024-01-07\n,east,2024-01-08\n\
         ,west,2024-01-09\n4,south,2024-01-10\n5,,2024-01-11\n6,north,2024-01-12\n7,east,2024-01-13\n8,west,2024-01-14\n\
         2,south,2024-01-06\n9,north,2024-01-15\n10,south,2024-01-16\n11,east,2024-01-17\n12,west,2024-01-18\n",
    )
    .expect("the customer fixture writes");
    csv
}

fn duckdb() -> DuckDbWarehouse {
    let warehouse = DuckDbWarehouse::in_memory(source(), posture(), result_budget()).expect("an in-memory database opens");
    warehouse
        .attach_csv(&TableName::parse(TABLE).expect("a test table is a table"), &fixture("duckdb"))
        .expect("the customer fixture attaches");
    warehouse
}

fn clickhouse() -> ClickHouse {
    let table = TableName::parse(TABLE).expect("a test table is a table");
    clickhouse_on(source(), vec![(table, fixture("clickhouse"))], ClickHouse::load_federated_csv)
}

fn leg_keyed_by(keys: Vec<PlanKey>) -> LegPlan {
    LegPlan::Lookup {
        source: source(),
        metric: MetricDigest::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: TableName::parse(TABLE).expect("a test table is a table").into(),
        keys,
        bindings: PlanBindings::none(),
    }
}

fn dimension(name: &str) -> PlanKey {
    PlanKey::new(
        ResultLabel::dimension(&DimensionName::parse(name).expect("a test dimension is a dimension")),
        column(name),
    )
}

/// The customer half of a federated question: the integer link key and the text region beside it.
fn lookup() -> LegPlan {
    leg_keyed_by(vec![
        PlanKey::new(ResultLabel::internal(InternalLabel::link(0)), column("customer_key")),
        dimension("region"),
    ])
}

/// The same table keyed by its signing date alone, which no dialect hashes.
fn dated() -> LegPlan {
    leg_keyed_by(vec![dimension("signed_on")])
}

fn duckdb_rows(warehouse: &DuckDbWarehouse, query: &GeneratedQuery) -> Vec<Vec<Value>> {
    assert!(query.params().is_empty(), "the fixture leg binds no value");
    let statement = RawStatement::parse(query.sql()).expect("a rendered leg is one statement");
    warehouse
        .execute_unscreened(&statement, deadline())
        .unwrap_or_else(|cause| panic!("{} did not run: {cause}", query.sql()))
        .rows()
        .to_vec()
}

fn clickhouse_rows(warehouse: &ClickHouse, query: &GeneratedQuery) -> Vec<Vec<Value>> {
    assert!(query.params().is_empty(), "the fixture leg binds no value");
    warehouse
        .rows_of(query, deadline())
        .unwrap_or_else(|cause| panic!("{} did not run: {cause}", query.sql()))
        .rows()
        .to_vec()
}

/// `leg` read whole, whose batches' schema is the columns `warehouse` itself describes for it.
fn read_whole<W>(warehouse: &W, leg: &LegPlan) -> ResultBatches
where
    W: Warehouse,
{
    warehouse
        .execute(Executable::Leg(leg), &presented(), deadline())
        .unwrap_or_else(|cause| panic!("the leg reads whole: {cause}"))
}

/// **The measurement.** For two, three and five buckets, every row of the leg read whole - the two
/// with no key among them - comes back from exactly one bucket, and no bucket answers a row the
/// whole leg does not.
fn every_row_comes_back_from_exactly_one_bucket<W>(
    warehouse: &W,
    dialect: Dialect,
    rows: impl Fn(&GeneratedQuery) -> Vec<Vec<Value>>,
) where
    W: Warehouse,
{
    let leg = lookup();
    let read = read_whole(warehouse, &leg);
    let described = read.batches().first().expect("the whole leg has rows").schema();
    let whole = rows(&generate_leg(&leg, dialect).expect("the leg renders"));
    assert_eq!(
        whole.len(),
        14,
        "fourteen distinct customer rows, two of them with no key: {whole:?}"
    );

    for count in [2, 3, 5] {
        let buckets: Vec<Vec<Vec<Value>>> = (0..count)
            .map(|index| {
                let bucket = Bucket::new(index, count).expect("a bucket below its count");
                rows(&generate_leg_buckets(&leg, &described, bucket, dialect).expect("the leg renders a bucket"))
            })
            .collect();
        for row in &whole {
            let holding = buckets.iter().flatten().filter(|other| *other == row).count();
            assert_eq!(holding, 1, "of {count} buckets, {holding} hold {row:?}");
        }
        let total: usize = buckets.iter().map(Vec::len).sum();
        assert_eq!(total, whole.len(), "{count} buckets answered a row the whole leg does not");
        if count == 2 {
            assert!(
                buckets.iter().all(|bucket| !bucket.is_empty()),
                "one of two buckets is empty: {buckets:?}"
            );
        }
    }
}

/// A leg keyed by a date alone has no key the source describes as an integer.
fn a_date_keyed_leg_is_refused<W>(warehouse: &W, dialect: Dialect)
where
    W: Warehouse,
{
    let leg = dated();
    let read = read_whole(warehouse, &leg);
    let described = read.batches().first().expect("the whole leg has rows").schema();
    let refused = generate_leg_buckets(&leg, &described, Bucket::new(0, 2).expect("a bucket"), dialect);
    assert!(matches!(refused, Err(BucketsRefused::NoIntegerKey)), "{refused:?}");
}

#[test]
fn every_row_of_a_lookup_leg_comes_back_from_exactly_one_bucket() {
    let warehouse = duckdb();
    every_row_comes_back_from_exactly_one_bucket(&warehouse, Dialect::DuckDb, |query| duckdb_rows(&warehouse, query));
}

#[test]
fn every_row_of_a_lookup_leg_comes_back_from_exactly_one_clickhouse_bucket() {
    if !runs_here::<ClickHouse>() {
        return;
    }
    let warehouse = clickhouse();
    every_row_comes_back_from_exactly_one_bucket(&warehouse, Dialect::ClickHouse, |query| clickhouse_rows(&warehouse, query));
}

#[test]
fn a_duckdb_leg_keyed_by_a_date_is_refused_a_hash_bucket() {
    a_date_keyed_leg_is_refused(&duckdb(), Dialect::DuckDb);
}

#[test]
fn a_clickhouse_leg_keyed_by_a_date_is_refused_a_hash_bucket() {
    if !runs_here::<ClickHouse>() {
        return;
    }
    a_date_keyed_leg_is_refused(&clickhouse(), Dialect::ClickHouse);
}
