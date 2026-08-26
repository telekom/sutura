//! The access control this service has, and the honest name for what it is not.
//!
//! **There is no per-caller identity in sutura today, and nothing here invents one.** No request
//! context reaches the query path, no credential is minted per request, and the `CredentialBroker`
//! port that would do it is deliberately absent because a port arrives with its adapter.
//! `examples/multi-player/README.md` is where that gap is written down for a reader.
//!
//! So what an [`AccessToken`] does is narrower than authentication, and the narrowness is the
//! point of this module documentation: a presented token proves the caller holds a secret the
//! deployment was configured with. It proves nothing about *which* caller, it cannot be scoped,
//! it cannot be revoked for one party without revoking it for all of them, and it does not reach
//! the data system - every query still runs with whatever access the process already had.
//!
//! It is worth having anyway, because the alternative on a non-loopback interface is an
//! unauthenticated way to read whatever the process can read. It is not worth mistaking for
//! identity, which is why [`SecuritySettings::describes_identity`] exists as an associated function
//! that always answers the same thing: the startup log prints it, so an operator cannot deploy this
//! believing otherwise.

use sha2::Digest as _;
use subtle::ConstantTimeEq as _;
use sutura_domain::identity::Secret;

/// A pre-shared secret a caller presents to reach the service.
///
/// Held as a [`Secret`], so the whole settings tree can be written to the startup log with
/// `Debug` and the token cannot come out with it.
///
/// **Not comparable with `==`, and that is inherited rather than reimplemented.** [`Secret`]
/// implements no `PartialEq` on purpose: a derived comparison on credential material returns on
/// the first differing byte, which is a timing oracle at whatever call site adds it. The
/// comparison lives here instead, once, as [`AccessToken::matches_in_constant_time`].
#[derive(Debug, Clone)]
pub struct AccessToken(Secret);

/// Why a string is not usable as an access token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAccessToken {
    /// Shorter than [`AccessToken::MIN_LENGTH`].
    ///
    /// Carries the length and never the value. A token quoted into an error message reaches
    /// stderr, a log collector and whatever reads that log, which is a longer life than the
    /// operator intended for it.
    #[error("an access token is at least {minimum} characters and this one is {found}")]
    TooShort { found: usize, minimum: usize },
    /// Whitespace at either end, which is almost always a copy-paste artefact and would
    /// otherwise make every request fail for a reason nobody can see in a log.
    #[error("an access token may not begin or end with whitespace")]
    Untrimmed,
}

impl AccessToken {
    /// The shortest token accepted.
    ///
    /// Thirty-two characters, because this is a bearer secret with no rate-limit-independent
    /// lockout behind it: the only thing standing between it and an offline guess is its length.
    /// It is a floor on a value the operator generates, not a strength estimate - eight random
    /// bytes hex-encoded would pass it and should not be used.
    pub const MIN_LENGTH: usize = 32;

    /// Reads a configured token.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidAccessToken> {
        let raw = raw.as_ref();
        if raw.trim() != raw {
            return Err(InvalidAccessToken::Untrimmed);
        }
        let found = raw.chars().count();
        if found < Self::MIN_LENGTH {
            return Err(InvalidAccessToken::TooShort {
                found,
                minimum: Self::MIN_LENGTH,
            });
        }
        Ok(Self(Secret::new(raw)))
    }

    /// Does `presented` equal the configured token?
    ///
    /// Named for the property rather than for the operation, because the property is the only
    /// reason this function exists rather than a `==`.
    ///
    /// **Both sides are hashed first, and that is not ceremony.** `subtle` compares equal-length
    /// byte slices without branching, which removes the early-return oracle - but comparing the
    /// raw strings would still have to decide what to do about differing lengths, and every
    /// answer to that leaks the length before it leaks anything else. Reducing both sides to a
    /// fixed 32 bytes removes the question: every comparison is over the same number of bytes
    /// whatever arrived.
    ///
    /// What this still does not do: it is not a password hash. There is no salt and no work
    /// factor, because the input is a high-entropy secret an operator generated rather than
    /// something a person chose, and nothing here is stored for an attacker to find offline.
    pub fn matches_in_constant_time(&self, presented: &str) -> bool {
        let expected = sha2::Sha256::digest(self.0.expose().as_bytes());
        let actual = sha2::Sha256::digest(presented.as_bytes());
        expected.ct_eq(&actual).into()
    }
}

/// The access posture, and the acknowledgement that goes with a non-loopback bind.
///
/// Two fields rather than one, because they answer different questions and collapsing them was
/// the tempting mistake: a token says *who may reach this*, and the acknowledgement says *the
/// operator meant to publish it*. A deployment that sets a token but binds the wildcard by
/// accident has answered only the first.
#[derive(Debug, Clone, Default)]
pub struct SecuritySettings {
    access_token: Option<AccessToken>,
    expose_beyond_loopback: bool,
}

impl SecuritySettings {
    #[inline]
    pub const fn new(access_token: Option<AccessToken>, expose_beyond_loopback: bool) -> Self {
        Self {
            access_token,
            expose_beyond_loopback,
        }
    }

    /// The configured token, if there is one.
    #[inline]
    pub const fn access_token(&self) -> Option<&AccessToken> {
        self.access_token.as_ref()
    }

    /// Did the operator explicitly say they meant to listen off-host?
    #[inline]
    pub const fn expose_beyond_loopback(&self) -> bool {
        self.expose_beyond_loopback
    }

    /// Whether a token is configured, as a word for the startup log.
    ///
    /// A method and not a `Debug` of the option, so the log line cannot become the token by
    /// somebody changing the field type later.
    #[inline]
    pub const fn token_state(&self) -> &'static str {
        if self.access_token.is_some() { "configured" } else { "absent" }
    }

    /// Does anything here establish who the caller is?
    ///
    /// Always `false`, and it is a function rather than a comment so the startup log and the
    /// documentation read the same value. When a `CredentialBroker` and a request context exist,
    /// this stops being a constant and the log line changes with it; until then a deployment is
    /// told, on every boot, that the token authenticates the deployment and not the caller.
    #[inline]
    pub const fn describes_identity() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessToken, InvalidAccessToken, SecuritySettings};

    /// Thirty-two characters, which is the floor.
    const GOOD: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_short_token_is_refused_and_the_value_is_not_in_the_message() {
        let error = AccessToken::parse("hunter2").expect_err("seven characters is not a token");
        assert_eq!(
            error,
            InvalidAccessToken::TooShort {
                found: 7,
                minimum: AccessToken::MIN_LENGTH
            }
        );
        // The half that matters: an error about a credential must not carry the credential.
        let rendered = error.to_string();
        assert!(!rendered.contains("hunter2"), "{rendered}");
    }

    #[test]
    fn the_minimum_length_is_accepted_and_one_character_less_is_not() {
        assert!(
            AccessToken::parse(GOOD)
                .expect("the floor is a token")
                .matches_in_constant_time(GOOD)
        );
        let short: String = GOOD.chars().take(AccessToken::MIN_LENGTH.saturating_sub(1)).collect();
        assert_eq!(
            AccessToken::parse(short).expect_err("one character short is not a token"),
            InvalidAccessToken::TooShort {
                found: AccessToken::MIN_LENGTH.saturating_sub(1),
                minimum: AccessToken::MIN_LENGTH
            }
        );
    }

    #[test]
    fn a_token_with_surrounding_whitespace_is_refused_rather_than_trimmed() {
        // Trimming would be friendlier and worse: the operator and the service would then hold
        // different strings, and every request would fail with nothing in the log to explain it.
        //
        // Written as `expect_err` rather than `assert_eq!` on the whole `Result`, and that is not
        // a style choice: `AccessToken` has no `PartialEq`, inherited from `Secret`, so comparing
        // two `Result<AccessToken, _>` values does not compile. The awkwardness is the invariant.
        assert_eq!(
            AccessToken::parse(format!("{GOOD} ")).expect_err("a trailing space is not a token"),
            InvalidAccessToken::Untrimmed
        );
        assert_eq!(
            AccessToken::parse(format!("\n{GOOD}")).expect_err("a leading newline is not a token"),
            InvalidAccessToken::Untrimmed
        );
    }

    #[test]
    fn a_token_is_not_printed_by_debug_at_any_depth() {
        // The reason the field is a `Secret`. The startup log prints the whole settings tree
        // with `Debug`, so this is the assertion that keeps that safe.
        let settings = SecuritySettings::new(Some(AccessToken::parse(GOOD).expect("a valid token")), false);
        let rendered = format!("{settings:?}");
        assert!(!rendered.contains(GOOD), "{rendered}");
        assert!(rendered.contains("REDACTED"), "{rendered}");
    }

    #[test]
    fn the_right_token_matches_and_the_wrong_one_does_not() {
        let token = AccessToken::parse(GOOD).expect("a valid token");
        assert!(token.matches_in_constant_time(GOOD));
        assert!(!token.matches_in_constant_time("0123456789abcdef0123456789abcdeF"));
        assert!(!token.matches_in_constant_time(""));
        // A prefix of the real token must not match. Hashing both sides is what makes the
        // length difference irrelevant to the comparison rather than something it branches on.
        assert!(!token.matches_in_constant_time("0123456789abcdef"));
        // Nor a superstring of it.
        assert!(!token.matches_in_constant_time(&format!("{GOOD}x")));
    }

    #[test]
    fn nothing_here_claims_to_know_who_the_caller_is() {
        // Load-bearing rather than tautological: this is the value the startup log prints, and a
        // future change that makes a shared token look like identity has to change this test.
        let with = SecuritySettings::new(Some(AccessToken::parse(GOOD).expect("a valid token")), true);
        let without = SecuritySettings::default();
        assert!(!SecuritySettings::describes_identity());
        assert_eq!(with.token_state(), "configured");
        assert_eq!(without.token_state(), "absent");
    }
}
