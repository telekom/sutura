//! Readiness of a freshly created fixture table for a read-only snapshot.

use std::time::{Duration, Instant};

use parking_lot::MutexGuard;

use super::FixtureError;
use crate::{DriverError, OracleWarehouse};

/// `ORA-01466`: the table's definition changed after the snapshot began.
const DEFINITION_CHANGED: usize = 1466;
const DEADLINE: Duration = Duration::from_secs(60);
const RETRY: Duration = Duration::from_millis(200);

/// A warehouse's execution lock, held: the connection it serializes is read through this and
/// nothing else. Only [`lock`] makes one.
pub(crate) struct ExecutionGuard<'a> {
    warehouse: &'a OracleWarehouse,
    _lock: MutexGuard<'a, ()>,
}

/// `warehouse`'s execution lock, taken.
pub(crate) fn lock(warehouse: &OracleWarehouse) -> ExecutionGuard<'_> {
    ExecutionGuard {
        warehouse,
        _lock: warehouse.execution_lock.lock(),
    }
}

/// Waits, bounded, until a `SET TRANSACTION READ ONLY` can read `table` on the connection `guard`
/// holds. For a short window after the DDL Oracle refuses that snapshot with `ORA-01466`, as it
/// would a split read; any other refusal, and `ORA-01466` at the deadline, is returned with its cause.
pub(crate) fn await_read_only(guard: &ExecutionGuard<'_>, table: &str) -> Result<(), FixtureError> {
    let refused = |cause: oracledb::Error| statement_refused(table, DriverError::from(cause));
    let connection = &guard.warehouse.connection;
    let probe = format!(r#"SELECT COUNT(*) FROM "{table}""#);
    let deadline = Instant::now() + DEADLINE;
    loop {
        connection.rollback().map_err(refused)?;
        connection.execute("SET TRANSACTION READ ONLY", &[]).map_err(refused)?;
        match connection.query_row(&probe, &[]) {
            Ok(_) => return connection.rollback().map_err(refused),
            Err(cause) => {
                let cause = DriverError::from(cause);
                if !cause.has_ora_code(DEFINITION_CHANGED) || Instant::now() >= deadline {
                    return Err(statement_refused(table, cause));
                }
                std::thread::sleep(RETRY);
            }
        }
    }
}

fn statement_refused(table: &str, cause: DriverError) -> FixtureError {
    FixtureError::Statement {
        subject: format!("reading {table} under a read-only snapshot"),
        cause,
    }
}
