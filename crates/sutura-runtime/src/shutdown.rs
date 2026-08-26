//! Stopping on purpose, and saying why.
//!
//! Three things have to be true of a shutdown, and each of them is a separate part of this module:
//!
//! * **A signal reaches the server.** An orchestrator sends `SIGTERM` and then, some seconds
//!   later, `SIGKILL`. A process that ignores the first is killed mid-request.
//! * **In-flight work drains.** `axum` waits for every open connection, which is what makes a
//!   rolling deployment not drop answers - and which is also how one wedged connection pins the
//!   process open past the kill deadline. So the drain is *bounded*: see
//!   [`Shutdown::grace_period`].
//! * **The reason is recorded.** A process that vanished and a process that was asked to stop look
//!   identical in a log that says nothing, and only one of them is a bug.
//!
//! The signal is translated into a [`Shutdown`] rather than being awaited directly by the server,
//! and that is what makes this testable: a test triggers the same value a signal would, with no
//! process-wide side effect and nothing to install.

use std::sync::Arc;
use std::time::Duration;

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
        }
    }

    /// How long the drain may take once this has been triggered.
    #[inline]
    pub const fn grace_period(&self) -> Duration {
        self.grace
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

/// Waits for the first operating-system shutdown signal and triggers `shutdown`.
///
/// Spawned as a task by the composition root. It returns once it has triggered, so a caller can
/// join it; it does not loop, because the first reason wins.
pub async fn listen(shutdown: Shutdown) {
    let reason = next_signal().await;
    shutdown.trigger(reason);
}

/// The first of the signals this platform can observe.
#[expect(
    clippy::integer_division_remainder_used,
    reason = "the `select!` macro expands through remainder arithmetic to pick a poll order; nothing here does"
)]
async fn next_signal() -> ShutdownReason {
    tokio::select! {
        () = interrupt() => ShutdownReason::Interrupt,
        () = terminate() => ShutdownReason::Terminate,
    }
}

/// An interactive interrupt. The one signal every supported platform has.
async fn interrupt() {
    if let Err(cause) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %cause, "the interrupt handler could not be installed");
        never().await;
    }
}

/// A termination request, on Unix.
#[cfg(unix)]
async fn terminate() {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut signal) => {
            signal.recv().await;
        }
        Err(cause) => {
            tracing::error!(error = %cause, "the terminate handler could not be installed");
            never().await;
        }
    }
}

/// A termination request, on Windows.
///
/// `ctrl_shutdown` is the closest equivalent: it is what the system sends when it is stopping, and
/// like `SIGTERM` it arrives with a deadline behind it. `ctrl_close` - the console window being
/// closed - is the other half, and both are selected over because a deployment may see either.
#[expect(
    clippy::integer_division_remainder_used,
    reason = "the `select!` macro expands through remainder arithmetic to pick a poll order; nothing here does"
)]
#[cfg(windows)]
async fn terminate() {
    let mut shutdown = match tokio::signal::windows::ctrl_shutdown() {
        Ok(signal) => signal,
        Err(cause) => {
            tracing::error!(error = %cause, "the shutdown handler could not be installed");
            never().await;
            return;
        }
    };
    let mut close = match tokio::signal::windows::ctrl_close() {
        Ok(signal) => signal,
        Err(cause) => {
            tracing::error!(error = %cause, "the close handler could not be installed");
            never().await;
            return;
        }
    };
    tokio::select! {
        _ = shutdown.recv() => {},
        _ = close.recv() => {},
    }
}

/// A termination request, on a platform with neither of the above.
#[cfg(not(any(unix, windows)))]
async fn terminate() {
    never().await;
}

/// A future that never resolves.
///
/// The idiom for a `select!` branch that must never win. Compiling the branch out instead would
/// mean two different `select!` expressions to keep in step, and a handler that failed to install
/// would otherwise resolve immediately and shut the process down on boot.
async fn never() {
    core::future::pending::<()>().await;
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
