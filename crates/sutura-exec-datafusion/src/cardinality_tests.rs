//! The declared-key probe's own suite, split out of `execute_tests.rs` under the same `max-lines`
//! reason every sibling file here carries: a pure move, nothing behavioural changed with the split.

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use sutura_domain::model::{ColumnName, Grain, SourceName, TableName};

use crate::DataFusionWarehouse;

fn roomy() -> crate::WorkingSet {
    crate::WorkingSet::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test ceiling is positive"))
}

fn batch(fields: Vec<Field>, columns: Vec<ArrayRef>) -> RecordBatch {
    RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).expect("a test batch is rectangular")
}

/// A compound key probe must count a row toward EITHER aggregate only when every column of the
/// whole key is non-null - the same rule `crates/sutura-sql/src/generate.rs`'s SQL probe carries.
///
/// **Red before the fix in `translate::key_counts`.** Three target rows: `(k1, m1)` twice and
/// `(NULL, m2)` once. The buggy shape counted `rows` from the FIRST column alone (`k1, k1, NULL` -
/// non-null count 2) and `distinct` from a `named_struct` that is never itself null even when one
/// of its fields is (`(k1, m1)` and `(NULL, m2)` - two distinct structs), so it read `rows == 2` and
/// `distinct == 2` and called `(k1, m1)`'s real duplicate unique. The fix excludes any row with a
/// null column from BOTH counts, so `rows == 2` (only the two `(k1, m1)` rows) and `distinct == 1`
/// (one distinct non-null pair), and `is_unique()` reports the real duplication rather than hiding
/// it behind the null-bearing third row.
#[test]
fn a_compound_probe_excludes_a_row_with_any_null_key_column_from_both_counts() {
    let batch = batch(
        vec![
            Field::new("subscription_key", DataType::Utf8, true),
            Field::new("month", DataType::Utf8, true),
        ],
        vec![
            Arc::new(StringArray::from(vec![Some("k1"), Some("k1"), None])),
            Arc::new(StringArray::from(vec![Some("m1"), Some("m1"), Some("m2")])),
        ],
    );
    let adapter = DataFusionWarehouse::new(
        SourceName::parse("local").expect("a test source is a source"),
        crate::test_posture(),
        roomy(),
    )
    .expect("a current-thread runtime builds");
    drop(
        adapter
            .context
            .register_batch("snapshot", batch)
            .expect("an in-memory batch registers"),
    );

    let column = |name: &str| ColumnName::parse(name).expect("a test column is a column");
    let relationship = sutura_domain::catalog::Relationship::new(
        sutura_domain::model::RelationshipName::parse("usage_subscription").expect("a test relationship is a relationship"),
        sutura_domain::model::ModelName::parse("daily_usage").expect("a test model is a model"),
        sutura_domain::model::ModelName::parse("snapshot").expect("a test model is a model"),
        sutura_domain::model::JoinType::ManyToOne,
        sutura_domain::catalog::JoinKeys::of(vec![
            sutura_domain::catalog::JoinKey::Equal {
                origin: column("subscription_key"),
                target: column("subscription_key"),
            },
            sutura_domain::catalog::JoinKey::TruncatedEqual {
                origin: column("usage_date"),
                grain: Grain::Month,
                target: column("month"),
            },
        ])
        .expect("two keys is a non-empty set"),
    );
    let definitions = sutura_domain::catalog::Definitions::assemble(
        vec![
            sutura_domain::catalog::Model::new(
                sutura_domain::model::ModelName::parse("daily_usage").expect("a test model is a model"),
                SourceName::parse("local").expect("a test source is a source"),
                TableName::parse("daily_usage").expect("a test table is a table"),
                std::collections::BTreeSet::from([column("subscription_key"), column("usage_date")]),
                sutura_domain::catalog::Description::default(),
            ),
            sutura_domain::catalog::Model::new(
                sutura_domain::model::ModelName::parse("snapshot").expect("a test model is a model"),
                SourceName::parse("local").expect("a test source is a source"),
                TableName::parse("snapshot").expect("a test table is a table"),
                std::collections::BTreeSet::from([column("subscription_key"), column("month")]),
                sutura_domain::catalog::Description::default(),
            ),
        ],
        vec![relationship.clone()],
        Vec::new(),
    )
    .expect("two models and one compound join are consistent");
    let key = sutura_domain::warehouse::cardinality::DeclaredKey::promised_by(&relationship, &definitions)
        .expect("a many-to-one promises a unique target");

    // The inherent `key_uniqueness`, not the `Warehouse::declared_key` port method: that method is
    // `clippy::disallowed_methods` restricted to its two permitted callers (the boot path and the
    // registry cell in `sutura-app`'s golden suite), and this cell tests the DataFusion-specific
    // aggregate `translate::key_counts` builds rather than the port's own dispatch.
    let answered = adapter
        .runtime()
        .expect("a test runtime is present")
        .block_on(adapter.key_uniqueness(&key))
        .unwrap_or_else(|e| panic!("the compound probe did not run: {e}"));
    let sutura_domain::warehouse::cardinality::KeyUniqueness::Counted(counts) = answered else {
        panic!("the compound probe must count, not skip: {answered:?}");
    };
    assert_eq!(counts.rows(), 2, "the null-bearing row must not be counted: {counts:?}");
    assert_eq!(
        counts.distinct(),
        1,
        "the two (k1, m1) rows are one distinct non-null pair: {counts:?}"
    );
    assert!(
        !counts.is_unique(),
        "(k1, m1) is a real duplicate; a null-bearing third row must not hide it: {counts:?}"
    );
}
