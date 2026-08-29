//! How the identity of a caller reaches this deployment - leg 1, and the one fact that decides it.
//!
//! `docs/adr/0014` decides two inbound modes and says plainly that **neither of them is a default**.
//! A deployment either *is* the resource server and validates the caller's token itself, or it sits
//! behind a component that already authenticated the caller and validates a short-lived **identity
//! assertion that component signed**. Both defaults are wrong in opposite directions: defaulting to
//! [`InboundIdentity::Direct`] makes a gateway deployment reject every caller, and defaulting to
//! [`InboundIdentity::BehindGateway`] makes a directly exposed deployment accept an assertion anybody
//! can mint.
//!
//! So the mode is a required key **inside** the declaration, and the declaration as a whole is
//! optional. Those are two different absences and the difference matters:
//!
//! | Written | What it means |
//! | --- | --- |
//! | no `security.inbound` block at all | this is a single-player deployment. There is no per-caller identity to establish, the bearer token authenticates the deployment, and nothing here becomes required. `docs/adr/0008` part 5a calls that a first-class shape rather than a degraded one |
//! | a block with no `mode` | a deployment that meant to establish identity and did not say how. It does not start |
//!
//! **What this does NOT deliver, and it must not be read as delivered:** leg 1 proves who is asking.
//! It does *not* make a data source execute as that person - that is leg 2, and it needs a credential
//! per leg plus a source that declares it can impersonate. A deployment with leg 1 and no leg 2 knows
//! who is asking and still reads every row as one identity. [`InboundIdentity::what_it_does_not_do`]
//! is that sentence as a value, printed at startup, for the same reason
//! [`TlsTermination::cleartext_hop`](crate::security::TlsTermination::cleartext_hop) is one: a log
//! line and this documentation read the same string, so neither can drift into claiming per-user
//! access because there is authentication.
//!
//! # `BehindGateway` does not mean "trust a header", and the type is what stops it meaning that
//!
//! A component asserting an identity in a header is not authentication - anything that can reach the
//! port can write that header, and the failure is invisible in a diff: a header named
//! `x-authenticated-user` that means "authenticated" because of where it is *expected* to come from.
//!
//! What [`TransitProof`] carries is therefore not a header holding a name. It is a header holding a
//! **signed token**, with an issuer, an audience, a key set and a pinned algorithm - the same four
//! things [`InboundIdentity::Direct`] validates - and the subject is derived by us from the claims of
//! a token whose signature checked out. There is no shape in this module that could hold "the name of
//! the header the username is in".
//!
//! **The limits, stated next to the claim, and there are three.** Under `BehindGateway` this
//! deployment trusts the component's *authentication of the caller*, because that is what the mode
//! means; what it does not trust is a string. The signature says the claims came from the component,
//! and nothing here can say the component authenticated correctly.
//!
//! And what a signed assertion proves is that **the component issued it**, not that *this request*
//! carried it there first. Review found the wording overstating exactly that: a proof was replayable
//! for as long as its `exp` allowed, and its `exp` was the component's to choose. Two of those three
//! are now bounded - [`ProofLifetime`] caps `exp - iat` and an `iat` is required, so the replay window
//! is a number this deployment chose rather than one it was handed. **Binding an assertion to a
//! particular request is not built**: there is no nonce store and nothing hashes a method, a path or a
//! body into the proof, so inside the lifetime window an intercepted assertion replays. That is why
//! this module and `docs/adr/0014` now call it a *gateway-issued identity assertion* rather than a
//! proof that the request transited anything, and why the trusted transport boundary - the hop between
//! the component and this process - is load-bearing rather than incidental.
//!
//! # One derived view, two named modes
//!
//! `docs/adr/0014` says the difference between the modes is "one fact rather than two code paths".
//! [`InboundIdentity::requirement`] is that sentence made mechanical: the enum keeps the two names an
//! operator writes and a reviewer reads, and the validator downstream consumes a single
//! [`TokenRequirement`] borrowed out of whichever variant is configured. There is one validator, so
//! there is one place algorithm pinning and the audience check can be got wrong.

mod primitive;

pub use crate::inbound::primitive::{
    InvalidAlgorithms, InvalidInboundValue, IssuerUrl, KeyFamily, KeySetFile, PinnedAlgorithms, ProofHeader, ProofLifetime,
    ResourceIdentifier, SigningAlgorithm, TokenType,
};

/// Which class of token this deployment will accept, out of the `typ` header.
///
/// **Two variants because the check has to be switchable and must not be switchable by silence.** The
/// finding it answers is cross-JWT substitution: without it, any JWT the issuer signed with this
/// audience verifies, an OIDC ID token included whenever the resource identifier equals the client id.
/// So [`Self::Exactly`] is the default in the `direct` mode - RFC 9068's `at+jwt` - and turning it off
/// is a value an operator writes, `any`, which the startup log prints at `WARN`.
///
/// There is no `Option<TokenType>` here, for the reason `docs/adr/0014` gives about `mode`: an absent
/// value reads as "not configured yet" at every call site, and the one thing that has to be legible is
/// whether a deployment decided to accept every class of token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequiredTokenType {
    /// A token whose `typ` is this, compared after the signature verified.
    Exactly { typ: TokenType },
    /// Any class of token the issuer signed for this audience.
    ///
    /// **Not a default anywhere.** In the `direct` mode it is written as `token_type: "any"`; in
    /// `behind-gateway` as `transit_token_type: "any"`, where it is also the only way to say "this
    /// component sets no `typ`". Either way [`InboundIdentity::type_check`] renders a sentence the
    /// startup log prints, so a deployment running with it is visible on every boot.
    Any,
}

impl RequiredTokenType {
    /// Reads the configured word: `any`, or a media type.
    ///
    /// `any` is compared on the folded value, so `Any` and `ANY` are the same answer - and a deployment
    /// whose component really does emit a `typ` of `any` cannot express it. That collision is worth
    /// having: the word is checked before the media type precisely so that turning the check off cannot
    /// happen by accident.
    pub fn parse(key: &'static str, raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        let raw = raw.as_ref();
        if raw.trim().eq_ignore_ascii_case(TokenType::ANY) {
            return Ok(Self::Any);
        }
        TokenType::parse(key, raw).map(|typ| Self::Exactly { typ })
    }

    /// RFC 9068's access-token type. The `direct` mode's default.
    #[must_use]
    pub fn access_token() -> Self {
        Self::Exactly {
            typ: TokenType::access_token(),
        }
    }

    /// Does a presented `typ` satisfy this?
    ///
    /// `None` is an ABSENT `typ` header, and it satisfies nothing but [`Self::Any`]: a token carrying no
    /// type is exactly the shape a class check exists to refuse, and treating absence as acceptable
    /// would make the check satisfiable by omission.
    #[must_use]
    pub fn accepts(&self, presented: Option<&TokenType>) -> bool {
        match *self {
            Self::Any => true,
            Self::Exactly { ref typ } => presented == Some(typ),
        }
    }

    /// What is required, as a word for a log line and for a refusal message.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match *self {
            Self::Any => TokenType::ANY,
            Self::Exactly { ref typ } => typ.as_str(),
        }
    }
}

impl core::fmt::Display for RequiredTokenType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where the token being validated arrives.
///
/// Two variants because the two modes put it in two places, and neither is a preference: an OAuth
/// 2.1 client puts its access token in `Authorization: Bearer` and has no option to do otherwise,
/// while a fronting component sets a header of its own and would collide with the deployment bearer
/// token if it used that one. [`ProofHeader::parse`] refuses `authorization` for exactly that reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenLocation<'header> {
    /// `Authorization: Bearer <token>`. Where RFC 6750 puts an access token.
    AuthorizationBearer,
    /// A named header whose whole value is the token. No scheme prefix: a component setting its own
    /// header has no reason to wrap the value, and a prefix nobody agreed on is a parse to get wrong.
    Header { name: &'header ProofHeader },
}

impl core::fmt::Display for TokenLocation<'_> {
    /// For the startup log, so an operator can see which header this deployment will read.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::AuthorizationBearer => f.write_str("authorization: Bearer"),
            Self::Header { name } => write!(f, "{name}"),
        }
    }
}

/// What a fronting component has to prove on every request.
///
/// Every field is a validation input, and that is the point of the type: there is no field here that
/// holds an asserted identity. See this module's documentation for why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitProof {
    header: ProofHeader,
    issuer: IssuerUrl,
    audience: ResourceIdentifier,
    key_set: KeySetFile,
    algorithms: PinnedAlgorithms,
    /// The class of token the component emits. See [`RequiredTokenType`].
    token_type: RequiredTokenType,
    /// The ceiling this deployment puts on a lifetime the component chose. See [`ProofLifetime`].
    max_lifetime: ProofLifetime,
}

impl TransitProof {
    /// Assembles a declaration from parts that have each already been parsed.
    ///
    /// Seven arguments, over `clippy.toml`'s threshold of five, and taken rather than grouped
    /// deliberately: a parts struct would need public fields, which `cargo xtask check-boundaries`
    /// refuses on a public struct in a library crate - for the reason it exists, that a public field is
    /// a second way to build a value without its invariant. Every one of these is already a parsed
    /// newtype, so the list is seven invariants rather than seven strings.
    #[expect(
        clippy::too_many_arguments,
        reason = "a parts struct would need public fields, which check-boundaries refuses; each argument is an already-parsed newtype"
    )]
    #[inline]
    #[must_use]
    pub const fn new(
        header: ProofHeader,
        issuer: IssuerUrl,
        audience: ResourceIdentifier,
        key_set: KeySetFile,
        algorithms: PinnedAlgorithms,
        token_type: RequiredTokenType,
        max_lifetime: ProofLifetime,
    ) -> Self {
        Self {
            header,
            issuer,
            audience,
            key_set,
            algorithms,
            token_type,
            max_lifetime,
        }
    }

    /// The header the proof arrives in.
    #[inline]
    #[must_use]
    pub const fn header(&self) -> &ProofHeader {
        &self.header
    }
}

/// How the identity of a caller reaches this deployment. Printed at startup, per deployment.
///
/// A closed enum with a required key, in the shape
/// [`TlsTermination`](crate::security::TlsTermination) already uses here - and for the same reason
/// `Environment` is a parsed enum rather than a string with a fallback: the value that decides a
/// posture must not be satisfiable by silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundIdentity {
    /// This deployment is the resource server. It validates the caller's token itself: signature,
    /// issuer, expiry, and an audience matching its own resource identifier.
    Direct {
        resource: ResourceIdentifier,
        authorization_server: IssuerUrl,
        key_set: KeySetFile,
        algorithms: PinnedAlgorithms,
        /// Which class of token, out of the `typ` header. Defaults to RFC 9068's `at+jwt`.
        token_type: RequiredTokenType,
    },
    /// A fronting component authenticated the caller. This deployment validates a **signed identity
    /// assertion** the component issued, and derives the subject from that assertion's own claims
    /// rather than from a string somebody set.
    ///
    /// **The wording used to say "a short-lived proof that the request transited that component", and
    /// review showed the code did not deliver either half.** The lifetime was the component's to choose
    /// and nothing capped it, and nothing bound an assertion to a request - so replaying the identical
    /// token worked for as long as its `exp` allowed. `docs/adr/0014` now says the same thing this doc
    /// comment does; the lifetime half is fixed by [`ProofLifetime`], and the binding half is a stated
    /// limit rather than a claim.
    BehindGateway { transit: TransitProof },
}

/// The whole validation this deployment performs, borrowed out of whichever mode is configured.
///
/// **A view and not a second configuration surface.** There is no constructor: the only way to one
/// of these is [`InboundIdentity::requirement`], so the validator cannot be handed a requirement
/// that does not correspond to a declaration an operator wrote and this crate refused or accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenRequirement<'inbound> {
    location: TokenLocation<'inbound>,
    issuer: &'inbound IssuerUrl,
    audience: &'inbound ResourceIdentifier,
    key_set: &'inbound KeySetFile,
    algorithms: &'inbound PinnedAlgorithms,
    token_type: &'inbound RequiredTokenType,
    /// The ceiling on `exp - iat`, where this deployment puts one.
    ///
    /// `None` in the `direct` mode, and that asymmetry is the decision rather than an omission: an
    /// access token's lifetime is the authorization server's to choose and a ceiling here would refuse
    /// tokens an issuer minted correctly. In `behind-gateway` there is a ceiling because the record
    /// calls the assertion short-lived, and a claim nothing enforces is what review found.
    max_lifetime: Option<ProofLifetime>,
}

impl<'inbound> TokenRequirement<'inbound> {
    /// Where the token arrives.
    #[inline]
    #[must_use]
    pub const fn location(&self) -> TokenLocation<'inbound> {
        self.location
    }

    /// The issuer the `iss` claim must equal, byte for byte.
    #[inline]
    #[must_use]
    pub const fn issuer(&self) -> &'inbound IssuerUrl {
        self.issuer
    }

    /// The value the `aud` claim must contain, byte for byte.
    ///
    /// **Unconditional, and that is the security decision in `docs/adr/0014`.** A client may send a
    /// resource indicator asking its authorization server for a narrower token; that is welcome and
    /// it is an optimisation. It is never what makes the token safe, and this check is not skippable
    /// when the indicator is absent.
    #[inline]
    #[must_use]
    pub const fn audience(&self) -> &'inbound ResourceIdentifier {
        self.audience
    }

    /// Where the signing keys are read from.
    #[inline]
    #[must_use]
    pub const fn key_set(&self) -> &'inbound KeySetFile {
        self.key_set
    }

    /// The algorithms this deployment will accept - never the one in the token's own header.
    #[inline]
    #[must_use]
    pub const fn algorithms(&self) -> &'inbound PinnedAlgorithms {
        self.algorithms
    }

    /// Which class of token, out of the `typ` header, checked after the signature verified.
    #[inline]
    #[must_use]
    pub const fn token_type(&self) -> &'inbound RequiredTokenType {
        self.token_type
    }

    /// The ceiling on `exp - iat`, where this deployment puts one. See the field.
    #[inline]
    #[must_use]
    pub const fn max_lifetime(&self) -> Option<ProofLifetime> {
        self.max_lifetime
    }
}

impl InboundIdentity {
    /// Every accepted spelling of the mode, so a refusal message and the parser cannot disagree.
    pub const MODES: &'static [&'static str] = &["direct", "behind-gateway"];

    /// The mode, as a stable word for the startup log and for a record field.
    ///
    /// A `&'static str` from an exhaustive match rather than a `Display` of something structured,
    /// because it has to be a value a query over logs can group by.
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> &'static str {
        match *self {
            Self::Direct { .. } => "direct",
            Self::BehindGateway { .. } => "behind-gateway",
        }
    }

    /// Who authenticated the caller, as a sentence for the startup log.
    ///
    /// A function rather than a comment for the reason
    /// [`TlsTermination::cleartext_hop`](crate::security::TlsTermination::cleartext_hop) is one: the
    /// log, this documentation and the type read the same value, so none of them can drift into
    /// claiming more than the mode does.
    #[inline]
    #[must_use]
    pub const fn who_authenticated(&self) -> &'static str {
        match *self {
            Self::Direct { .. } => {
                "this deployment is the resource server: it verifies the caller's token itself, \
                 against its own resource identifier and a pinned algorithm"
            }
            Self::BehindGateway { .. } => {
                "a fronting component authenticated the caller; this deployment verifies a signed \
                 proof that the request transited it and derives the subject from that proof's own \
                 claims"
            }
        }
    }

    /// The sentence that keeps leg 1 from being read as leg 2.
    ///
    /// The same for both modes, deliberately: `docs/adr/0014`'s table says the mode changes who
    /// authenticates the caller and changes **nothing** about who is responsible for the chain or for
    /// leg 2. A constant rather than a `match` would have said that less clearly than a function
    /// whose whole body is one string does.
    #[inline]
    #[must_use]
    pub const fn what_it_does_not_do() -> &'static str {
        "this establishes WHO is asking. It does not make a data source execute as that person: \
         that is leg 2, it needs a credential per leg and a source that declares it can \
         impersonate, and none of it is built - so every question is still answered with whatever \
         access this process already had"
    }

    /// The one validation this deployment performs, whichever mode it is in.
    ///
    /// See this module's documentation: the two modes are one fact and not two code paths, so there
    /// is one validator and one place the audience check can be got wrong.
    #[must_use]
    pub const fn requirement(&self) -> TokenRequirement<'_> {
        match *self {
            Self::Direct {
                ref resource,
                ref authorization_server,
                ref key_set,
                ref algorithms,
                ref token_type,
            } => TokenRequirement {
                // Where RFC 6750 puts it, and where an OAuth 2.1 client has no option but to put it.
                // That is what makes `security.access_token` and this mode a collision on one
                // header, refused at startup - see `NotFitToServe::DeploymentTokenSharesTheHeader`.
                location: TokenLocation::AuthorizationBearer,
                issuer: authorization_server,
                audience: resource,
                key_set,
                algorithms,
                token_type,
                // No ceiling: an access token's lifetime belongs to the authorization server. See the
                // field on `TokenRequirement` for why the two modes differ here.
                max_lifetime: None,
            },
            Self::BehindGateway { ref transit } => TokenRequirement {
                location: TokenLocation::Header { name: &transit.header },
                issuer: &transit.issuer,
                audience: &transit.audience,
                key_set: &transit.key_set,
                algorithms: &transit.algorithms,
                token_type: &transit.token_type,
                max_lifetime: Some(transit.max_lifetime),
            },
        }
    }

    /// What the `typ` check does on this deployment, as a sentence for the startup log.
    ///
    /// **A sentence rather than a boolean, because the interesting value is the one that reads as
    /// nothing.** A deployment that wrote `any` has switched off the check that stops an OIDC ID token
    /// from establishing a caller, and `type_check = "any"` on a log line does not say that. This does,
    /// and `crate::security::SecuritySettings` prints it at `WARN`.
    #[must_use]
    pub const fn type_check(&self) -> &'static str {
        match *self.requirement().token_type() {
            RequiredTokenType::Exactly { .. } => {
                "a token of another class - an OIDC ID token, most of all - is refused even when its \
                 issuer and audience match"
            }
            RequiredTokenType::Any => {
                "ANY class of token this issuer signed for this audience is accepted, an OIDC ID token \
                 included wherever the resource identifier is also a client id. That is a deployment \
                 decision and it is written down as `any`"
            }
        }
    }

    /// Is the class check switched off?
    ///
    /// Read by the startup log to decide the level, so the answer is a value rather than a comparison
    /// somebody writes at the call site.
    #[must_use]
    pub const fn accepts_any_token_class(&self) -> bool {
        matches!(*self.requirement().token_type(), RequiredTokenType::Any)
    }

    /// Does this deployment read the deployment bearer token's own header?
    ///
    /// The one question a refusal needs answered, and it is asked of the *requirement* rather than of
    /// the variant, so a mode added later that also lands in `Authorization` cannot slip past it.
    #[must_use]
    pub const fn reads_the_authorization_header(&self) -> bool {
        matches!(self.requirement().location(), TokenLocation::AuthorizationBearer)
    }
}

#[cfg(test)]
mod tests;
