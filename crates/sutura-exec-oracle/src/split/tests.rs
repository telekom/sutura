//! What a split reads off the dictionary's answers, and the refusals a partition read makes before
//! any round trip. The statements themselves run in the Oracle tier only
//! (`crates/sutura-exec-oracle/tests/flashback.rs`).

use core::num::NonZeroUsize;
use core::time::Duration;
use std::collections::BTreeSet;

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::model::{ColumnName, DimensionName, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{EstimatedRows, InternalLabel, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel};
use sutura_domain::warehouse::deadline::Budget;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{RowSet, Value};
use sutura_sql::{KeyBucket, Scn};

use super::{Root, admit, cut, fresh, hashed_key, root, snapshot};
use crate::OracleError;

fn answer(columns: &[&str], rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(columns.iter().map(|&name| String::from(name)).collect(), rows).expect("a rectangular answer")
}

fn text(value: &str) -> Value {
    Value::Text(String::from(value))
}

fn column(table: &str, name: &str) -> PlanColumn {
    PlanColumn::new(
        TableName::parse(table).expect("a test table is a table"),
        ColumnName::parse(name).expect("a test column is a column"),
    )
}

/// A lookup leg over `dim_customer`, keyed by `keys` in order.
fn lookup(keys: Vec<PlanKey>) -> LegPlan {
    LegPlan::Lookup {
        source: SourceName::parse("crm").expect("a test source is a source"),
        metric: MetricDigest::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: TableName::parse("dim_customer").expect("a test table is a table").into(),
        keys,
        bindings: PlanBindings::none(),
    }
}

fn dimension(table: &str, name: &str) -> PlanKey {
    PlanKey::new(
        ResultLabel::dimension(&DimensionName::parse(name).expect("a test dimension is a dimension")),
        column(table, name),
    )
}

/// The dictionary's statistics multiply into bytes, every integer column is collected, a table with
/// no statistics estimates at zero, and a name the dictionary holds no table of is no root at all.
#[test]
fn the_dictionary_answer_is_the_roots_rows_bytes_and_integer_columns() {
    let columns = ["NUM_ROWS", "AVG_ROW_LEN", "COLUMN_NAME"];
    let read = root(&answer(
        &columns,
        vec![
            vec![Value::Integer(1_000), Value::Integer(48), text("customer_key")],
            vec![Value::Integer(1_000), Value::Integer(48), text("region_key")],
        ],
    ))
    .expect("the answer reads");
    assert_eq!(
        read,
        Some(Root {
            rows: EstimatedRows::parse(1_000),
            bytes: EstimatedBytes::parse(48_000),
            integers: BTreeSet::from([String::from("customer_key"), String::from("region_key")]),
        })
    );
    let unanalysed = root(&answer(&columns, vec![vec![Value::Null, Value::Null, Value::Null]])).expect("reads");
    assert_eq!(
        unanalysed,
        Some(Root {
            rows: EstimatedRows::parse(0),
            bytes: EstimatedBytes::parse(0),
            integers: BTreeSet::new(),
        })
    );
    assert_eq!(root(&answer(&columns, Vec::new())).expect("reads"), None);
    let negative = root(&answer(
        &columns,
        vec![vec![Value::Integer(-1), Value::Integer(1), Value::Null]],
    ));
    assert!(matches!(negative, Err(OracleError::Unanswered { .. })), "{negative:?}");
}

/// **The hashed key is the first that is an integer column of the root itself**: a joined table's
/// column of the same name is not the root's, and a text key is not hashed.
#[test]
fn the_hashed_key_is_the_first_integer_column_of_the_root() {
    let integers = BTreeSet::from([String::from("customer_key"), String::from("region_key")]);
    let leg = lookup(vec![
        dimension("dim_region", "region_key"),
        dimension("dim_customer", "segment"),
        PlanKey::new(
            ResultLabel::internal(InternalLabel::link(0)),
            column("dim_customer", "customer_key"),
        ),
    ]);
    assert_eq!(hashed_key(&leg, &integers), Some(2));
    let textual = lookup(vec![dimension("dim_customer", "segment")]);
    assert_eq!(hashed_key(&textual, &integers), None);
}

/// One descriptor per bucket, every one at the same snapshot and of the same key.
#[test]
fn a_split_is_one_bucket_per_partition_at_one_snapshot() {
    let scn = Scn::parse(77).expect("not zero");
    let cut = cut(scn, 1, NonZeroUsize::new(3).expect("three"));
    let buckets: Vec<KeyBucket> = cut.iter().map(super::PartitionDescriptor::bucket).collect();
    assert_eq!(
        buckets,
        (0..3)
            .map(|index| KeyBucket::new(1, index, 3).expect("a bucket"))
            .collect::<Vec<_>>()
    );
    assert!(cut.iter().all(|partition| partition.scn() == scn));
}

/// A snapshot is one positive number; anything else is the database answering something the
/// statement does not project.
#[test]
fn a_snapshot_is_one_positive_number() {
    let scn = snapshot(&answer(&["SCN"], vec![vec![Value::Integer(9)]])).expect("one number");
    assert_eq!(scn.get(), 9);
    for refused in [
        vec![vec![Value::Integer(0)]],
        vec![vec![Value::Null]],
        vec![vec![Value::Integer(9)], vec![Value::Integer(9)]],
        Vec::new(),
    ] {
        let read = snapshot(&answer(&["SCN"], refused));
        assert!(matches!(read, Err(OracleError::Unanswered { .. })), "{read:?}");
    }
}

/// **D4's refusal**: the database answering that the snapshot is older than the budget refuses the
/// read, naming the budget; its answer that it is not lets the read go on.
#[test]
fn a_snapshot_older_than_the_request_budget_is_refused() {
    let budget = Budget::parse(Duration::from_secs(30)).expect("a budget");
    let stale = fresh(&answer(&["FRESH"], vec![vec![Value::Integer(0)]]), budget);
    assert!(
        matches!(stale, Err(OracleError::SnapshotStale { most_seconds: 30 })),
        "{stale:?}"
    );
    fresh(&answer(&["FRESH"], vec![vec![Value::Integer(1)]]), budget).expect("a fresh snapshot reads");
}

/// **A source that did not declare and probe its split reads no partition**, before the descriptor
/// is even parsed; one that did parses it.
#[test]
fn a_partition_at_a_source_without_a_flashback_split_is_refused() {
    let at = SourceName::parse("crm").expect("a test source is a source");
    let descriptor =
        super::PartitionDescriptor::new(Scn::parse(5).expect("not zero"), KeyBucket::new(0, 1, 2).expect("a bucket")).to_bytes();
    let refused = admit(false, &at, &descriptor);
    assert!(
        matches!(refused, Err(OracleError::FlashbackNotDeclared { ref at }) if at == "crm"),
        "{refused:?}"
    );
    admit(true, &at, &descriptor).expect("a declared split parses its descriptor");
    let malformed = admit(true, &at, &[]);
    assert!(
        matches!(malformed, Err(OracleError::MalformedDescriptor { .. })),
        "{malformed:?}"
    );
}
