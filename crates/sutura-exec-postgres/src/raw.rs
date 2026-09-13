//! `docs/adr/0013`'s raw SQL tool: this adapter's own execution path - split out of `lib.rs`
//! because that file hit the thousand-line limit `cargo xtask max-lines` enforces.
//!
//! [`PostgresWarehouse::run_raw`] is what `Warehouse::execute_raw` delegates to; `source_refused`
//! reads the same [`PostgresError`] the certified path does, so both paths classify
//! `25006 read_only_sql_transaction` and `42501 insufficient_privilege` the same way.

use sutura_domain::identity::Presented;
use tokio_postgres::types::Type;

use crate::{PostgresError, PostgresWarehouse, Value, execute_err_mapped};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the raw SQL tool's own methods are kept apart from the certified path's, in their own \
              file, for the same reason `crates/sutura-domain/src/knowledge/bundle.rs` states: \
              `cargo xtask max-lines` fails at a thousand lines and the answer is to split the file"
)]
impl PostgresWarehouse {
    /// The raw SQL tool's own execution path: one caller statement, run through the extended query
    /// protocol inside a transaction this adapter opens `READ ONLY` and always rolls back.
    ///
    /// **`docs/adr/0013`'s amendment is the record for every decision here.** Three properties, and
    /// none of them is a session-level setting the base ADR already rejects:
    ///
    /// - **One statement, over the extended protocol.** `self.client.prepare` then `self.client.query`
    ///   is `Parse`/`Bind`/`Execute` on the wire, and Postgres refuses more than one command in a
    ///   `Parse` message - so `SELECT 1; DROP TABLE t` is refused by the SERVER as a syntax error,
    ///   never executed in part. `BEGIN READ ONLY` and `ROLLBACK` go over the SIMPLE protocol
    ///   (`batch_execute`), which is fine precisely because that text is sutura's own fixed literal,
    ///   never the caller's.
    /// - **Wrapped in a transaction opened `READ ONLY`, by this adapter, around exactly this one
    ///   call.** A write inside it is refused by the server (`25006 read_only_sql_transaction`)
    ///   independent of what the connecting role could otherwise do - the real, server-enforced
    ///   second control the amendment adds beside the operator's role grant.
    /// - **Always rolled back**, whether the statement answered, errored, or the connection's own
    ///   `statement_timeout` fired: there is no `COMMIT` on this path, so nothing the call did
    ///   persists. A caller statement that read `SET TRANSACTION READ WRITE` flips nothing that
    ///   survives past this one call, because there is no later statement in the same transaction for
    ///   it to apply to before the rollback below runs.
    ///
    /// **The limit, named rather than left implicit:** a `READ ONLY` transaction bounds SQL-visible
    /// writes - `INSERT`/`UPDATE`/`DELETE`/most DDL - and says nothing about a VOLATILE function's own
    /// side effects once the connecting role may call it. Naming such a function in the statement is
    /// outside both this transaction and the role grant it sits beside.
    pub(crate) fn run_raw(
        &self,
        statement: &sutura_domain::raw::RawStatement,
        presented: &Presented,
    ) -> Result<sutura_domain::warehouse::RawRows, PostgresError> {
        self.deliverable(presented)?;
        let sql = statement.as_str();
        self.runtime.block_on(async {
            self.client
                .batch_execute("BEGIN READ ONLY")
                .await
                .map_err(|cause| PostgresError::RawTransaction { cause })?;
            let outcome = self.run_raw_statement(sql).await;
            // Rolled back unconditionally: no `COMMIT` exists on this path, so nothing the call did
            // persists whether it answered, errored, or timed out. A rollback that itself fails is
            // the connection's problem on its way out, not the caller's statement's - logged nowhere
            // here because this adapter takes no logging dependency; the connection is left to the
            // pool's own next use, which will open its own fresh transaction regardless.
            drop(self.client.batch_execute("ROLLBACK").await);
            outcome
        })
    }

    /// The one caller statement, over the extended protocol - split out of
    /// [`Self::run_raw`](PostgresWarehouse::run_raw) so the transaction wrapper above reads as
    /// unconditional rollback around one expression, not around a multi-statement block a future
    /// edit could grow an early return out of.
    async fn run_raw_statement(&self, sql: &str) -> Result<sutura_domain::warehouse::RawRows, PostgresError> {
        let prepared = self
            .client
            .prepare(sql)
            .await
            .map_err(|cause| PostgresError::Prepare { cause })?;
        let columns: Vec<(String, Type)> = prepared
            .columns()
            .iter()
            .map(|column| (column.name().to_owned(), column.type_().clone()))
            .collect();
        let rows = self.client.query(&prepared, &[]).await.map_err(execute_err_mapped)?;
        let labels: Vec<String> = columns.iter().map(|(name, _)| name.to_owned()).collect();
        let mut out: Vec<Vec<Value>> = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut cells = Vec::with_capacity(columns.len());
            for (index, (label, column_type)) in columns.iter().enumerate() {
                cells.push(Self::cell(label, column_type, row, index)?);
            }
            out.push(cells);
        }
        Ok(sutura_domain::warehouse::RawRows::of(labels, out))
    }
}

/// The predicate `Warehouse::source_refused` delegates to for the raw path - see that method for
/// what it means for a caller.
pub(crate) fn source_refused(error: &PostgresError) -> bool {
    matches!(
        *error,
        PostgresError::Prepare { ref cause } | PostgresError::Execute { ref cause }
            if matches!(
                cause.code(),
                Some(
                    &tokio_postgres::error::SqlState::READ_ONLY_SQL_TRANSACTION
                        | &tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE
                )
            )
    )
}
