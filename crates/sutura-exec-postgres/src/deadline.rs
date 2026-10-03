//! `docs/adr/0029`'s per-statement `SET LOCAL statement_timeout`: the local refusal of a spent
//! deadline and the value the server is told. `adbc::session` sends it; this is the arithmetic.

use core::num::NonZeroU32;
use std::time::Instant;

use sutura_domain::warehouse::deadline::Deadline;

use crate::PostgresError;

/// Refuses locally, no round trip, if `deadline` is already spent - `sutura_app`'s own pre-call
/// check runs before the driver loads and connects, so it cannot see a budget spent DURING that.
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
/// clamped to `ceiling_ms` - `AdbcPostgres`'s `statement_timeout_ceiling_ms`, the deployment's
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
