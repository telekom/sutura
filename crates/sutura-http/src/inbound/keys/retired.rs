//! The key set a rotation replaced, and the grace it keeps verifying for.
//!
//! `docs/adr/20261015095237` decides it: a key set file that holds only the current key cannot overlap
//! a rotation, so behind a gateway the outgoing set keeps verifying for the assertion lifetime ceiling
//! plus the clock-skew leeway. `direct` has no ceiling, so it has no grace. One set only - a second
//! rotation inside the grace drops the older one - and only a look that adopts a new set starts it, so
//! a broken new file cannot extend it.

use std::time::{Duration, Instant};

use jsonwebtoken::DecodingKey;

use super::{KeyId, KeySet};

/// The key set a rotation replaced, and the instant its grace closes.
pub(super) struct Retired {
    keys: KeySet,
    until: Instant,
}

impl Retired {
    /// Retires `keys` for `grace` from `now`. `None` where there is no grace, and on an instant that
    /// does not fit, so neither keeps a key verifying.
    pub(super) fn after(keys: KeySet, grace: Option<Duration>, now: Instant) -> Option<Self> {
        let until = now.checked_add(grace?)?;
        Some(Self { keys, until })
    }

    /// The verifier for `id`, while the grace is open.
    pub(super) fn get(&self, id: &KeyId, now: Instant) -> Option<DecodingKey> {
        if now >= self.until {
            return None;
        }
        self.keys.get(id)
    }
}
