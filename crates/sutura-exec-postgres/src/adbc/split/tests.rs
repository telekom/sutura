//! The page cut, the snapshot id's admission, and what the export and a partition send, over
//! `super::super::tests`' fake connection. The tier half is `tests/split.rs`.

use core::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use arrow_array::StringArray;

use super::super::session::{self, SnapshotId};
use super::super::tests::{CEILING_MS, FakeConnection, Sent, a_number, one_column, open_for};
use super::cut;

fn partitions(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("a test asks for at least one partition")
}

#[test]
fn one_partition_is_the_whole_table_with_no_bound() {
    assert_eq!(cut(40, partitions(1)), vec![(0, None)]);
}

#[test]
fn more_partitions_than_pages_cut_one_page_each_and_leave_the_last_open() {
    assert_eq!(cut(3, partitions(10)), vec![(0, Some(1)), (1, Some(2)), (2, None)]);
}

#[test]
fn a_table_the_catalog_has_no_pages_for_is_one_open_range() {
    assert_eq!(cut(0, partitions(8)), vec![(0, None)]);
}

#[test]
fn the_ranges_tile_the_pages_with_no_gap_and_no_overlap() {
    let ranges = cut(1_000, partitions(7));
    assert_eq!(ranges.len(), 7);
    assert_eq!(ranges.first(), Some(&(0, Some(142))));
    for pair in ranges.windows(2) {
        let [(_, to), (from, _)] = *pair else {
            panic!("windows(2) yields pairs");
        };
        assert_eq!(to, Some(from), "{ranges:?}");
    }
    assert_eq!(ranges.last(), Some(&(857, None)));
}

#[test]
fn a_snapshot_id_is_hex_digits_and_hyphens_and_nothing_else() {
    assert!(SnapshotId::parse("00000003-0000001B-1").is_some());
    for refused in [
        "",
        "0000'; COMMIT; --",
        "00000003 0000001B",
        "00000003-0000001B-1\n",
        &"0".repeat(65),
    ] {
        assert!(SnapshotId::parse(refused).is_none(), "{refused:?} was admitted");
    }
}

#[test]
fn the_export_reads_its_snapshot_and_pages_and_leaves_its_transaction_open() {
    let mut connection = FakeConnection::replying(one_column("id", Arc::new(StringArray::from(vec!["00000003-0000001B-1"]))));
    let relation = Some(a_number());
    let outcome = session::export(&mut connection, relation, CEILING_MS, open_for(Duration::from_secs(60)));
    assert!(outcome.is_ok(), "{outcome:?}");
    let sent = connection.sent();
    assert!(
        !sent.contains(&Sent::Rollback),
        "the exporting transaction must stay open: {sent:?}"
    );
    let [
        Sent::AutoCommit(ref autocommit),
        Sent::Update(ref setting),
        Sent::Query { sql: ref export, .. },
        Sent::Bound {
            statement: 2,
            rows: 1,
            columns: 1,
        },
        Sent::Query {
            statement: 2,
            sql: ref pages,
        },
    ] = *sent.as_slice()
    else {
        panic!("autocommit off, the setting, the export, and the bound page read: {sent:?}");
    };
    assert_eq!(autocommit, "false");
    assert!(
        setting.starts_with(
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY; SET LOCAL statement_timeout = 15000; \
             SET LOCAL idle_in_transaction_session_timeout = "
        ),
        "{setting}"
    );
    assert_eq!(export, "SELECT pg_catalog.pg_export_snapshot()");
    assert!(pages.contains("pg_catalog.to_regclass($1) AND c.relkind = 'r'"), "{pages}");
}

#[test]
fn a_partition_imports_the_snapshot_before_its_statement_and_rolls_back() {
    let snapshot = SnapshotId::parse("00000003-0000001B-1").expect("a server-shaped id");
    let mut connection = FakeConnection::replying(a_number());
    let outcome = session::partition(
        &mut connection,
        &snapshot,
        "SELECT 1",
        None,
        CEILING_MS,
        open_for(Duration::from_secs(60)),
    );
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(
        connection.sent(),
        vec![
            Sent::AutoCommit(String::from("false")),
            Sent::Update(String::from(
                "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY; \
                 SET TRANSACTION SNAPSHOT '00000003-0000001B-1'; SET LOCAL statement_timeout = 15000"
            )),
            Sent::Query {
                statement: 1,
                sql: String::from("SELECT 1"),
            },
            Sent::Rollback,
        ]
    );
}
