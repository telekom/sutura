//! One call's budget: what is LEFT of it, what refusal names it, and the request body built from it.
//!
use super::{CallBody, DryRun, WireError, Wired};
use crate::transport::{JobDeadline, JobRequest};
use crate::wire::bounds::{CallDeadline, JobBounds};
use crate::wire::document::body;

/// What one call may spend, read at `now`: the port's own `Deadline` where the request carries one,
/// or this transport's configured [`JobBounds`] at the boot path. `None` only for the first case,
/// when the port's own budget is already spent - the boot path always opens a fresh window, so it
/// has nothing to be spent yet.
///
/// **A free function of neither `self` nor the credential source, taking `now` as a parameter rather
/// than reading a clock**, so a test can ask what the `Boot` arm answers far in the future without
/// sleeping through a real one - `wire::tests::deadline` proves the boot window stays the configured
/// bound regardless of elapsed time. `now` arrives here for the same reason `Deadline::remaining_at`
/// takes one rather than reading a clock itself.
pub(super) fn remaining_of_the_ports_deadline(
    clock: JobDeadline,
    bounds: JobBounds,
    now: std::time::Instant,
) -> Option<core::time::Duration> {
    match clock {
        JobDeadline::Port(deadline) => deadline.remaining_at(now),
        JobDeadline::Boot => Some(bounds.deadline().budget()),
    }
}

/// What a [`WireError::DeadlineSpent`] this call produces names: the port's own configured budget
/// where the request carries a `Deadline`, and this transport's own configured [`JobBounds`] at the
/// boot path, which has no caller's budget to name.
pub(super) const fn configured_budget_seconds(clock: JobDeadline, bounds: JobBounds) -> u64 {
    match clock {
        JobDeadline::Port(deadline) => deadline.budget().seconds(),
        JobDeadline::Boot => bounds.deadline().budget().as_secs(),
    }
}

/// The request body for one call, built from what is LEFT of `call`'s own budget - never the whole
/// of it. See [`CallBody`] for why the remainder travels back with it.
///
/// **A pure function, split out of `submit` so a test can pin `call` at a chosen instant and read the
/// two timeout fields directly, with no socket.** `call` is opened already; this reads only
/// `call.remaining()`, never `Instant::now()` itself, so a past `CallDeadline::opened_at` is a
/// fixture rather than a race - the recorded `timeoutMs`/`jobTimeoutMs` is PROVABLY the remainder,
/// not the configured ceiling `body` would carry if handed the whole budget instead.
///
/// `budget_seconds`, for the refusal, still asks `request.deadline()` rather than `call` alone: a
/// `CallDeadline` does not say which of the port's own budget or this transport's configured one it
/// was opened from - `configured_budget_seconds` does.
pub(super) fn call_body<'job, C>(
    request: &'job JobRequest<'job>,
    dry_run: DryRun,
    bounds: JobBounds,
    call: CallDeadline,
) -> Wired<CallBody<'job>, C>
where
    C: core::error::Error + 'static,
{
    let left = call.remaining().ok_or(WireError::DeadlineSpent {
        budget_seconds: configured_budget_seconds(request.deadline(), bounds),
    })?;
    Ok((body(request, dry_run, bounds, left), left))
}
