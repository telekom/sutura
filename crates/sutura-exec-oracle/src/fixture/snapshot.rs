//! Readiness of a freshly created fixture table for a read-only snapshot.

use std::time::{Duration, Instant};

use parking_lot::MutexGuard;

use super::FixtureError;
use crate::{DriverError, OracleError, OracleWarehouse};

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

/// Probes `table` through a fresh shared-user session per attempt, while `guard` holds the fixture
/// lock. Only `ORA-01466` retries, for up to 60 seconds; every other refusal is returned with its
/// cause. Connect, handshake and cleanup are not bounded by that retry deadline.
pub(crate) fn await_read_only(guard: &ExecutionGuard<'_>, table: &str) -> Result<(), FixtureError> {
    let config = guard
        .warehouse
        .raw_config
        .as_ref()
        .ok_or(FixtureError::SnapshotNeedsSharedUser)?;
    let probe = format!(r#"SELECT COUNT(*) FROM "{table}""#);
    let deadline = Instant::now() + DEADLINE;
    let mut attempt = 0_u16;
    loop {
        attempt = attempt.saturating_add(1);
        let refused = |phase, cause: oracledb::Error| statement_refused(table, phase, attempt, DriverError::from(cause));
        crate::oracle_trace::refuse()
            .map_err(OracleError::from)
            .map_err(FixtureError::SnapshotConnect)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "oracle_trace::refuse was asked immediately before this probe dial"
        )]
        let connection = oracledb::connect(config.clone())
            .map_err(crate::connect_err)
            .map_err(FixtureError::SnapshotConnect)?;
        connection
            .rollback()
            .map_err(|cause| refused("rollback before probe", cause))?;
        connection
            .execute("SET TRANSACTION READ ONLY", &[])
            .map_err(|cause| refused("begin read-only", cause))?;
        match connection.query_row(&probe, &[]) {
            Ok(_) => return connection.rollback().map_err(|cause| refused("rollback after probe", cause)),
            Err(cause) => {
                let cause = DriverError::from(cause);
                if !cause.has_ora_code(DEFINITION_CHANGED) || Instant::now() >= deadline {
                    return Err(statement_refused(table, "count probe", attempt, cause));
                }
                drop(connection);
                std::thread::sleep(RETRY);
            }
        }
    }
}

fn statement_refused(table: &str, phase: &'static str, attempt: u16, cause: DriverError) -> FixtureError {
    FixtureError::Statement {
        subject: format!("reading {table} under a read-only snapshot: phase={phase}, attempt={attempt}"),
        cause,
    }
}
