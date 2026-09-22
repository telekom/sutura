//! One absolute deadline per answer: how long is left, read against an instant a caller supplies.
//!
//! The domain reads no clock - `crate::identity::Expiry::passed_by` is the precedent, and the same
//! shape applies here: `now` arrives as an argument to [`Deadline::remaining_at`] rather than being
//! read, so the one comparison this module makes lives here and not at whichever call site happens
//! to hold a clock. `docs/adr/0029` is the record; this module is its first slice. It is
//! carried by the port and enforced at three data systems, each by that system's own mechanism:
//! Postgres with `SET LOCAL statement_timeout`, `ClickHouse` with `max_execution_time`, and
//! `BigQuery` over ADBC with the driver's `bigquery.query.job_timeout` statement option, which is
//! the service's own `jobTimeoutMs`. The in-process engine stops at a cooperative yield rather than
//! at the instant. **The limit, beside the claim:** an adapter's boot path has no caller to read a
//! deadline from and is bounded by none of those.

use std::time::{Duration, Instant};

/// The instant one answer's execution has to be finished by.
///
/// Opened ONCE per request, by the transport, at the instant the request arrived - before
/// admission, so the wait for a concurrency slot sits inside the caller's bound rather than adds to
/// it. Shared by the pre-flight, every leg of a federated answer and the re-check between them:
/// [`Self::remaining_at`] is a comparison against an instant every reader shares, so *what is left*
/// is a read and never a division.
///
/// Two `Copy` words - an `Instant` and a [`Budget`] - so it crosses into a
/// `sutura_runtime::spawn_carrying_span` closure by value and carries no secret.
///
/// `PartialEq`/`Eq` so a test can assert that two legs of one federated answer were handed the SAME
/// value - one instant, never re-derived - without exposing the instant itself as an accessor a
/// caller could otherwise be tempted to compare against its own clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    opened: Instant,
    budget: Budget,
}

impl Deadline {
    /// Opens a deadline at `opened`, good for `budget`.
    #[inline]
    #[must_use]
    pub const fn opened_at(opened: Instant, budget: Budget) -> Self {
        Self { opened, budget }
    }

    /// What is left at `now`, or `None` when it is spent.
    ///
    /// **Never a zero duration.** `Some(Duration::ZERO)` reads as *no timeout at all* to
    /// `tokio::time::timeout` and to every client underneath it, so handing one on would turn a
    /// spent budget into an unbounded wait rather than a stopped one - the same rule
    /// `sutura_exec_bigquery`'s `CallDeadline::remaining` already follows for its own budget.
    #[must_use]
    pub fn remaining_at(self, now: Instant) -> Option<Duration> {
        let elapsed = now.saturating_duration_since(self.opened);
        let remaining = self.budget.0.checked_sub(elapsed)?;
        if remaining.is_zero() { None } else { Some(remaining) }
    }

    /// The budget this deadline opened with - what a refusal that ran out of time names, and never
    /// how long is left (that is [`Self::remaining_at`], and it needs a `now` this type does not
    /// have).
    #[inline]
    #[must_use]
    pub const fn budget(self) -> Budget {
        self.budget
    }
}

/// How long one answer may execute.
///
/// Parses: non-zero. A zero budget would read as *no timeout* the moment it reached a client
/// underneath - the same rule [`Deadline::remaining_at`] applies to what is left mid-flight,
/// applied here to what a caller may configure to begin with.
///
/// The only production constructor is `sutura_config::RequestTimeout::budget`, which subtracts a
/// fixed reply margin from the configured request timeout once. [`Self::parse`] stays `pub`
/// because a test - and a third transport - needs to build one directly; what holds production is
/// that one call site and review, and `docs/adr/0029` states the limit next to the claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget(Duration);

impl Budget {
    /// Parses a budget, refusing zero.
    pub const fn parse(budget: Duration) -> Result<Self, NoBudget> {
        if budget.is_zero() {
            return Err(NoBudget);
        }
        Ok(Self(budget))
    }

    /// The budget, in whole seconds - what `RefusalReason::DeadlineExceeded` carries, because it is
    /// a configured number and not how long the question would have taken, which nobody knows.
    #[inline]
    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.0.as_secs()
    }

    /// The duration, for the one call site (`Deadline::remaining_at`) that does arithmetic on it.
    #[inline]
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.0
    }
}

/// Why a duration is not a budget: it is zero.
///
/// Declared last in this file: `xtask check-boundaries`'s pub-field scan misreads a unit struct
/// followed by an `impl` block as the struct's own fields, a gate defect noted as a PR follow-up
/// rather than worked around with more prose here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a budget of zero reads as `no timeout` to every client underneath it, and is the opposite")]
pub struct NoBudget;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{Budget, Deadline, NoBudget};

    #[test]
    fn a_zero_budget_is_refused() {
        assert_eq!(Budget::parse(Duration::ZERO).unwrap_err(), NoBudget);
    }

    #[test]
    fn what_is_left_shrinks_toward_none_and_never_touches_zero() {
        let budget = Budget::parse(Duration::from_secs(2)).expect("two seconds is a budget");
        let opened = Instant::now();
        let deadline = Deadline::opened_at(opened, budget);

        assert_eq!(deadline.remaining_at(opened), Some(Duration::from_secs(2)));
        assert_eq!(
            deadline.remaining_at(opened + Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        // At exactly the budget, nothing is left - and `None`, never `Some(Duration::ZERO)`: a zero
        // duration reads as *no timeout* to a client underneath, which is the opposite of spent.
        assert_eq!(deadline.remaining_at(opened + Duration::from_secs(2)), None);
        // Well past the budget is the same answer, not a duration going negative.
        assert_eq!(deadline.remaining_at(opened + Duration::from_secs(60)), None);
    }

    #[test]
    fn the_budget_travels_with_the_deadline_for_a_refusal_to_name() {
        let budget = Budget::parse(Duration::from_secs(29)).expect("twenty-nine seconds is a budget");
        let deadline = Deadline::opened_at(Instant::now(), budget);
        assert_eq!(deadline.budget().seconds(), 29);
    }
}
