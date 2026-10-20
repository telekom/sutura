//! What one look's document does to the cache, and what the look says about it.
//!
//! Split from `keys.rs` at the thousand-line cap along the seam already there: nothing here awaits,
//! reads a source or takes a lock - the caller holds the write lock for [`adopt`] and none for
//! [`announce`].

use std::time::{Duration, Instant};

use crate::inbound::keys::{Cached, InvalidKeySet, KeySet, Refreshed, Retired};

/// Decides what a freshly read document does to the cache, under the write lock.
///
/// Split out of [`super::KeySetCache::poll_once`] for the cognitive-complexity threshold in
/// `clippy.toml`, and the split is where the reasoning is anyway: this function is *what a candidate
/// does to the state*, and nothing in it awaits or reads a source.
///
/// **The document is recorded whichever way it went**, including when it was rejected. That is the
/// distinction `crate::tls::Renewal::seen` already carries: a candidate that was examined and refused
/// is still examined, so identical bytes on the next look are `Unchanged` rather than a second
/// rejection - and [`announce`] logs that look as one that confirmed nothing.
pub(super) fn adopt(
    cached: &mut Cached,
    read: String,
    candidate: Result<KeySet, InvalidKeySet>,
    grace: Option<Duration>,
    now: Instant,
) -> Refreshed {
    if read == cached.document {
        return Refreshed::Unchanged;
    }
    let examined = core::mem::replace(&mut cached.document, read);
    match candidate {
        Ok(keys) => {
            if cached.adopted.take().unwrap_or(examined) != cached.document {
                let outgoing = core::mem::replace(&mut cached.keys, keys);
                cached.retired = Retired::after(outgoing, grace, now);
            }
            Refreshed::Rotated
        }
        Err(_refused) => {
            cached.adopted.get_or_insert(examined);
            Refreshed::Rejected
        }
    }
}

/// Says what one look did, at the level the outcome deserves.
///
/// A function of its own so [`super::KeySetCache::poll_once`] holds no lock while a `tracing` macro expands,
/// and so the levels are decided in one place: a rotation is `info`, a refused candidate is `error`
/// because the deployment is now serving keys that disagree with the source, and a look that
/// confirmed the keys says nothing - a quiet deployment must not emit a line a minute saying so.
/// **Every look that reaches here without confirming them logs**, the same refused bytes read again
/// included, with `stale_for` beside the ceiling: the two numbers an operator alerts on before every
/// caller is refused. A look whose caller left, or whose read has not returned, never reaches here.
pub(super) fn announce(
    outcome: Refreshed,
    confirmed: bool,
    (stale_for, max_stale): (Duration, Duration),
    unavailable: Option<&str>,
) {
    let (stale_for_ms, max_stale_ms) = (stale_for.as_millis(), max_stale.as_millis());
    match outcome {
        // `NotDue` is no look, and is reached on every request past the age horizon that loses the
        // reservation, so it is the noisiest outcome and says the least.
        Refreshed::NotDue => {}
        Refreshed::Unchanged if confirmed => {}
        Refreshed::Rotated => tracing::info!("the key set was re-read and replaced"),
        Refreshed::Unavailable => tracing::warn!(
            error = unavailable.unwrap_or("the look did not finish"),
            stale_for_ms,
            max_stale_ms,
            "could not re-read the key set; keeping the keys in use until max_stale_ms after a look last confirmed them"
        ),
        Refreshed::Rejected => tracing::error!(
            stale_for_ms,
            max_stale_ms,
            "the key set source changed and is NOT usable; still verifying with the previous keys until \
             max_stale_ms after a look last confirmed them. Nothing rotates until it parses and holds a key of \
             the pinned family"
        ),
        Refreshed::Unchanged => tracing::error!(
            stale_for_ms,
            max_stale_ms,
            "the key set source still holds the document this deployment refused; still verifying with the \
             previous keys until max_stale_ms after a look last confirmed them"
        ),
    }
}
