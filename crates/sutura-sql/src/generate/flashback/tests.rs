//! A snapshot's and a bucket's refusals, and the statement an Oracle partition reads, pinned as
//! text. Nothing here runs it: `crates/sutura-exec-oracle/tests/flashback.rs` does, in the Oracle
//! tier.

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::model::{
    ColumnName, DimensionName, JoinType, MetricName, QualifiedTable, RelationshipName, SourceName, TableName,
};
use sutura_domain::nonempty::NonEmpty;
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{
    InternalLabel, LegPlan, PlanBindings, PlanColumn, PlanJoin, PlanJoinKey, PlanKey, ResultLabel, StatementTables,
};

use super::{AsOfRefused, InvalidKeyBucket, KeyBucket, Scn, generate_leg_as_of, generate_table_as_of};

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a test table is a table")
}

fn column(on: &str, name: &str) -> PlanColumn {
    PlanColumn::new(table(on), ColumnName::parse(name).expect("a test column is a column"))
}

fn source() -> SourceName {
    SourceName::parse("crm").expect("a test source is a source")
}

/// Customers by their integer link key, with the region a join to `dim_region` names.
fn lookup(tables: StatementTables) -> LegPlan {
    LegPlan::Lookup {
        source: source(),
        metric: MetricDigest::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: tables,
        keys: vec![
            PlanKey::new(
                ResultLabel::internal(InternalLabel::link(0)),
                column("dim_customer", "customer_key"),
            ),
            PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension is a dimension")),
                column("dim_region", "region"),
            ),
        ],
        bindings: PlanBindings::none(),
    }
}

fn joined() -> LegPlan {
    let join = PlanJoin::new(
        RelationshipName::parse("customer_region").expect("a test relationship is one"),
        QualifiedTable::new(None, table("dim_region")),
        JoinType::ManyToOne,
        NonEmpty::parse(vec![PlanJoinKey::Equal {
            origin: column("dim_customer", "region_key"),
            target: column("dim_region", "region_key"),
        }])
        .expect("a test join declares one key"),
    );
    lookup(StatementTables::parse(table("dim_customer"), vec![join]).expect("two tables, two names"))
}

fn scn() -> Scn {
    Scn::parse(4_242_424_242).expect("a test snapshot is not zero")
}

#[test]
fn a_snapshot_is_never_zero_and_a_bucket_is_one_of_two_or_more_below_the_last() {
    assert_eq!(Scn::parse(0), None);
    assert_eq!(Scn::parse(7).map(Scn::get), Some(7));
    assert_eq!(KeyBucket::new(0, 0, 1), Err(InvalidKeyBucket::TooFew { count: 1 }));
    assert_eq!(
        KeyBucket::new(0, 2, 2),
        Err(InvalidKeyBucket::PastTheLast { index: 2, count: 2 })
    );
    let last = KeyBucket::new(1, 2, 3).expect("bucket 2 of 3 is the last one");
    assert_eq!((last.key(), last.index(), last.count()), (1, 2, 3));
}

/// **Every table reference reads the one snapshot**: the root and the joined table each carry
/// `AS OF SCN`, before the join's `ON`, so neither reads the table as it is now.
#[test]
fn every_table_of_a_leg_is_read_at_the_one_snapshot() {
    let query = generate_leg_as_of(&joined(), scn(), None).expect("the leg renders at a snapshot");
    assert_eq!(
        query.sql().matches("AS OF SCN 4242424242").count(),
        2,
        "the root and the joined table: {}",
        query.sql()
    );
    assert!(
        query
            .sql()
            .contains("FROM \"dim_customer\" AS OF SCN 4242424242 LEFT JOIN \"dim_region\" AS OF SCN 4242424242 ON"),
        "{}",
        query.sql()
    );
    assert!(!query.sql().contains("ORA_HASH"), "no bucket, no hash: {}", query.sql());
}

/// **A NULL key is in bucket 0 and in no other**: bucket 0 keeps it beside its hash, every later
/// bucket drops it before hashing.
#[test]
fn bucket_zero_keeps_a_null_key_and_every_later_bucket_drops_it() {
    let leg = joined();
    let first = generate_leg_as_of(&leg, scn(), Some(KeyBucket::new(0, 0, 3).expect("a bucket"))).expect("bucket 0 renders");
    assert!(
        first.sql().contains(
            "WHERE (\"dim_customer\".\"customer_key\" IS NULL OR ORA_HASH(\"dim_customer\".\"customer_key\", 2) = 0) GROUP BY"
        ),
        "{}",
        first.sql()
    );
    let later = generate_leg_as_of(&leg, scn(), Some(KeyBucket::new(0, 2, 3).expect("a bucket"))).expect("bucket 2 renders");
    assert!(
        later.sql().contains(
            "WHERE (NOT \"dim_customer\".\"customer_key\" IS NULL AND ORA_HASH(\"dim_customer\".\"customer_key\", 2) = 2) GROUP BY"
        ),
        "{}",
        later.sql()
    );
}

/// The hashed key is the one at the bucket's position, and a position past the leg's keys is
/// refused rather than hashed by another key.
#[test]
fn a_bucket_hashes_the_key_at_its_position_and_no_key_past_the_last() {
    let leg = joined();
    let region = generate_leg_as_of(&leg, scn(), Some(KeyBucket::new(1, 1, 2).expect("a bucket"))).expect("renders");
    assert!(
        region.sql().contains("ORA_HASH(\"dim_region\".\"region\", 1) = 1"),
        "{}",
        region.sql()
    );
    let refused = generate_leg_as_of(&leg, scn(), Some(KeyBucket::new(2, 0, 2).expect("a bucket")));
    assert!(
        matches!(refused, Err(AsOfRefused::NoKey { position: 2, keys: 2 })),
        "{refused:?}"
    );
}

/// The probe reads no row of its one table, at the snapshot.
#[test]
fn a_table_probe_reads_no_row_of_its_table_at_the_snapshot() {
    let probe = generate_table_as_of(
        &source(),
        &QualifiedTable::parse("sales.dim_customer").expect("a two-part path"),
        scn(),
    )
    .expect("the probe renders");
    assert_eq!(
        probe.sql(),
        "SELECT 1 FROM \"sales\".\"dim_customer\" AS OF SCN 4242424242 WHERE 1 = 0"
    );
    assert_eq!(probe.params(), []);
}
