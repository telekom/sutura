//! `docs/adr/0029`'s per-request budget, this transport's own mechanism: the pinned driver's
//! `bigquery.query.job_timeout` statement option.
//!
//! # What it bounds, and what it does not
//!
//! The option maps to `queryConfig.JobTimeout` on the Go side, which is `BigQuery`'s own
//! `jobTimeoutMs` **job configuration** field - so the SERVICE stops the job when it runs past it,
//! whether or not this process is still waiting. That is the half `telekom/sutura#929`'s eighth
//! round found missing: the caller's wait was bounded and the job behind it was not.
//!
//! **It is not the query API's `timeoutMs`, and the difference is the whole point of sending this
//! one.** `timeoutMs` bounds how long the *service waits before replying*, leaving the job running;
//! `jobTimeoutMs` bounds the *job*. The pinned driver's option table carries no key for the former
//! at all, so there is nothing here to choose between - `bigquery.query.job_timeout` is the only
//! time bound the driver accepts.
//!
//! **Its limits, beside the claim.** The service ends the job at that bound; work already done is
//! already billed, and this bound does not refund it - `super::MAX_BYTES_BILLED_OPTION` is the
//! separate bound in the separate unit, and neither substitutes for the other. And it is the
//! service's clock, not this process's: the two are not synchronised, so a job may be ended
//! slightly after this deployment has already stopped waiting.
//!
//! **What is NOT bounded: the boot path.** [`JobDeadline::Boot`] carries no deadline to derive a
//! bound from - `verify_anchor`, the identity read and a fixture load have no caller - and this
//! crate depends on no settings crate, so there is no configured number here to use instead. Those
//! calls are submitted with no time bound, exactly as before.
//!
//! # A cancellation path, and why this is not one
//!
//! The pinned driver implements one (`go/statement_cancel.go`'s `Cancel`, reaching
//! `jobs.cancel`), and `adbc_core::Statement::cancel` exposes it. It is not used here and the
//! reason is a shape, not an oversight: `JobTransport::run` is synchronous and blocks inside
//! `ManagedStatement::execute`, so cancelling it needs a second thread holding the same `&mut`
//! statement. Nothing in `adbc_driver_manager` makes that sound. A job timeout needs no second
//! thread and bounds the job at the service either way, which is why it is what got built.

use std::time::{Duration, Instant};

use crate::transport::JobDeadline;

use super::AdbcError;

/// The pinned driver's own name for `BigQuery`'s `jobTimeoutMs` job configuration.
///
/// **Read off the driver's option table rather than guessed** - `go/driver.go`'s
/// `OptionQueryJobTimeout`, which `go/statement.go`'s `SetOptionInt` assigns to
/// `queryConfig.JobTimeout` as `time.Duration(value) * time.Millisecond`. The flake pins that
/// source (`bigquery-adbc-src`, tag `go/v1.13.0`), so the string, the UNIT and the version they
/// are true of move together.
///
/// **An INTEGER option, which decides how it is sent** - `SetOptionString`'s own match does not
/// carry this key, so a string here comes back `NotImplemented`. See
/// [`MAX_BYTES_BILLED_OPTION`](super::MAX_BYTES_BILLED_OPTION), which is sent the same way for the
/// same reason.
pub(super) const JOB_TIMEOUT_OPTION: &str = "bigquery.query.job_timeout";

/// How long the service may let one job run, in whole milliseconds.
///
/// Parses: strictly positive, and the type is what holds that. `queryConfig.JobTimeout = 0` is
/// *unset* to the Go client, so a zero here would reach the service as NO bound - the same
/// "zero reads as unbounded" trap `sutura_domain::warehouse::deadline::Deadline::remaining_at`
/// refuses at the source and `sutura_exec_clickhouse::deadline` documents for its own setting. The
/// only constructor is [`job_timeout`], and it cannot produce one: `remaining_at` answers `None`
/// rather than a zero duration, which is refused there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct JobTimeout(i64);

impl JobTimeout {
    /// The bound, as the driver's integer option takes it.
    pub(super) const fn as_int(self) -> i64 {
        self.0
    }
}

/// The job timeout for one call: how long is left of the port's deadline, or nothing on the boot
/// path.
///
/// Three outcomes and each is a different thing, which is why this is not an `Option` alone:
///
/// - [`JobDeadline::Port`] with time left - `Some`, and [`super::prepared`] sends it.
/// - [`JobDeadline::Port`] already spent - [`AdbcError::DeadlineSpent`], refused by
///   [`super::AdbcBigQuery::connect`] before the driver is loaded. **Refused rather than submitted
///   with no bound**, which is the outcome this function exists to make impossible: a spent
///   deadline has nothing to derive a positive bound from, and the alternative to refusing is a job
///   the service is asked to run without a time bound on behalf of a caller who is no longer owed
///   an answer.
/// - [`JobDeadline::Boot`] - `None`, and the module header states what that leaves unbounded.
///
/// Ceiling-rounded rather than truncated, for `sutura_exec_clickhouse::deadline`'s reason: rounding
/// down would make the SERVICE's bound tighter than the [`Deadline`](sutura_domain::warehouse::deadline::Deadline)
/// itself grants, and the difference is where a question that would have answered gets stopped
/// instead.
pub(super) fn job_timeout(deadline: JobDeadline, now: Instant) -> Result<Option<JobTimeout>, AdbcError> {
    let JobDeadline::Port(deadline) = deadline else {
        return Ok(None);
    };
    let remaining = deadline.remaining_at(now).ok_or(AdbcError::DeadlineSpent)?;
    let whole_millis = remaining.as_millis();
    // Rounded UP if truncating to whole milliseconds lost anything, compared as DURATIONS rather
    // than with a remainder: `clippy::integer_division_remainder_used` is denied across this
    // workspace. A width that does not fit a `u64` compares equal and rounds down, which the clamp
    // below then bounds anyway.
    let truncated = u64::try_from(whole_millis).map_or(remaining, Duration::from_millis);
    let ceiling = if truncated < remaining {
        whole_millis + 1
    } else {
        whole_millis
    };
    // `remaining` is strictly positive - `remaining_at` answers `None` rather than a zero duration -
    // so the ceiling is at least one and `JobTimeout`'s parse rule holds by construction rather than
    // by a `max(1)` floor no path could reach.
    //
    // The clamp is the one lossy step and it is stated rather than hidden: a budget past
    // `i64::MAX` milliseconds - some 292 million years - is not representable as this option, and
    // `sutura_config::RequestTimeout` cannot configure one. Clamping DOWN keeps it a bound; a
    // wrapping cast would not.
    Ok(Some(JobTimeout(i64::try_from(ceiling).unwrap_or(i64::MAX))))
}

#[cfg(test)]
mod tests {
    use sutura_domain::warehouse::deadline::{Budget, Deadline};

    use super::{AdbcError, Duration, Instant, JobDeadline, job_timeout};

    fn opened(budget: Duration) -> (Instant, JobDeadline) {
        let at = Instant::now();
        let budget = Budget::parse(budget).expect("a non-zero budget is a budget");
        (at, JobDeadline::Port(Deadline::opened_at(at, budget)))
    }

    #[test]
    fn what_is_left_of_the_port_deadline_is_what_the_job_is_bounded_by() {
        let (at, deadline) = opened(Duration::from_secs(29));
        let bound = job_timeout(deadline, at)
            .expect("a deadline with time left is not a refusal")
            .expect("a port deadline yields a bound");
        // Whole milliseconds, which is the unit the driver's own `SetOptionInt` multiplies by.
        assert_eq!(bound.as_int(), 29_000);
        // And it SHRINKS as the request ages, so a cell over a constant cannot pass: the bound the
        // service gets is what is left at submit, not the budget the request opened with.
        let later = job_timeout(deadline, at + Duration::from_secs(9))
            .expect("twenty seconds are still left")
            .expect("a port deadline yields a bound");
        assert_eq!(later.as_int(), 20_000);
    }

    #[test]
    fn a_fractional_remainder_rounds_up_and_never_down_to_zero() {
        let (at, deadline) = opened(Duration::from_micros(1_500));
        let bound = job_timeout(deadline, at)
            .expect("a deadline with time left is not a refusal")
            .expect("a port deadline yields a bound");
        assert_eq!(bound.as_int(), 2);
        // The narrowest case there is, and the one a truncating cast would send as `0` - which the
        // Go client reads as *unset*, and so as no bound at all.
        let (at, sliver) = opened(Duration::from_nanos(1));
        let bound = job_timeout(sliver, at)
            .expect("a nanosecond is time left")
            .expect("a port deadline yields a bound");
        assert_eq!(bound.as_int(), 1);
    }

    #[test]
    fn a_spent_port_deadline_is_refused_rather_than_submitted_unbounded() {
        let (at, deadline) = opened(Duration::from_millis(1));
        let refused = job_timeout(deadline, at + Duration::from_secs(1))
            .expect_err("a spent deadline cannot bound a job and must not be sent without one");
        assert!(matches!(refused, AdbcError::DeadlineSpent), "{refused:?}");
    }

    #[test]
    fn the_boot_path_has_no_deadline_to_send_and_is_not_a_refusal() {
        // The third outcome, and it is the one the module header states the limit for: a boot-path
        // call carries no caller and no budget, so there is no positive bound to derive. `None` is
        // not `Err` - refusing here would stop `verify_anchor` and the identity read at boot.
        assert_eq!(
            job_timeout(JobDeadline::Boot, Instant::now()).expect("the boot path is not a refusal"),
            None
        );
    }
}
