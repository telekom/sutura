//! `docs/adr/0029`'s per-statement `SET LOCAL statement_timeout` mechanism - split out of `lib.rs`
//! because that file hit the thousand-line limit `cargo xtask max-lines` enforces, the same reason
//! `raw.rs` exists beside it.
//!
//! [`PostgresWarehouse::run`] is the boot path's own statement runner - no deadline, the
//! connect-time ceiling alone - and [`PostgresWarehouse::run_with_deadline`] is what
//! `Warehouse::execute` calls; both end at [`PostgresWarehouse::prepare_and_query`], reading `lib.rs`'s
//! private `bind` and `cell` the same way `raw.rs` already does.

use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{RowSet, Value};
use sutura_sql::GeneratedQuery;
use tokio_postgres::Row;
use tokio_postgres::types::Type;

use crate::{PgParam, PostgresError, PostgresWarehouse, execute_err_mapped, lock_execution};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the per-statement deadline mechanism is kept apart from the rest of the certified \
              path, in its own file, for the same reason `raw.rs` states: `cargo xtask max-lines` \
              fails at a thousand lines and the answer is to split the file"
)]
impl PostgresWarehouse {
    /// Runs a statement and collects its rows, under the SESSION's own `statement_timeout` - the
    /// connect-time ceiling, never a request's. **Only the boot path calls this**:
    /// `Warehouse::verify_anchor` and `Warehouse::declared_key` have no deadline to scope a `SET
    /// LOCAL` to, and no listener is bound yet for either to be racing a caller's budget.
    /// `Warehouse::execute` calls [`Self::run_with_deadline`] instead.
    pub(crate) fn run(&self, query: &GeneratedQuery) -> Result<RowSet, PostgresError> {
        let _guard = lock_execution(&self.execution_lock);
        let (columns, rows) = self.runtime.block_on(Self::prepare_and_query(&self.client, query))?;
        Self::rows_from_columns(&columns, rows)
    }

    /// Runs a statement inside a transaction this adapter opens and always rolls back, with `SET
    /// LOCAL statement_timeout` set to what `deadline` has left - `docs/adr/0029`'s Postgres row.
    ///
    /// **One extra round trip for the `BEGIN`/`SET LOCAL` pair, one more for the `ROLLBACK`** -
    /// `SET LOCAL` only takes effect inside a transaction block, so scoping it to one statement
    /// needs one, and there is nothing here to commit: a certified plan only ever `SELECT`s, so
    /// discarding the transaction either way is exact. The rollback's own outcome is dropped, the
    /// same choice `raw::run_raw` makes for its own wrapper - a rollback that itself fails is the
    /// connection's problem on its way out, not this statement's.
    pub(crate) fn run_with_deadline(&self, query: &GeneratedQuery, deadline: Deadline) -> Result<RowSet, PostgresError> {
        let timeout_ms = deadline_statement_timeout_ms(self.statement_timeout_ceiling_ms, deadline);
        let _guard = lock_execution(&self.execution_lock);
        let (columns, rows) = self.runtime.block_on(async {
            self.client
                .batch_execute(&format!("BEGIN; SET LOCAL statement_timeout = {timeout_ms}"))
                .await
                .map_err(|cause| PostgresError::Transaction { cause })?;
            let outcome = Self::prepare_and_query(&self.client, query).await;
            drop(self.client.batch_execute("ROLLBACK").await);
            outcome
        })?;
        Self::rows_from_columns(&columns, rows)
    }

    /// The one round trip both [`Self::run`] and [`Self::run_with_deadline`] make: prepare, bind,
    /// run, and hand back the columns' names and declared types beside the raw rows - reading labels
    /// from the PREPARED statement rather than guessing, so an answer with no rows still carries its
    /// projection. Calls `lib.rs`'s private `Self::bind`, the same cross-module reach `raw.rs` already
    /// relies on for `Self::cell` below.
    async fn prepare_and_query(
        client: &tokio_postgres::Client,
        query: &GeneratedQuery,
    ) -> Result<(Vec<(String, Type)>, Vec<Row>), PostgresError> {
        let bound = Self::bind(query.params());
        let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = bound.iter().map(PgParam::as_ref).collect();
        let statement = client
            .prepare(query.sql())
            .await
            .map_err(|cause| PostgresError::Prepare { cause })?;
        let columns: Vec<(String, Type)> = statement
            .columns()
            .iter()
            .map(|column| (column.name().to_owned(), column.type_().clone()))
            .collect();
        let rows = client.query(&statement, refs.as_slice()).await.map_err(execute_err_mapped)?;
        Ok((columns, rows))
    }

    /// Maps prepared columns and raw rows into a [`RowSet`], one cell at a time.
    fn rows_from_columns(columns: &[(String, Type)], rows: Vec<Row>) -> Result<RowSet, PostgresError> {
        let labels: Vec<String> = columns.iter().map(|(name, _)| name.to_owned()).collect();
        let width = labels.len();
        let mut out: Vec<Vec<Value>> = Vec::with_capacity(rows.len());
        for row in rows {
            let mut cells = Vec::with_capacity(width);
            for (index, (label, column_type)) in columns.iter().enumerate() {
                cells.push(Self::cell(label, column_type, &row, index)?);
            }
            out.push(cells);
        }
        RowSet::new(labels, out).map_err(|cause| PostgresError::Shape { cause })
    }
}

/// The `SET LOCAL statement_timeout` value for one statement under `deadline`: what is left of it,
/// clamped to `ceiling_ms` - `PostgresWarehouse::statement_timeout_ceiling_ms`, the connect-time
/// value, which `docs/adr/0029` keeps as the outer bound a request's own budget may only narrow,
/// never widen. A free function rather than a method so a test can drive it with no connection at
/// all: the clamp is pure arithmetic, and the tier is for what a `57014` actually looks like on the
/// wire, not for this.
///
/// A `ceiling_ms` of zero (the tuning value's own *disabled* spelling) is read as no ceiling at all,
/// rather than as the tightest one - clamping to a zero-to-zero range would panic, and reading zero
/// as unbounded matches Postgres's own meaning for the setting.
///
/// **Never zero**, for `Deadline::remaining_at`'s reason: zero reads as *no timeout at all* to the
/// server. `remaining_at` returning `None` here is not re-checked as a refusal - `sutura_app::answer`
/// and `federated::execute_leg` already ask before this call is ever reached - so the smallest
/// non-zero value is what a statement this close to spent gets, and the server's own `57014` on that
/// statement, through this adapter's own `deadline_exceeded`, is how it is refused.
pub(crate) fn deadline_statement_timeout_ms(ceiling_ms: u32, deadline: Deadline) -> u32 {
    let remaining_ms = deadline
        .remaining_at(std::time::Instant::now())
        .map_or(1, |left| u32::try_from(left.as_millis()).unwrap_or(u32::MAX))
        .max(1);
    if ceiling_ms == 0 {
        remaining_ms
    } else {
        remaining_ms.min(ceiling_ms)
    }
}
