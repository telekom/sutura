//! `docs/adr/0029`'s per-statement `SET LOCAL statement_timeout` mechanism - split out of `lib.rs`
//! because that file hit the thousand-line limit `cargo xtask max-lines` enforces, the same reason
//! `raw.rs` exists beside it.
//!
//! [`PostgresWarehouse::run`] is the boot path's own statement runner - no deadline, the
//! connect-time ceiling alone - and [`PostgresWarehouse::run_with_deadline`] is what
//! `Warehouse::execute` calls; both end at [`PostgresWarehouse::prepare_and_query`], reading `lib.rs`'s
//! private `bind` and `cell` the same way `raw.rs` already does. [`deadline_exceeded`] is what
//! `Warehouse::deadline_exceeded` delegates to, the same shape `raw::source_refused` has.

use core::num::NonZeroU32;
use std::time::Instant;

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
    /// **The lock is acquired FIRST, then the deadline is re-checked** (via [`refuse_if_spent`]):
    /// waiting for `execution_lock` is itself outside the deadline - unbounded, and invisible to
    /// `sutura_app`'s own pre-call check, which runs before this wait starts
    /// (`telekom/sutura#687`'s review, finding 2).
    ///
    /// **`ROLLBACK` always runs, even when `BEGIN`/`SET LOCAL` itself failed** - a refused `SET
    /// LOCAL` (an out-of-range value, say) still opens the transaction it rode beside, leaving the
    /// connection ABORTED for every later caller otherwise (finding NIT 6). `SET LOCAL` needs a
    /// transaction block to take effect at all, and there is nothing here to commit - a certified
    /// plan only ever `SELECT`s - so discarding it either way is exact. **That failure arm is
    /// unexercised by any cell**: a refused `SET LOCAL` needs a value over `i32::MAX`, reachable
    /// only with a disabled ceiling (`SUTURA_DEV_STATEMENT_TIMEOUT_MS=0`) and a `Budget` over
    /// ~24.8 days - unreachable from production configuration (`RequestTimeout::MAX_SECONDS`
    /// bounds it to 300 s), so this is held by reading (`telekom/sutura#687`'s round-2 review,
    /// finding 2).
    pub(crate) fn run_with_deadline(&self, query: &GeneratedQuery, deadline: Deadline) -> Result<RowSet, PostgresError> {
        let _guard = lock_execution(&self.execution_lock);
        refuse_if_spent(deadline)?;
        let timeout_ms = deadline_statement_timeout_ms(self.statement_timeout_ceiling_ms, deadline);
        let (columns, rows) = self.runtime.block_on(async {
            let began = begin_with_timeout(&self.client, timeout_ms).await;
            let outcome = match began {
                Ok(()) => Self::prepare_and_query(&self.client, query).await,
                Err(cause) => Err(cause),
            };
            drop(self.client.batch_execute("ROLLBACK").await);
            outcome
        })?;
        Self::rows_from_columns(&columns, rows)
    }

    /// `Warehouse::dry_run`'s own transaction: the same lock-then-check, `BEGIN`/`SET LOCAL`,
    /// always-`ROLLBACK` shape as [`Self::run_with_deadline`], but only `PREPARE`s and never runs
    /// the statement - that method's own contract.
    pub(crate) fn prepare_with_deadline(&self, query: &GeneratedQuery, deadline: Deadline) -> Result<(), PostgresError> {
        let _guard = lock_execution(&self.execution_lock);
        refuse_if_spent(deadline)?;
        let timeout_ms = deadline_statement_timeout_ms(self.statement_timeout_ceiling_ms, deadline);
        self.runtime.block_on(async {
            let began = begin_with_timeout(&self.client, timeout_ms).await;
            let outcome = match began {
                Ok(()) => self
                    .client
                    .prepare(query.sql())
                    .await
                    .map(drop)
                    .map_err(|cause| PostgresError::Prepare { cause }),
                Err(cause) => Err(cause),
            };
            drop(self.client.batch_execute("ROLLBACK").await);
            outcome
        })
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

/// The `BEGIN`/`SET LOCAL statement_timeout` pair [`PostgresWarehouse::run_with_deadline`] and
/// [`PostgresWarehouse::prepare_with_deadline`] both open their transaction with, in one round
/// trip over the simple query protocol.
async fn begin_with_timeout(client: &tokio_postgres::Client, timeout_ms: NonZeroU32) -> Result<(), PostgresError> {
    client
        .batch_execute(&format!("BEGIN; SET LOCAL statement_timeout = {timeout_ms}"))
        .await
        .map_err(|cause| PostgresError::Transaction { cause })
}

/// Refuses locally, no round trip, if `deadline` is already spent - the check
/// [`PostgresWarehouse::run_with_deadline`]'s and `dry_run`'s own doc explain: `sutura_app`'s own
/// pre-call check runs before the wait for `execution_lock`, so it cannot see a budget spent
/// DURING that wait.
pub(crate) fn refuse_if_spent(deadline: Deadline) -> Result<(), PostgresError> {
    if deadline.remaining_at(Instant::now()).is_none() {
        return Err(PostgresError::DeadlineSpent);
    }
    Ok(())
}

/// Postgres's `statement_timeout` GUC is a signed `int`, so `i32::MAX` milliseconds (~24.8 days)
/// is the largest value the server accepts as a `SET LOCAL` - `4294967295` (`u32::MAX`) is refused
/// as "value exceeds integer range" and leaves the transaction ABORTED
/// (`telekom/sutura#687`'s review, probe E).
const POSTGRES_TIMEOUT_MAX_MS: u32 = 0x7FFF_FFFF; // i32::MAX, 2_147_483_647

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
/// **`NonZeroU32`, so the compiler holds "never zero" the way `Budget::parse` holds zero out of a
/// budget** (`telekom/sutura#687`'s review, finding 1) - a `u32` result with a `.max(1)` floor held
/// the same claim by one expression only, and a mutation deleting it went unnoticed because no cell
/// drove the branch where a LIVE remaining under 1 ms truncates to `0` in `as_millis` (the only
/// existing cell over a spent deadline hits the `None` arm, already defaulted). `0` reads as *no
/// timeout at all* to the server, so this is the one value further from *stopped* than every other.
/// `remaining_at` returning `None` here is not re-checked as a refusal: [`refuse_if_spent`] is
/// checked before this is ever reached.
pub(crate) fn deadline_statement_timeout_ms(ceiling_ms: u32, deadline: Deadline) -> NonZeroU32 {
    let remaining_ms = deadline
        .remaining_at(Instant::now())
        .map_or(0, |left| u32::try_from(left.as_millis()).unwrap_or(POSTGRES_TIMEOUT_MAX_MS));
    let clamped = if ceiling_ms == 0 {
        remaining_ms
    } else {
        remaining_ms.min(ceiling_ms)
    };
    NonZeroU32::new(clamped).unwrap_or(NonZeroU32::MIN)
}

/// `Warehouse::deadline_exceeded` delegates to this, the same shape `raw::source_refused` has for
/// its own two codes: `DeadlineSpent` is [`refuse_if_spent`]'s own local refusal, and `57014
/// query_canceled` (via `Prepare` or `Execute`) is what `SET LOCAL statement_timeout` produces when
/// it fires - `docs/adr/0029`'s limit: the same code is what a manual `pg_cancel_backend` produces,
/// and this predicate cannot tell the two apart.
pub(crate) fn deadline_exceeded(error: &PostgresError) -> bool {
    match *error {
        PostgresError::DeadlineSpent => true,
        PostgresError::Prepare { ref cause } | PostgresError::Execute { ref cause } => {
            cause.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED)
        }
        _ => false,
    }
}
