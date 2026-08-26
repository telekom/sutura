//! The two layers that stand in front of a handler, and the reason there are exactly two.
//!
//! # The token gate
//!
//! [`require_token`] compares a bearer token against the configured one in constant time and
//! answers `401` otherwise. **It is not authentication of a caller.** It proves the caller holds a
//! secret an operator configured, which is a different and much smaller claim - see
//! `sutura_config::security` for what that does and does not buy, and the startup log for the line
//! an operator reads about it.
//!
//! When no token is configured the gate passes everything through. That is only reachable on a
//! loopback bind outside production, because `sutura-config` refuses to start in any other
//! combination - so the permissive branch here is guarded by a startup refusal rather than by this
//! function being careful.
//!
//! # The limiter
//!
//! Two tiers, and a third function that is a no-op with the same shape.
//!
//! **What a bucket is keyed on is configured, not decided here, and the reason is in
//! [`crate::client_address`].** Neither of `tower_governor`'s own extractors is right for both of
//! this service's deployments: the peer address is unforgeable and is the ingress controller's for
//! every request behind one, and a forwarded header is per-caller and is a value any caller can
//! write. So the key comes from [`ClientAddress`], which reads the header only from a hop the
//! operator named.
//!
//! Rate limiting is not authentication. It bounds how fast something can be done, not who may do
//! it.
//!
//! # The limiter has to be reaped, and that is not tuning
//!
//! `governor`'s keyed store grows one entry per distinct key and sheds nothing until it is asked
//! to. Nothing asked. That was survivable while the token gate sat *outside* the limiter, because
//! an unauthenticated request was refused before it could create a bucket - so the only
//! unauthenticated path into a limiter was liveness.
//!
//! [`crate::router`] now puts the limiter outside the gate, which is the point of the reordering:
//! a wrong-token attempt has to cost a cell or it is an unlimited guessing loop. That makes every
//! reachable path a path an unauthenticated caller can create a bucket on, and with the header
//! keying above it is one bucket per real client rather than one per ingress. **So the reordering
//! and [`spawn_reaper`] are one change and must not be separated:** either alone is worse than
//! neither.

use std::sync::{Arc, Weak};
use std::time::Duration;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use governor::middleware::StateInformationMiddleware;
use sutura_config::Quota;
use tower_governor::GovernorLayer;
use tower_governor::governor::{GovernorConfig, GovernorConfigBuilder};

use crate::client_address::ClientAddress;
use crate::problem::Failure;
use crate::state::ServiceState;

/// The header a token arrives in, and its scheme.
const AUTHORIZATION: &str = "authorization";
const BEARER: &str = "Bearer ";

/// A limiter layer, keyed by whatever [`ClientAddress`] attributes a request to, reporting its
/// state in response headers.
///
/// A named alias because the inline form is over the complexity threshold in `clippy.toml`. The
/// `StateInformationMiddleware` in it is not incidental: it is what makes the layer emit the
/// remaining-quota headers, and it is part of the type because that choice is made at construction.
pub type RateLimit = GovernorLayer<ClientAddress, StateInformationMiddleware, axum::body::Body>;

/// The configuration one tier is built from, and the keyed state that lives inside it.
type TierConfig = GovernorConfig<ClientAddress, StateInformationMiddleware>;

/// One tier the sweeper is watching: which tier it is, and a non-owning view of its keyed state.
///
/// A named alias because the pair is over the complexity threshold in `clippy.toml`, and naming it
/// says what the pairing is for: the name is only ever used in a log line, and the `Weak` is what
/// makes the sweeper stop when the router it was sweeping for is gone.
type WatchedTier = (&'static str, Weak<TierConfig>);

/// A built limiter tier: the layer a router installs, and the handle a sweep needs.
///
/// A named alias for the same reason - and it names the invariant the tuple exists for, which is
/// that the two halves are produced together. See [`layer`].
type BuiltTier = Result<(RateLimit, LimiterHandle), LimiterNotBuilt>;

/// How often the keyed state is swept.
///
/// A constant rather than a configuration key. It is not a posture decision - nothing a caller can
/// do changes what the right answer is - and a sweep is `O(keys)` over a concurrent map, so a knob
/// here would only ever be set wrong. One minute is far below any horizon at which the map's size
/// matters and far above the cost of the sweep.
pub const REAP_INTERVAL: Duration = Duration::from_secs(60);

/// One built tier, and the handle on its keyed state.
///
/// **This exists because `GovernorLayer::new(Arc::new(config))` used to be the last anyone saw of
/// the configuration.** The layer keeps its own `Arc` and exposes nothing, so with no handle kept
/// there was no way to call `retain_recent` and nothing anywhere did - the store grew one entry per
/// distinct key for the life of the process.
#[derive(Debug, Clone)]
pub struct LimiterHandle {
    tier: &'static str,
    config: Arc<TierConfig>,
}

impl LimiterHandle {
    /// Which tier this is, for a log line.
    #[inline]
    #[must_use]
    pub const fn tier(&self) -> &'static str {
        self.tier
    }

    /// How many keys the store is holding.
    ///
    /// An estimate by the store's own documentation, which is what makes it a size to watch rather
    /// than a number to assert an exact bound on. A test asserts that it goes up with distinct
    /// callers and back down after a sweep, which is the property that matters.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.config.limiter().len()
    }

    /// Drops every key whose state is indistinguishable from a fresh one, and gives the memory
    /// back.
    ///
    /// Dropping such a key changes no decision: a caller whose bucket was reaped gets a fresh
    /// bucket, and a fresh bucket is exactly what the reaped state said they had.
    pub fn reap(&self) {
        shed_idle_buckets(self.tier, &self.config);
    }

    /// A non-owning view, for the sweeper.
    ///
    /// `Weak` and not `Arc`, so the sweeper cannot be the reason a router stays alive: when the
    /// last layer holding this tier is dropped the upgrade fails and the thread stops. That is what
    /// keeps a test suite that assembles a router per test from accumulating one thread per test.
    fn watch(&self) -> Weak<TierConfig> {
        Arc::downgrade(&self.config)
    }
}

/// Sweeps every tier's keyed state on a fixed interval, until nothing is left to sweep.
///
/// **An OS thread and not a `tokio` task, and that is forced rather than preferred.** The router is
/// assembled before the runtime exists - the composition root builds it, then builds the runtime,
/// because the engine behind the `Warehouse` port drives its own and `Runtime::block_on` panics
/// inside one. A `tokio::spawn` here would therefore panic at startup, and a spawn guarded by
/// `Handle::try_current` would silently do nothing, which is the failure mode this whole function
/// exists to remove. The work is a `retain` over a concurrent map and does not need an executor.
///
/// Returns an error rather than carrying on without a sweeper: a process that cannot start a
/// housekeeping thread is a process whose keyed store grows without bound, and that should be a
/// refusal to start rather than a line in a log.
pub fn spawn_reaper(handles: &[LimiterHandle], interval: Duration) -> Result<(), std::io::Error> {
    if handles.is_empty() {
        return Ok(());
    }
    let watched: Vec<WatchedTier> = handles.iter().map(|h| (h.tier, h.watch())).collect();
    let thread = std::thread::Builder::new()
        .name(String::from("sutura-limiter-reaper"))
        .spawn(move || sweep_until_dropped(&watched, interval))?;
    // Detached on purpose: it stops on its own when the router it is sweeping for is gone, and
    // joining it would mean waiting for that.
    drop(thread);
    Ok(())
}

/// The sweeper's body. Stops when every tier it was given has been dropped.
///
/// Three functions rather than one, and the split is where the nesting was: a loop holding a loop
/// holding a `Weak` upgrade, a size comparison and two log sites was over the cognitive-complexity
/// threshold in `clippy.toml`. Each of the three now says one thing - when to stop, which tiers are
/// still there, and what sweeping one means - and the third is the one [`LimiterHandle::reap`]
/// calls, so a sweep on a schedule and a sweep on demand are the same code.
fn sweep_until_dropped(watched: &[WatchedTier], interval: Duration) {
    loop {
        std::thread::sleep(interval);
        if sweep_once(watched) == 0 {
            tracing::debug!("every rate limit tier was dropped; the sweeper is stopping");
            return;
        }
    }
}

/// Sweeps every tier that is still alive, and says how many that was.
///
/// The count is the stop condition and nothing else: zero means every router that installed one of
/// these layers has been dropped, so there is nothing left for the thread to do.
fn sweep_once(watched: &[WatchedTier]) -> usize {
    let mut live = 0_usize;
    for &(tier, ref weak) in watched {
        // The upgrade failing is the ordinary end of a tier, not an error: the router that held the
        // layer was dropped.
        let Some(config) = weak.upgrade() else { continue };
        live = live.saturating_add(1);
        shed_idle_buckets(tier, &config);
    }
    live
}

/// Drops one tier's keys whose state is indistinguishable from a fresh one, and gives the memory
/// back.
///
/// Logged only when it actually shed something, so a quiet deployment does not emit a line a minute
/// saying nothing happened.
fn shed_idle_buckets(tier: &'static str, config: &TierConfig) {
    let limiter = config.limiter();
    let before = limiter.len();
    limiter.retain_recent();
    limiter.shrink_to_fit();
    let after = limiter.len();
    if before != after {
        tracing::debug!(tier, before, after, "swept idle rate-limit buckets");
    }
}

/// Requires a bearer token, when one is configured.
///
/// A `from_fn_with_state` middleware rather than an extractor, because an extractor runs per
/// handler and this has to run for a whole subtree. It runs for every path in that subtree that
/// RESOLVES to a handler; a path under the prefix matching no route skips it and falls through to
/// the top-level `404`, which `crate::router` states along with why that is acceptable.
pub async fn require_token(State(state): State<ServiceState>, request: Request, next: Next) -> Response {
    let Some(expected) = state.settings().security().access_token() else {
        // No token configured. Reachable only on a loopback bind outside production; the startup
        // refusals in `sutura-config` are what make that true.
        return next.run(request).await;
    };
    let presented = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix(BEARER));
    // `unwrap_or_default` and then compare, rather than returning early on an absent header: the
    // comparison is constant-time, and skipping it when the header is missing would make "no
    // header" measurably faster than "wrong token". The empty string cannot match a token, because
    // a token has a minimum length.
    if expected.matches_in_constant_time(presented.unwrap_or_default()) {
        return next.run(request).await;
    }
    tracing::warn!(
        path = %request.uri().path(),
        presented = presented.is_some(),
        "rejected a request with no valid bearer token"
    );
    Failure::Unauthorized.into_response()
}

/// A configured quota did not produce a limiter.
///
/// **Unreachable, and an error rather than a panic or a fallback anyway.** `finish` returns `None`
/// only for a zero replenishment period or a zero burst, and `Quota::parse` refuses both - so this
/// cannot happen from a loaded configuration. It is an error because the two alternatives are both
/// wrong: a panic takes the process down for something the type system already ruled out, and a
/// silent fallback to an unlimited layer is a service that reports a configured limiter and has
/// none. This way an impossible state is a refusal to start.
#[derive(Debug, thiserror::Error)]
#[error("the {tier} rate limit tier did not build a limiter from a quota that parsed")]
pub struct LimiterNotBuilt {
    tier: &'static str,
}

/// The tier for what an unauthenticated caller can reach.
pub fn probe_rate_limit_layer(quota: Quota, key: ClientAddress) -> BuiltTier {
    layer(quota, "public", key)
}

/// The tier for the versioned API.
pub fn api_rate_limit_layer(quota: Quota, key: ClientAddress) -> BuiltTier {
    layer(quota, "general", key)
}

/// A limiter that limits nothing.
///
/// **The type is deliberately not `RateLimit`, and it still composes.** `axum::Router::layer` is
/// generic over the layer and returns a plain `Router` - the service type is erased - so the two
/// arms of "limiting on" and "limiting off" have the same type at the call site without either of
/// them pretending to be the other. `Identity` is a zero-sized type and this is a `const fn`, so
/// the disabled path costs nothing at runtime and nothing at all in the response path.
///
/// A quota set so high it never fires would have been the alternative, and it is worse: it reads as
/// a configured limit in a log and in a review, and it is not one.
#[must_use]
pub const fn disabled_rate_limit_layer() -> tower::layer::util::Identity {
    tower::layer::util::Identity::new()
}

/// Builds one tier, and hands back the handle on its keyed state.
///
/// **Both halves, always.** The `Arc` is what the layer holds and what a sweep needs, so returning
/// only the layer is how the store came to grow for the life of the process: there was nothing left
/// to call `retain_recent` on.
fn layer(quota: Quota, tier: &'static str, key: ClientAddress) -> BuiltTier {
    let config = GovernorConfigBuilder::default()
        // Per nanosecond rather than `per_second`, so a sustained rate below one request per second
        // is expressible. `NonZeroU32` is why this division cannot be by zero.
        .per_nanosecond(replenishment_nanoseconds(quota))
        .burst_size(quota.burst().get())
        // Emits the remaining-quota headers, which is what lets a well-behaved caller pace itself
        // instead of discovering the limit by being refused.
        .use_headers()
        .key_extractor(key)
        .finish()
        .ok_or(LimiterNotBuilt { tier })?;
    tracing::info!(
        tier,
        per_second = quota.per_second().get(),
        burst = quota.burst().get(),
        "rate limiter built"
    );
    let config = Arc::new(config);
    let handle = LimiterHandle {
        tier,
        config: Arc::clone(&config),
    };
    // The error handler is where a refused request becomes the same failure body as everything
    // else, rather than the limiter's own default response - so a client parsing one failure shape
    // parses them all.
    let layer = GovernorLayer::new(config).error_handler(|_error| Failure::RateLimited.into_response());
    Ok((layer, handle))
}

/// Gives up on a request that outran the configured bound, with the documented body.
///
/// **Written here rather than taken from `tower_http`, and the reason is a body.** Pinned
/// `tower-http` 0.6.11 implements `TimeoutLayer::with_status_code` as
/// `Response::new(B::default())` - the status and an *empty* body - so the `408` this surface
/// documents, and which `problem.rs` promises carries a [`crate::problem::ProblemBody`] like every
/// other failure, was a status nothing put a body behind. `Failure::Timeout` existed and was never
/// constructed. Ten lines here is the whole cost of the response shape being one shape.
///
/// It bounds *the response*, which is what a caller experiences, and not the work: a question
/// already handed to the blocking pool keeps running until the data system answers it. Cancelling
/// that needs a cancellation token the `Warehouse` port does not have.
pub async fn enforce_timeout(State(bound): State<Duration>, request: Request, next: Next) -> Response {
    let path = String::from(request.uri().path());
    match tokio::time::timeout(bound, next.run(request)).await {
        Ok(response) => response,
        Err(_elapsed) => {
            tracing::warn!(
                path = %path,
                timeout_seconds = bound.as_secs(),
                "gave up on a request that exceeded the configured bound"
            );
            Failure::Timeout.into_response()
        }
    }
}

/// One second divided by the sustained rate, in nanoseconds.
///
/// `NonZeroU32` at the input and `saturating_div` for the shape of the expression: the workspace
/// bans unchecked arithmetic, and there is no divisor here that could be zero.
fn replenishment_nanoseconds(quota: Quota) -> u64 {
    const ONE_SECOND_NANOSECONDS: u64 = 1_000_000_000;
    let per_second = core::num::NonZeroU64::from(quota.per_second()).get();
    // At least one nanosecond: a rate above a billion per second would otherwise round to zero,
    // which `finish` rejects.
    ONE_SECOND_NANOSECONDS.saturating_div(per_second).max(1)
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;
    use std::net::IpAddr;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use sutura_config::{ClientAddressSource, Quota, TrustedProxies};

    use super::{ClientAddress, api_rate_limit_layer, replenishment_nanoseconds, spawn_reaper};

    fn quota(per_second: u32) -> Quota {
        Quota::parse("test", per_second, per_second.max(1)).expect("a test quota is a quota")
    }

    fn peer_keyed() -> ClientAddress {
        ClientAddress::new(ClientAddressSource::Peer, Arc::new(TrustedProxies::default()))
    }

    /// One address per index, so a test can create as many distinct buckets as it likes.
    ///
    /// A `u8` index rather than a wider one reduced modulo 250: the workspace bans remainder
    /// arithmetic, and the last octet of an address is a byte, so the byte is the index.
    fn caller(index: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, index])
    }

    #[test]
    fn a_bucket_is_created_per_distinct_caller_and_a_sweep_gives_them_back() {
        // The leak, and the fix, in one test. Before this change the layer was the last anyone saw
        // of the configuration, so there was nothing to call `retain_recent` on and nothing did:
        // the store held one entry per distinct key for the life of the process. With header keying
        // in front of an ingress that is one entry per real client rather than one per ingress,
        // which is what turns a slow leak into a fast one.
        //
        // A fast tier on purpose: `retain_recent` drops a key whose state is indistinguishable from
        // fresh, which for a one-nanosecond replenishment period is true a moment after the
        // request. A production tier sheds the same keys on the same rule, later.
        let (_layer, handle) = api_rate_limit_layer(quota(u32::MAX), peer_keyed()).expect("a tier builds");
        assert_eq!(handle.tracked(), 0, "a fresh tier holds nothing");

        for index in 0_u8..64 {
            // Through the limiter itself rather than through a router, because what is being
            // asserted is the STORE and not the routing: one key per caller, held until swept.
            let _outcome = handle.config.limiter().check_key(&caller(index));
        }
        assert!(handle.tracked() > 1, "distinct callers did not become distinct buckets");

        // Long enough that every bucket above is indistinguishable from a fresh one.
        std::thread::sleep(Duration::from_millis(20));
        handle.reap();
        assert_eq!(handle.tracked(), 0, "the sweep gave nothing back");
    }

    #[test]
    fn the_sweeper_thread_reaps_without_anybody_calling_it() {
        // The half the assertion above cannot make: that something actually calls `reap` in a
        // running process. `spawn_reaper` is what `crate::router` starts, and a store that is only
        // swept when a test remembers to is the state this whole change is about.
        let (_layer, handle) = api_rate_limit_layer(quota(u32::MAX), peer_keyed()).expect("a tier builds");
        for index in 0_u8..32 {
            let _outcome = handle.config.limiter().check_key(&caller(index));
        }
        assert!(handle.tracked() > 1);

        spawn_reaper(core::slice::from_ref(&handle), Duration::from_millis(5)).expect("the sweeper starts");
        // Polled rather than slept-then-asserted: the assertion is "this happens", and a fixed
        // sleep either makes the test slow or makes it flaky on a loaded machine.
        let deadline = Instant::now() + Duration::from_secs(5);
        while handle.tracked() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(handle.tracked(), 0, "the sweeper thread never swept");
    }

    #[test]
    fn the_sweeper_stops_when_the_router_it_swept_for_is_gone() {
        // Why the sweeper holds `Weak` and not `Arc`. Without this the test suite would accumulate
        // one live thread per assembled router, and a long-lived process that rebuilt its router
        // would accumulate one per rebuild.
        let (layer, handle) = api_rate_limit_layer(quota(10), peer_keyed()).expect("a tier builds");
        let watch = handle.watch();
        spawn_reaper(core::slice::from_ref(&handle), Duration::from_millis(5)).expect("the sweeper starts");
        drop(layer);
        drop(handle);
        let deadline = Instant::now() + Duration::from_secs(5);
        while watch.upgrade().is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            watch.upgrade().is_none(),
            "the sweeper is still holding the tier it was sweeping"
        );
    }

    #[test]
    fn a_sweeper_with_nothing_to_sweep_starts_no_thread() {
        // The disabled-limiter path: there are no tiers, so there is nothing to sweep and no thread
        // to leave running.
        spawn_reaper(&[], Duration::from_millis(5)).expect("no tiers is not a failure");
    }

    #[test]
    fn the_replenishment_period_is_one_second_divided_by_the_rate() {
        assert_eq!(replenishment_nanoseconds(quota(1)), 1_000_000_000);
        assert_eq!(replenishment_nanoseconds(quota(10)), 100_000_000);
        assert_eq!(replenishment_nanoseconds(quota(1000)), 1_000_000);
    }

    #[test]
    fn an_enormous_rate_still_yields_a_non_zero_period() {
        // The one arithmetic edge that matters: a period of zero is what `finish` rejects, and it
        // would turn a configured limiter into the fallback one.
        let enormous = Quota::parse("test", u32::MAX, u32::MAX).expect("a large quota is a quota");
        assert!(replenishment_nanoseconds(enormous) >= 1);
        assert!(NonZeroU32::new(enormous.per_second().get()).is_some());
    }
}
