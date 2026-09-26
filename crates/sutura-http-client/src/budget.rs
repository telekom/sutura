//! One shared deadline across a reader's paged requests.

use std::time::{Duration, Instant};

/// How long a socket may stay open past what is left of the shared deadline: connection setup and
/// the last bytes of the answer. A deadline of zero would mean *no timeout* to the client
/// underneath, and this margin is what keeps the socket's own bound from reading that way.
const CONNECT_MARGIN: Duration = Duration::from_secs(5);

/// One shared budget across a reader's `read()` call and its several requests.
///
/// The same shape `sutura_domain::warehouse::deadline::Deadline` holds for a job's execution, and
/// for the same reason: a budget opened per request lets independent timeouts sum to more than a
/// deployment declared - an instant opened once, read as what is left rather than re-derived.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    started: Instant,
    total: Duration,
}

impl Budget {
    #[must_use]
    pub fn opened(total: Duration) -> Self {
        Self {
            started: Instant::now(),
            total,
        }
    }

    /// What is left of the budget, or `None` when it is spent. `None` rather than a zero duration,
    /// for the reason `Deadline::remaining_at` gives: a zero timeout means *no timeout* to the
    /// client underneath.
    #[must_use]
    pub fn remaining(self) -> Option<Duration> {
        self.total.checked_sub(self.started.elapsed()).filter(|left| !left.is_zero())
    }

    #[must_use]
    pub const fn socket(left: Duration) -> Duration {
        left.saturating_add(CONNECT_MARGIN)
    }
}
