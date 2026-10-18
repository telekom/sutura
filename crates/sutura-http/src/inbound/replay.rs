//! The gateway assertions this process has admitted, where `security.inbound.transit_single_use`
//! declares each one single use - `docs/adr/20261015094920`.
//!
//! **Per process, and that is the limit.** With N replicas one captured assertion is admitted up to N
//! times inside its window, and a restart forgets every entry. A store shared across replicas is the
//! same open decision as `docs/adr/0030`'s shared counter. Single use refuses a second presentation,
//! never a first one by the wrong party, so the hop from the component stays a trusted boundary.
//!
//! **A full store refuses and never evicts a live entry**: evicting one would reopen its replay. An
//! entry is dropped only once its assertion is past `exp` plus the validator's leeway, which is when
//! the validator refuses that assertion anyway.

use std::collections::HashMap;

use parking_lot::Mutex;

/// The longest `jti` this deployment holds, in bytes. A UUID is 36.
pub(crate) const MAX_ASSERTION_ID_BYTES: usize = 128;

/// How many unexpired assertions one process holds.
///
/// A throughput limit as well as a memory one: an entry keeps its place until its assertion expires,
/// so a component minting at the full lifetime gets at most this many requests per lifetime.
pub(crate) const CAPACITY: usize = 1 << 18;

/// A `jti`, bounded. No `Debug` and no `Display`: it is caller-adjacent text, never logged.
#[derive(PartialEq, Eq, Hash)]
pub(crate) struct AssertionId(String);

impl AssertionId {
    /// A string of 1 to [`MAX_ASSERTION_ID_BYTES`] bytes, or nothing.
    pub(crate) fn parse(claim: &serde_json::Value) -> Option<Self> {
        claim
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= MAX_ASSERTION_ID_BYTES)
            .map(|id| Self(String::from(id)))
    }
}

/// Why an assertion was not admitted.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NotAdmitted {
    /// Its `jti` is held, and its assertion has not expired.
    Replayed,
    /// Its `until` is before the store's own clock.
    Expired,
    /// Every place is held by an unexpired assertion. `announce` is `true` once per window, so a full
    /// store is logged once rather than once per refused request.
    Full { announce: bool },
}

/// The admitted `jti`s, each with the last second its assertion is accepted.
pub(crate) struct SeenAssertions {
    capacity: usize,
    /// The longest an entry can live, in seconds: the lifetime ceiling plus the leeway on both ends.
    window: i64,
    state: Mutex<Seen>,
}

struct Seen {
    until: HashMap<AssertionId, i64>,
    /// No later than the earliest `until` held, so a full store of live entries is not swept on every
    /// request. It only falls on insert, so after a replaced entry it can start a sweep that drops
    /// nothing.
    next_expiry: i64,
    /// The second until which a full store is not announced again.
    quiet_until: i64,
    /// The latest `now` any admission was asked at: the store's own clock, which never moves back.
    latest: i64,
}

impl SeenAssertions {
    pub(crate) fn new(capacity: usize, window: i64) -> Self {
        Self {
            capacity,
            window,
            state: Mutex::new(Seen {
                until: HashMap::new(),
                next_expiry: i64::MAX,
                quiet_until: i64::MIN,
                latest: i64::MIN,
            }),
        }
    }

    /// Admits `id` once and holds it through `until`, both in unix seconds against `now`.
    ///
    /// **The check and the record are one lock acquisition**, so of two concurrent presentations of
    /// one assertion exactly one is admitted. **The store's own clock decides** - `now`, or the latest
    /// `now` it was asked at if that is later - and an `until` before it is refused and recorded nowhere.
    pub(crate) fn admit(&self, id: AssertionId, until: i64, now: i64) -> Result<(), NotAdmitted> {
        let mut seen = self.state.lock();
        let now = now.max(seen.latest);
        seen.latest = now;
        if until < now {
            return Err(NotAdmitted::Expired);
        }
        if seen.until.get(&id).is_some_and(|held| *held >= now) {
            return Err(NotAdmitted::Replayed);
        }
        if seen.until.len() >= self.capacity && !seen.until.contains_key(&id) {
            if now > seen.next_expiry {
                seen.until.retain(|_, held| *held >= now);
                seen.next_expiry = seen.until.values().copied().min().unwrap_or(i64::MAX);
            }
            if seen.until.len() >= self.capacity {
                let announce = now > seen.quiet_until;
                if announce {
                    seen.quiet_until = now.saturating_add(self.window);
                }
                return Err(NotAdmitted::Full { announce });
            }
        }
        seen.next_expiry = seen.next_expiry.min(until);
        let _replaced_an_expired_entry = seen.until.insert(id, until);
        drop(seen);
        Ok(())
    }
}

impl core::fmt::Debug for SeenAssertions {
    /// The bound and the count, never an entry.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SeenAssertions")
            .field("capacity", &self.capacity)
            .field("held", &self.state.lock().until.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{AssertionId, NotAdmitted, SeenAssertions};

    fn id(value: &str) -> AssertionId {
        AssertionId::parse(&serde_json::Value::from(value)).expect("a test jti is one")
    }

    #[test]
    fn a_seen_id_is_held_through_its_last_second_and_forgotten_after_it() {
        let seen = SeenAssertions::new(4, 180);
        assert_eq!(seen.admit(id("a"), 100, 10), Ok(()));
        assert_eq!(
            seen.admit(id("a"), 100, 100),
            Err(NotAdmitted::Replayed),
            "held through its last second"
        );
        assert_eq!(seen.admit(id("a"), 200, 101), Ok(()), "forgotten after it");
    }

    #[test]
    fn an_assertion_already_past_its_window_is_refused_and_recorded_nowhere() {
        let seen = SeenAssertions::new(4, 180);
        assert_eq!(seen.admit(id("a"), 100, 101), Err(NotAdmitted::Expired));
        assert!(seen.state.lock().until.is_empty(), "recorded nowhere");
    }

    #[test]
    fn the_store_clock_never_moves_back() {
        let seen = SeenAssertions::new(4, 180);
        assert_eq!(seen.admit(id("a"), 300, 200), Ok(()));
        assert_eq!(
            seen.admit(id("b"), 195, 190),
            Err(NotAdmitted::Expired),
            "asked at 190 after 200, the store reads 200"
        );
    }

    #[test]
    fn a_full_store_refuses_a_new_id_and_never_evicts_a_live_one() {
        let seen = SeenAssertions::new(2, 180);
        assert_eq!(seen.admit(id("a"), 100, 10), Ok(()));
        assert_eq!(seen.admit(id("b"), 150, 10), Ok(()));
        assert!(
            matches!(seen.admit(id("c"), 150, 20), Err(NotAdmitted::Full { .. })),
            "a full store admits nothing new"
        );
        assert_eq!(
            seen.admit(id("a"), 100, 20),
            Err(NotAdmitted::Replayed),
            "and evicted nothing to try"
        );
        assert_eq!(
            seen.admit(id("b"), 150, 20),
            Err(NotAdmitted::Replayed),
            "and evicted nothing to try"
        );
        assert_eq!(
            seen.admit(id("c"), 150, 101),
            Ok(()),
            "a place frees once its assertion expires"
        );
    }

    #[test]
    fn a_sweep_keeps_an_entry_through_its_last_second() {
        let seen = SeenAssertions::new(2, 180);
        assert_eq!(seen.admit(id("a"), 100, 10), Ok(()));
        assert_eq!(seen.admit(id("b"), 150, 10), Ok(()));
        assert_eq!(seen.admit(id("c"), 200, 150), Ok(()), "the sweep at 150 frees `a`'s place");
        assert_eq!(
            seen.admit(id("b"), 150, 150),
            Err(NotAdmitted::Replayed),
            "and keeps `b` in its last second"
        );
    }

    #[test]
    fn a_full_store_is_announced_once_per_window() {
        let seen = SeenAssertions::new(1, 180);
        assert_eq!(seen.admit(id("a"), 1000, 10), Ok(()));
        assert_eq!(seen.admit(id("b"), 1000, 10), Err(NotAdmitted::Full { announce: true }));
        assert_eq!(seen.admit(id("c"), 1000, 190), Err(NotAdmitted::Full { announce: false }));
        assert_eq!(seen.admit(id("d"), 1000, 191), Err(NotAdmitted::Full { announce: true }));
    }

    #[test]
    fn a_jti_that_is_not_a_bounded_string_is_no_assertion_id() {
        assert!(AssertionId::parse(&serde_json::Value::from("")).is_none());
        assert!(AssertionId::parse(&serde_json::Value::from(7)).is_none());
        assert!(AssertionId::parse(&serde_json::Value::from("x".repeat(129))).is_none());
        assert!(AssertionId::parse(&serde_json::Value::from("x".repeat(128))).is_some());
    }
}
