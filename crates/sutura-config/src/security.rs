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
use sutura_domain::source::{AcknowledgementReason, InvalidOperatorText};

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
    /// A character no `Authorization` header could carry to us.
    ///
    /// **This is the variant that closes a service which starts and can never authenticate.** The
    /// gate reads the header with `HeaderValue::to_str`, which accepts visible ASCII and nothing
    /// else, so a token holding anything outside that set is one no request can present: the
    /// process boots, every call is a `401`, and nothing anywhere says why. Refusing it at startup
    /// turns a silent outage into a message.
    ///
    /// Carries the position and never the character, for the same reason [`Self::TooShort`]
    /// carries the length and never the value.
    #[error(
        "an access token is an RFC 6750 `b64token` - letters, digits, `-`, `.`, `_`, `~`, `+`, \
         `/`, and `=` only as trailing padding - and the character at position {position} \
         (counting from zero) is not one. The `Authorization` header carries visible ASCII only, \
         so a token outside that set is one no request could ever present"
    )]
    NotRepresentableOnTheWire { position: usize },
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
    ///
    /// **Parses the wire grammar, not merely a length.** The type is named for a value that
    /// arrives in an `Authorization` header, so what it accepts is what such a header can carry:
    /// see [`Self::wire_grammar`] and [`InvalidAccessToken::NotRepresentableOnTheWire`].
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
        Self::wire_grammar(raw)?;
        Ok(Self(Secret::new(raw)))
    }

    /// Is every character one an `Authorization: Bearer` value may hold?
    ///
    /// RFC 6750 `b64token`: `1*( ALPHA / DIGIT / "-" / "." / "_" / "~" / "+" / "/" ) *"="`. That
    /// is a subset of the visible ASCII `HeaderValue::to_str` will hand back, and it is the
    /// grammar the scheme actually defines - so a token that passes here is one the gate can
    /// receive AND one no intermediary has to guess at the quoting of.
    ///
    /// Deliberately stricter than "visible ASCII". A token holding a space, a comma or a quote is
    /// representable in a header and is a value that some proxy, shell or manifest will mangle;
    /// a startup refusal naming the position is cheaper than finding that out from a `401`.
    fn wire_grammar(raw: &str) -> Result<(), InvalidAccessToken> {
        let mut padding = false;
        for (position, character) in raw.chars().enumerate() {
            let permitted = match character {
                // `=` is padding, and padding is only ever a suffix.
                '=' => {
                    padding = true;
                    true
                }
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' | '+' | '/' => !padding,
                _ => false,
            };
            if !permitted {
                return Err(InvalidAccessToken::NotRepresentableOnTheWire { position });
            }
        }
        Ok(())
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

/// Where TLS is terminated for this deployment.
///
/// **A declaration, not a control.** Nothing here encrypts anything except
/// [`Self::InProcess`]; the other three name a terminator that lives somewhere else, and the point
/// of writing it down is that the *cleartext hop* it implies is then a stated fact rather than an
/// assumption. The bearer token crosses that hop in the clear, and how far the hop reaches is the
/// whole difference between the three:
///
/// | Declared | What terminates TLS | What the token crosses in cleartext |
/// | --- | --- | --- |
/// | `none` | nothing | the whole path from the caller. Only sane on loopback |
/// | `sidecar` | a proxy in this pod | a loopback hop inside the pod |
/// | `ingress` | an ingress controller or gateway | the pod network, from that hop to this process |
/// | `in-process` | this process | nothing - the connection ends here |
///
/// So `ingress` is not a weaker `sidecar`: it is the same posture with a longer cleartext segment,
/// and whether that segment is acceptable is a question about the cluster network - a mesh with
/// mutual TLS between pods answers it differently from a flat one. This type does not pretend to
/// know, and a startup log that said "TLS enabled" would be pretending.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TlsTermination {
    /// Nothing terminates TLS. Plaintext from the caller to here.
    #[default]
    None,
    /// A terminator inside this pod or on this host, reached over loopback.
    Sidecar,
    /// An ingress controller or gateway. The hop from it to this process crosses the pod network.
    Ingress,
    /// This process. Requires the `tls` feature and a certificate and key.
    InProcess,
}

/// The configured value did not name a place TLS is terminated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not say where TLS is terminated - one of: {}", TlsTermination::NAMES.join(", "))]
pub struct UnknownTlsTermination {
    found: String,
}

impl TlsTermination {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["none", "sidecar", "ingress", "in-process"];

    /// Reads the configured value.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownTlsTermination> {
        match raw.as_ref().trim() {
            "none" => Ok(Self::None),
            "sidecar" => Ok(Self::Sidecar),
            "ingress" => Ok(Self::Ingress),
            "in-process" => Ok(Self::InProcess),
            other => Err(UnknownTlsTermination {
                found: String::from(other),
            }),
        }
    }

    /// The spelling, for the startup log.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sidecar => "sidecar",
            Self::Ingress => "ingress",
            Self::InProcess => "in-process",
        }
    }

    /// Does this process hold the TLS connection itself?
    #[inline]
    pub const fn terminates_here(self) -> bool {
        matches!(self, Self::InProcess)
    }

    /// Was anything said at all?
    ///
    /// [`Self::None`] is the default, so "not declared" and "declared as nothing" are the same
    /// value - which is why the refusal for a non-loopback bind is keyed on this rather than on an
    /// `Option`. An operator who means plaintext on loopback writes nothing and gets it.
    #[inline]
    pub const fn is_declared(self) -> bool {
        !matches!(self, Self::None)
    }

    /// The cleartext hop this declaration implies, as a sentence for the startup log.
    ///
    /// A function rather than a comment for the same reason
    /// [`SecuritySettings::describes_identity`] is one: the log, the documentation and this type
    /// read the same value, so none of them can drift into claiming end-to-end encryption.
    #[inline]
    pub const fn cleartext_hop(self) -> &'static str {
        match self {
            Self::None => "the whole path from the caller is cleartext, this bearer token included",
            Self::Sidecar => "the hop from the terminator to this process is cleartext over loopback",
            Self::Ingress => {
                "the hop from the ingress to this process is cleartext across the pod network, this bearer token included"
            }
            Self::InProcess => "the connection is terminated here, so there is no cleartext hop in front of this process",
        }
    }
}

impl core::fmt::Display for TlsTermination {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which kind of deployment this is, and therefore where a shared source's acknowledgement may come
/// from.
///
/// **Two modes that differ in kind rather than in degree, and the deployment DECLARES which it is.**
///
/// *Single-user* means credentials are static configuration: one user, one host, not multi-tenant.
/// There is no per-request identity to establish, so a shared source is correct for **everything** -
/// the one user reads all, by design, and the configured credential is that user's own.
/// `examples/single-player` is this, and it is a first-class deployment rather than a degraded one.
///
/// *Multi-user* means the caller's identity arrives per request. Shared sources are still permitted,
/// and that is the whole difficulty: the deployment has to say so **per source**, on purpose.
///
/// # It is declared and never derived, and the derivation that was on offer is unsound
///
/// The tempting derivation is "every source shared means single-user, any source impersonating means
/// multi-user". It fails in exactly the configuration that most needs the check: a genuinely
/// multi-tenant deployment whose sources are *all* shared derives to single-user, and the
/// acknowledgement is required in multi-user mode only - so the derivation would exempt from the
/// acknowledgement the one deployment where every caller reads every source as somebody else's
/// identity. The failure is silent, it is one user's data served to another, and it arrives by leaving
/// a field out.
///
/// So there is **no `Default`**, no derivation, and a deployment that configures a source without
/// declaring the mode does not boot -
/// [`NotFitToServe::DeploymentIdentityUndeclared`](crate::NotFitToServe::DeploymentIdentityUndeclared).
/// The refusal is keyed on a source being configured rather than raised unconditionally, and that is
/// not a softening: a deployment with no source configured cannot answer anything, and the composition
/// root refuses it on the catalog naming a source with no declaration - so every deployment that can
/// serve a question has to declare the mode.
///
/// # What flipping the mode does
///
/// It re-evaluates every source. A single-user deployment legitimately holds every source under one
/// static credential; the same file in multi-user mode serves every one of those sources to every
/// caller as one identity. The mode is an input to the whole check rather than to an incremental view
/// of what changed, so a deployment that flips it and has acknowledged nothing does not boot.
///
/// # The variant names are not the configured words, and that is deliberate
///
/// A deployment writes `single-user` or `multi-user` - [`Self::as_str`] and [`Self::NAMES`] own those
/// spellings, and they are the vocabulary
/// [a credential per leg](https://github.com/telekom/sutura/blob/main/docs/adr/0008-a-credential-per-leg-for-the-calling-subject.md)
/// 5a names. The variants are named for the *property each mode decides* instead, because
/// `SingleUser`/`MultiUser` share a postfix and `clippy::enum_variant_names` is denied - and the names
/// that survived that say more: what changes between the two is whether credentials are static
/// configuration or a subject arrives per request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentIdentity {
    /// Static credentials, one user, one host - the `single-user` mode. Carries the operator's own
    /// reason, so the mode is unreachable by leaving a key out.
    StaticCredentials { declared: AcknowledgementReason },
    /// A subject per request, established by the transport - the `multi-user` mode.
    ///
    /// **Nothing establishes one today** - the bearer gate authenticates the deployment - so this mode
    /// is currently a statement of intent whose only mechanical effect is that every shared source has
    /// to be acknowledged on its own entry. That is the honest description and it is worth having: the
    /// acknowledgements are what a deployment needs in place *before* a subject arrives, not after.
    SubjectPerRequest,
}

/// The configured value did not name a deployment mode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not name a deployment mode - one of: {}", DeploymentIdentity::NAMES.join(", "))]
pub struct UnknownDeploymentIdentity {
    found: String,
}

impl DeploymentIdentity {
    /// The key the mode is written under.
    pub const KEY: &'static str = "security.identity";
    /// The key the single-user reason is written under.
    pub const REASON_KEY: &'static str = "security.single_user_because";
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["single-user", "multi-user"];

    /// Reads the declared mode and, for single-user, the operator's reason.
    ///
    /// The reason is **required** for single-user and **refused** for multi-user, which is the same
    /// rule `server.tls_certificate` gets: a value nothing reads is a control that appears to be in
    /// place. Both halves are returned as one typed error rather than checked later, because the mode
    /// and its witness are one declaration.
    pub fn parse(word: &str, reason: Option<&str>) -> Result<Self, InvalidDeploymentIdentity> {
        let reason = reason.map(str::trim).filter(|value| !value.is_empty());
        match word.trim() {
            "single-user" => {
                let Some(text) = reason else {
                    return Err(InvalidDeploymentIdentity::SingleUserWithoutAReason);
                };
                let declared = AcknowledgementReason::written_under(Self::REASON_KEY, text)
                    .map_err(|cause| InvalidDeploymentIdentity::Reason { cause })?;
                Ok(Self::StaticCredentials { declared })
            }
            "multi-user" => {
                if reason.is_some() {
                    return Err(InvalidDeploymentIdentity::ReasonWithoutSingleUser);
                }
                Ok(Self::SubjectPerRequest)
            }
            other => Err(InvalidDeploymentIdentity::Unknown {
                cause: UnknownDeploymentIdentity {
                    found: String::from(other),
                },
            }),
        }
    }

    /// The spelling, for the startup log.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match *self {
            Self::StaticCredentials { .. } => "single-user",
            Self::SubjectPerRequest => "multi-user",
        }
    }

    /// The reason a shared source may borrow as its acknowledgement, if this mode supplies one.
    ///
    /// `Some` for single-user only, and an exhaustive match rather than an `is_single_user()` boolean:
    /// what the mode contributes is the *witness*, so returning the value is what a caller needs and a
    /// boolean would leave every caller to work out where the witness comes from.
    #[inline]
    #[must_use]
    pub const fn shared_witness(&self) -> Option<&AcknowledgementReason> {
        match *self {
            Self::StaticCredentials { ref declared } => Some(declared),
            Self::SubjectPerRequest => None,
        }
    }

    /// Does a shared source need an acknowledgement on its own entry under this mode?
    #[inline]
    #[must_use]
    pub const fn needs_per_source_acknowledgement(&self) -> bool {
        matches!(*self, Self::SubjectPerRequest)
    }
}

/// Why a deployment mode declaration is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDeploymentIdentity {
    #[error("`{}` does not name a deployment mode", DeploymentIdentity::KEY)]
    Unknown {
        #[source]
        cause: UnknownDeploymentIdentity,
    },
    /// Single-user mode with no reason written.
    ///
    /// The reason is what makes the mode a declaration rather than a word: a single-user deployment
    /// serves every source under one identity, and the operator's own sentence for why is what a
    /// reviewer reads and what a shared source borrows as its acknowledgement.
    #[error(
        "`{}` is `single-user` and `{}` is not set. Single-user means every source is read under one \
         static credential, which is correct when that credential is the one user's own - write why, \
         because it is the sentence a reviewer needs and the one a shared source borrows",
        DeploymentIdentity::KEY,
        DeploymentIdentity::REASON_KEY
    )]
    SingleUserWithoutAReason,
    /// A single-user reason on a multi-user deployment, where nothing would read it.
    #[error(
        "`{}` is set and `{}` is `multi-user`, so nothing would read it - a shared source in \
         multi-user mode is acknowledged on its own entry. Remove it, or declare `single-user`",
        DeploymentIdentity::REASON_KEY,
        DeploymentIdentity::KEY
    )]
    ReasonWithoutSingleUser,
    #[error("`{}` is not usable as a reason", DeploymentIdentity::REASON_KEY)]
    Reason {
        #[source]
        cause: InvalidOperatorText,
    },
}

/// The access posture, and the declaration that goes with a non-loopback bind.
///
/// Two fields rather than one, because they answer different questions and collapsing them was
/// the tempting mistake: a token says *who may reach this*, and the declaration says *what, if
/// anything, encrypts the path it travels*. A deployment that sets a token but binds the wildcard
/// with nothing in front has answered only the first.
///
/// **The declaration replaced a boolean, and that is the point of it.** The boolean it replaced -
/// `expose_beyond_loopback` - recorded that somebody meant to publish the service and said nothing
/// about what protects the token in flight, so a wildcard bind with no terminator anywhere read
/// exactly like one behind a gateway. A value naming the terminator cannot be satisfied by
/// agreeing that off-host is intended.
///
/// **Three fields now, and the third answers a third question**: who a query runs *as*. It is an
/// `Option` because it has no default and its absence is a refusal rather than a value - see
/// [`DeploymentIdentity`], which explains why no combination of source postures may answer it on the
/// operator's behalf.
#[derive(Debug, Clone, Default)]
pub struct SecuritySettings {
    access_token: Option<AccessToken>,
    tls_termination: TlsTermination,
    identity: Option<DeploymentIdentity>,
}

impl SecuritySettings {
    #[inline]
    pub const fn new(
        access_token: Option<AccessToken>,
        tls_termination: TlsTermination,
        identity: Option<DeploymentIdentity>,
    ) -> Self {
        Self {
            access_token,
            tls_termination,
            identity,
        }
    }

    /// Which kind of deployment this is, if the operator declared one.
    #[inline]
    #[must_use]
    pub const fn identity(&self) -> Option<&DeploymentIdentity> {
        self.identity.as_ref()
    }

    /// The configured token, if there is one.
    #[inline]
    pub const fn access_token(&self) -> Option<&AccessToken> {
        self.access_token.as_ref()
    }

    /// Where the operator said TLS is terminated.
    #[inline]
    pub const fn tls_termination(&self) -> TlsTermination {
        self.tls_termination
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
    ///
    /// **[`Self::identity`] does not change this answer, and that is deliberate.** A declared
    /// `multi-user` mode says what the deployment *intends* and decides where a shared source's
    /// acknowledgement has to be written; it does not make a caller identity arrive. Reading the
    /// declaration back as "this deployment knows who is asking" is the exact confusion this function
    /// exists to prevent.
    #[inline]
    pub const fn describes_identity() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccessToken, DeploymentIdentity, InvalidAccessToken, InvalidDeploymentIdentity, SecuritySettings, TlsTermination,
    };

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
        let settings = SecuritySettings::new(
            Some(AccessToken::parse(GOOD).expect("a valid token")),
            TlsTermination::None,
            Some(DeploymentIdentity::SubjectPerRequest),
        );
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
        let with = SecuritySettings::new(
            Some(AccessToken::parse(GOOD).expect("a valid token")),
            TlsTermination::Ingress,
            Some(DeploymentIdentity::SubjectPerRequest),
        );
        let without = SecuritySettings::default();
        assert!(!SecuritySettings::describes_identity());
        assert_eq!(with.token_state(), "configured");
        assert_eq!(without.token_state(), "absent");
        // A DECLARED multi-user mode does not change the answer, and that is the confusion this
        // assertion exists for: the declaration says what the deployment intends and decides where a
        // shared source's acknowledgement has to be written. It does not make a caller identity arrive.
        assert_eq!(with.identity(), Some(&DeploymentIdentity::SubjectPerRequest));
        assert!(!SecuritySettings::describes_identity());
        assert_eq!(without.identity(), None, "the mode has no default");
    }

    #[test]
    fn the_deployment_mode_is_declared_with_a_reason_or_not_at_all() {
        // Single-user needs the reason: it is the sentence a reviewer reads and the one a shared source
        // borrows as its acknowledgement, so a mode without it is a word rather than a declaration.
        assert_eq!(
            DeploymentIdentity::parse("single-user", None).expect_err("single-user needs a reason"),
            InvalidDeploymentIdentity::SingleUserWithoutAReason
        );
        assert_eq!(
            DeploymentIdentity::parse("single-user", Some("   ")).expect_err("whitespace is not a reason"),
            InvalidDeploymentIdentity::SingleUserWithoutAReason
        );
        let single = DeploymentIdentity::parse("single-user", Some("one operator, their own files"))
            .expect("a declared single-user mode parses");
        assert_eq!(single.as_str(), "single-user");
        assert!(
            single.shared_witness().is_some(),
            "single-user mode is where a shared source's witness comes from"
        );
        assert!(!single.needs_per_source_acknowledgement());
    }

    #[test]
    fn the_multi_user_mode_refuses_the_reason_the_other_one_requires() {
        // Split from the test above by `cognitive_complexity`, and the split is along the seam the two
        // modes already have: one requires the reason and the other refuses it.
        //
        // Multi-user refuses it for the reason a certificate nothing reads is refused: a value nothing
        // reads is a control that appears to be in place.
        assert_eq!(
            DeploymentIdentity::parse("multi-user", Some("because")).expect_err("nothing would read it"),
            InvalidDeploymentIdentity::ReasonWithoutSingleUser
        );
        let multi = DeploymentIdentity::parse("multi-user", None).expect("multi-user needs nothing else");
        assert_eq!(multi, DeploymentIdentity::SubjectPerRequest);
        assert_eq!(multi.shared_witness(), None);
        assert!(multi.needs_per_source_acknowledgement());

        // And a third word is not a third mode.
        let unknown = DeploymentIdentity::parse("impersonating", None).expect_err("there are two modes");
        assert!(matches!(unknown, InvalidDeploymentIdentity::Unknown { .. }));
        let rendered = unknown.to_string();
        assert!(rendered.contains("security.identity"), "{rendered}");

        // Every listed spelling parses, so `NAMES` cannot offer a mode the parser refuses. The reason is
        // supplied for exactly the mode that requires one, which is what makes this a round trip rather
        // than a loop that only exercises one arm.
        for name in DeploymentIdentity::NAMES {
            let reason = (*name == "single-user").then_some("a stated reason");
            assert_eq!(
                DeploymentIdentity::parse(name, reason)
                    .expect("a listed name parses")
                    .as_str(),
                *name
            );
        }
    }

    #[test]
    fn a_token_the_authorization_header_could_not_carry_is_refused_at_startup() {
        // THE bug this variant exists for. Thirty-two characters, so the length floor is satisfied,
        // and not one of them is representable in a header value - `HeaderValue::to_str` accepts
        // visible ASCII only. Without this the process starts, every request is a 401, and nothing
        // in the log connects the two.
        //
        // Written with an escape rather than the character itself because `clippy::non_ascii_literal`
        // is on: an invisible byte in a source literal is exactly what that lint is for.
        let unrepresentable = "\u{e9}".repeat(AccessToken::MIN_LENGTH);
        assert_eq!(unrepresentable.chars().count(), AccessToken::MIN_LENGTH);
        assert_eq!(
            AccessToken::parse(&unrepresentable).expect_err("a token no header can carry is not a token"),
            InvalidAccessToken::NotRepresentableOnTheWire { position: 0 }
        );
    }

    #[test]
    fn a_control_character_inside_a_token_is_refused_and_the_position_is_named() {
        // The interior case, which the length and trim checks both pass: a newline in the middle of
        // a pasted token is a copy-paste artefact that no request could present either.
        let interior = String::from("0123456789abcdef\u{1}23456789abcdef0");
        assert_eq!(interior.chars().count(), AccessToken::MIN_LENGTH);
        assert_eq!(
            AccessToken::parse(&interior).expect_err("an interior control character is not a token"),
            InvalidAccessToken::NotRepresentableOnTheWire { position: 16 }
        );
        // And the value is not in the message, which is the rule every variant here obeys.
        let rendered = AccessToken::parse(&interior)
            .expect_err("an interior control character is not a token")
            .to_string();
        assert!(!rendered.contains(&interior), "{rendered}");
    }

    #[test]
    fn the_b64token_alphabet_is_accepted_and_padding_is_only_a_suffix() {
        // The positive side, without which every assertion above is satisfied by refusing
        // everything. Base64 with either alphabet, and a hex token, are what an operator generates.
        for good in [
            "0123456789abcdef0123456789abcdef",
            "abcdefghijklmnopqrstuvwxyz-._~+/",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        ] {
            assert!(AccessToken::parse(good).is_ok(), "{good} should be a token");
        }
        // Padding in the middle is not `b64token`, and a token with a `=` in it is one some
        // manifest or shell will split.
        let interior_padding = "AAAAAAAAAAAAAAAA=BBBBBBBBBBBBBBB";
        assert_eq!(
            AccessToken::parse(interior_padding).expect_err("interior padding is not a token"),
            InvalidAccessToken::NotRepresentableOnTheWire { position: 17 }
        );
        // A space is representable in a header and is refused anyway - see `wire_grammar`.
        let spaced = "0123456789abcdef 123456789abcdef";
        assert!(matches!(
            AccessToken::parse(spaced),
            Err(InvalidAccessToken::NotRepresentableOnTheWire { position: 16 })
        ));
    }

    #[test]
    fn a_termination_declaration_round_trips_and_says_what_crosses_in_cleartext() {
        for name in TlsTermination::NAMES {
            let parsed = TlsTermination::parse(name).expect("a listed name parses");
            assert_eq!(parsed.as_str(), *name);
            // Every declaration says something about the hop, and only one of them says there is
            // none. That sentence is what the startup log prints, so it is asserted here rather
            // than trusted.
            assert!(!parsed.cleartext_hop().is_empty());
        }
        assert_eq!(TlsTermination::default(), TlsTermination::None);
        assert!(!TlsTermination::None.is_declared());
        assert!(TlsTermination::Ingress.is_declared());
        assert!(!TlsTermination::Ingress.terminates_here());
        assert!(TlsTermination::InProcess.terminates_here());
        assert!(TlsTermination::InProcess.cleartext_hop().contains("no cleartext hop"));
        assert!(TlsTermination::Ingress.cleartext_hop().contains("pod network"));
        TlsTermination::parse("tls").unwrap_err();
    }
}
