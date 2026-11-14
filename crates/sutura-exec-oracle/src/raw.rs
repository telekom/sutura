//! Raw queries use a fresh shared-user session and a read-only transaction. The account's grants
//! remain the boundary for autonomous functions and external effects. Driver timeouts apply to socket reads and writes;
//! connect handshakes and drop cleanup are not a total deadline. Native recovery may disconnect.

use sutura_domain::identity::Presented;
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{RawRows, ResultBudget};

use crate::{DIAL_DEADLINE, OracleError, OracleWarehouse, batches, connect_err, execute_err_mapped, refuse_if_spent};

pub(crate) fn run(
    warehouse: &OracleWarehouse,
    config: &oracledb::Config,
    text: &RawStatement,
    presented: &Presented,
    deadline: Deadline,
) -> Result<RawRows, OracleError> {
    warehouse.routing.deliverable(presented)?;
    if text.as_str().contains(';') || text.as_str().ends_with('/') {
        return Err(OracleError::RawNotQuery);
    }
    let remaining = refuse_if_spent(deadline)?;
    crate::oracle_trace::refuse()?;
    #[expect(
        clippy::disallowed_methods,
        reason = "oracle_trace::refuse was asked immediately before this dial"
    )]
    let connection = oracledb::connect(
        config
            .clone()
            .set_transport_connect_timeout(Some(remaining.min(DIAL_DEADLINE))),
    )
    .map_err(connect_err)?;
    let outcome = (|| {
        timed(&connection, deadline)?;
        crate::datetime::pin_utc(&connection, Some(deadline))?;
        timed(&connection, deadline)?;
        connection.rollback().map_err(execute_err_mapped)?;
        timed(&connection, deadline)?;
        connection
            .execute("SET TRANSACTION READ ONLY", &[])
            .map_err(execute_err_mapped)?;
        query(&connection, text, warehouse.result_budget, deadline)
    })();
    // A failed rollback is followed by discard of this call's session, never reuse or commit.
    drop(connection.rollback());
    outcome
}

fn timed(connection: &oracledb::Connection, deadline: Deadline) -> Result<(), OracleError> {
    let remaining = refuse_if_spent(deadline)?;
    connection
        .set_call_timeout(Some(remaining))
        .map_err(|cause| OracleError::CallTimeout { cause: cause.into() })
}

fn query(
    connection: &oracledb::Connection,
    text: &RawStatement,
    budget: ResultBudget,
    deadline: Deadline,
) -> Result<RawRows, OracleError> {
    let mut statement = connection
        .statement(text.as_str())
        .map_err(execute_err_mapped)?
        .arrow_batch_per_round_trip()
        .exact_number_text()
        .fetch_lobs()
        .prefetch_rows(0)
        // ponytail: one row per fetch bounds transient values; size batches after measuring latency.
        .fetch_array_size(1)
        .exclude_from_cache()
        .build()
        .map_err(execute_err_mapped)?;
    if !statement.is_query() {
        return Err(OracleError::RawNotQuery);
    }
    timed(connection, deadline)?;
    statement.ensure_fully_parsed().map_err(execute_err_mapped)?;
    if statement.out_metadata().is_empty() {
        return Err(OracleError::RawNotQuery);
    }
    admit_columns(statement.out_metadata(), budget)?;
    timed(connection, deadline)?;
    let (rows, _) = batches::prepared(
        statement,
        &[],
        batches::Stop {
            budget,
            most_rows: Some(sutura_domain::plan::MAX_ROWS as usize + 1),
        },
        || timed(connection, deadline),
    )?;
    let (columns, rows) = rows.into_parts();
    Ok(RawRows::of(columns, rows))
}

fn admit_columns(columns: &[oracledb::Metadata], budget: ResultBudget) -> Result<(), OracleError> {
    let mut most_row_bytes = 0usize;
    for column in columns {
        let kind = column.db_type();
        if ![
            oracledb::DB_TYPE_NUMBER,
            oracledb::DB_TYPE_VARCHAR,
            oracledb::DB_TYPE_NVARCHAR,
            oracledb::DB_TYPE_CHAR,
            oracledb::DB_TYPE_NCHAR,
            oracledb::DB_TYPE_DATE,
            oracledb::DB_TYPE_TIMESTAMP,
            oracledb::DB_TYPE_TIMESTAMP_TZ,
            oracledb::DB_TYPE_TIMESTAMP_LTZ,
            oracledb::DB_TYPE_BINARY_DOUBLE,
            oracledb::DB_TYPE_BOOLEAN,
        ]
        .contains(&kind)
        {
            return Err(OracleError::UnsupportedType {
                column: String::from(column.name()),
                oracle_type: kind.name(),
            });
        }
        most_row_bytes = most_row_bytes.saturating_add((column.max_size() as usize).saturating_mul(4).saturating_add(64));
        if most_row_bytes > budget.bytes() {
            return Err(OracleError::OverBudget {
                most_bytes: budget.bytes(),
            });
        }
    }
    Ok(())
}
