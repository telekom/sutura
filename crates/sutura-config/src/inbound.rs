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
//! `direct` may also name a fronting component's own `behind-gateway` entry, with `token_header` and
//! `requires_gateway`: the caller still authenticates with their own token, read from `token_header`
//! instead of `Authorization`, and the named entry's assertion must also verify, in full, in
//! `Authorization` on the same request - `github.com/telekom/sutura#2089`. See
//! `sutura_http::InboundGate`'s module documentation for how that pairing is the one explicit
//! exception to refusing two configured locations at once, and what it does not bind.
//!
//! So the mode is a required key **inside** the declaration, and the declaration as a whole is
//! optional. Those are two different absences and the difference matters:
//!
//! | Written | What it means |
//! | --- | --- |
//! | no `security.inbound` block at all | this is a single-player deployment. There is no per-caller identity to establish and the bearer token authenticates the deployment, so no key of this block becomes required. `sutura serve` starts `/v1` that way only with `security.identity: single-user` and its reason, and over no `impersonation-at-source` source. `docs/adr/0008` part 5a calls that a first-class shape rather than a degraded one |
//! | a block with no `mode` | a deployment that meant to establish identity and did not say how. It does not start |
//!
//! **What this does NOT deliver, and it must not be read as delivered:** leg 1 proves who is asking.
//! It does *not* make a data source execute as that person - that is leg 2, and it needs a credential
//! per leg plus a source that declares it can impersonate. A deployment with leg 1 and no leg 2 knows
//! who is asking while a shared-identity source still reads rows as one identity. `BigQuery`'s
//! per-subject path is built, but no observed served run proves source acceptance.
//! [`InboundIdentity::what_it_does_not_do`] states that limit at startup, for the same reason
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
//! particular request is not built**: nothing hashes a method, a path or a body into the proof, so
//! inside the lifetime window an intercepted assertion replays - unless the deployment declares
//! [`TransitProof::SINGLE_USE_KEY`], which refuses a second presentation per process and never a
//! first one by the wrong party (`docs/adr/20261015094920`). That is why
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

mod address_claim;
mod audience;
mod declaration;
pub mod google;
mod key_set;
mod lifetime;
mod primitive;
mod request_path;
mod scope;
mod token_bytes;

pub use crate::inbound::address_claim::{AddressClaim, ClaimName};
pub use crate::inbound::audience::{ClientId, TokenAudience};
pub use crate::inbound::declaration::{InboundVerifier, InvalidMcpPath, McpPath};
pub use crate::inbound::key_set::{KeySetLocation, KeySetMaxStale, KeySetUrl};
pub use crate::inbound::lifetime::{ProofLifetime, TokenType};
pub use crate::inbound::primitive::{
    InvalidAlgorithms, InvalidInboundValue, IssuerUrl, KeyFamily, KeySetFile, PinnedAlgorithms, ProofHeader, ResourceIdentifier,
    SigningAlgorithm,
};
pub use crate::inbound::request_path::RequestPathPrefix;
pub use crate::inbound::scope::{ExtraScopeClaims, GrantedScopes};
pub use crate::inbound::token_bytes::MaxTokenBytes;

/// Which class of token this deployment will accept, out of the `typ` header.
///
/// **Two variants because the check has to be switchable and must not be switchable by silence.** The
/// finding it answers is cross-JWT substitution: without it, any JWT the issuer signed with this
/// audience verifies, an OIDC ID token included whenever the resource identifier equals the client id.
/// So [`Self::Exactly`] is the default in the `direct` mode - RFC 9068's `at+jwt` - and turning it off
/// is a value an operator writes, `any`, which the startup log prints at `WARN`. In `direct` it does
/// not start without `accept_any_token_type: true` beside it.
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
    /// **Not a default anywhere.** In the `direct` mode it is written as `token_type: "any"` plus
    /// `accept_any_token_type: true`; in `behind-gateway` as `transit_token_type: "any"` alone, where
    /// it is also the only way to say "this component sets no `typ`", so an opt-in would be a second
    /// spelling of the same word. Either way [`InboundIdentity::type_check`] renders a sentence the
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
/// An OAuth 2.1 client puts its access token in `Authorization: Bearer` and has no option to do
/// otherwise. A fronting component either sets a header of its own or replaces the caller's token in
/// `Authorization: Bearer` with its assertion. Both `Authorization` variants collide with the
/// deployment bearer token, and `NotFitToServe::DeploymentTokenSharesTheHeader` refuses either one
/// beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenLocation<'header> {
    /// `Authorization: Bearer <token>`, read by the resource server itself. Where RFC 6750 puts an
    /// access token, so this is the one location a `Bearer` challenge and RFC 9728 metadata describe.
    AuthorizationBearer,
    /// `Authorization: Bearer <assertion>`, set by a fronting component that replaced the caller's
    /// token. A separate variant from [`Self::AuthorizationBearer`] because the component's issuer is
    /// not an authorization server a client can use: no challenge and no metadata point a client at it.
    GatewayBearer,
    /// A named header whose whole value is the token. No scheme prefix: a component setting its own
    /// header has no reason to wrap the value, and a prefix nobody agreed on is a parse to get wrong.
    Header { name: &'header ProofHeader },
}

impl core::fmt::Display for TokenLocation<'_> {
    /// For the startup log, so an operator can see which header this deployment will read.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::AuthorizationBearer | Self::GatewayBearer => f.write_str("authorization: Bearer"),
            Self::Header { name } => write!(f, "{name}"),
        }
    }
}

/// What binds a gateway-signed assertion to the provider it was minted for.
///
/// **Two variants because one gateway shape has no `aud` to check.** The ordinary case is
/// [`Self::Audience`]: `aud` must equal a configured value, the same unconditional comparison
/// [`InboundIdentity::Direct`] makes. Some gateways route every provider behind one issuer with one
/// assertion shape and give a provider no way to ask for an `aud` of its own; what binds the
/// assertion to a particular provider there is the route the gateway dispatched on, carried in a
/// `requestPath` claim - [`Self::RequestPath`] is that alternative, and `absent_because` is the
/// written acknowledgement that this deployment chose it deliberately rather than by omission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitBinding {
    /// `aud` must equal this, byte for byte.
    Audience(TokenAudience),
    /// No `aud` claim exists to check: `requestPath` must lie under `prefix` instead (see [`RequestPathPrefix::matches`]).
    RequestPath {
        prefix: RequestPathPrefix,
        /// Why `transit_audience` is absent - `sutura_domain::source::AcknowledgementReason`'s
        /// pattern, required rather than a flag because the reason is what a reviewer needs.
        absent_because: sutura_domain::source::AcknowledgementReason,
    },
}

impl TransitBinding {
    /// The key this value is written under, where it is [`Self::Audience`] - the other spelling.
    pub const AUDIENCE_ABSENT_BECAUSE_KEY: &'static str = "security.inbound.transit_audience_absent_because";
}

/// What a fronting component has to prove on every request.
///
/// Every field is a validation input, and that is the point of the type: there is no field here that
/// holds an asserted identity. See this module's documentation for why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitProof {
    max_token_bytes: MaxTokenBytes,
    header: ProofHeader,
    issuer: IssuerUrl,
    binding: TransitBinding,
    key_set: KeySetLocation,
    algorithms: PinnedAlgorithms,
    /// The class of token the component emits. See [`RequiredTokenType`].
    token_type: RequiredTokenType,
    /// The ceiling this deployment puts on a lifetime the component chose. See [`ProofLifetime`].
    max_lifetime: ProofLifetime,
    /// What an assertion with no `scope` claim is granted. See [`GrantedScopes`].
    grant: Option<GrantedScopes>,
    /// Whether each assertion is admitted once, by its `jti`. See [`TransitProof::SINGLE_USE_KEY`].
    single_use: bool,
    /// Which claim names the caller's address. See [`AddressClaim`].
    address_claim: AddressClaim,
}

impl TransitProof {
    /// Sets the parsed token bound for this assertion verifier.
    pub(crate) fn limiting_token_bytes(self, max_token_bytes: MaxTokenBytes) -> Self {
        Self { max_token_bytes, ..self }
    }

    /// Assembles a declaration from parts that have each already been parsed.
    ///
    /// Seven arguments, over `clippy.toml`'s threshold of five, and taken rather than grouped
    /// deliberately: a parts struct would need public fields, which `cargo xtask check-boundaries`
    /// refuses on a public struct in a library crate - for the reason it exists, that a public field is
    /// a second way to build a value without its invariant. Every one of these is already a parsed
    /// newtype, so the list is seven invariants rather than seven strings.
    #[inline]
    #[must_use]
    pub const fn new(
        header: ProofHeader,
        issuer: IssuerUrl,
        binding: TransitBinding,
        key_set: KeySetLocation,
        algorithms: PinnedAlgorithms,
        token_type: RequiredTokenType,
        max_lifetime: ProofLifetime,
    ) -> Self {
        Self {
            max_token_bytes: MaxTokenBytes::DEFAULT,
            header,
            issuer,
            binding,
            key_set,
            algorithms,
            token_type,
            max_lifetime,
            grant: None,
            single_use: false,
            address_claim: AddressClaim::VerifiedEmail,
        }
    }

    /// The key that declares each assertion single use - `docs/adr/20261015094920`. Off by default:
    /// a component that reuses one assertion across requests would have every request after the
    /// first refused, and only the operator knows how the component mints.
    pub const SINGLE_USE_KEY: &'static str = "security.inbound.transit_single_use";

    /// The same declaration, admitting each assertion once by its `jti` where `single_use` is `true`.
    #[must_use]
    pub fn admitting_once(self, single_use: bool) -> Self {
        Self { single_use, ..self }
    }

    /// The same declaration, reading the caller's address from `address_claim`.
    #[must_use]
    pub fn reading_address_from(self, address_claim: AddressClaim) -> Self {
        Self { address_claim, ..self }
    }

    /// The same declaration, granting `grant` to an assertion that carries no `scope` claim.
    #[must_use]
    pub fn granting_when_absent(self, grant: GrantedScopes) -> Self {
        Self {
            grant: Some(grant),
            ..self
        }
    }

    /// The header the proof arrives in.
    #[inline]
    #[must_use]
    pub const fn header(&self) -> &ProofHeader {
        &self.header
    }
}

/// A `direct` verifier's own token header, paired with the id of the `behind-gateway` entry whose
/// own assertion must also be present, in `Authorization`, on the same request -
/// `github.com/telekom/sutura#2089`.
///
/// **What this holds: one complete declaration, never a half-written one.** `token_header` and
/// `requires_gateway` used to be two independent `Option` fields on [`InboundIdentity::Direct`],
/// and the rule that they come together was held only by
/// `crate::settings::inbound::parse_inbound`'s
/// [`crate::settings::SettingsError::RequiresGatewayPairIncomplete`] refusal - not by the type.
/// Code building the variant directly, rather than through the parser, could write one field
/// `Some` and the other `None`; [`InboundIdentity::requirement`] would then read the caller's
/// token from that header with **no** gateway assertion required, which is exactly the shape
/// `github.com/telekom/sutura#2089`'s review ruled out. There is no public field and the only
/// constructor is [`Self::new`], which takes both - so that shape is unrepresentable rather than
/// merely refused. The parser's refusal is unchanged: it still refuses one key written without
/// the other, before either reaches this type.
///
/// The fields are private, so a literal naming both is refused outright rather than only by
/// convention - distinct from [`InboundIdentity`]'s own `compile_fail,E0559` doctest, which holds
/// the CARRIER (`Direct` has no `token_header` or `requires_gateway` field to name any more, so
/// that shape fails before this type's privacy is ever reached):
///
/// ```compile_fail,E0451
/// use sutura_config::{GatewayPairing, ProofHeader};
/// use sutura_domain::identity::VerifierId;
///
/// fn _literal(header: ProofHeader, requires_gateway: VerifierId) -> GatewayPairing {
///     GatewayPairing { header, requires_gateway }
/// }
/// ```
///
/// ```
/// use sutura_config::{GatewayPairing, ProofHeader};
/// use sutura_domain::identity::VerifierId;
///
/// fn _constructed(header: ProofHeader, requires_gateway: VerifierId) -> GatewayPairing {
///     GatewayPairing::new(header, requires_gateway)
/// }
/// ```
///
/// **What this does not do: enforce the check itself.** Holding a complete pairing says what a
/// caller must also present; nothing about holding one makes anything verify it at request time.
/// That runs only where a gate builder reads [`Self::requires_gateway`] and attaches the named
/// target - `sutura_http::InboundGate::from_verifiers_with_outbound` does, for every declared
/// verifier it is handed. A builder that sees only this one declaration cannot resolve the target
/// at all, so it refuses to build (`sutura_http`'s `InboundNotUsable::PairedAlone`) rather than
/// returning a gate that silently checks nothing beyond the caller's own token -
/// `github.com/telekom/sutura#2096`.
///
/// **What this does not check: that the pairing is admissible.** A `GatewayPairing` can name a
/// target id this type has no way to see - absent, not `behind-gateway`, not reading
/// `Authorization`, or colliding with another verifier's own header. Those are checked only at
/// **load time**, over every declared verifier at once: `crate::settings::admission::verifier_refusals`
/// pushes a `NotFitToServe::RequiresGateway*` refusal for each, before a deployment built from
/// settings this crate loaded can start.
///
/// A declaration assembled directly in Rust, rather than parsed from settings, is not bound by
/// that load-time check - which is exactly why `sutura_http::InboundGate`'s own gate builders
/// enforce a second, independent of it, at **build time**: that `requires_gateway` names a
/// declared verifier at all (`InboundNotUsable::RequiresGatewayTargetMissing`); that the paired
/// declaration still has a built gate, by issuer, to attach the target to
/// (`InboundNotUsable::RequiresGatewaySlotMissing`); that no two of the declarations handed to
/// `InboundGate::from_verifiers_with_outbound` - paired or not, standalone or themselves a nested
/// target - share an issuer (`InboundNotUsable::DuplicateVerifierIssuer`); and that a builder
/// seeing only one declaration refuses rather than silently building a pairing it cannot resolve
/// (`InboundNotUsable::PairedAlone`). What the gate builders do **not** re-check is the target's
/// mode, location or header: a collision there stays a load-time-only concern, decided once by
/// the `NotFitToServe::RequiresGateway*` refusals above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayPairing {
    header: ProofHeader,
    requires_gateway: sutura_domain::identity::VerifierId,
}

impl GatewayPairing {
    /// Pairs the header the caller's own token arrives in with the id of the `behind-gateway`
    /// entry whose own assertion this pairing also requires.
    #[inline]
    #[must_use]
    pub const fn new(header: ProofHeader, requires_gateway: sutura_domain::identity::VerifierId) -> Self {
        Self {
            header,
            requires_gateway,
        }
    }

    /// The header the caller's own token arrives in, instead of `Authorization`.
    #[inline]
    #[must_use]
    pub const fn header(&self) -> &ProofHeader {
        &self.header
    }

    /// The id of the `behind-gateway` entry whose own assertion must also be present, in
    /// `Authorization`, on the same request. The one named exception to the rule that a request
    /// presenting credentials in two configured locations is refused - see
    /// `crate::settings::admission::verifier_refusals` for what makes the pairing admissible, and
    /// `sutura_http::InboundGate` for how the second check never becomes the caller.
    #[inline]
    #[must_use]
    pub const fn requires_gateway(&self) -> &sutura_domain::identity::VerifierId {
        &self.requires_gateway
    }
}

/// How the identity of a caller reaches this deployment. Printed at startup, per deployment.
///
/// A closed enum with a required key, in the shape
/// [`TlsTermination`](crate::security::TlsTermination) already uses here - and for the same reason
/// `Environment` is a parsed enum rather than a string with a fallback: the value that decides a
/// posture must not be satisfiable by silence.
///
/// **`Direct`'s own gateway pairing is one field, [`GatewayPairing`], and not the two independent
/// `Option`s it replaced - `github.com/telekom/sutura#2096`.** Nothing here names a
/// `token_header` or `requires_gateway` field on this variant any more, so the old construction
/// does not compile against this type:
///
/// ```compile_fail,E0559
/// use sutura_config::{
///     AddressClaim, InboundIdentity, IssuerUrl, KeySetFile, KeySetLocation, PinnedAlgorithms, ProofHeader,
///     RequiredTokenType, ResourceIdentifier, SigningAlgorithm,
/// };
///
/// // `Direct` has no `token_header` or `requires_gateway` field - #2096 replaced both with `pairing`.
/// fn _half_paired(header: ProofHeader) -> InboundIdentity {
///     InboundIdentity::Direct {
///         resource: ResourceIdentifier::parse("https://example.com/mcp").expect("a doctest resource is a resource"),
///         authorization_server: IssuerUrl::parse("https://issuer.example.com").expect("a doctest issuer is an issuer"),
///         key_set: KeySetLocation::File(KeySetFile::parse("/keys.json").expect("a doctest path is a path")),
///         algorithms: PinnedAlgorithms::of(SigningAlgorithm::Es256),
///         token_type: RequiredTokenType::access_token(),
///         address_claim: AddressClaim::VerifiedEmail,
///         token_header: Some(header),
///         requires_gateway: None,
///     }
/// }
/// ```
///
/// And the twin, so a rename cannot make that block pass vacuously - the complete pairing arrives
/// as one [`GatewayPairing`], built by [`GatewayPairing::new`]:
///
/// ```
/// use sutura_config::{
///     AddressClaim, GatewayPairing, InboundIdentity, IssuerUrl, KeySetFile, KeySetLocation, PinnedAlgorithms,
///     ProofHeader, RequiredTokenType, ResourceIdentifier, SigningAlgorithm,
/// };
/// use sutura_domain::identity::VerifierId;
///
/// fn _paired(header: ProofHeader, target: VerifierId) -> InboundIdentity {
///     InboundIdentity::Direct {
///         resource: ResourceIdentifier::parse("https://example.com/mcp").expect("a doctest resource is a resource"),
///         authorization_server: IssuerUrl::parse("https://issuer.example.com").expect("a doctest issuer is an issuer"),
///         key_set: KeySetLocation::File(KeySetFile::parse("/keys.json").expect("a doctest path is a path")),
///         algorithms: PinnedAlgorithms::of(SigningAlgorithm::Es256),
///         token_type: RequiredTokenType::access_token(),
///         address_claim: AddressClaim::VerifiedEmail,
///         pairing: Some(GatewayPairing::new(header, target)),
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundIdentity {
    /// This deployment is the resource server. It validates the caller's token itself: signature,
    /// issuer, expiry, and an audience matching its own resource identifier.
    Direct {
        resource: ResourceIdentifier,
        authorization_server: IssuerUrl,
        key_set: KeySetLocation,
        algorithms: PinnedAlgorithms,
        /// Which class of token, out of the `typ` header. Defaults to RFC 9068's `at+jwt`.
        token_type: RequiredTokenType,
        /// Which claim names the caller's address. See [`AddressClaim`].
        address_claim: AddressClaim,
        /// The paired gateway requirement, where this verifier's own token header is paired with a
        /// `behind-gateway` entry - `github.com/telekom/sutura#2089`. See [`GatewayPairing`] for why
        /// a half-written pairing is unrepresentable rather than merely refused.
        pairing: Option<GatewayPairing>,
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
    max_token_bytes: MaxTokenBytes,
    location: TokenLocation<'inbound>,
    /// Who authenticated the caller - the `InboundIdentity` variant this requirement came from,
    /// carried explicitly rather than inferred from `location` or `resource`: a custom header is
    /// not on its own evidence of either mode, since `direct` paired with `requires_gateway`
    /// (`#2089`) and `behind-gateway` both read a token from one.
    mode: sutura_domain::identity::VerifierMode,
    issuer: &'inbound IssuerUrl,
    /// `aud` must equal this. `None` only where [`TransitBinding::RequestPath`] binds the assertion
    /// by its request path instead - see [`Self::request_path_prefix`].
    audience: Option<&'inbound TokenAudience>,
    /// `requestPath` must lie under this (see [`RequestPathPrefix::matches`]). `Some` only where [`TransitBinding::RequestPath`] is
    /// configured, which is the only shape with no [`Self::audience`].
    request_path_prefix: Option<&'inbound RequestPathPrefix>,
    /// The identifier this deployment publishes. `Some` in `direct` only.
    resource: Option<&'inbound ResourceIdentifier>,
    /// What a token with no `scope` claim is granted. `Some` behind a gateway that declares one.
    grant: Option<&'inbound GrantedScopes>,
    extra_scope_claims: ExtraScopeClaims,
    key_set: &'inbound KeySetLocation,
    algorithms: &'inbound PinnedAlgorithms,
    token_type: &'inbound RequiredTokenType,
    /// The ceiling on `exp - iat`, where this deployment puts one.
    ///
    /// `None` in the `direct` mode, and that asymmetry is the decision rather than an omission: an
    /// access token's lifetime is the authorization server's to choose and a ceiling here would refuse
    /// tokens an issuer minted correctly. In `behind-gateway` there is a ceiling because the record
    /// calls the assertion short-lived, and [`ProofLifetime::parse`] enforces it: a configured value
    /// above [`ProofLifetime::MAX_SECONDS`] is refused at boot as
    /// [`InvalidInboundValue::LifetimeOutOfRange`]. The ceiling bounds the replay window of a captured
    /// assertion, not the assertion's binding to a request - see the module documentation.
    max_lifetime: Option<ProofLifetime>,
    /// Whether each assertion is admitted once. `false` in `direct`, where `true` is refused at load.
    single_use: bool,
    address_claim: &'inbound AddressClaim,
}

impl<'inbound> TokenRequirement<'inbound> {
    /// The configured bound, before any JWT decoding or key lookup.
    #[must_use]
    pub const fn max_token_bytes(&self) -> MaxTokenBytes {
        self.max_token_bytes
    }

    /// Where the token arrives.
    #[inline]
    #[must_use]
    pub const fn location(&self) -> TokenLocation<'inbound> {
        self.location
    }

    /// Who authenticated the caller - the mode a [`sutura_domain::identity::AdmittingVerifier`]
    /// is built with. Carried from the `InboundIdentity` variant, never inferred from `location`.
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> sutura_domain::identity::VerifierMode {
        self.mode
    }

    /// The issuer the `iss` claim must equal, byte for byte.
    #[inline]
    #[must_use]
    pub const fn issuer(&self) -> &'inbound IssuerUrl {
        self.issuer
    }

    /// The value the `aud` claim must contain, byte for byte - `None` only where
    /// [`Self::request_path_prefix`] is `Some` instead.
    ///
    /// **Unconditional in every other shape, and that is the security decision in `docs/adr/0014`.**
    /// A client may send a resource indicator asking its authorization server for a narrower token;
    /// that is welcome and it is an optimisation. It is never what makes the token safe, and this
    /// check is not skippable when the indicator is absent. The one declared exception is
    /// [`TransitBinding::RequestPath`], where there is no `aud` claim an operator could configure one
    /// of these against - see that variant's documentation.
    #[inline]
    #[must_use]
    pub const fn audience(&self) -> Option<&'inbound TokenAudience> {
        self.audience
    }

    /// The prefix a `requestPath` claim must lie under - `Some` only where [`Self::audience`] is
    /// `None`, because [`TransitBinding::RequestPath`] is the only configuration with no `aud` to
    /// check and this is what binds the assertion to this provider instead.
    #[inline]
    #[must_use]
    pub const fn request_path_prefix(&self) -> Option<&'inbound RequestPathPrefix> {
        self.request_path_prefix
    }

    /// The identifier RFC 9728's metadata and a challenge name, whatever `aud` is. `None` behind a
    /// gateway, which publishes none.
    #[inline]
    #[must_use]
    pub const fn resource(&self) -> Option<&'inbound ResourceIdentifier> {
        self.resource
    }

    /// What a token with no `scope` claim is granted - never added to one that has a `scope`.
    #[inline]
    #[must_use]
    pub const fn grant(&self) -> Option<&'inbound GrantedScopes> {
        self.grant
    }

    /// Which claims beside `scope` grant scopes.
    #[inline]
    #[must_use]
    pub const fn extra_scope_claims(&self) -> ExtraScopeClaims {
        self.extra_scope_claims
    }

    /// Where the signing keys are read from.
    #[inline]
    #[must_use]
    pub const fn key_set(&self) -> &'inbound KeySetLocation {
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

    /// Whether each assertion is admitted once, by its `jti`. See [`TransitProof::SINGLE_USE_KEY`].
    #[inline]
    #[must_use]
    pub const fn single_use(&self) -> bool {
        self.single_use
    }

    /// Which claim names the caller's address.
    #[inline]
    #[must_use]
    pub const fn address_claim(&self) -> &'inbound AddressClaim {
        self.address_claim
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
        "this establishes WHO is asking; it does not prove that a data source executes as that \
         person. BigQuery can carry the caller's verified assertion through its declared \
         per-subject account map, but no served run has yet shown the source accepting that \
         identity. Other sources execute under their declared shared identity. A question cannot \
         execute without a credential for its source, and an undeclared BigQuery subject is refused \
         rather than answered as this process"
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
                ref address_claim,
                ref pairing,
            } => TokenRequirement {
                // Where RFC 6750 puts it, and where an OAuth 2.1 client has no option but to put it -
                // unless `pairing` names a different one. That is what makes `security.access_token`
                // and the default and this mode a collision on one header, refused at startup - see
                // `NotFitToServe::DeploymentTokenSharesTheHeader`.
                max_token_bytes: resource.max_token_bytes(),
                location: match *pairing {
                    Some(ref pairing) => TokenLocation::Header { name: &pairing.header },
                    None => TokenLocation::AuthorizationBearer,
                },
                mode: sutura_domain::identity::VerifierMode::Direct,
                issuer: authorization_server,
                audience: Some(resource.audience()),
                request_path_prefix: None,
                resource: Some(resource),
                grant: None,
                extra_scope_claims: resource.extra_scope_claims(),
                key_set,
                algorithms,
                token_type,
                single_use: false,
                address_claim,
                // No ceiling: an access token's lifetime belongs to the authorization server. See the
                // field on `TokenRequirement` for why the two modes differ here.
                max_lifetime: None,
            },
            Self::BehindGateway { ref transit } => {
                let (audience, request_path_prefix) = match &transit.binding {
                    TransitBinding::Audience(audience) => (Some(audience), None),
                    TransitBinding::RequestPath { prefix, .. } => (None, Some(prefix)),
                };
                TokenRequirement {
                    max_token_bytes: transit.max_token_bytes,
                    location: if transit.header.is_authorization() {
                        TokenLocation::GatewayBearer
                    } else {
                        TokenLocation::Header { name: &transit.header }
                    },
                    mode: sutura_domain::identity::VerifierMode::Gateway,
                    issuer: &transit.issuer,
                    audience,
                    request_path_prefix,
                    resource: None,
                    grant: transit.grant.as_ref(),
                    extra_scope_claims: ExtraScopeClaims::none(),
                    key_set: &transit.key_set,
                    algorithms: &transit.algorithms,
                    token_type: &transit.token_type,
                    max_lifetime: Some(transit.max_lifetime),
                    single_use: transit.single_use,
                    address_claim: &transit.address_claim,
                }
            }
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
    /// the variant. The `match` is exhaustive, so a location added later has to answer it.
    #[must_use]
    pub const fn reads_the_authorization_header(&self) -> bool {
        match self.requirement().location() {
            TokenLocation::AuthorizationBearer | TokenLocation::GatewayBearer => true,
            TokenLocation::Header { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests;
