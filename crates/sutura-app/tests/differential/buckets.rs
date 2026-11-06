//! A lookup leg split into hash buckets, executed: the buckets together are the leg read whole.
//!
//! Over one embedded `DuckDB` database, the one dialect `sutura_sql::generate_leg_buckets` renders
//! for that runs offline. The columns it hashes by are the ones the database itself described for
//! the whole leg. The `ClickHouse` and Oracle buckets are rendered and pinned in `sutura-sql`'s own
//! cells and executed nowhere here.

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::model::{ColumnName, DimensionName, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{Executable, InternalLabel, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::{Value, Warehouse as _};
use sutura_exec_duckdb::DuckDbWarehouse;
use sutura_sql::{Bucket, Dialect, GeneratedQuery, generate_leg, generate_leg_buckets};

use crate::adapters::{deadline, posture, presented, result_budget};

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

/// Customers keyed by an integer, two of them with no key and one with no region, and a repeated
/// row the leg's grouping folds into one.
fn warehouse() -> DuckDbWarehouse {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("leg-buckets-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("could not create {}: {e}", dir.display()));
    let csv = dir.join("customers.csv");
    std::fs::write(
        &csv,
        "customer_key,region\n1,north\n2,south\n3,north\n,east\n,west\n4,south\n5,\n6,north\n7,east\n8,west\n2,south\n\
         9,north\n10,south\n11,east\n12,west\n",
    )
    .expect("the customer fixture writes");
    let warehouse = DuckDbWarehouse::in_memory(source(), posture(), result_budget()).expect("an in-memory database opens");
    warehouse
        .attach_csv(&TableName::parse(TABLE).expect("a test table is a table"), &csv)
        .expect("the customer fixture attaches");
    warehouse
}

/// The customer half of a federated question: the integer link key and the text region beside it.
fn lookup() -> LegPlan {
    LegPlan::Lookup {
        source: source(),
        metric: MetricDigest::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: TableName::parse(TABLE).expect("a test table is a table").into(),
        keys: vec![
            PlanKey::new(ResultLabel::internal(InternalLabel::link(0)), column("customer_key")),
            PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension is a dimension")),
                column("region"),
            ),
        ],
        bindings: PlanBindings::none(),
    }
}

fn rows(warehouse: &DuckDbWarehouse, query: &GeneratedQuery) -> Vec<Vec<Value>> {
    assert!(query.params().is_empty(), "the fixture leg binds no value");
    let statement = RawStatement::parse(query.sql()).expect("a rendered leg is one statement");
    warehouse
        .execute_unscreened(&statement, deadline())
        .unwrap_or_else(|cause| panic!("{} did not run: {cause}", query.sql()))
        .rows()
        .to_vec()
}

/// **The measurement.** For two, three and five buckets, every row of the leg read whole - the two
/// with no key among them - comes back from exactly one bucket, and no bucket answers a row the
/// whole leg does not.
#[test]
fn every_row_of_a_lookup_leg_comes_back_from_exactly_one_bucket() {
    let warehouse = warehouse();
    let leg = lookup();
    let read = warehouse
        .execute(Executable::Leg(&leg), &presented(), deadline())
        .expect("the leg reads whole");
    let described = read.batches().first().expect("the whole leg has rows").schema();
    let whole = rows(&warehouse, &generate_leg(&leg, Dialect::DuckDb).expect("the leg renders"));
    assert_eq!(
        whole.len(),
        14,
        "fourteen distinct customer rows, two of them with no key: {whole:?}"
    );

    for count in [2, 3, 5] {
        let buckets: Vec<Vec<Vec<Value>>> = (0..count)
            .map(|index| {
                let bucket = Bucket::new(index, count).expect("a bucket below its count");
                let query = generate_leg_buckets(&leg, &described, bucket, Dialect::DuckDb).expect("the leg renders a bucket");
                rows(&warehouse, &query)
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
