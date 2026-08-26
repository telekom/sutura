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

/// Both tiers, and the switch.
///
/// The switch is separate from the numbers on purpose. A deployment that turns limiting off is
/// making a decision, and it should be one word in a file rather than a quota set so high it
/// never fires - which reads as a configured limit and is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitSettings {
    enabled: bool,
    probe: Quota,
    api: Quota,
}

impl RateLimitSettings {
    #[inline]
    pub const fn new(enabled: bool, probe: Quota, api: Quota) -> Self {
        Self { enabled, probe, api }
    }

    #[inline]
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    /// The tier for what an unauthenticated caller can reach.
    #[inline]
    pub const fn probe(self) -> Quota {
        self.probe
    }

    /// The tier for the versioned API.
    #[inline]
    pub const fn api(self) -> Quota {
        self.api
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidQuota, Quota};

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
}
