//! `docs/adr/0029`'s per-request budget, this adapter's own mechanism: `ClickHouse`'s
//! `max_execution_time` setting.
//!
//! # Is the deadline honourable here? What was checked, and what was not
//!
//! **`max_execution_time` is a TOTAL wall-clock budget the server enforces, not the per-read idle
//! timeout `#127`'s Oracle work found in that driver.** `ClickHouse`'s own documentation
//! (`https://clickhouse.com/docs/en/operations/settings/query-complexity#max-execution-time`)
//! states the check runs against elapsed time since the query started, and a query that has run
//! longer than the setting is stopped and the server answers `Code: 159. DB::Exception: Timeout
//! exceeded` - the same shape as Postgres's own `statement_timeout`, and unlike the idle-read
//! timeout the Oracle record names as the trap to check for. **What this adapter has NOT measured
//! against a real server: the exact granularity of that check for a query with no natural
//! "stage" boundary** (a single unbounded aggregate scan, say) - `ClickHouse`'s own documentation
//! notes the check happens at block boundaries during execution, so a single block that itself
//! runs long could, in principle, overrun the setting by one block's worth of work before the
//! next check point. That is the same shape of limit `docs/adr/0029`'s Postgres row already
//! accepts for `SET LOCAL statement_timeout` (checked between statements of the interpreter, not
//! mid-expression), stated here rather than assumed.
//!
//! `Deadline::remaining_at` returning `None` when expired is read as *stop now*, never as *wait
//! forever*: [`refuse_if_spent`] refuses locally before a request is ever sent, and
//! [`max_execution_time_seconds`] answers `None` (which `crate::transport::Http::run` reads as
//! *do not send the setting at all*) only after [`refuse_if_spent`] has already refused a spent
//! deadline - there is no path where `None` reaches the wire as *no limit*. A `0` is never sent
//! either: `ClickHouse` reads `max_execution_time = 0` as *no limit*, the same "zero reads as
//! unbounded" trap `sutura_exec_postgres::deadline` documents for `statement_timeout`. The
//! mechanism that prevents a `0` from reaching the wire is `remaining_at`'s `None` itself: a
//! spent deadline has `None`, which [`refuse_if_spent`] catches before
//! [`max_execution_time_seconds`] is ever asked, so the remaining budget is always strictly
//! positive when this function runs. The `ceiling.max(1)` floor in
//! [`max_execution_time_seconds`] is therefore a dead defensive bound - kept because a future
//! caller that bypassed [`refuse_if_spent`] should still never send a `0`, but unreachable on
//! every path that goes through the transport.

use std::time::Instant;

use sutura_domain::warehouse::deadline::Deadline;

/// The deadline was already spent before a request was ever sent - `crate::transport::HttpError`'s
/// own `From` impl is what turns this into that crate's error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the deadline was already spent before a request could be sent")]
pub(crate) struct DeadlineSpent;

/// Refuses locally, no round trip, if `deadline` is already spent.
pub(crate) fn refuse_if_spent(deadline: Deadline) -> Result<(), DeadlineSpent> {
    if deadline.remaining_at(Instant::now()).is_none() {
        return Err(DeadlineSpent);
    }
    Ok(())
}

/// The `max_execution_time` value for one statement under `deadline`, in whole seconds - or `None`
/// when nothing is left, which [`refuse_if_spent`] has already refused before this is ever asked.
///
/// Ceiling-rounded rather than truncated: a caller with 1.2s left gets `max_execution_time=2`
/// rather than `1`, because rounding down would make the SERVER'S bound tighter than what the
/// [`Deadline`] itself grants, and the difference between the two is where a request that would
/// have answered gets `Code: 159` instead.
pub(crate) fn max_execution_time_seconds(deadline: Deadline, now: Instant) -> Option<u64> {
    let remaining = deadline.remaining_at(now)?;
    let whole_seconds = remaining.as_secs();
    let ceiling = if remaining.subsec_nanos() > 0 {
        whole_seconds + 1
    } else {
        whole_seconds
    };
    Some(ceiling.max(1))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sutura_domain::warehouse::deadline::Budget;

    use super::*;

    #[test]
    fn a_spent_deadline_has_nothing_left_to_send() {
        let budget = Budget::parse(Duration::from_millis(1)).expect("a non-zero budget");
        let opened = Instant::now();
        let deadline = Deadline::opened_at(opened, budget);
        assert_eq!(max_execution_time_seconds(deadline, opened + Duration::from_secs(1)), None);
    }

    #[test]
    fn a_fractional_remainder_rounds_up_never_down() {
        let budget = Budget::parse(Duration::from_millis(1_200)).expect("a non-zero budget");
        let opened = Instant::now();
        let deadline = Deadline::opened_at(opened, budget);
        assert_eq!(max_execution_time_seconds(deadline, opened), Some(2));
    }

    #[test]
    fn a_whole_second_remainder_is_not_rounded_up() {
        let budget = Budget::parse(Duration::from_secs(5)).expect("a non-zero budget");
        let opened = Instant::now();
        let deadline = Deadline::opened_at(opened, budget);
        assert_eq!(max_execution_time_seconds(deadline, opened), Some(5));
    }

    #[test]
    fn a_spent_deadline_is_refused_before_any_request_is_sent() {
        let budget = Budget::parse(Duration::from_millis(1)).expect("a non-zero budget");
        let opened = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("one second ago is representable");
        let deadline = Deadline::opened_at(opened, budget);
        // `remaining_at` returns `None` when expired, and forwarding `None` to an HTTP client
        // means *wait forever* - so `refuse_if_spent` is the whole guard. This cell holds the
        // refusal itself, not the `max_execution_time_seconds` predicate the sibling test above
        // asserts on.
        let error = refuse_if_spent(deadline).expect_err("a spent deadline is refused before any round trip");
        assert_eq!(error, DeadlineSpent);
    }
}
