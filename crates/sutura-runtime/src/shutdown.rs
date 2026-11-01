//! Stopping on purpose, and saying why.
//!
//! Three things have to be true of a shutdown, and each of them is a separate part of this module:
//!
//! * **A signal reaches the server.** An orchestrator sends `SIGTERM` and then, some seconds
//!   later, `SIGKILL`. A process that ignores the first is killed mid-request. [`listen`] registers
//!   the handlers before it returns, so a caller that calls it before opening its listener answers
//!   no request while a `SIGTERM` would still end the process undrained.
//! * **In-flight work drains.** `axum` waits for every open connection, which is what makes a
//!   rolling deployment not drop answers - and which is also how one wedged connection pins the
//!   process open past the kill deadline. So the drain is *bounded*: see
//!   [`Shutdown::grace_period`], and [`Shutdown::remaining_grace`] for what is left of that budget
//!   once the drain has had its turn. The two together are what make the number a bound on
//!   *stopping* rather than on the connection drain alone: dropping an `axum` serve future ends the
//!   drain, and the runtime then still waits for every blocking task it cannot cancel.
//! * **The reason is recorded.** A process that vanished and a process that was asked to stop look
//!   identical in a log that says nothing, and only one of them is a bug.
//!
//! The signal is translated into a [`Shutdown`] rather than being awaited directly by the server,
//! and that is what makes this testable: a test triggers the same value a signal would, with no
//! process-wide side effect and nothing to install.

use core::pin::Pin;
use core::task::{Context, Poll};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::watch;

/// Why the process is stopping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// An interactive interrupt. A person pressed a key.
    Interrupt,
    /// A termination request. An orchestrator is replacing this process, and a deadline is running.
    Terminate,
    /// Something in this process asked for it - a fatal error on a path that is not the server, or
    /// a test.
    Requested,
}

impl ShutdownReason {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interrupt => "interrupt",
            Self::Terminate => "terminate",
            Self::Requested => "requested",
        }
    }
}

impl core::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A shared "stop now" flag that remembers why.
///
/// Cloneable and cheap, so the server, the signal listener and anything else that has to wind down
/// all hold the same one. Built on a `watch` channel rather than a cancellation token from a
/// utility crate: the channel is in `tokio` already, and carrying the *reason* in the value is
/// what makes the log line at the end say something.
#[derive(Debug, Clone)]
pub struct Shutdown {
    sender: Arc<watch::Sender<Option<ShutdownReason>>>,
    grace: Duration,
    /// When stopping was first asked for, for [`Shutdown::remaining_grace`].
    ///
    /// A `OnceLock` rather than another field in the watched value, and the reason is the same rule
    /// the reason itself follows: the first trigger wins, and a cell that can be written once
    /// cannot have its clock restarted by a second signal. Beside the sender rather than inside it
    /// because nothing waits on it - a waiter already learns that stopping was asked for from the
    /// channel.
    started: Arc<OnceLock<Instant>>,
}

impl Shutdown {
    /// The default bound on the drain, once shutdown has been signalled.
    ///
    /// Fifteen seconds, chosen against the deadline on the other side rather than as a round
    /// number: an orchestrator's usual `SIGTERM`-to-`SIGKILL` window is thirty, so a process that
    /// gives up after fifteen exits on its own terms with room to spare. A drain that overruns is
    /// a connection nothing is going to close, and waiting for it costs the difference between a
    /// clean exit and being killed.
    pub const DEFAULT_GRACE: Duration = Duration::from_secs(15);

    /// A fresh, untriggered shutdown with the default grace period.
    #[must_use]
    pub fn new() -> Self {
        Self::with_grace(Self::DEFAULT_GRACE)
    }

    /// A fresh shutdown with an explicit grace period. What a test uses.
    #[must_use]
    pub fn with_grace(grace: Duration) -> Self {
        let (sender, _receiver) = watch::channel(None);
        Self {
            sender: Arc::new(sender),
            grace,
            started: Arc::new(OnceLock::new()),
        }
    }

    /// The whole budget for stopping, from the moment stopping is asked for.
    #[inline]
    pub const fn grace_period(&self) -> Duration {
        self.grace
    }

    /// What is left of that budget.
    ///
    /// **The connection drain is not the whole of stopping, and this is the difference.** Dropping
    /// an `axum` serve future ends the drain and returns; the runtime then still waits for every
    /// blocking task, because `tokio` documents that a started `spawn_blocking` task cannot be
    /// aborted and that runtime shutdown waits for one. A process that spent its whole grace period
    /// draining connections and then waited a full grace period again for the blocking pool would
    /// take twice the number an operator configured - and that number was chosen against their
    /// orchestrator's kill timer, so twice it is being killed mid-answer.
    ///
    /// The full grace period before stopping has been asked for, because there is nothing to count
    /// from yet: this is a bound on the *rest* of stopping, and stopping has not started.
    ///
    /// Saturating, so an overrun is zero rather than a wrapped duration. Zero is a legitimate
    /// answer and means the budget is spent: whatever waits on it should not wait at all.
    ///
    /// Measured on `std::time::Instant` and not on `tokio`'s clock, deliberately. What this number
    /// is racing is an orchestrator's kill timer, which is wall time, and what consumes it is
    /// `tokio::runtime::Runtime::shutdown_timeout`, which is also wall time. A test-controllable
    /// clock here would make the two disagree in exactly the deployment where it matters.
    #[must_use]
    pub fn remaining_grace(&self) -> Duration {
        self.started
            .get()
            .map_or(self.grace, |began| self.grace.saturating_sub(began.elapsed()))
    }

    /// Asks for shutdown, and logs why.
    ///
    /// **The first reason wins, and a second call is a no-op.** A `SIGINT` arriving while a
    /// `SIGTERM` drain is already running should not restart the clock or rewrite the record of
    /// what started it.
    pub fn trigger(&self, reason: ShutdownReason) {
        let mut already = None;
        self.sender.send_if_modified(|current| {
            already = *current;
            let first = current.is_none();
            if first {
                *current = Some(reason);
                // In the same critical section as the reason, so the instant the grace period is
                // measured from belongs to the trigger that was recorded rather than to a later
                // signal that lost the race. `set` returning `Err` is that same race and is
                // therefore not a failure: the winner already wrote the instant.
                let _lost_the_race = self.started.set(Instant::now());
            }
            first
        });
        already.map_or_else(
            || tracing::info!(reason = %reason, "shutdown requested - draining in-flight work"),
            |existing| {
                tracing::debug!(
                    reason = %reason,
                    already = %existing,
                    "shutdown already in progress; the first reason stands"
                );
            },
        );
    }

    /// The reason, if shutdown has been asked for.
    #[must_use]
    pub fn reason(&self) -> Option<ShutdownReason> {
        *self.sender.borrow()
    }

    /// Resolves when shutdown has been asked for.
    ///
    /// Checks the current value before waiting, which is the bug this shape exists to avoid: a
    /// `watch` receiver created after the send marks that value as already seen, so a naive
    /// `changed().await` on a shutdown that has *already* happened waits for a second one that
    /// never comes.
    pub async fn requested(&self) -> ShutdownReason {
        let mut receiver = self.sender.subscribe();
        loop {
            let current: Option<ShutdownReason> = *receiver.borrow_and_update();
            if let Some(reason) = current {
                return reason;
            }
            if receiver.changed().await.is_err() {
                // Only reachable if every sender is dropped, and this value holds one - so it is
                // not reachable. Treated as a request rather than as a hang, because a future that
                // never resolves is the worse failure of the two.
                return ShutdownReason::Requested;
            }
        }
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

/// Registers a handler for each shutdown signal this platform has, now, and returns the future that
/// triggers `shutdown` on the first of them.
///
/// **A plain `fn` rather than an `async fn`, and that is `github.com/telekom/sutura#1706`.** An
/// `async fn` runs nothing until its future is first polled, and a spawned task is polled whenever
/// the scheduler reaches it - which on a multi-thread runtime can be after the listener already
/// answers. A `SIGTERM` in that window met the default disposition and ended the process with no
/// drain. Registration here is done before this returns, so a caller that calls it before opening
/// its listener has no such window. The limit: nothing makes a caller do that, and a signal that
/// arrives before this is called still meets the default.
///
/// A handler that cannot be installed is logged and its signal never wins, rather than the process
/// refusing to start - the behaviour this had before. A branch that resolved on failure instead
/// would shut the process down on boot.
///
/// # Panics
///
/// On Unix, outside a `tokio` runtime whose I/O driver is enabled, because `tokio::signal::unix::signal`
/// does.
pub fn listen(shutdown: Shutdown) -> Listening {
    Listening {
        shutdown,
        interrupt: installed(platform::interrupt(), "interrupt"),
        terminate: installed(platform::terminate(), "terminate"),
    }
}

/// The shutdown signals, registered, waiting for the first. Only [`listen`] builds one.
///
/// Resolves once it has triggered its [`Shutdown`], so a caller can join it. It does not loop,
/// because the first reason wins. On Unix a handler stays installed for the life of the process, so
/// a signal arriving after this resolved - or after it was dropped unpolled - is caught and acted on
/// by nothing.
#[derive(Debug)]
#[must_use = "a registered signal reaches the shutdown only while this is polled"]
pub struct Listening {
    shutdown: Shutdown,
    /// `None` when the handler could not be installed, which `installed` has logged.
    interrupt: Option<platform::Interrupt>,
    terminate: Option<platform::Terminate>,
}

impl Future for Listening {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let reason = if this.interrupt.as_mut().is_some_and(|signal| signal.poll_recv(cx).is_ready()) {
            ShutdownReason::Interrupt
        } else if this.terminate.as_mut().is_some_and(|signal| signal.poll_recv(cx).is_ready()) {
            ShutdownReason::Terminate
        } else {
            return Poll::Pending;
        };
        this.shutdown.trigger(reason);
        Poll::Ready(())
    }
}

/// The handler, or the logged reason there is none.
fn installed<T>(registered: std::io::Result<T>, signal: &'static str) -> Option<T> {
    registered
        .inspect_err(|cause| {
            tracing::error!(error = %cause, signal, "the handler could not be installed; this signal will not start a drain");
        })
        .ok()
}

/// `SIGINT` and `SIGTERM`, each registered by `tokio::signal::unix::signal`, which installs the
/// handler when it is called. `tokio::signal::ctrl_c` is not used because it is an `async fn`.
#[cfg(unix)]
mod platform {
    use tokio::signal::unix::{Signal, SignalKind, signal};

    pub(super) type Interrupt = Signal;
    pub(super) type Terminate = Signal;

    pub(super) fn interrupt() -> std::io::Result<Interrupt> {
        signal(SignalKind::interrupt())
    }

    pub(super) fn terminate() -> std::io::Result<Terminate> {
        signal(SignalKind::terminate())
    }
}

/// `ctrl_c` for an interrupt, and for a termination request both `ctrl_shutdown` - what the system
/// sends when it is stopping, with a deadline behind it like `SIGTERM` - and `ctrl_close`, the
/// console window being closed, because a deployment may see either. Each constructor registers
/// when it is called.
#[cfg(windows)]
mod platform {
    use core::task::{Context, Poll};

    use tokio::signal::windows::{CtrlClose, CtrlShutdown, ctrl_close, ctrl_shutdown};

    pub(super) type Interrupt = tokio::signal::windows::CtrlC;

    pub(super) fn interrupt() -> std::io::Result<Interrupt> {
        tokio::signal::windows::ctrl_c()
    }

    /// Both or neither: either one failing to install means neither is listened to.
    #[derive(Debug)]
    pub(super) struct Terminate {
        shutdown: CtrlShutdown,
        close: CtrlClose,
    }

    impl Terminate {
        pub(super) fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<()>> {
            match self.shutdown.poll_recv(cx) {
                Poll::Pending => self.close.poll_recv(cx),
                delivered @ Poll::Ready(_) => delivered,
            }
        }
    }

    pub(super) fn terminate() -> std::io::Result<Terminate> {
        Ok(Terminate {
            shutdown: ctrl_shutdown()?,
            close: ctrl_close()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Shutdown, ShutdownReason};

    #[tokio::test]
    async fn a_waiter_created_before_the_trigger_is_woken() {
        let shutdown = Shutdown::new();
        let waiter = shutdown.clone();
        let joined = tokio::spawn(async move { waiter.requested().await });
        shutdown.trigger(ShutdownReason::Terminate);
        assert_eq!(joined.await.expect("the waiter task ran"), ShutdownReason::Terminate);
    }

    #[tokio::test]
    async fn a_waiter_created_after_the_trigger_does_not_hang() {
        // THE BUG THIS TEST EXISTS FOR. A `watch` receiver subscribed after a send treats that
        // value as already seen, so `changed().await` on an already-triggered shutdown waits for a
        // second trigger that never arrives - and the process hangs at exactly the moment it was
        // asked to stop. Checking the current value before waiting is what fixes it.
        let shutdown = Shutdown::new();
        shutdown.trigger(ShutdownReason::Interrupt);
        let reason = tokio::time::timeout(Duration::from_secs(1), shutdown.requested())
            .await
            .expect("an already-triggered shutdown resolves immediately");
        assert_eq!(reason, ShutdownReason::Interrupt);
    }

    #[tokio::test]
    async fn the_first_reason_wins() {
        // A second signal during a drain must not rewrite the record of what started it.
        let shutdown = Shutdown::new();
        shutdown.trigger(ShutdownReason::Terminate);
        shutdown.trigger(ShutdownReason::Interrupt);
        assert_eq!(shutdown.reason(), Some(ShutdownReason::Terminate));
        assert_eq!(shutdown.requested().await, ShutdownReason::Terminate);
    }

    #[test]
    fn an_untriggered_shutdown_has_no_reason() {
        assert_eq!(Shutdown::new().reason(), None);
        assert_eq!(Shutdown::new().grace_period(), Shutdown::DEFAULT_GRACE);
        assert_eq!(
            Shutdown::with_grace(Duration::from_millis(5)).grace_period(),
            Duration::from_millis(5)
        );
    }

    #[tokio::test]
    async fn the_budget_left_for_stopping_starts_full_and_then_only_goes_down() {
        // The bug this exists for: giving the connection drain the whole grace period and then
        // giving the blocking pool the whole grace period AGAIN, which is twice the number the
        // operator chose against their orchestrator's kill timer. The full budget before stopping
        // has been asked for, because there is nothing to count from yet.
        //
        // Real time and a generous budget, with one-sided assertions: `remaining_grace` measures
        // wall time on purpose - see the note on it - so a loaded machine may take longer over the
        // sleep than asked, and an assertion on how MUCH was spent would be measuring the machine.
        let shutdown = Shutdown::with_grace(Duration::from_secs(600));
        assert_eq!(shutdown.remaining_grace(), Duration::from_secs(600));
        shutdown.trigger(ShutdownReason::Terminate);
        tokio::time::sleep(Duration::from_millis(30)).await;
        let left = shutdown.remaining_grace();
        assert!(left < Duration::from_secs(600), "the clock never started: {left:?}");
        assert!(
            left > Duration::from_secs(300),
            "thirty milliseconds spent half the budget: {left:?}"
        );
    }

    #[tokio::test]
    async fn a_budget_that_is_spent_is_zero_rather_than_a_wrapped_duration() {
        // Zero is a legitimate answer and means "do not wait at all". The alternative - an
        // underflow - would be the largest duration there is, which is the opposite of a bound.
        let shutdown = Shutdown::with_grace(Duration::from_millis(10));
        shutdown.trigger(ShutdownReason::Interrupt);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(shutdown.remaining_grace(), Duration::ZERO);
    }

    #[tokio::test]
    async fn a_second_signal_does_not_restart_the_clock() {
        // The same rule the reason follows, and for the same deployment reason: a `SIGINT` arriving
        // during a `SIGTERM` drain must not buy the process another full grace period, because the
        // kill timer on the other side did not restart either. Asserted as monotonicity, which is
        // the property, and which no amount of scheduling noise can make pass by accident.
        let shutdown = Shutdown::with_grace(Duration::from_secs(600));
        shutdown.trigger(ShutdownReason::Terminate);
        tokio::time::sleep(Duration::from_millis(30)).await;
        let before = shutdown.remaining_grace();
        shutdown.trigger(ShutdownReason::Interrupt);
        let after = shutdown.remaining_grace();
        assert!(
            after <= before,
            "the second signal restarted the grace period: {before:?} then {after:?}"
        );
    }

    #[test]
    fn every_reason_renders_as_a_distinct_word() {
        // The reason reaches a log as this string, so two variants sharing one word would make the
        // record ambiguous about which happened.
        let words = [
            ShutdownReason::Interrupt.as_str(),
            ShutdownReason::Terminate.as_str(),
            ShutdownReason::Requested.as_str(),
        ];
        let mut unique = words;
        unique.sort_unstable();
        let count = unique.len();
        let mut deduped = unique.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), count);
    }
}
