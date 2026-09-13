//! `docs/adr/0013`'s raw SQL tool: this adapter's own execution path - split out of `lib.rs`
//! because that file hit the thousand-line limit `cargo xtask max-lines` enforces.
//!
//! [`PostgresWarehouse::run_raw`] is what `Warehouse::execute_raw` delegates to; `source_refused`
//! reads the same [`PostgresError`] the certified path does, so both paths classify
//! `25006 read_only_sql_transaction` and `42501 insufficient_privilege` the same way.

use futures_util::TryStreamExt as _;
use sutura_domain::identity::Presented;
use tokio_postgres::types::{ToSql, Type};

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
    /// **`docs/adr/0013`'s amendment is the record for every decision here.** Four properties, and
    /// none of them is a session-level setting the base ADR already rejects:
    ///
    /// - **`SET LOCAL statement_timeout`, inside this same transaction** - `docs/adr/0029`'s
    ///   Postgres row. `LOCAL` rather than the session-level `SET` because it resets itself at
    ///   `ROLLBACK` rather than outliving this one call on a connection every caller shares. The
    ///   value is the connect-time ceiling (`statement_timeout_ceiling_ms`), not a request's own
    ///   budget: `Warehouse::execute_raw` carries no [`sutura_domain::warehouse::deadline::Deadline`]
    ///   to narrow it with - the limit `docs/adr/0029` states next to this row.
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
        // Held for the whole `BEGIN` / statement / `ROLLBACK` triple: this client pipelines, so
        // without this a concurrent caller's own exchange interleaves on the wire mid-transaction
        // - see `PostgresWarehouse::execution_lock` for what was measured without it.
        let _guard = crate::lock_execution(&self.execution_lock);
        // `SET LOCAL statement_timeout`, scoped to this one call's own transaction - `docs/adr/0029`.
        // **The limit, stated where the value comes from:** `Warehouse::execute_raw` carries no
        // per-request `Deadline` (unlike `dry_run`/`execute`), so this is always the connect-time
        // ceiling (`statement_timeout_ceiling_ms`), never a request's own narrower budget. A caller
        // statement is still stopped - `telekom/sutura#129`'s cancellation prerequisite for the raw
        // tool, discharged to that ceiling - just not to whatever is left of the asker's own request.
        let ceiling_ms = self.statement_timeout_ceiling_ms;
        self.runtime.block_on(async {
            self.client
                .batch_execute(&format!("BEGIN READ ONLY; SET LOCAL statement_timeout = {ceiling_ms}"))
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
    ///
    /// **Streamed, and read no further than [`sutura_domain::plan::MAX_ROWS`] plus one.** The
    /// certified path bounds the same way with a `LIMIT` the compiler adds to the rendered SQL; a
    /// raw statement is unparsed text with no clause this adapter may add - `docs/adr/0013`'s own
    /// rule against inspecting it applies here too - so the bound has to be how many rows this
    /// loop is willing to pull off the wire, not what the statement says. `Client::query` would
    /// have materialised every row the statement produced before anything downstream could count
    /// them; `Client::query_raw` yields rows one at a time, so a statement that would return far
    /// more than the cap is stopped here rather than after it is already in this process's heap.
    /// One row past the cap - not exactly at it - so the caller's own `exceeds_row_cap` check
    /// still sees a count over the limit and refuses it, rather than a truncated result that
    /// looks like a complete answer of exactly the cap's size.
    ///
    /// **The limit, named rather than left implied (`#666` round 2, finding 2):** this stop bounds
    /// the HEAP, not the wire. The server still computes and ships the whole result regardless of
    /// how many rows this loop keeps, and dropping the stream early does not cancel the query - the
    /// connection's own driver task drains every remaining `DataRow` off the socket before
    /// `ROLLBACK` can run. A statement that would return far more than the cap still costs the
    /// server the full computation and this connection the full transfer, bounded only by the
    /// connect-time `statement_timeout`. The real bound - a portal opened with `max_rows` via
    /// `Client::transaction`/`Transaction::query_portal`, which asks the SERVER to send only that
    /// many rows per `Execute` - needs `&mut Client`, which `execution_lock` does not yet provide;
    /// see that field's own documentation for why a `Mutex<Client>` is the follow-up this stop is
    /// standing in for.
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
        let labels: Vec<String> = columns.iter().map(|(name, _)| name.to_owned()).collect();
        let params: [&(dyn ToSql + Sync); 0] = [];
        let mut stream = Box::pin(self.client.query_raw(&prepared, params).await.map_err(execute_err_mapped)?);
        let cap = usize::try_from(sutura_domain::plan::MAX_ROWS).unwrap_or(usize::MAX);
        let mut out: Vec<Vec<Value>> = Vec::new();
        while let Some(row) = stream.try_next().await.map_err(execute_err_mapped)? {
            let mut cells = Vec::with_capacity(columns.len());
            for (index, (label, column_type)) in columns.iter().enumerate() {
                cells.push(Self::cell(label, column_type, &row, index)?);
            }
            out.push(cells);
            if out.len() > cap {
                break;
            }
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
