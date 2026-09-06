//! How many questions may be executing at once, and how long a caller waits for a turn.
//!
//! # Why this is a bound on the work and not a bound on the reply
//!
//! `server.request_timeout_seconds` is a deadline on the **reply**. When it expires the caller is
//! answered `408` and the handler future is dropped - and a started `tokio::task::spawn_blocking`
//! task cannot be aborted, so the question keeps running. Without something else in the picture a
//! caller asking questions that cost more than the timeout gets a fast turnaround while the
//! deployment keeps the whole cost, and in-flight work accumulates at the rate limit with nothing
//! shedding it. The blocking pool defaults to 512 threads with an unbounded queue, so that backlog
//! is bounded by memory.
//!
//! [`Admission`] is the bound. It is the number of questions that may be *executing*, and the slot
//! is meant to be held by the blocking task rather than by the handler future - so a timed-out
//! request does not hand its slot back until the work it started actually finishes.
//!
//! # What it does not do, stated first because it is the part that gets assumed
//!
//! **It does not cancel anything.** The `Warehouse` port is synchronous and has no cancellation
//! token, so a question already handed to the blocking pool runs to completion whatever the caller
//! is told. A timed-out request therefore still holds its slot until the data system answers it -
//! which is exactly why the slot matters: the backlog becomes a number somebody chose instead of
//! memory.
//!
//! **It is not a per-caller budget.** It bounds the deployment, not a principal. One caller filling
//! every slot sheds every other caller, and nothing here can tell the two apart - that needs an
//! identity, which this service does not have. `sutura_config::RateLimitSettings` is what bounds a
//! single address's rate, and it counts requests rather than execution.
//!
//! **It is not a queue.** Waiting is bounded by the admission timeout and a waiter that runs out of
//! it is shed. A refused waiter costs a dropped future rather than a thread, which is what stops
//! the queue in front of the bound from being a second unbounded thing.
//!
//! # Why it lives here rather than in the transport
//!
//! The resource it bounds is the *process*: a blocking-pool thread holding a data system. A second
//! transport - an MCP surface, say - would need the same bound over the same pool, and two
//! independently sized semaphores would be two controls each reporting a limit that the other can
//! exceed. So the composition root builds one, exactly as it builds one [`crate::Shutdown`].
//!
//! # What holds the "one", and what it does not reach
//!
//! That paragraph used to end there, and `telekom/sutura#340` is what it cost: **nothing held the
//! word *one*.** Two things hold it now, and neither of them is the sentence above.
//!
//! * **A transport cannot build one.** `Admission::new` is `pub(crate)`, so the only public door
//!   is [`Admission::from_settings`] - the group the two keys live in - and both transports'
//!   constructors *take* the value. `sutura_http`'s request state DERIVED its own until #340, which
//!   made a second `ServiceState` a second permit set; that edge is gone, so every permit set in a
//!   process is built by a composition root and handed down.
//! * **A composition root builds at most one, and a serving root builds exactly one.**
//!   `cargo xtask check-one-bound`, in `just hygiene`, counts the construction sites: at most one
//!   per crate, only in a crate that has a `src/main.rs`, and one in every such crate that composes
//!   a transport. Its own header states the ways it fails closed and what a text scan cannot see.
//!
//! **The limit, stated with the claim: the gate counts SITES, and a site is not a permit set.** A
//! root calling the door once inside a loop builds one bound per iteration and reads as one site,
//! and no text scan can see a bound obtained through a function pointer or a re-export. What
//! nothing here reaches at all is two *processes*: this bound is per process by construction, so
//! two replicas admit twice the number - which is what a replica is for.

use std::sync::Arc;
use std::time::Duration;

use sutura_config::{AdmissionTimeout, QueryConcurrency, RuntimeSettings};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The right to execute one question, held for as long as the question runs.
///
/// An alias for `tokio`'s owned permit rather than a newtype around it, because the type's whole
/// contract is its `Drop`: the slot comes back when the value is dropped, and a wrapper would only
/// add a way to get that wrong. Owned rather than borrowed so it can be moved into the blocking
/// task, which is the whole point - see the module documentation.
pub type Slot = OwnedSemaphorePermit;

/// Nothing was free inside the admission timeout.
///
/// Carries both numbers because the answer to it differs by which one is wrong: a bound that is too
/// small for the machine is a configuration change, and a wait that expires under normal load is a
/// deployment that needs another replica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("all {bound} execution slots were taken for the whole {} second admission window", waited.as_secs())]
pub struct AtCapacity {
    bound: usize,
    waited: Duration,
}

impl AtCapacity {
    /// How many slots there are in total.
    #[inline]
    #[must_use]
    pub const fn bound(self) -> usize {
        self.bound
    }

    /// How long the caller waited before being shed.
    #[inline]
    #[must_use]
    pub const fn waited(self) -> Duration {
        self.waited
    }
}

/// The bound on how many questions execute at once, and the bound on waiting for a turn.
///
/// Cloneable and cheap: every clone shares one permit set, which is the property that makes this a
/// bound at all. A per-request or per-connection copy would be a number that reads like a limit and
/// bounds nothing.
#[derive(Debug, Clone)]
pub struct Admission {
    slots: Arc<Semaphore>,
    bound: usize,
    wait: Duration,
}

impl Admission {
    /// Builds the bound from two values that have each already been parsed.
    ///
    /// **`pub(crate)` since `telekom/sutura#340`, and that is one half of the mechanism rather than
    /// tidying.** Two public constructors meant a bound could be assembled from two numbers a
    /// CALLER chose; with only [`Self::from_settings`] left, every `Admission` outside this crate
    /// is the pair of numbers an operator wrote, read from the group they live in. The other half
    /// is that a transport takes one - see the module documentation.
    #[must_use]
    pub(crate) fn new(concurrency: QueryConcurrency, timeout: AdmissionTimeout) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(concurrency.count())),
            bound: concurrency.count(),
            wait: timeout.duration(),
        }
    }

    /// The bound this process's settings describe.
    ///
    /// **The only public door**, so the two values cannot be taken from different places and cannot
    /// be numbers a caller chose. What a composition root calls, and `cargo xtask check-one-bound`
    /// is what holds *once per root* - a transport that called this would be a second permit set,
    /// which is the shape the module documentation calls not-a-bound.
    #[must_use]
    pub fn from_settings(runtime: RuntimeSettings) -> Self {
        Self::new(runtime.max_concurrent_queries(), runtime.admission_timeout())
    }

    /// Waits for a slot, for at most the admission timeout.
    ///
    /// The returned [`Slot`] must be moved into whatever does the work rather than held by the
    /// awaiting future, or the bound becomes a bound on *starting* work instead of on running it -
    /// see the module documentation.
    pub async fn admit(&self) -> Result<Slot, AtCapacity> {
        let waiting = Arc::clone(&self.slots).acquire_owned();
        if let Ok(Ok(slot)) = tokio::time::timeout(self.wait, waiting).await {
            return Ok(slot);
        }
        // Two ways to get here and one answer, which is deliberate. The window expiring is the
        // ordinary one. The other is a closed permit set - unreachable, because nothing here closes
        // it and this value holds it - and it is shed rather than admitted anyway: a closed set
        // means no slot was taken, so calling it an admission would run the work with nothing
        // bounding it. Fail closed.
        Err(AtCapacity {
            bound: self.bound,
            waited: self.wait,
        })
    }

    /// How many questions may execute at once.
    #[inline]
    #[must_use]
    pub const fn bound(&self) -> usize {
        self.bound
    }

    /// How long a question may wait for a slot.
    #[inline]
    #[must_use]
    pub const fn wait(&self) -> Duration {
        self.wait
    }

    /// How many slots are free right now.
    ///
    /// For a log line and for a test. Not for a decision: it is true when it is read and can be
    /// false by the time it is acted on, which is what [`Self::admit`] exists to avoid.
    #[inline]
    #[must_use]
    pub fn free(&self) -> usize {
        self.slots.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sutura_config::{AdmissionTimeout, QueryConcurrency};
    // `tokio`'s instant and not `std`'s: these tests run on a paused clock, which advances only
    // `tokio`'s. A `std::time::Instant` would report microseconds while the window elapsed.
    use tokio::time::Instant;

    use super::Admission;

    fn admission(bound: usize, seconds: u64) -> Admission {
        Admission::new(
            QueryConcurrency::parse(bound).expect("a test bound is a bound"),
            AdmissionTimeout::parse(seconds).expect("a test wait is a wait"),
        )
    }

    #[tokio::test]
    async fn a_slot_is_held_until_it_is_dropped_and_then_comes_back() {
        // The whole mechanism in one assertion. The slot is a value, so what returns it is a drop -
        // which is why it can be moved into the blocking task and why a dropped handler future does
        // not release it.
        let admission = admission(1, 1);
        assert_eq!(admission.free(), 1);
        let slot = admission.admit().await.expect("a free bound admits");
        assert_eq!(admission.free(), 0);
        drop(slot);
        assert_eq!(admission.free(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_question_that_cannot_get_a_slot_inside_the_window_is_shed_rather_than_queued() {
        // The bug this type exists for: without a bound the second question runs anyway, and
        // without the timeout the waiter is a queue nothing empties. `start_paused` is what makes
        // the assertion about the WINDOW rather than about a sleep - the clock advances when
        // nothing is ready, so this runs in microseconds and still proves the full wait elapsed.
        let admission = admission(1, 5);
        let held = admission.admit().await.expect("the first question is admitted");
        let began = Instant::now();
        let shed = admission.admit().await.expect_err("the second question has no slot");
        assert_eq!(shed.bound(), 1);
        assert_eq!(shed.waited(), Duration::from_secs(5));
        assert!(
            began.elapsed() >= Duration::from_secs(5),
            "the waiter was shed before its window elapsed: {:?}",
            began.elapsed()
        );
        drop(held);
    }

    #[tokio::test(start_paused = true)]
    async fn a_waiter_is_admitted_the_moment_a_slot_comes_back_rather_than_waiting_out_the_window() {
        // The other side of the same window: the timeout is a ceiling on waiting, not a delay. A
        // slot released early has to admit the waiter immediately, or a bound of one would serialise
        // questions at one per admission timeout.
        let admission = admission(1, 60);
        let held = admission.admit().await.expect("the first question is admitted");
        let waiter = {
            let admission = admission.clone();
            tokio::spawn(async move { admission.admit().await.map(drop) })
        };
        tokio::task::yield_now().await;
        drop(held);
        let admitted = tokio::time::timeout(Duration::from_secs(30), waiter)
            .await
            .expect("a released slot admits the waiter inside the window")
            .expect("the waiting task ran");
        assert!(admitted.is_ok(), "a released slot did not admit the waiter");
    }

    #[tokio::test(start_paused = true)]
    async fn the_bound_is_the_number_of_slots_and_the_next_one_after_it_is_shed() {
        // A bound above one, so the assertion is about the NUMBER rather than about the existence
        // of a permit set. Four in, the fifth out.
        let admission = admission(4, 1);
        let mut held = Vec::new();
        for _ in 0_u8..4 {
            held.push(admission.admit().await.expect("a free slot admits"));
        }
        assert_eq!(admission.free(), 0);
        let shed = admission.admit().await.expect_err("the fifth question has no slot");
        assert_eq!(shed.bound(), 4);
        drop(held);
        assert_eq!(admission.free(), 4);
    }

    #[test]
    fn the_two_numbers_are_read_back_exactly_as_configured() {
        // What the startup log prints and what the failure body's `Retry-After` is computed from,
        // so a value that arrived here transformed would be a number an operator cannot reconcile
        // with their own configuration.
        let admission = admission(8, 5);
        assert_eq!(admission.bound(), 8);
        assert_eq!(admission.wait(), Duration::from_secs(5));
    }
}
