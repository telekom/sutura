//! The access control this service has, and the honest name for what each part of it is not.
//!
//! **Two controls that answer two different questions, and collapsing them is the mistake this
//! module is arranged against:**
//!
//! - the [`AccessToken`]: *may this caller reach this service at all*
//! - [`InboundIdentity`]: *who is asking*
//!
//! What an [`AccessToken`] does is narrower than authentication, and the narrowness is the point: a
//! presented token proves the caller holds a secret the deployment was configured with. It proves
//! nothing about *which* caller, it cannot be scoped, it cannot be revoked for one party without
//! revoking it for all of them, and it does not reach the data system. It is worth having anyway,
//! because the alternative on a non-loopback interface is an unauthenticated way to read whatever the
//! process can read - and it is not worth mistaking for identity.
//!
//! [`SecuritySettings::describes_identity`] is what keeps that distinction printable. It used to be
//! an associated function that always answered `false`; `crate::inbound` is what made it a value, and
//! the startup log prints it on every boot so an operator cannot deploy either shape believing it is
//! the other.
//!
//! **The limit that survives all of it:** a caller whose identity is established is still a caller
//! whose questions run with whatever access this process already had.
//! [`InboundIdentity::what_it_does_not_do`] is that sentence, and the startup log prints it beside the
//! mode rather than leaving a reader to infer it.

use sha2::Digest as _;
use subtle::ConstantTimeEq as _;
use sutura_domain::identity::Secret;

use crate::inbound::InboundIdentity;

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
/// **Three fields now, and the third is the one that changes what the other two mean.**
/// [`Self::inbound`] is how the identity of a *caller* reaches this deployment, and the whole reason
/// it lives here rather than in a group of its own is [`Self::describes_identity`]: that function used
/// to be a constant answering `false`, and a deployment that establishes a caller identity has to be
/// able to make it answer otherwise from a value rather than from a rewrite.
#[derive(Debug, Clone, Default)]
pub struct SecuritySettings {
    access_token: Option<AccessToken>,
    tls_termination: TlsTermination,
    inbound: Option<InboundIdentity>,
}

impl SecuritySettings {
    /// Assembles the group from parts that have each already been parsed.
    ///
    /// The inbound declaration is an `Option` because its absence is a posture rather than a gap: a
    /// deployment that establishes no per-caller identity is a single-player deployment, which
    /// `docs/adr/0008` part 5a calls a first-class shape. What is *not* optional is saying which mode,
    /// once a block exists at all - and that refusal lives in `crate::settings::parse_inbound`,
    /// because the shape here cannot hold "a mode nobody named".
    #[inline]
    pub const fn new(
        access_token: Option<AccessToken>,
        tls_termination: TlsTermination,
        inbound: Option<InboundIdentity>,
    ) -> Self {
        Self {
            access_token,
            tls_termination,
            inbound,
        }
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

    /// How the identity of a caller reaches this deployment, if it does.
    ///
    /// `None` is the shape that ships today and the shape a single-player deployment keeps: the
    /// bearer token authenticates the deployment, and there is no per-request identity to establish.
    #[inline]
    pub const fn inbound(&self) -> Option<&InboundIdentity> {
        self.inbound.as_ref()
    }

    /// Does anything here establish who the caller is?
    ///
    /// **This stopped being a constant, which is the change `docs/adr/0014` predicted.** It was an
    /// associated function that always answered `false`, with a comment saying it would change when a
    /// request context and an inbound credential existed. They exist, so it reads a value: `true`
    /// exactly when an inbound declaration is configured, and `false` for the deployment token alone -
    /// which authenticates the deployment and not the caller, whatever else is set.
    ///
    /// It is still a function rather than a comment so the startup log and this documentation read
    /// the same value.
    ///
    /// **The limit, next to the claim:** `true` here says a caller's identity is *established*. It
    /// does not say a data source executes as that caller - see
    /// [`InboundIdentity::what_it_does_not_do`], which the startup log prints beside this.
    #[inline]
    pub const fn describes_identity(&self) -> bool {
        self.inbound.is_some()
    }

    /// Which mode establishes the caller's identity, as a word for the startup log.
    ///
    /// `"none"` rather than an `Option` because the caller is a log line, and a field that is
    /// sometimes absent reads as a field that is sometimes broken.
    #[inline]
    pub const fn inbound_mode(&self) -> &'static str {
        match self.inbound {
            Some(ref inbound) => inbound.mode(),
            None => "none",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::inbound::{IssuerUrl, KeySetFile, PinnedAlgorithms, ResourceIdentifier, SigningAlgorithm};

    use super::{AccessToken, InboundIdentity, InvalidAccessToken, SecuritySettings, TlsTermination};

    /// Thirty-two characters, which is the floor.
    const GOOD: &str = "0123456789abcdef0123456789abcdef";

    /// A deployment that is its own resource server. The narrowest declaration that exists.
    fn direct() -> InboundIdentity {
        InboundIdentity::Direct {
            resource: ResourceIdentifier::parse("https://sutura.example.com").expect("a test resource is a resource"),
            authorization_server: IssuerUrl::parse("https://issuer.example.com").expect("a test issuer is an issuer"),
            key_set: KeySetFile::parse("/etc/sutura/jwks.json").expect("a test path is a path"),
            algorithms: PinnedAlgorithms::of(SigningAlgorithm::Rs256),
            token_type: crate::inbound::RequiredTokenType::access_token(),
        }
    }

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
            None,
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
    fn a_shared_token_does_not_claim_to_know_who_the_caller_is_and_an_inbound_declaration_does() {
        // The distinction the startup log prints, and the reason `describes_identity` stopped being a
        // constant. A token - in EVERY termination posture, which is why one is set here - proves the
        // caller holds a secret an operator distributed, and says nothing about which caller.
        let with_token = SecuritySettings::new(
            Some(AccessToken::parse(GOOD).expect("a valid token")),
            TlsTermination::Ingress,
            None,
        );
        let without = SecuritySettings::default();
        assert!(!with_token.describes_identity(), "a shared token is not an identity");
        assert!(!without.describes_identity());
        assert_eq!(with_token.token_state(), "configured");
        assert_eq!(without.token_state(), "absent");
        assert_eq!(with_token.inbound_mode(), "none");

        // And the other half, which is what makes the assertions above load-bearing rather than a
        // tautology about a constant: a deployment that validates a caller's token DOES establish an
        // identity, and the same function says so.
        let verifying = SecuritySettings::new(None, TlsTermination::Ingress, Some(direct()));
        assert!(verifying.describes_identity());
        assert_eq!(verifying.inbound_mode(), "direct");
        assert_eq!(verifying.token_state(), "absent");
        assert!(verifying.inbound().is_some());
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
