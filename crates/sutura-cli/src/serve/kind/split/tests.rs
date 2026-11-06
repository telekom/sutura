//! A granted partition past the reader's per-stream bound answers the typed refusal the planning
//! node reads back, and any other failure of the read stays the engine's.

use sutura_domain::model::SourceName;
use sutura_domain::warehouse::UnannouncedBatch;
use sutura_exec_datafusion::Unread;
use sutura_exec_postgres::PostgresError;
use sutura_exec_postgres::adbc::AdbcError;

use super::unread;

fn source() -> SourceName {
    SourceName::parse("crm").expect("a source name")
}

#[test]
fn only_a_partition_past_the_readers_row_bound_is_read_as_one_that_did_not_fit() {
    let over = PostgresError::Adbc {
        cause: AdbcError::Unannounced(UnannouncedBatch::OverBound { most: 1 }),
    };
    let read = unread(&source(), over);
    assert!(matches!(read, Unread::DidNotFit), "{read:?}");
    let other = unread(&source(), PostgresError::DeadlineSpent);
    assert!(matches!(other, Unread::Failed(_)), "{other:?}");
}
