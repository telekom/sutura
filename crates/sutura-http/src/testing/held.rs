//! The switch that holds a [`FakeWarehouse`](super::FakeWarehouse)'s statements, and counts how
//! many are held.
//!
//! Split out of `testing.rs` for that file's own `max-lines` reason.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// The longest a held statement is held, whatever the test does.
pub(super) const HELD_AT_MOST: Duration = Duration::from_secs(20);

/// How long [`Held::until_holding`] waits for a statement to arrive before it fails the test.
const ARRIVAL_WITHIN: Duration = Duration::from_secs(10);

/// What the test and the adapter share: whether statements are held, and how many are inside now.
#[derive(Default)]
pub(super) struct Switch {
    held: AtomicBool,
    holding: AtomicUsize,
    /// The `sutura_correlation` label of the first job the warehouse was asked to run.
    labelled: OnceLock<String>,
}

impl Switch {
    /// Blocks while armed, the way a statement that has not returned yet does.
    ///
    /// Held rather than slept, and that is about the test suite rather than about realism. A
    /// `spawn_blocking` task that sleeps keeps running after the assertion, and dropping a `tokio`
    /// runtime waits for the blocking pool - so a fixed sleep long enough to outrun the request
    /// bound would be added to the wall time of the whole suite. A flag the test clears costs
    /// nothing once the assertion is made.
    ///
    /// The cap is a bug guard, not a timeout: it is what a forgotten `release` costs instead of
    /// hanging the suite.
    pub(super) fn hold(&self) {
        self.holding.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + HELD_AT_MOST;
        while self.held.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.holding.fetch_sub(1, Ordering::SeqCst);
    }

    /// Keeps the first job's label; a later one is not recorded.
    pub(super) fn label(&self, correlation: &str) {
        drop(self.labelled.set(String::from(correlation)));
    }
}

/// Whether the fake is currently refusing to return.
///
/// A separate handle because the warehouse itself is moved into the service at `start`, and what a
/// test needs to hold is the switch rather than the adapter. It is armed only AFTER `start`, because
/// `start` re-executes every anchor and a held statement there would hold startup.
pub(crate) struct Held(pub(super) Arc<Switch>);

impl Held {
    /// From here on, a statement does not come back.
    pub(crate) fn arm(&self) {
        self.0.held.store(true, Ordering::SeqCst);
    }

    /// The `sutura_correlation` label of the first job run, if one ran.
    pub(crate) fn labelled(&self) -> Option<&str> {
        self.0.labelled.get().map(String::as_str)
    }

    /// Let whatever is waiting finish, so the blocking pool drains with the test.
    pub(crate) fn release(&self) {
        self.0.held.store(false, Ordering::SeqCst);
    }

    /// Returns once `n` statements are inside the warehouse; fails the test if they never arrive.
    pub(crate) async fn until_holding(&self, n: usize) {
        let gave_up = Instant::now() + ARRIVAL_WITHIN;
        while self.0.holding.load(Ordering::SeqCst) < n {
            assert!(
                Instant::now() < gave_up,
                "fewer than {n} statements reached the warehouse within {ARRIVAL_WITHIN:?}"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
