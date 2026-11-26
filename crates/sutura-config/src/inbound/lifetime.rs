//! `TokenType` and `ProofLifetime` - split out of `primitive.rs` for `xtask max-lines`.

use super::primitive::{InvalidInboundValue, MAX_LENGTH};

const TYPE_CHARACTERS: &str = "letters, digits, and any of + - . _";

/// A `typ` header value this deployment will accept on a token.
///
/// RFC 9068 section 4 requires `at+jwt` for the JWT access-token profile. RFC 8725 section 3.12 is the
/// wider rule and the reason this is configurable rather than hard-coded: an issuer using another
/// profile still has to give a deployment *some* way to distinguish token classes mechanically, and
/// which way that is is the deployment's fact rather than ours.
///
/// # What normalisation happens, and why exactly this much
///
/// Case is folded and a leading `application/` is stripped, both at construction. RFC 7515 section
/// 4.1.9 says `typ` is a media type and that the `application/` prefix **may be omitted**, so
/// `at+jwt`, `AT+JWT` and `application/at+jwt` are three spellings of one value - and a comparison
/// treating them as three would refuse tokens that are correct. This is deliberately the opposite
/// decision from [`super::ResourceIdentifier`], where nothing is normalised: an audience is an opaque
/// string an operator and an issuer configured identically, and a media type is a value a registry
/// defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenType(String);

impl TokenType {
    /// The key a token type is written under, in the `direct` mode.
    pub const KEY: &'static str = "security.inbound.token_type";

    /// RFC 9068's media type for a JWT access token. The default in the `direct` mode.
    pub const ACCESS_TOKEN: &'static str = "at+jwt";

    /// The word that turns the check off. Never a default - see `crate::inbound::RequiredTokenType`.
    pub const ANY: &'static str = "any";

    /// The prefix RFC 7515 allows a `typ` to omit.
    const OPTIONAL_PREFIX: &'static str = "application/";

    /// RFC 9068's access-token type.
    #[must_use]
    pub fn access_token() -> Self {
        Self(String::from(Self::ACCESS_TOKEN))
    }

    /// Reads a configured or a presented token type.
    ///
    /// **One parser for both sides of the comparison**, which is what makes them agree by construction
    /// rather than by review: the configured value and the value out of a verified token header are
    /// folded and stripped by the same code.
    pub fn parse(key: &'static str, raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        let folded = raw.as_ref().trim().to_ascii_lowercase();
        let stripped = folded.strip_prefix(Self::OPTIONAL_PREFIX).unwrap_or(&folded);
        if stripped.is_empty() {
            return Err(InvalidInboundValue::Empty { key });
        }
        let found = stripped.chars().count();
        if found > MAX_LENGTH {
            return Err(InvalidInboundValue::TooLong {
                key,
                found,
                limit: MAX_LENGTH,
            });
        }
        for (position, character) in stripped.chars().enumerate() {
            // The `token` production a media-type subtype is drawn from, narrowed to what a registry
            // actually uses. Everything outside ASCII is refused with it, which is how an invisible or
            // direction-changing code point is refused without a second check for one.
            let permitted = character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.' | '_');
            if !permitted {
                return Err(InvalidInboundValue::NotPermitted {
                    key,
                    position,
                    accepted: TYPE_CHARACTERS,
                });
            }
        }
        Ok(Self(String::from(stripped)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for TokenType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The longest lifetime a transit proof may declare.
///
/// **A server-chosen ceiling on somebody else's token**, and the reason it exists is that
/// `docs/adr/0014` calls a transit proof *short-lived* while the lifetime is entirely the fronting
/// component's to choose. Review demonstrated a proof with `exp` ten years out being accepted, and
/// accepted again on a replay of the identical token. So the deployment declares what it will call
/// short-lived, and a proof claiming more is refused.
///
/// Bounded above because a ceiling of a year is not a ceiling. Bounded below by one second, because a
/// zero would refuse every proof - a way of turning the mode off that reads like a tuning value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProofLifetime {
    seconds: u64,
}

impl ProofLifetime {
    /// The key a lifetime is written under.
    pub const KEY: &'static str = "security.inbound.transit_max_lifetime_seconds";

    /// The default, in seconds.
    ///
    /// Two minutes: long enough for a clock skew plus a slow hop, short enough that a captured proof is
    /// worth little. A DEFAULT rather than a required key because a wrong value here fails in the safe
    /// direction - it refuses a request that can be retried - which is the opposite of
    /// `security.inbound.mode`, where the wrong value fails silently.
    pub const DEFAULT_SECONDS: u64 = 120;

    /// The ceiling, in seconds. An hour: past this, the word "short-lived" is not being used.
    pub const MAX_SECONDS: u64 = 3600;

    /// The longest lifetime that starts without `security.inbound.accept_long_transit_lifetime`.
    /// Five minutes: the default's clock skew and slow hop, with room to spare.
    pub const SHORT_LIVED_SECONDS: u64 = 300;

    /// Reads a configured lifetime.
    pub const fn parse(seconds: u64) -> Result<Self, InvalidInboundValue> {
        if seconds == 0 || seconds > Self::MAX_SECONDS {
            return Err(InvalidInboundValue::LifetimeOutOfRange {
                found: seconds,
                limit: Self::MAX_SECONDS,
            });
        }
        Ok(Self { seconds })
    }

    /// The default lifetime.
    #[must_use]
    pub const fn default_lifetime() -> Self {
        Self {
            seconds: Self::DEFAULT_SECONDS,
        }
    }

    /// The ceiling, in whole seconds, which is how the claims express it.
    #[inline]
    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.seconds
    }
}

impl core::fmt::Display for ProofLifetime {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}s", self.seconds)
    }
}
