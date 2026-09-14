//! The exchanged-credential cache `WorkloadIdentityBroker` wraps itself in - `docs/adr/0031`.
//!
//! **A child module of `sts`, not a sibling.** Every type below is module-private to `super`
//! (`sts`), and a child may see a parent's private items - so nothing here needs `pub(crate)` and
//! there is still no way to reach a value of this module from outside `sts` at all. The only door is
//! [`super::WorkloadIdentityBroker::with_cache`], and the only caller of [`CredentialCache::get`]
//! and [`CredentialCache::put`] is that broker's own `mint`.
//!
//! # Why this exists, and what it does NOT change
//!
//! `sutura_domain::identity::credential`'s own module doc named caching a broker's answer an
//! architecture decision and not an optimisation: a cache keyed on anything but the subject is a
//! cross-subject leak, and a credential cache has to be keyed on the deadline it is holding too.
//! This is that decision, taken narrowly: **credentials only, never a result, never a plan, never
//! anything not keyed by who asked.** `github.com/telekom/sutura#381` is the issue; its own comments
//! correct the cost analysis (revocation is bounded by a token's own remaining life either way,
//! whether it was cached or freshly minted) and add the reason it is worth doing now: an enterprise
//! `IdP`'s token endpoint is rate-limited for session establishment, not for the synchronous inner
//! loop of every question, and #378 turns one exchange per question into N sources times M hops.
//!
//! # What is cached, and what is not
//!
//! Only a **granted** leg for an **impersonating** source - the one case
//! [`super::WorkloadIdentityBroker`]'s [`CredentialBroker::mint`](sutura_domain::identity::CredentialBroker::mint)
//! pays a round trip for. A shared leg is a declared witness with no material to save a call on, and
//! a refusal or an `Err` is never inserted - [`CredentialCache::put`] is called from exactly one place, immediately after a successful
//! [`super::StsExchange::exchange`], so there is no code path from a refusal or an error into this
//! map at all. That is "no negative caching" held by absence rather than by a flag defaulting off.
//!
//! # The key
//!
//! [`ExchangeKey`] is `(Subject, audience, scope)` - the verified subject
//! [`super::WorkloadIdentityBroker`]'s `mint` already attributes the answer to
//! (`RequestContext::chain().subject()`, the same value `LegCredentials::minted` takes as
//! `asked_by`), plus the literal input to [`super::StsExchange::exchange`]. Not the whole
//! `SourceSet` a request happened to ask about - two requests naming different subsets of sources
//! must still hit per source - and not the `SourceName` either: a source's audience and scope are
//! declared once at startup and never change while a process runs, but the issue's own wording asks
//! for the literal exchange input, and this is the shape that stays correct if that ever stops being
//! true. Not the whole `PrincipalChain`: the second and third positions (an acting agent, a task)
//! are always absent today and `LegCredentials::asked_by` is a bare `Subject` already - keying on
//! more than the port itself attributes the credential to would be a distinction with no data behind
//! it.

use std::collections::HashMap;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sutura_domain::identity::{Expiry, Secret, Subject};

use super::WorkloadIdentity;

/// `(Subject, audience, scope)` - see the module header for why each piece is there and why
/// nothing else is.
///
/// No public constructor outside this module: [`Self::of`] is the only door, and it takes a
/// [`Subject`] and a [`WorkloadIdentity`] the caller already holds - never a value assembled from
/// parts a request body could supply. `Hash`/`Eq` are derived so it can key a map; there is
/// deliberately no `Ord`, for the reason `sutura_domain::identity::PrincipalChain` gives one none
/// either - an ordering over subjects has no meaning anybody would agree on, and nothing here needs
/// one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExchangeKey {
    subject: Subject,
    audience: String,
    scope: String,
}

impl ExchangeKey {
    /// Builds the key for one source's exchange, from the subject the request is attributed to and
    /// that source's declared audience and scope.
    fn of(subject: &Subject, workload: &WorkloadIdentity) -> Self {
        Self {
            subject: subject.clone(),
            audience: workload.audience().to_owned(),
            scope: workload.scope().to_owned(),
        }
    }
}

/// What a hit hands back: enough to rebuild a `Presented::SubjectToken` leg, and the credential's
/// own deadline - **the true one**, never a refreshed-looking one, which is what keeps an audit
/// record over a cached leg honest about how long it was actually good for.
///
/// Fields are `pub(super)` rather than private: `mint` lives in the PARENT module, and Rust
/// privacy runs the other way from what a first guess expects - a child module (this one) sees a
/// parent's private items for free, but a parent sees nothing of a child's unless the child widens
/// it. This is the one type that has to cross that direction.
pub(super) struct CacheHit {
    pub(super) material: Secret,
    pub(super) not_after: Expiry,
}

/// One stored entry: the material, its own true expiry, and the instant THIS cache stops serving it
/// - never later than `not_after` and usually earlier, see [`Self::stored_until`].
#[derive(Debug, Clone)]
struct Entry {
    material: Secret,
    not_after: Expiry,
    /// Unix seconds. Past this, [`CredentialCache::get`] treats the entry as gone rather than stale
    /// - there is no partial credit for an entry one second past its own bound.
    valid_until_unix: u64,
    /// When this entry was written, for eviction: the oldest entry loses its place first once the
    /// map is at capacity and a sweep has already removed everything actually expired.
    inserted_at_unix: u64,
}

impl Entry {
    /// The one direction of the whole TTL argument, folded into a single instant: never later than
    /// the credential's own life minus the floor `docs/adr/0008` part 6 already refuses inside, and
    /// never later than the operator's configured window either - `min`, not `max`, because a
    /// setting can only shorten how long this cache serves an entry, never lengthen what was
    /// actually minted. That is the sense in which the window is a ceiling: nothing here can make a
    /// served credential outlive what `min` already bounds it to.
    ///
    /// `floor` is `None` exactly when the broker holds no floor at all, in which case the
    /// credential's own `not_after` is the only ceiling from that side - the same absence
    /// `super::clears_floor` treats as "refuses nothing".
    ///
    /// `Expiry::NothingExpires` cannot reach an impersonating leg in practice - an exchanged token
    /// always carries a lifetime - but is answered rather than unreachable, because this is not a
    /// test body and `unwrap_used` is denied: it contributes no ceiling from that side, same as an
    /// absent floor.
    fn stored_until(not_after: Expiry, now_unix_seconds: u64, window: Duration, floor: Option<NonZeroU64>) -> Option<u64> {
        let from_expiry = match not_after {
            Expiry::NothingExpires => None,
            Expiry::At { unix_seconds } => {
                let margin = floor.map_or(0, NonZeroU64::get);
                let ceiling = unix_seconds.saturating_sub(margin);
                if ceiling <= now_unix_seconds {
                    // Already inside the floor, or already past. Nothing to cache - the fresh mint
                    // this call just made is handed to the one caller that asked, and the next one
                    // pays for its own exchange, same as if no cache existed.
                    return None;
                }
                Some(ceiling)
            }
        };
        let from_window = now_unix_seconds.saturating_add(window.as_secs());
        Some(from_expiry.map_or(from_window, |expiry| expiry.min(from_window)))
    }
}

/// The bounded, per-process map one broker consults before spending a round trip.
///
/// **`parking_lot::Mutex`, not `std::sync::Mutex` or `std::sync::RwLock`** - both are denied in
/// `clippy.toml` for the async-executor deadlock the ban exists for, which does not apply here
/// (`CredentialBroker::mint` is synchronous, called from inside one `block_on`, never held across an
/// `.await`) but the type is banned outright regardless, so `parking_lot` is the answer for a
/// synchronous critical section the way `tokio::sync::RwLock` is for an asynchronous one
/// (`sutura_http::inbound::keys::KeySetCache`).
///
/// Held behind `Arc` by [`super::WorkloadIdentityBroker`] rather than owned - the broker derives
/// `Clone`, and a clone that got an EMPTY cache instead of the shared one would silently halve
/// whatever composed two of them's hit rate. Sharing state across a clone is what `Arc` is for;
/// nothing here escapes the borrow checker with it.
#[derive(Debug)]
pub(super) struct CredentialCache {
    entries: parking_lot::Mutex<HashMap<ExchangeKey, Entry>>,
    capacity: NonZeroUsize,
    window: Duration,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl CredentialCache {
    pub(super) fn new(capacity: NonZeroUsize, window: Duration) -> Self {
        Self {
            entries: parking_lot::Mutex::new(HashMap::new()),
            capacity,
            window,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// A live entry for `subject`/`workload` at `now_unix_seconds`, or `None` - counted as a miss
    /// either for having nothing stored or for finding only something already past
    /// [`Entry::valid_until_unix`].
    ///
    /// A stale entry found here is removed rather than left for the next lookup to skip past again
    /// - the same argument `KeySetCache` makes for not accumulating dead state under its own lock.
    pub(super) fn get(&self, subject: &Subject, workload: &WorkloadIdentity, now_unix_seconds: u64) -> Option<CacheHit> {
        let key = ExchangeKey::of(subject, workload);
        // The lock is scoped to this block alone - `significant_drop_tightening` wants the guard
        // gone before the counters below are touched, not merely by the end of the function.
        let hit = {
            let mut entries = self.entries.lock();
            match entries.get(&key) {
                Some(entry) if entry.valid_until_unix > now_unix_seconds => Some(CacheHit {
                    material: entry.material.clone(),
                    not_after: entry.not_after,
                }),
                Some(_expired) => {
                    drop(entries.remove(&key));
                    None
                }
                None => None,
            }
        };
        if hit.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        hit
    }

    /// Stores a freshly minted leg, if it clears the floor by enough to be worth storing at all.
    ///
    /// **Called from exactly one call site**, immediately after a successful
    /// [`super::StsExchange::exchange`] - there is no `put` on the refusal or the error path, which
    /// is the whole of "no negative caching" for this cache: the property is that the call does not
    /// exist there, not that a flag on it defaults to off.
    pub(super) fn put(
        &self,
        subject: &Subject,
        workload: &WorkloadIdentity,
        material: Secret,
        not_after: Expiry,
        floor: Option<NonZeroU64>,
        now_unix_seconds: u64,
    ) {
        let Some(valid_until_unix) = Entry::stored_until(not_after, now_unix_seconds, self.window, floor) else {
            return;
        };
        let key = ExchangeKey::of(subject, workload);
        let mut entries = self.entries.lock();
        // Expiry-first: drop everything already past its OWN bound before capacity is even asked
        // about, so a cache under steady load evicts the thing that earned it rather than whatever
        // happens to be oldest.
        entries.retain(|_, entry| entry.valid_until_unix > now_unix_seconds);
        if entries.len() >= self.capacity.get() && !entries.contains_key(&key) {
            let oldest = entries
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at_unix)
                .map(|(oldest_key, _)| oldest_key.clone());
            if let Some(oldest) = oldest {
                drop(entries.remove(&oldest));
            }
        }
        drop(entries.insert(
            key,
            Entry {
                material,
                not_after,
                valid_until_unix,
                inserted_at_unix: now_unix_seconds,
            },
        ));
    }

    /// How many lookups found a live entry. Read by a composition root's own startup/health log,
    /// never on the request path - a counter is not a secret, but it is also not something a caller
    /// asks a broker for.
    pub(super) fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// How many lookups found nothing usable - absent, or past its stored bound.
    pub(super) fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::num::NonZeroUsize;
    use std::rc::Rc;
    use std::time::Duration;

    use sutura_domain::identity::{
        CredentialBroker as _, Expiry, Minted, PrincipalChain, RequestContext, Secret, SourceSet, Subject, SubjectId,
    };
    use sutura_domain::model::SourceName;

    use super::super::{StsCredential, StsExchange, UnixClock, WorkloadIdentityBroker};
    use super::WorkloadIdentity;

    /// A fixed instant every test in this module measures against, well clear of the Unix epoch so
    /// `saturating_sub` in [`super::Entry::stored_until`] never underflows on a short lifetime.
    const NOW: u64 = 1_800_000_000;

    struct FixedClock;

    impl UnixClock for FixedClock {
        fn unix_seconds(&self) -> Result<u64, std::time::SystemTimeError> {
            Ok(NOW)
        }
    }

    fn subject(id: &str) -> Subject {
        Subject::Verified {
            id: SubjectId::parse(id).expect("a test subject id is a subject id"),
        }
    }

    fn workload() -> WorkloadIdentity {
        WorkloadIdentity::of(
            String::from("//iam.example/pool"),
            String::from("https://example.invalid/scope"),
        )
    }

    fn source() -> SourceName {
        SourceName::parse("bq").expect("a test source name is a source name")
    }

    fn one_source() -> SourceSet {
        SourceSet::of(source())
    }

    fn context_for(who: Subject) -> RequestContext {
        RequestContext::with_assertion(PrincipalChain::of(who), Secret::new("a-caller-assertion"))
    }

    #[derive(Debug, thiserror::Error)]
    #[error("the exchange failed")]
    struct ExchangeFailed;

    /// The handle a fake exchange's owner keeps to read its own call count back, after the fake
    /// itself has been moved into a broker with no accessor to it.
    type Calls = Rc<Cell<usize>>;

    /// A fake that counts every call through a handle the test keeps outside the broker - the
    /// broker owns its exchange with no accessor back to it, so the counter's owner has to be
    /// something the test still holds after the broker is built. Same property `CountingBroker`
    /// (`sutura_app::tests_support`) pins one port up: "the one assertion a granting fake cannot
    /// make is that it was NOT called" - this crate cannot use that fake directly, because
    /// dependencies point inward and `sutura-exec-bigquery` does not depend on `sutura-app`.
    struct CountingExchange {
        calls: Rc<Cell<usize>>,
        lifetime_seconds: u64,
    }

    impl CountingExchange {
        fn lasting(lifetime_seconds: u64) -> (Self, Calls) {
            let calls = Rc::new(Cell::new(0));
            (
                Self {
                    calls: Rc::clone(&calls),
                    lifetime_seconds,
                },
                calls,
            )
        }
    }

    impl StsExchange for CountingExchange {
        type Error = ExchangeFailed;

        fn exchange(&self, _audience: &str, _scope: &str, _subject_token: &Secret) -> Result<StsCredential, Self::Error> {
            let call = self.calls.get().saturating_add(1);
            self.calls.set(call);
            Ok(StsCredential::of(
                Secret::new(format!("exchanged-{call}")),
                Expiry::At {
                    unix_seconds: NOW.saturating_add(self.lifetime_seconds),
                },
            ))
        }
    }

    struct FailingExchange {
        calls: Rc<Cell<usize>>,
    }

    impl FailingExchange {
        fn new() -> (Self, Calls) {
            let calls = Rc::new(Cell::new(0));
            (
                Self {
                    calls: Rc::clone(&calls),
                },
                calls,
            )
        }
    }

    impl StsExchange for FailingExchange {
        type Error = ExchangeFailed;

        fn exchange(&self, _audience: &str, _scope: &str, _subject_token: &Secret) -> Result<StsCredential, Self::Error> {
            self.calls.set(self.calls.get().saturating_add(1));
            Err(ExchangeFailed)
        }
    }

    fn broker_with_cache(
        exchange: CountingExchange,
        capacity: usize,
        window_seconds: u64,
    ) -> WorkloadIdentityBroker<CountingExchange, FixedClock> {
        WorkloadIdentityBroker::empty(exchange)
            .measured_against(FixedClock)
            .impersonating(source(), workload())
            .with_cache(
                NonZeroUsize::new(capacity).expect("a test capacity is non-zero"),
                Duration::from_secs(window_seconds),
            )
    }

    #[test]
    fn a_second_mint_for_the_same_subject_and_source_does_not_reach_the_exchange() {
        let (exchange, calls) = CountingExchange::lasting(600);
        let broker = broker_with_cache(exchange, 8, 300);
        let context = context_for(subject("alice"));

        let _first = broker.mint(&context, &one_source()).expect("a fixture mint does not error");
        let _second = broker.mint(&context, &one_source()).expect("a fixture mint does not error");

        assert_eq!(calls.get(), 1, "the second mint must be served from the cache");
    }

    #[test]
    fn two_subjects_at_the_same_source_never_share_a_credential() {
        let (exchange, calls) = CountingExchange::lasting(600);
        let broker = broker_with_cache(exchange, 8, 300);

        let _alice = broker
            .mint(&context_for(subject("alice")), &one_source())
            .expect("a fixture mint does not error");
        let _bob = broker
            .mint(&context_for(subject("bob")), &one_source())
            .expect("a fixture mint does not error");

        // If the key dropped the subject, `bob`'s mint would hit `alice`'s entry and this would
        // stay at 1 - which is exactly the cross-subject leak this key shape exists to make
        // unrepresentable.
        assert_eq!(calls.get(), 2, "two different subjects must each pay their own round trip");
    }

    #[test]
    fn a_credential_within_the_floor_is_never_served_from_cache() {
        // A ten-second exchange lifetime and a floor of thirty: `with_floor` refuses a FRESH mint
        // of this too, and the cache must agree rather than serve something a second mint would
        // have been refused for.
        let (exchange, calls) = CountingExchange::lasting(10);
        let broker = WorkloadIdentityBroker::empty(exchange)
            .measured_against(FixedClock)
            .impersonating(source(), workload())
            .with_floor(30)
            .with_cache(
                NonZeroUsize::new(8).expect("a test capacity is non-zero"),
                Duration::from_secs(300),
            );
        let context = context_for(subject("alice"));

        let first = broker.mint(&context, &one_source()).expect("a fixture mint does not error");
        assert!(
            matches!(first, Minted::Refused { .. }),
            "a mint inside the floor is refused, cache or not"
        );

        let second = broker.mint(&context, &one_source()).expect("a fixture mint does not error");
        assert!(
            matches!(second, Minted::Refused { .. }),
            "nothing inside the floor may ever have been cached, so the second call must ask again and be refused again"
        );
        assert_eq!(calls.get(), 2, "an entry that never cleared the floor must never be stored");
    }

    #[test]
    fn a_source_with_no_declared_credential_is_refused_before_any_exchange() {
        // No `impersonating` entry at all: refused by the pre-check before the exchange loop even
        // starts, on every call - which is what keeps a refused source from ever reaching the one
        // call site `put` is wired to.
        let (exchange, calls) = CountingExchange::lasting(600);
        let broker = WorkloadIdentityBroker::empty(exchange)
            .measured_against(FixedClock)
            .with_cache(
                NonZeroUsize::new(8).expect("a test capacity is non-zero"),
                Duration::from_secs(300),
            );
        let context = context_for(subject("alice"));

        let _first = broker.mint(&context, &one_source()).expect("a fixture mint does not error");
        let _second = broker.mint(&context, &one_source()).expect("a fixture mint does not error");

        assert_eq!(
            calls.get(),
            0,
            "a source with no declared credential never reaches the exchange at all"
        );
    }

    #[test]
    fn a_refusal_for_a_missing_assertion_is_never_cached() {
        // The source IS declared impersonating - the refusal here is the OTHER arm, for a caller
        // with no assertion to exchange. A cache entry inserted on that refusal would make the
        // NEXT caller for the same subject and source - one who DOES present an assertion - get
        // served from a cache that was never populated by a real exchange.
        let (exchange, calls) = CountingExchange::lasting(600);
        let broker = broker_with_cache(exchange, 8, 300);
        let who = subject("alice");

        let refused = broker
            .mint(&RequestContext::of(PrincipalChain::of(who.clone())), &one_source())
            .expect("a fixture mint does not error");
        assert!(
            matches!(refused, Minted::Refused { .. }),
            "no assertion to exchange is a refusal"
        );
        assert_eq!(calls.get(), 0, "a refusal must never reach the exchange");

        let granted = broker
            .mint(&context_for(who), &one_source())
            .expect("a fixture mint does not error");
        assert!(
            matches!(granted, Minted::Granted { .. }),
            "a caller with an assertion is granted"
        );
        assert_eq!(
            calls.get(),
            1,
            "the refusal above must not have cached anything - this call must still pay for its own exchange"
        );
    }

    #[test]
    fn an_error_from_the_exchange_is_never_cached() {
        let (exchange, calls) = FailingExchange::new();
        let broker = WorkloadIdentityBroker::empty(exchange)
            .measured_against(FixedClock)
            .impersonating(source(), workload())
            .with_cache(
                NonZeroUsize::new(8).expect("a test capacity is non-zero"),
                Duration::from_secs(300),
            );
        let context = context_for(subject("alice"));

        drop(broker.mint(&context, &one_source()).unwrap_err());
        drop(broker.mint(&context, &one_source()).unwrap_err());
        // Both calls reached the exchange - if the first error had been cached, the second `mint`
        // would need no exchange call to fail again the same way.
        assert_eq!(calls.get(), 2, "an error must never be served from cache on the next call");
    }

    #[test]
    fn the_configured_window_bounds_a_much_longer_lived_credential() {
        // Direct against the fold, rather than through the broker: a `FixedClock` cannot advance
        // inside one process, so this is the one property here nothing indirect can observe -
        // `min`, not the credential's own hour-long life, must decide.
        let bounded = super::Entry::stored_until(
            Expiry::At {
                unix_seconds: NOW.saturating_add(3600),
            },
            NOW,
            Duration::from_secs(5),
            None,
        );
        assert_eq!(
            bounded,
            Some(NOW.saturating_add(5)),
            "a five-second window must win over an hour-long credential"
        );
    }

    #[test]
    fn no_key_type_is_reachable_from_outside_this_crate() {
        // `ExchangeKey` is private to `sts::cache`, `CredentialCache` is `pub(super)` and every
        // constructor it offers takes a `Subject` and a `WorkloadIdentity` this test built through
        // the domain's own parsers - never a value assembled from a raw string at the key's own
        // level. There is no `pub` path to either type from another crate, which a `compile_fail`
        // doctest cannot even express: a doctest runs as an external crate and could not name a
        // private type to fail on trying to construct - the absence of any visible path IS the
        // stronger statement.
        let cache = super::CredentialCache::new(
            NonZeroUsize::new(1).expect("a test capacity is non-zero"),
            Duration::from_secs(60),
        );
        assert!(cache.get(&subject("alice"), &workload(), NOW).is_none());
    }
}
