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
//! **The key is the peer address, not a forwarded header, and that is a security decision.**
//! `tower_governor` also offers an extractor that reads `X-Forwarded-For` and `X-Real-IP`, which is
//! what a service behind a trusted reverse proxy wants - and which any caller can set on a service
//! that is *not* behind one, making every bucket a caller's to choose. Since nothing here can know
//! whether a trusted proxy is in front, the unspoofable key is the correct default. The consequence
//! is stated rather than hidden: behind a proxy, every request appears to come from the proxy and
//! shares one bucket.
//!
//! Rate limiting is not authentication. It bounds how fast something can be done, not who may do
//! it.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use governor::middleware::StateInformationMiddleware;
use sutura_config::Quota;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::PeerIpKeyExtractor;

use crate::problem::Failure;
use crate::state::ServiceState;

/// The header a token arrives in, and its scheme.
const AUTHORIZATION: &str = "authorization";
const BEARER: &str = "Bearer ";

/// A limiter layer, keyed by peer address, reporting its state in response headers.
///
/// A named alias because the inline form is over the complexity threshold in `clippy.toml`. The
/// `StateInformationMiddleware` in it is not incidental: it is what makes the layer emit the
/// remaining-quota headers, and it is part of the type because that choice is made at construction.
pub type RateLimit = GovernorLayer<PeerIpKeyExtractor, StateInformationMiddleware, axum::body::Body>;

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
pub fn probe_rate_limit_layer(quota: Quota) -> Result<RateLimit, LimiterNotBuilt> {
    layer(quota, "public")
}

/// The tier for the versioned API.
pub fn api_rate_limit_layer(quota: Quota) -> Result<RateLimit, LimiterNotBuilt> {
    layer(quota, "general")
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

/// Builds one tier.
fn layer(quota: Quota, tier: &'static str) -> Result<RateLimit, LimiterNotBuilt> {
    let config = GovernorConfigBuilder::default()
        // Per nanosecond rather than `per_second`, so a sustained rate below one request per second
        // is expressible. `NonZeroU32` is why this division cannot be by zero.
        .per_nanosecond(replenishment_nanoseconds(quota))
        .burst_size(quota.burst().get())
        // Emits the remaining-quota headers, which is what lets a well-behaved caller pace itself
        // instead of discovering the limit by being refused.
        .use_headers()
        .key_extractor(PeerIpKeyExtractor)
        .finish()
        .ok_or(LimiterNotBuilt { tier })?;
    tracing::info!(
        tier,
        per_second = quota.per_second().get(),
        burst = quota.burst().get(),
        "rate limiter built"
    );
    // The error handler is where a refused request becomes the same failure body as everything
    // else, rather than the limiter's own default response - so a client parsing one failure shape
    // parses them all.
    Ok(GovernorLayer::new(Arc::new(config)).error_handler(|_error| Failure::RateLimited.into_response()))
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

    use sutura_config::Quota;

    use super::replenishment_nanoseconds;

    fn quota(per_second: u32) -> Quota {
        Quota::parse("test", per_second, per_second.max(1)).expect("a test quota is a quota")
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
