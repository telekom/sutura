use std::time::Duration;

/// How long a plan send, a plan build or a wait for a plan takes before it is logged.
const NOTED_AFTER: Duration = Duration::from_millis(250);

/// `elapsed` in milliseconds when it is long enough to log, so a healthy question logs nothing.
pub(crate) fn noted_ms(elapsed: Duration) -> Option<u64> {
    (elapsed >= NOTED_AFTER).then(|| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}
