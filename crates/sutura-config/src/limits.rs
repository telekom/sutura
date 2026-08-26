//! How many requests a caller gets, and the two tiers that answer differently.
//!
//! **Rate limiting is not authentication.** It bounds how fast something can be done, not who
//! may do it, and on a surface with no per-caller identity - see [`crate::security`] - the key
//! it counts against is a network address rather than a principal. A shared egress address is
//! therefore one bucket for everybody behind it, and a caller with many addresses has many
//! buckets. Both of those are properties of the mechanism, not bugs in the configuration, and
//! neither is repaired by tightening the numbers.
//!
//! What it does buy is real: it turns an unbounded loop against a data system into a bounded
//! one, and a question here is an aggregate over up to ten years of history, so the cost of one
//! request is not small.
//!
//! **The switch follows the environment, and the refusal does not.** `rate_limit.enabled` has no
//! fixed default: it is off in development and test and on in production, the same shape
//! `telemetry.format` and `api.docs` already have here. That is a convenience in one direction only.
//! An explicit `false` in production is still a refusal to start - see
//! [`crate::Settings::refusals`] - because a default nobody had to write down and a control an
//! operator switched off are different facts, and the second one has to be visible.
//!
//! **Which address the bucket is keyed on is a configuration decision, and it has to be.** The
//! peer address is unforgeable and is the proxy's for every request behind an ingress controller,
//! which is one bucket for the whole internet; a forwarded header is per-caller and is a value any
//! caller can write. [`crate::proxy`] is where that trade lives, and the refusal that keeps the
//! header from being believed without a named hop is in [`crate::Settings::refusals`].
//!
//! Two tiers, because the two surfaces have different shapes. The *probe* tier covers what a
//! caller may poll - liveness, and the generated `OpenAPI` document - and is tight, because nothing
//! there changes between two requests. The *api* tier covers the versioned API, where a legitimate
//! caller asks several questions in a row.
//!
//! The names avoid the word that would be natural for the first tier, and not for a style reason:
//! `cargo xtask check-boundaries` flags a field whose name *begins* with `pub` as a public field,
//! because its scan is line-oriented and tests `starts_with("pub")`. A field called `public` is
//! therefore a gate failure on correct code. Renaming here is the cheap side of that trade, and the
//! names are more precise anyway - the first tier is not the unauthenticated one, since the
//! interface description sits behind the access token when one is configured.

use core::num::NonZeroU32;

use crate::proxy::{ClientAddressSource, TrustedProxies};

/// A sustained rate and the burst allowed above it.
///
/// Both non-zero: a quota of zero requests per second is a closed door, which is what
/// `enabled: false` says properly, and a zero burst is a limiter that rejects the first request
/// of every idle period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    per_second: NonZeroU32,
    burst: NonZeroU32,
}

/// Why a pair of numbers is not a quota.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidQuota {
    /// One of the two was zero.
    #[error("{name} may not be zero - use `rate_limit.enabled: false` to turn limiting off")]
    Zero { name: &'static str },
    /// The burst is below the sustained rate, which is a limiter that cannot sustain its own
    /// rate: the bucket refills faster than it can hold.
    #[error("{name}: a burst of {burst} is below the sustained rate of {per_second} per second")]
    BurstBelowRate {
        name: &'static str,
        burst: u32,
        per_second: u32,
    },
}

impl Quota {
    /// Reads a tier.
    ///
    /// `name` is the configuration key this tier came from, so the error says which of the two
    /// tiers is wrong rather than that one of them is.
    pub const fn parse(name: &'static str, per_second: u32, burst: u32) -> Result<Self, InvalidQuota> {
        let Some(per_second_nz) = NonZeroU32::new(per_second) else {
            return Err(InvalidQuota::Zero { name });
        };
        let Some(burst_nz) = NonZeroU32::new(burst) else {
            return Err(InvalidQuota::Zero { name });
        };
        if burst < per_second {
            return Err(InvalidQuota::BurstBelowRate { name, burst, per_second });
        }
        Ok(Self {
            per_second: per_second_nz,
            burst: burst_nz,
        })
    }

    #[inline]
    pub const fn per_second(self) -> NonZeroU32 {
        self.per_second
    }

    #[inline]
    pub const fn burst(self) -> NonZeroU32 {
        self.burst
    }
}

/// Both tiers, the switch, and what a bucket is keyed on.
///
/// The switch is separate from the numbers on purpose. A deployment that turns limiting off is
/// making a decision, and it should be one word in a file rather than a quota set so high it
/// never fires - which reads as a configured limit and is not one.
///
/// **The switch itself has no fixed default; it follows [`crate::Environment`].** Off on a laptop,
/// because a limiter that fires while somebody is iterating is a bug report about sutura that is
/// really a bug report about the tier; on in production, because that is the deployment an
/// unbounded caller costs something. The same shape as `telemetry.format` and `api.docs`, including
/// the recorded flag - see [`Self::enabled_default_for`].
///
/// **Not `Copy`, and that is the trusted-proxy list.** It is a `Vec`, so this group is cloned
/// rather than copied and the accessors borrow. Every call site is inside the assembled router,
/// once, at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitSettings {
    enabled: bool,
    /// Whether [`Self::enabled`] was written down or derived from the environment.
    ///
    /// Recorded rather than recomputed, for the reason `telemetry.format` records it: the
    /// derivation is not reversible once the value is stored. `false` on a laptop looks identical
    /// whether a developer chose it or nobody did, and those are worth different words in the
    /// startup log - the first is a decision, the second is a default somebody may not know about.
    enabled_was_explicit: bool,
    probe: Quota,
    api: Quota,
    client_address: ClientAddressSource,
    trusted_proxies: TrustedProxies,
}

impl RateLimitSettings {
    #[inline]
    #[expect(
        clippy::too_many_arguments,
        reason = "six settings with no sub-grouping that is not arbitrary; `clippy.toml` sets the \
                  threshold to five, and the alternative - a wrapper type for the switch and its \
                  flag - would be a third spelling of the pattern `api.docs` and `telemetry.format` \
                  already share"
    )]
    pub const fn new(
        enabled: bool,
        enabled_was_explicit: bool,
        probe: Quota,
        api: Quota,
        client_address: ClientAddressSource,
        trusted_proxies: TrustedProxies,
    ) -> Self {
        Self {
            enabled,
            enabled_was_explicit,
            probe,
            api,
            client_address,
            trusted_proxies,
        }
    }

    /// The default for an environment: on in production, off everywhere else.
    ///
    /// **A default and not a refusal in one direction, and a refusal in the other.** Nothing here
    /// stops a developer switching the limiter on, and nothing here decides production: an explicit
    /// `enabled: false` in production is refused by
    /// [`crate::Settings::refusals`] regardless of what this function
    /// would have returned. The two must not be conflated - default-off in development is a
    /// convenience, and silently-off in production is how a deployment loses a control nobody
    /// noticed it had.
    ///
    /// A total match rather than a comparison, so a fourth environment has to state its own answer
    /// instead of inheriting whichever branch it happens to fall into.
    ///
    /// [`crate::Environment::Test`] is grouped with development, and that is an argument rather than
    /// a convenience. A test that exercises the limiter cannot rely on this value anyway: asserting
    /// a refusal needs a quota small enough to exhaust in two requests, so such a test writes
    /// `rate_limit.enabled` and a tier down together - which is what
    /// `sutura-http`'s harness already does. So defaulting on here would buy no coverage, and would
    /// charge every unrelated test in the suite a limiter it never asked for, at a tier
    /// (`probe_burst: 5`) that a loop over fixtures can exhaust. A limiter nothing exercises is
    /// untested code; the answer to that is a test that names the switch, not a default that fires
    /// during somebody else's assertion.
    #[inline]
    pub const fn enabled_default_for(environment: crate::Environment) -> bool {
        match environment {
            crate::Environment::Production => true,
            crate::Environment::Development | crate::Environment::Test => false,
        }
    }

    #[inline]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Did an operator write the switch down, or did the environment decide it?
    ///
    /// For the startup log, which says which arm was taken and whether anybody chose it.
    #[inline]
    pub const fn enabled_was_explicit(&self) -> bool {
        self.enabled_was_explicit
    }

    /// The tier for what an unauthenticated caller can reach.
    #[inline]
    pub const fn probe(&self) -> Quota {
        self.probe
    }

    /// The tier for the versioned API.
    #[inline]
    pub const fn api(&self) -> Quota {
        self.api
    }

    /// Where the address a bucket is keyed on comes from.
    #[inline]
    pub const fn client_address(&self) -> ClientAddressSource {
        self.client_address
    }

    /// The hops whose forwarded header is believed.
    #[inline]
    pub const fn trusted_proxies(&self) -> &TrustedProxies {
        &self.trusted_proxies
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidQuota, Quota, RateLimitSettings};
    use crate::Environment;

    #[test]
    fn a_zero_rate_is_refused_and_points_at_the_switch() {
        // Refused rather than accepted as a closed door: a zero here and a disabled limiter are
        // different intentions, and only one of them is visible in a log.
        assert_eq!(
            Quota::parse("rate_limit.probe", 0, 5),
            Err(InvalidQuota::Zero {
                name: "rate_limit.probe"
            })
        );
        assert_eq!(
            Quota::parse("rate_limit.api", 5, 0),
            Err(InvalidQuota::Zero { name: "rate_limit.api" })
        );
    }

    #[test]
    fn a_burst_below_the_sustained_rate_is_refused() {
        // A bucket that refills faster than it holds cannot deliver the rate it advertises, so
        // the configured rate would be a number nothing achieves.
        let error = Quota::parse("rate_limit.api", 20, 5).expect_err("a burst below the rate is not a quota");
        assert_eq!(
            error,
            InvalidQuota::BurstBelowRate {
                name: "rate_limit.api",
                burst: 5,
                per_second: 20
            }
        );
    }

    #[test]
    fn a_burst_equal_to_the_rate_is_accepted() {
        // The boundary, so a `<=` written where `<` belongs fails this.
        let quota = Quota::parse("rate_limit.probe", 5, 5).expect("burst equal to rate is a quota");
        assert_eq!(quota.per_second().get(), 5);
        assert_eq!(quota.burst().get(), 5);
    }

    #[test]
    fn the_limiter_is_off_by_default_in_development_and_on_in_production() {
        // The split, asserted rather than described - the same assertion `api.docs` and
        // `telemetry.format` each carry, and the reason a fourth environment cannot inherit an
        // answer nobody chose for it: `enabled_default_for` is a total match.
        assert!(RateLimitSettings::enabled_default_for(Environment::Production));
        assert!(!RateLimitSettings::enabled_default_for(Environment::Development));
        // Grouped with development on purpose. A test that exercises the limiter writes the switch
        // and a two-request tier down together, so defaulting on here would buy no coverage and
        // would charge every unrelated test a limiter it never asked for.
        assert!(!RateLimitSettings::enabled_default_for(Environment::Test));
    }
}
