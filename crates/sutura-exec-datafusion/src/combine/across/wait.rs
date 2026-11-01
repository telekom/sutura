//! How many other nodes a question that goes across wants, and the bounded wait for them.

use core::num::NonZeroU64;
use core::time::Duration;
use std::time::Instant;

use url::Url;

use crate::combine::CombineError;

/// How often a waiting question resolves the other nodes again.
const POLL: Duration = Duration::from_millis(200);

/// The other nodes a question whose legs hold `legs_bytes` wants: one for each share of
/// `across_above`, or of `ceiling_bytes` when that is unset, they hold beyond this node's share, and
/// one at least, since it goes across.
pub(super) fn others_wanted(across_above: Option<NonZeroU64>, legs_bytes: u64, ceiling_bytes: u64) -> usize {
    let share = across_above.map_or(ceiling_bytes, NonZeroU64::get).max(1);
    usize::try_from(legs_bytes.div_ceil(share))
        .unwrap_or(usize::MAX)
        .saturating_sub(1)
        .max(1)
}

/// The other nodes `resolve` names now, asked again every [`POLL`] while fewer than `wanted` are,
/// until `wait` ends.
///
/// # Errors
///
/// [`CombineError::WorkersNotReady`] if `resolve` named none when the wait ended.
pub(super) fn ready(wanted: usize, wait: Duration, mut resolve: impl FnMut() -> Vec<Url>) -> Result<Vec<Url>, CombineError> {
    let started = Instant::now();
    let mut ready = resolve();
    while ready.len() < wanted {
        let left = wait.saturating_sub(started.elapsed());
        if left.is_zero() {
            break;
        }
        std::thread::sleep(left.min(POLL));
        ready = resolve();
    }
    if ready.is_empty() {
        return Err(CombineError::WorkersNotReady { wanted, ready: 0 });
    }
    Ok(ready)
}

/// The wait over a fake resolver, and the count it waits for.
#[cfg(test)]
mod tests;
