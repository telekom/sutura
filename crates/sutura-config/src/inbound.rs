//! How the identity of a caller reaches this deployment - leg 1, and the one fact that decides it.
//!
//! `docs/adr/0014` decides two inbound modes and says plainly that **neither of them is a default**.
//! A deployment either *is* the resource server and validates the caller's token itself, or it sits
//! behind a component that already authenticated the caller and validates a proof the request
//! transited that component. Both defaults are wrong in opposite directions: defaulting to
//! [`InboundIdentity::Direct`] makes a gateway deployment reject every caller, and defaulting to
//! [`InboundIdentity::BehindGateway`] makes a directly exposed deployment accept a forged proof.
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
//! **The limit, stated next to the claim:** under `BehindGateway` this deployment trusts the
//! component's *authentication of the caller*, because that is what the mode means. What it does not
//! trust is a string. The signature says the claims came from the component; nothing here can say the
//! component authenticated correctly, and no configuration could.
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
    InvalidAlgorithms, InvalidInboundValue, IssuerUrl, KeyFamily, KeySetFile, PinnedAlgorithms, ProofHeader, ResourceIdentifier,
    SigningAlgorithm,
};

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
}

impl TransitProof {
    /// Assembles a declaration from parts that have each already been parsed.
    #[inline]
    #[must_use]
    pub const fn new(
        header: ProofHeader,
        issuer: IssuerUrl,
        audience: ResourceIdentifier,
        key_set: KeySetFile,
        algorithms: PinnedAlgorithms,
    ) -> Self {
        Self {
            header,
            issuer,
            audience,
            key_set,
            algorithms,
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
    },
    /// A fronting component authenticated the caller. This deployment validates a short-lived proof
    /// that the request transited that component, and derives the subject from the claims of that
    /// proof rather than from a string somebody set.
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
            } => TokenRequirement {
                // Where RFC 6750 puts it, and where an OAuth 2.1 client has no option but to put it.
                // That is what makes `security.access_token` and this mode a collision on one
                // header, refused at startup - see `NotFitToServe::DeploymentTokenSharesTheHeader`.
                location: TokenLocation::AuthorizationBearer,
                issuer: authorization_server,
                audience: resource,
                key_set,
                algorithms,
            },
            Self::BehindGateway { ref transit } => TokenRequirement {
                location: TokenLocation::Header { name: &transit.header },
                issuer: &transit.issuer,
                audience: &transit.audience,
                key_set: &transit.key_set,
                algorithms: &transit.algorithms,
            },
        }
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
