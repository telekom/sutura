//! The values an inbound-identity declaration is made of, each parsed rather than checked.
//!
//! Five newtypes and one enum, and every one of them exists because the value it holds is compared
//! against something a caller sent. That is the whole reason they are types: an audience claim is
//! compared **byte for byte** against [`ResourceIdentifier`], so anything that could make two
//! spellings of the same identifier compare unequal - or two different identifiers compare equal -
//! is a hole, and the place to close it is where the value comes into existence.
//!
//! # Why there is no URL parser here
//!
//! `url` is already in this workspace's lock file and is deliberately not a dependency of this
//! crate, because **a URL parser normalises and normalisation is the wrong behaviour for this
//! value.** `https://Example.com:443/` and `https://example.com/` are the same URL and are not the
//! same audience: RFC 8707 says a resource indicator is compared as a string, and an authorization
//! server puts into `aud` whatever it was configured with. A parser that lower-cased the host or
//! dropped a default port would make this deployment accept a token minted for a *different*
//! spelling than the one an operator wrote down, which is the opposite of what the check is for.
//!
//! So what happens instead is a parse in the strict sense: the accepted shape is an allowlist of
//! characters plus a required scheme plus two refused delimiters, and the value is stored exactly as
//! it was written. Nothing is folded, nothing is trimmed except the surrounding whitespace a YAML
//! file adds, and the refusals name a position rather than quoting the value back.
//!
//! # Why the algorithms are an enum with no symmetric variant
//!
//! `docs/adr/0014` asks for three things: pin the accepted algorithms, never read the algorithm out
//! of the token being validated, refuse `none`, and refuse a symmetric algorithm where an asymmetric
//! one is expected. The last two are **unrepresentable** here rather than checked:
//! [`SigningAlgorithm`] has no `None` variant and no `HS*` variant, so a configuration naming either
//! cannot produce a value. The refusals still name them one by one, because an operator who wrote
//! `HS256` needs to read *why* rather than "unknown algorithm".

use std::path::{Path, PathBuf};

/// The longest identifier, issuer or header name accepted.
///
/// Generous for a URL and far below anything that could be a payload. The bound is here because
/// every one of these values is read out of configuration into a startup log line, and because an
/// unbounded input is a denial-of-service primitive whatever else it is.
const MAX_LENGTH: usize = 512;

/// The scheme every identifier and issuer here must carry.
///
/// **`http://` is refused and that is not pedantry.** An issuer reached over cleartext is an issuer
/// whose key set an active network attacker chooses, and a resource identifier compared byte-exact
/// against a token's audience is a value that has to be the same everywhere - so allowing two
/// schemes would allow two spellings of one deployment.
const REQUIRED_SCHEME: &str = "https://";

/// Why a value in an inbound-identity declaration is not one.
///
/// One error for every newtype in this module, because they are one parse with different
/// vocabularies: the same five things can be wrong about each of them, and three hand-written copies
/// of that list is three things to keep in step.
///
/// **No variant quotes the whole value back.** A resource identifier is not a credential, but an
/// issuer URL and a header name are both deployment topology, and this crate's own rule is that an
/// error carries the typed context and not a rendering of the input. A position is what an operator
/// needs to find the character; the character itself is often the one that draws nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidInboundValue {
    /// Empty, or whitespace only.
    ///
    /// Empty is not absent. An unset variable arrives in a shell as `""`, and a key present and
    /// empty means somebody meant to write a value - so it is a refusal naming the key rather than
    /// a fall back to a default nobody chose.
    #[error("{key} is empty - write the value, or remove the whole inbound declaration")]
    Empty { key: &'static str },
    #[error("{key} is at most {limit} characters and this one is {found}")]
    TooLong { key: &'static str, found: usize, limit: usize },
    /// The scheme is missing or is not `https`.
    #[error(
        "{key} must be an absolute `https://` URI. An issuer reached over cleartext is an issuer \
         whose signing keys an active network attacker chooses, and an audience compared byte for \
         byte against a token claim must have exactly one spelling"
    )]
    NotHttps { key: &'static str },
    /// A `#` or a `?`.
    ///
    /// Both are refused for the same reason and it is not a style rule: a resource indicator with a
    /// query or a fragment is a value an authorization server may legitimately hand back **without**
    /// it, so the byte-exact audience comparison would fail against a token that was minted
    /// correctly. Refusing it at startup is a message; allowing it is every request answered `401`.
    #[error(
        "{key} carries a `{delimiter}` at position {position}. A resource identifier is compared \
         byte for byte against a token's audience, and an authorization server may drop a query or \
         a fragment when it mints one - so a value carrying either is one no token would match"
    )]
    HasDelimiter {
        key: &'static str,
        delimiter: char,
        position: usize,
    },
    /// A character outside the accepted set.
    ///
    /// The position and not the character, for the reason the module documentation gives: every
    /// invisible and direction-changing code point lands here, because all of them are outside an
    /// ASCII allowlist - and one of those printed into a message would print as though it were
    /// nothing at all.
    #[error(
        "{key} holds a character at position {position} (counting from zero) that does not belong \
         in this value. What is accepted is: {accepted}"
    )]
    NotPermitted {
        key: &'static str,
        position: usize,
        accepted: &'static str,
    },
    /// A header name that would collide with the deployment bearer token.
    ///
    /// **The one refusal in this list that is about a combination rather than a shape**, and it is
    /// here rather than in `NotFitToServe` because the value itself is what is wrong: a proof read
    /// out of `Authorization` is a proof read out of the header the deployment token already owns,
    /// and there is no deployment in which that is what somebody meant.
    #[error(
        "{key} is `{found}`, which is the header the deployment bearer token already arrives in. A \
         request cannot present two different credentials in one header - name a header of this \
         component's own"
    )]
    ReservedHeader { key: &'static str, found: String },
}

/// What this deployment calls itself when it validates an audience.
///
/// **This is the security decision in `docs/adr/0014` given a type.** A token is accepted only if
/// its audience matches this value. A client may also *ask* its authorization server for a token
/// scoped to this resource - RFC 8707's resource indicator - and that is welcome and is an
/// optimisation: it makes the token narrower before it ever arrives. It is never what makes the
/// token safe. The check is ours, it is unconditional, and it is not skippable when the client sent
/// no indicator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceIdentifier(String);

/// Where the tokens this deployment accepts are minted.
///
/// Compared against the `iss` claim, byte for byte, for the reason [`ResourceIdentifier`] is: an
/// issuer is a configured string on both sides and normalising ours would make it disagree with
/// theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuerUrl(String);

/// Where the signing keys are read from.
///
/// **A file and not a URL, and that gap is named rather than left to be discovered.** A JWKS
/// endpoint needs an outbound HTTP client, which is a supply-chain change with its own review and
/// its own failure mode - `docs/adr/0014` says plainly that the authorization server becomes a hard
/// runtime dependency and that an outage there must stay distinguishable from a dead data system.
/// None of that is built. What is built is the *rotation* mechanism: the key set is cached, refetched
/// when a key id is not in it, and that refetch is rate limited - and every one of those properties
/// is the same whether the source is a file a sidecar rewrites or an endpoint. See
/// `sutura_http::inbound::keys`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySetFile(PathBuf);

/// The header a transit proof arrives in.
///
/// Lower case, because that is what HTTP/2 puts on the wire and what a header map is keyed on here -
/// so folding it at construction is what makes a configured `X-Transit` and an arriving `x-transit`
/// the same header rather than a lookup that silently misses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofHeader(String);

/// A signature algorithm this deployment will accept.
///
/// **No `none` and no `HS*`, and their absence is the enforcement.** Algorithm confusion is the
/// classic direct-validation defect and it is silent when it works: a token signed with `HS256`
/// using the issuer's *public* key as the HMAC secret verifies, if the validator will accept a
/// symmetric algorithm. There is no variant here that could.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SigningAlgorithm {
    /// RSASSA-PKCS1-v1_5 with SHA-256. What most authorization servers issue.
    Rs256,
    Rs384,
    Rs512,
    /// RSASSA-PSS with SHA-256.
    Ps256,
    Ps384,
    Ps512,
    /// ECDSA over P-256 with SHA-256.
    Es256,
    /// ECDSA over P-384 with SHA-384.
    Es384,
    /// `EdDSA` over Ed25519.
    EdDsa,
}

/// Which kind of key verifies a [`SigningAlgorithm`].
///
/// Here rather than left implicit because **a pinned set spanning two families is a set that can
/// verify nothing**, and finding that out from a `401` is expensive. One key set holds keys of
/// whatever kinds the issuer publishes; one *token* is verified by one key with one algorithm, and
/// the validator this feeds refuses a permitted-algorithm list whose family disagrees with the key
/// it looked up. So the mixed list is refused at startup instead - see [`PinnedAlgorithms::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFamily {
    Rsa,
    EllipticCurve,
    EdwardsCurve,
}

impl KeyFamily {
    /// The spelling, for a refusal message.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rsa => "RSA",
            Self::EllipticCurve => "elliptic curve",
            Self::EdwardsCurve => "Edwards curve",
        }
    }
}

/// Why a pinned algorithm list is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAlgorithms {
    /// The list was empty.
    ///
    /// Refused rather than defaulted. A default here would be this crate deciding what an issuer
    /// signs with, and the whole point of pinning is that the deployment states it.
    #[error(
        "security.inbound.algorithms is empty. Pinning is the control: name the algorithm this \
         deployment's issuer signs with, so the algorithm in a token's own header can never be the \
         one that gets used"
    )]
    Empty,
    /// `none`.
    #[error(
        "`none` is not a signature algorithm - it is the absence of one, and a validator that \
         accepts it accepts a token anybody can write. It cannot be configured here"
    )]
    Unsigned,
    /// An `HS*` algorithm.
    #[error(
        "`{found}` is a symmetric algorithm and this is an asymmetric validation. Accepting one is \
         how algorithm confusion works: a token signed with the issuer's PUBLIC key as an HMAC \
         secret verifies, and the caller minted it themselves. Use an `RS*`, `PS*`, `ES*` or \
         `EdDSA` algorithm"
    )]
    Symmetric { found: String },
    #[error("`{found}` does not name a signature algorithm - one of: {}", SigningAlgorithm::NAMES.join(", "))]
    Unknown { found: String },
    /// Two families in one list.
    ///
    /// See [`KeyFamily`]: the validator verifies one token with one key, and it refuses a permitted
    /// list whose family disagrees with that key - so a mixed list is a configuration that fails
    /// every request. A restart is the cost of moving an issuer from RSA to elliptic curve, and that
    /// is cheaper than a deployment that starts and authenticates nobody.
    #[error(
        "security.inbound.algorithms names both a {first} algorithm (`{first_named}`) and a \
         {second} one (`{second_named}`). One token is verified by one key, and a permitted list \
         spanning two key kinds verifies nothing - pin the family this issuer signs with"
    )]
    MixedFamilies {
        first: &'static str,
        first_named: &'static str,
        second: &'static str,
        second_named: &'static str,
    },
}

/// The algorithms this deployment will accept, all of one family and at least one.
///
/// Non-emptiness and single-family-ness are both structural: the first link lives in its own field,
/// which is the shape `sutura_domain::identity::ActorChain` already uses for the same reason - there
/// is no state of this type that means "nothing is permitted", so nothing downstream has to check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedAlgorithms {
    /// The one that is always there.
    first: SigningAlgorithm,
    /// The rest, in the order they were written. Empty for a single pinned algorithm.
    rest: Vec<SigningAlgorithm>,
}

impl SigningAlgorithm {
    /// Every accepted spelling, so a refusal message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &[
        "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256", "ES384", "EdDSA",
    ];

    /// Reads one algorithm name.
    ///
    /// The order of the checks is the diagnostic and it is deliberate: `none` and `HS*` are named
    /// before the unknown-name refusal, because an operator who wrote either of those needs to read
    /// why it is refused rather than a list that does not contain it. Case is folded for the
    /// comparison and never stored - these are fixed identifiers from a registry, not values from
    /// this deployment.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidAlgorithms> {
        let raw = raw.as_ref().trim();
        let folded = raw.to_ascii_uppercase();
        if folded == "NONE" {
            return Err(InvalidAlgorithms::Unsigned);
        }
        if folded.starts_with("HS") {
            return Err(InvalidAlgorithms::Symmetric {
                found: String::from(raw),
            });
        }
        match folded.as_str() {
            "RS256" => Ok(Self::Rs256),
            "RS384" => Ok(Self::Rs384),
            "RS512" => Ok(Self::Rs512),
            "PS256" => Ok(Self::Ps256),
            "PS384" => Ok(Self::Ps384),
            "PS512" => Ok(Self::Ps512),
            "ES256" => Ok(Self::Es256),
            "ES384" => Ok(Self::Es384),
            "EDDSA" => Ok(Self::EdDsa),
            _ => Err(InvalidAlgorithms::Unknown {
                found: String::from(raw),
            }),
        }
    }

    /// The registry spelling, for the startup log and for the adapter that maps this onto its
    /// library's own enum.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rs256 => "RS256",
            Self::Rs384 => "RS384",
            Self::Rs512 => "RS512",
            Self::Ps256 => "PS256",
            Self::Ps384 => "PS384",
            Self::Ps512 => "PS512",
            Self::Es256 => "ES256",
            Self::Es384 => "ES384",
            Self::EdDsa => "EdDSA",
        }
    }

    /// Which kind of key verifies it.
    #[inline]
    pub const fn family(self) -> KeyFamily {
        match self {
            Self::Rs256 | Self::Rs384 | Self::Rs512 | Self::Ps256 | Self::Ps384 | Self::Ps512 => KeyFamily::Rsa,
            Self::Es256 | Self::Es384 => KeyFamily::EllipticCurve,
            Self::EdDsa => KeyFamily::EdwardsCurve,
        }
    }
}

impl TryFrom<String> for SigningAlgorithm {
    type Error = InvalidAlgorithms;

    /// Delegates to [`Self::parse`] rather than repeating it: one constructor stays the source of
    /// truth.
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for SigningAlgorithm {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PinnedAlgorithms {
    /// Reads the configured list.
    ///
    /// Refuses an empty list, every name [`SigningAlgorithm::parse`] refuses, and a list spanning
    /// two key families. A duplicate is *not* refused: naming `RS256` twice says the same thing
    /// once, and a refusal for it would be a refusal for a copy-paste rather than for a posture.
    pub fn parse(raw: &[String]) -> Result<Self, InvalidAlgorithms> {
        let mut parsed = Vec::with_capacity(raw.len());
        for name in raw {
            parsed.push(SigningAlgorithm::parse(name)?);
        }
        let Some((&first, rest)) = parsed.split_first() else {
            return Err(InvalidAlgorithms::Empty);
        };
        let family = first.family();
        if let Some(&other) = rest.iter().find(|candidate| candidate.family() != family) {
            return Err(InvalidAlgorithms::MixedFamilies {
                first: family.as_str(),
                first_named: first.as_str(),
                second: other.family().as_str(),
                second_named: other.as_str(),
            });
        }
        Ok(Self {
            first,
            rest: rest.to_vec(),
        })
    }

    /// One algorithm, for a caller assembling a declaration in code rather than from a file.
    #[inline]
    #[must_use]
    pub const fn of(algorithm: SigningAlgorithm) -> Self {
        Self {
            first: algorithm,
            rest: Vec::new(),
        }
    }

    /// The family every algorithm in this list belongs to. Infallible, because the type cannot hold
    /// two.
    #[inline]
    #[must_use]
    pub const fn family(&self) -> KeyFamily {
        self.first.family()
    }

    /// The algorithms, in the order they were written. At least one.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = SigningAlgorithm> + '_ {
        core::iter::once(self.first).chain(self.rest.iter().copied())
    }

    /// How many were pinned. At least one.
    ///
    /// Named `count` rather than `len` deliberately, for the reason
    /// `sutura_domain::identity::ActorChain::count` is: a `len` invites an `is_empty` beside it, and
    /// an `is_empty` that can only answer `false` teaches the wrong thing about the type.
    #[inline]
    #[must_use]
    pub const fn count(&self) -> usize {
        self.rest.len().saturating_add(1)
    }
}

impl core::fmt::Display for PinnedAlgorithms {
    /// The list, for the startup log line an operator reads to check what is pinned.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (position, algorithm) in self.iter().enumerate() {
            if position > 0 {
                f.write_str(", ")?;
            }
            f.write_str(algorithm.as_str())?;
        }
        Ok(())
    }
}

/// What an absolute `https` URI may hold here, after the scheme.
///
/// RFC 3986's unreserved and sub-delimiter sets plus the path and authority punctuation, minus `?`
/// and `#` - both of which get their own refusal above, because the reason for refusing them is
/// worth a sentence rather than a position. Everything outside ASCII is therefore refused, which is
/// how every invisible and direction-changing code point is refused without a second check for
/// them.
const URI_CHARACTERS: &str = "letters, digits, and any of - . _ ~ : / [ ] @ ! $ & ' ( ) * + , ; = %";

/// What a header name may hold. RFC 9110 says more than this; a configured value has no reason to.
const HEADER_CHARACTERS: &str = "lower-case letters, digits, `-` and `_`";

/// Parses one absolute `https` URI, in the strict sense this module's documentation describes.
///
/// Shared by [`ResourceIdentifier`] and [`IssuerUrl`] because they are one parse with two names: an
/// audience and an issuer are both configured strings compared byte for byte against a claim, and a
/// rule that held for one and not the other would be a hole with its matching pair next to it.
fn parse_https_uri(key: &'static str, raw: &str) -> Result<String, InvalidInboundValue> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(InvalidInboundValue::Empty { key });
    }
    let found = trimmed.chars().count();
    if found > MAX_LENGTH {
        return Err(InvalidInboundValue::TooLong {
            key,
            found,
            limit: MAX_LENGTH,
        });
    }
    if !trimmed.starts_with(REQUIRED_SCHEME) {
        return Err(InvalidInboundValue::NotHttps { key });
    }
    for (position, character) in trimmed.chars().enumerate() {
        if character == '#' || character == '?' {
            return Err(InvalidInboundValue::HasDelimiter {
                key,
                delimiter: character,
                position,
            });
        }
        let permitted = character.is_ascii_alphanumeric()
            || matches!(
                character,
                '-' | '.'
                    | '_'
                    | '~'
                    | ':'
                    | '/'
                    | '['
                    | ']'
                    | '@'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | '%'
            );
        if !permitted {
            return Err(InvalidInboundValue::NotPermitted {
                key,
                position,
                accepted: URI_CHARACTERS,
            });
        }
    }
    // Stored exactly as written past the trim. See the module documentation: nothing is normalised,
    // because the comparison downstream is byte for byte against what an issuer was configured with.
    Ok(String::from(trimmed))
}

impl ResourceIdentifier {
    /// The key this value is written under, so the refusal and the documentation name the same one.
    pub const KEY: &'static str = "security.inbound.resource";

    /// Reads the identifier this deployment declares for itself.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        parse_https_uri(Self::KEY, raw.as_ref()).map(Self)
    }

    /// Reads a transit proof's expected audience, which is the same shape under a different key.
    ///
    /// A second entry point rather than a second parser: the only thing that differs is which key a
    /// refusal names, and that is what makes an operator able to find it.
    pub fn parse_transit_audience(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        parse_https_uri("security.inbound.transit_audience", raw.as_ref()).map(Self)
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ResourceIdentifier {
    type Error = InvalidInboundValue;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for ResourceIdentifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl IssuerUrl {
    /// The key an authorization server is written under.
    pub const KEY: &'static str = "security.inbound.authorization_server";

    /// Reads the authorization server that governs this resource.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        parse_https_uri(Self::KEY, raw.as_ref()).map(Self)
    }

    /// Reads the issuer a transit proof must carry.
    pub fn parse_transit_issuer(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        parse_https_uri("security.inbound.transit_issuer", raw.as_ref()).map(Self)
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for IssuerUrl {
    type Error = InvalidInboundValue;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for IssuerUrl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl KeySetFile {
    /// The key a key set is written under.
    pub const KEY: &'static str = "security.inbound.key_set_file";

    /// Reads the path.
    ///
    /// Existence is deliberately not checked here, for the reason
    /// [`InstructionsFile`](crate::prompt::InstructionsFile) does not check its own: a check at parse
    /// time is a claim that is stale by the time the file is read. The read happens at the
    /// composition root, before the listener opens, and a key set that cannot be read is a refusal
    /// to start rather than a deployment that authenticates nobody.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidInboundValue::Empty { key: Self::KEY });
        }
        let found = trimmed.chars().count();
        if found > MAX_LENGTH {
            return Err(InvalidInboundValue::TooLong {
                key: Self::KEY,
                found,
                limit: MAX_LENGTH,
            });
        }
        Ok(Self(PathBuf::from(trimmed)))
    }

    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl core::fmt::Display for KeySetFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl ProofHeader {
    /// The key a proof header is written under.
    pub const KEY: &'static str = "security.inbound.transit_header";

    /// The header the deployment bearer token arrives in, which this may not be.
    const RESERVED: &'static str = "authorization";

    /// Reads the header name, folding case.
    ///
    /// **Sanitize, then validate, both inside the constructor.** The fold happens before the
    /// character check, so the accepted set can be the lower-case one and a configured `X-Transit`
    /// and an arriving `x-transit` are the same header rather than a lookup that misses an entry
    /// that is present.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidInboundValue> {
        let folded = raw.as_ref().trim().to_ascii_lowercase();
        if folded.is_empty() {
            return Err(InvalidInboundValue::Empty { key: Self::KEY });
        }
        let found = folded.chars().count();
        if found > MAX_LENGTH {
            return Err(InvalidInboundValue::TooLong {
                key: Self::KEY,
                found,
                limit: MAX_LENGTH,
            });
        }
        for (position, character) in folded.chars().enumerate() {
            if !(character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-' || character == '_') {
                return Err(InvalidInboundValue::NotPermitted {
                    key: Self::KEY,
                    position,
                    accepted: HEADER_CHARACTERS,
                });
            }
        }
        if folded == Self::RESERVED {
            return Err(InvalidInboundValue::ReservedHeader {
                key: Self::KEY,
                found: folded,
            });
        }
        Ok(Self(folded))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProofHeader {
    type Error = InvalidInboundValue;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for ProofHeader {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}
