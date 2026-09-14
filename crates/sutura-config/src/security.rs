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
use sutura_domain::source::{AcknowledgementReason, InvalidOperatorText};

use crate::identity_cache::CredentialCacheSettings;
use crate::inbound::InboundIdentity;

/// A pre-shared secret a caller presents to reach the service.
///
/// The configured token is reduced to its SHA-256 digest at parse and nothing else is retained, so
/// the whole settings tree can be written to the startup log with `Debug` and the token cannot
/// come out with it. `Debug` prints a placeholder for the same reason `Secret`'s does.
///
/// **Not comparable with `==`.** `AccessToken` implements no `PartialEq`: a derived comparison on
/// credential material returns on the first differing byte, which is a timing oracle at whatever
/// call site adds it. The comparison lives here instead, once, as
/// [`AccessToken::matches_in_constant_time`].
#[derive(Clone)]
pub struct AccessToken {
    /// SHA-256 of the configured token, computed once at parse so the raw value is not retained.
    digest: [u8; 32],
}

impl core::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("AccessToken(REDACTED)")
    }
}

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
    /// Longer than [`AccessToken::MAX_LENGTH`].
    ///
    /// An unbounded configured string is an availability surface, and a bearer token has no reason
    /// to reach this size. Carries the length and never the value, as [`Self::TooShort`] does.
    #[error("an access token is at most {limit} characters and this one is {found}")]
    TooLong { found: usize, limit: usize },
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

    /// The longest token accepted.
    ///
    /// A ceiling, not a strength estimate: an operator-generated bearer token has no reason to
    /// reach this size, and an unbounded configured string is an availability surface. It is the
    /// far side of [`Self::MIN_LENGTH`] and shares its framing - a bound on a value the operator
    /// generates, decided so a misconfiguration is refused at startup rather than accepted.
    pub const MAX_LENGTH: usize = 1024;

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
        if found > Self::MAX_LENGTH {
            return Err(InvalidAccessToken::TooLong {
                found,
                limit: Self::MAX_LENGTH,
            });
        }
        Self::wire_grammar(raw)?;
        // Digest at parse: the configured token is reduced to its SHA-256 before it is stored, so
        // the raw value is not retained after boot and a request comparison hashes only the
        // presented value. An operator-generated high-entropy token is exactly the case where a
        // plain digest is a safe stand-in - `matches_in_constant_time` does not need to recover
        // the token, and nothing here is stored for an attacker to crack offline.
        let digest = sha2::Sha256::digest(raw.as_bytes()).into();
        Ok(Self { digest })
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
    /// **The expected side is the digest stored at parse, and the presented side is hashed here.**
    /// `subtle` compares equal-length byte slices without branching, which removes the
    /// early-return oracle - but comparing the raw strings would still have to decide what to do
    /// about differing lengths, and every answer to that leaks the length before it leaks anything
    /// else. Reducing both sides to a fixed 32 bytes removes the question: every comparison is over
    /// the same number of bytes whatever arrived, and the configured token is never re-hashed per
    /// request because it is already a digest.
    ///
    /// What this still does not do: it is not a password hash. There is no salt and no work
    /// factor, because the input is a high-entropy secret an operator generated rather than
    /// something a person chose, and nothing here is stored for an attacker to find offline.
    pub fn matches_in_constant_time(&self, presented: &str) -> bool {
        let actual = sha2::Sha256::digest(presented.as_bytes());
        self.digest.ct_eq(&actual).into()
    }

    /// Whether this token is the same as another configured token.
    ///
    /// **The one comparison two configured credentials need, and it is not the value-comparison a
    /// `PartialEq` would be.** Both sides are already digests of at-rest configuration, so neither
    /// is an attacker-presented value arriving at a timing-sensitive boundary; comparing them at
    /// boot with `subtle`'s constant-time equality keeps even that much out. It exists because
    /// `docs/adr/0015` Decision 1 refuses a metrics token equal to the API token, and the refusal
    /// needs the two digests compared once, at startup.
    pub fn equals(&self, other: &Self) -> bool {
        self.digest.ct_eq(&other.digest).into()
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

/// The deployment-wide outbound trust declaration - `security.outbound`.
///
/// **Distinct from a per-source `transport_anchors`** (`crate::sources::transport::TrustAnchors`),
/// and deliberately a second, smaller type rather than the same one reused: a per-source declaration
/// is refused when the source's own kind has no dial target the anchor could attach to
/// (`crate::sources::refuse_foreign_keys` on `files`/`bigquery`), and the outbound clients this
/// settles - the `BigQuery` wire, the STS token exchange - dial a HOST THAT IS A COMPILE-TIME CONSTANT.
/// There is no source entry a per-entry `transport_anchors` on a `bigquery` kind could mean anything
/// on, which is exactly why #125 keeps that refusal rather than lifting it: a deployment-wide
/// declaration is the shape that has something to attach to. See `docs/adr/0010`'s amendment.
///
/// **Anchors only - no client identity.** Every fixed-host client this covers takes a bearer token,
/// not a certificate, so a `ClientIdentity` field here would be a shape nothing exercises.
///
/// Absent `security.outbound` is not a refusal, unlike a source that asks for TLS and names no
/// anchors: these clients always speak TLS regardless of configuration, and an absent block means
/// "verify against `ureq`'s own compiled-in roots", which is today's (and every prior release's)
/// behaviour. A PRESENT block with no `transport_anchors` IS a refusal - see
/// [`InvalidOutbound::NoAnchors`] - because a block that names nothing declares nothing, the same
/// argument `security.inbound` with no `mode` already makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundAnchors {
    /// A PEM bundle at this absolute path.
    Bundle(std::path::PathBuf),
    /// The host's own trust store, chosen by name.
    System,
}

/// Why a `security.outbound` declaration was not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidOutbound {
    /// `security.outbound` was written with no `transport_anchors`.
    #[error(
        "`security.outbound` is declared and names no `transport_anchors` - a block that names no \
         trust store declares nothing. Say which authority signs the fixed-host endpoints this \
         deployment reaches (a PEM bundle path, or `system`), or remove the block"
    )]
    NoAnchors,
    /// A `security.outbound.transport_anchors` path that is relative.
    #[error(
        "`security.outbound.transport_anchors` is `{path}`, which is relative and resolves against \
         this process's working directory - a different directory on every host. Write an absolute path"
    )]
    RelativePath { path: std::path::PathBuf },
}

/// Reads the outbound trust declaration from its one written field.
///
/// `None` means `security.outbound` was absent, which is not a refusal - see
/// [`OutboundAnchors`]'s own doc for why. `Some(None)` (a present block, no field written) is what
/// reaches this function as `Some("")`/`Some(None)` from an empty or unset `transport_anchors`, and
/// it is [`InvalidOutbound::NoAnchors`].
pub(crate) fn parse_outbound(anchors: Option<&str>) -> Result<OutboundAnchors, InvalidOutbound> {
    match anchors.map(str::trim).filter(|text| !text.is_empty()) {
        Some("system") => Ok(OutboundAnchors::System),
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            if path.is_relative() {
                return Err(InvalidOutbound::RelativePath { path });
            }
            Ok(OutboundAnchors::Bundle(path))
        }
        None => Err(InvalidOutbound::NoAnchors),
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
///
/// **Five fields now, with the last three answering separate deployment questions.**
/// [`Self::inbound`] is how the identity of a *caller* reaches this deployment; [`Self::identity`] is
/// who a query then runs *as*; [`Self::metrics_token`] independently gates `/metrics`. **Neither
/// inbound nor identity implies the other, and that is the fact worth writing down rather than the
/// count:** a deployment can verify exactly who is asking and still read every row under one
/// configured identity, because leg 2 - a credential per execution leg - is not built. The reverse
/// holds too, and is the shape that ships: a single-user deployment with no inbound block knows what
/// a query runs as and nothing about who asked. Omitting the metrics token is likewise an explicit
/// choice not to gate that route; it does not alter either query authentication or caller identity.
///
/// The inbound declaration lives in this group rather than one of its own because of
/// [`Self::describes_identity`]: that function used to be a constant answering `false`, and a
/// deployment that establishes a caller identity has to be able to make it answer otherwise from a
/// value rather than from a rewrite. The deployment declaration is an `Option` for a different
/// reason - it has no default and its absence is a refusal rather than a value; see
/// [`DeploymentIdentity`], which explains why no combination of source postures may answer it on the
/// operator's behalf.
#[derive(Debug, Clone, Default)]
pub struct SecuritySettings {
    access_token: Option<AccessToken>,
    tls_termination: TlsTermination,
    inbound: Option<InboundIdentity>,
    identity: Option<DeploymentIdentity>,
    metrics_token: Option<AccessToken>,
    /// `security.credential_cache` - `docs/adr/0031`. Never an `Option`: the group is infallible
    /// once parsed and off is a value of it, the same shape `ToolsSettings` uses for a capability
    /// nobody turned on.
    credential_cache: CredentialCacheSettings,
    outbound: Option<OutboundAnchors>,
}

impl SecuritySettings {
    /// Assembles the group from parts that have each already been parsed.
    ///
    /// The inbound declaration is an `Option` because its absence is a posture rather than a gap: a
    /// deployment that establishes no per-caller identity is a single-player deployment, which
    /// `docs/adr/0008` part 5a calls a first-class shape. What is *not* optional is saying which mode,
    /// once a block exists at all - and that refusal lives in `crate::settings::parse_inbound`,
    /// because the shape here cannot hold "a mode nobody named".
    ///
    /// The metrics token is an `Option` the same way: a deployment that chooses not to gate
    /// `/metrics` is making a posture, not leaving a gap.
    ///
    /// `outbound` is `None` for the ordinary deployment - see [`OutboundAnchors`]'s own doc for why
    /// that is not a gap either.
    #[inline]
    pub const fn new(
        access_token: Option<AccessToken>,
        tls_termination: TlsTermination,
        inbound: Option<InboundIdentity>,
        identity: Option<DeploymentIdentity>,
        metrics_token: Option<AccessToken>,
        credential_cache: CredentialCacheSettings,
        outbound: Option<OutboundAnchors>,
    ) -> Self {
        Self {
            access_token,
            tls_termination,
            inbound,
            identity,
            metrics_token,
            credential_cache,
            outbound,
        }
    }

    /// The exchanged-credential cache's own settings - `docs/adr/0031`.
    #[inline]
    #[must_use]
    pub const fn credential_cache(&self) -> CredentialCacheSettings {
        self.credential_cache
    }

    /// The deployment-wide trust anchors a fixed-host outbound client verifies against, if declared.
    ///
    /// `None` means every such client verifies against its own compiled-in roots - see
    /// [`OutboundAnchors`]. A composition root reads this once at boot and hands the resolved
    /// material to `WireAgent::secured` (or its equivalent) rather than each call site reading
    /// settings for itself.
    #[inline]
    #[must_use]
    pub const fn outbound(&self) -> Option<&OutboundAnchors> {
        self.outbound.as_ref()
    }

    /// The token that gates `/metrics`, when one is configured.
    #[inline]
    pub const fn metrics_token(&self) -> Option<&AccessToken> {
        self.metrics_token.as_ref()
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
    /// **[`Self::identity`] does not change this answer either, and that is deliberate.** A declared
    /// `multi-user` mode says what the deployment *intends* and decides where a shared source's
    /// acknowledgement has to be written; it does not make a caller identity arrive. Reading the
    /// declaration back as "this deployment knows who is asking" is the exact confusion this function
    /// exists to prevent, and the two keys are independent for that reason.
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
mod tests;
