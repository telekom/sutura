//! A mock authorization server, inside the test sandbox.
//!
//! Leg 1 is the product's first identity claim, and until now every test of it built its own key pair
//! and its own tokens inside the crate that was being tested. That is right for a unit test and it
//! stops one step short of the venue this module adds: **an issuer any crate can link**, so leg 1 and
//! the credential path can be driven through an assembled router - and, later, through a composed
//! binary - on every run, with no network, no docker and no secret.
//!
//! # What this venue can answer, and the two things it may never be cited for
//!
//! It is the default venue and it is not a substitute for an enterprise identity provider. The split
//! is not a compromise; it is what each venue can honestly claim.
//!
//! **Answered here:** a signature, `kid` selection, algorithm confusion, a symmetric key refused, the
//! issuer, the audience against this deployment's own resource identifier, expiry, the `iat` ceiling,
//! the token class, and - the reason this hands back a *published document* rather than a struct - a
//! **key rotation against a source that changes**, which is the one bound whose failure is silent.
//!
//! **Never cite this for:**
//!
//! 1. **Whether a real identity provider will mint an ID token whose `aud` is a third party's client
//!    id.** A mock answers *yes* by construction, because [`Token`] takes the audience as a parameter.
//!    That question has exactly one venue - a real provider - and `docs/where-identity-is-proven.md`
//!    says so.
//! 2. **Whether a token exchange endpoint accepts what we send it, or whether two subjects read two
//!    row sets.** Nothing here talks to a data system.
//!
//! # The constraint that makes it worth having
//!
//! **It produces real signatures over real documents.** `rcgen` generates the key pair, `jsonwebtoken`
//! signs the claim set, and the public half goes into a JWK the way an issuer publishes one - so a
//! verifier under test runs its real code path. A mock handing back a decoded claim set would be
//! testing our test, which `AGENTS.md` calls a test asserting on source text.
//!
//! **Every knob is a parameter**, because the useful tests are the negative ones: a wrong audience, a
//! wrong issuer, an `alg` of `none`, a symmetric key in the set, an ID token where an access token is
//! required, an `iat` dated forward, an `exp` past the lifetime ceiling. A fixture that could only
//! mint a *good* token would leave every one of those to be hand-rolled again per crate.
//!
//! # Which algorithms are reachable, stated rather than implied
//!
//! [`Curve`] has three variants and they are the three the linked crypto backend can *generate* a key
//! for: `ES256`, `ES384` and `EdDSA`. `RS*` and `PS*` are **not** mintable here - an RSA key needs a
//! dependency nothing in this workspace wants, and a committed private key in a public repository is a
//! committed private key whatever the comment beside it says. What covers those is the family refusal
//! [`MockIssuer::key_set_of_rsa_keys`] provokes, an exhaustive match in the verifier, and a reviewer.
//! Saying that plainly is the point; three of nine tested behind a list of nine would read as coverage.
//!
//! # Which knobs have a caller today, said out loud
//!
//! **A fixture is not exempt from this file's own rule about stating limits.** What the first suites to
//! use this module drive: `kid` selection, the audience (its own, none, and a wrong one), the issuer,
//! `exp`, `nbf`, the `typ` in three states, `alg: none`, a stranger's signature, a duplicate key id,
//! all three curves, and both names a deployment is configured with.
//!
//! **Four knobs have no caller yet, and they are named rather than left to be found:**
//! [`Token::issued_ago`], [`Token::stating_no_issued_at`], [`Token::living_for`] and
//! [`Token::for_audiences`]. The first three are the gateway mode's replay-window arithmetic and the
//! fourth is the `aud` array; all four already have standing tests **at the gate**, over that
//! transport's own in-place fixtures, so minting them here as well would be a second venue for the
//! same refusals - which `AGENTS.md` calls the shape that reads as coverage. The venue page,
//! `docs/where-identity-is-proven.md`, marks those rows **can** rather than **yes** for that reason.
//!
//! They are built now rather than when somebody wants them because the whole argument for a *builder*
//! is that a negative test costs one call - and a builder that had to grow a method per negative would
//! send the next author back to hand-rolling a claim set, which is the thing this module exists to stop.
//!
//! # Errors rather than panics, which is a lint and not a preference
//!
//! This is library code in a crate the workspace lints, so `expect_used` and `indexing_slicing` are
//! denied here as everywhere else - the test-only exemption in `clippy.toml` does not reach it. Every
//! fallible step therefore returns [`IssuerDefect`], whose four variants are the four things that can
//! go wrong and none of which a correct caller reaches.

use std::path::{Path, PathBuf};

/// The elliptic curves this issuer can generate a signing key for.
///
/// Three, and each is one of the algorithms a deployment can pin. The name is the curve rather than
/// the algorithm because the curve is what gets generated; [`Curve::algorithm`] is the mapping, and it
/// is a match rather than a lookup so a fourth curve does not compile until somebody answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// `ES256`. What every fixture uses unless it is about something else.
    P256,
    /// `ES384`.
    P384,
    /// `EdDSA` over Ed25519.
    Ed25519,
}

impl Curve {
    /// The JWS algorithm identifier a key on this curve signs with.
    #[must_use]
    pub const fn algorithm(self) -> &'static str {
        match self {
            Self::P256 => "ES256",
            Self::P384 => "ES384",
            Self::Ed25519 => "EdDSA",
        }
    }

    /// The `rcgen` parameters that generate a key pair for it.
    const fn parameters(self) -> &'static rcgen::SignatureAlgorithm {
        match self {
            Self::P256 => &rcgen::PKCS_ECDSA_P256_SHA256,
            Self::P384 => &rcgen::PKCS_ECDSA_P384_SHA384,
            Self::Ed25519 => &rcgen::PKCS_ED25519,
        }
    }

    /// How many bytes one coordinate of a point on this curve takes.
    ///
    /// **Declared rather than derived from the point's length**, and not only because
    /// `integer_division` is denied in this workspace: the curve is already known here, so halving a
    /// length would be re-deriving something the type says - and getting it wrong would silently write
    /// a JWK whose `x` and `y` are the wrong spans of one key.
    ///
    /// `Ed25519` shares the arm with `P-256` at 32 bytes and the coincidence is not read by anything:
    /// an `OKP` key has one coordinate and no tag, so its JWK is written from the whole point and never
    /// asks this.
    const fn coordinate_bytes(self) -> usize {
        match self {
            Self::P256 | Self::Ed25519 => 32,
            Self::P384 => 48,
        }
    }

    /// The signing algorithm in the JWT library's own vocabulary.
    const fn signing(self) -> jsonwebtoken::Algorithm {
        match self {
            Self::P256 => jsonwebtoken::Algorithm::ES256,
            Self::P384 => jsonwebtoken::Algorithm::ES384,
            Self::Ed25519 => jsonwebtoken::Algorithm::EdDSA,
        }
    }
}

/// What went wrong, as four variants a correct caller does not reach.
///
/// Hand-written `Display` and `Error`, the way `crate::discovery` does it: this crate carries no error
/// derive, and four variants do not earn one.
#[derive(Debug)]
pub enum IssuerDefect {
    /// A key pair would not generate. The backend said so; there is nothing a caller can do.
    KeyPairUngeneratable,
    /// A token named a key id this issuer does not hold.
    NoSuchKey {
        /// The id that was asked for.
        id: String,
    },
    /// The signing step failed, which for a generated key and a JSON claim set means a library defect.
    Unsignable {
        /// The algorithm that was being signed with.
        algorithm: &'static str,
    },
    /// The key set could not be written where it was asked for.
    Unpublishable {
        /// Where the write was attempted.
        path: PathBuf,
        /// What the filesystem said.
        cause: std::io::Error,
    },
}

impl core::fmt::Display for IssuerDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::KeyPairUngeneratable => write!(f, "the crypto backend would not generate a key pair"),
            Self::NoSuchKey { ref id } => write!(f, "this issuer holds no key with the id `{id}`"),
            Self::Unsignable { algorithm } => write!(f, "a claim set would not sign with {algorithm}"),
            Self::Unpublishable { ref path, .. } => write!(f, "the key set could not be written to {}", path.display()),
        }
    }
}

impl std::error::Error for IssuerDefect {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            Self::Unpublishable { ref cause, .. } => Some(cause),
            Self::KeyPairUngeneratable | Self::NoSuchKey { .. } | Self::Unsignable { .. } => None,
        }
    }
}

/// One key the issuer publishes and signs with.
struct SigningKey {
    id: String,
    curve: Curve,
    pair: rcgen::KeyPair,
}

impl SigningKey {
    /// This key's public half, as the JWK an issuer publishes.
    fn jwk(&self) -> String {
        self.jwk_named(&self.id)
    }

    /// The same JWK under an id of the caller's choosing, so a set can carry two keys under one id.
    ///
    /// `public_key_raw` is the uncompressed point - `0x04 || X || Y` - so the two halves after the tag
    /// are the `x` and `y`, base64url with no padding. An Ed25519 key has no tag and no `y`, and which
    /// shape to write is decided by the curve this key was **generated for** rather than guessed from a
    /// length: the curve is already known, and a length guess is one more thing a reader has to
    /// re-derive.
    fn jwk_named(&self, id: &str) -> String {
        let point = self.pair.public_key_raw();
        let algorithm = self.curve.algorithm();
        if self.curve == Curve::Ed25519 {
            let x = base64url(point);
            return format!(r#"{{"kty":"OKP","crv":"Ed25519","use":"sig","alg":"{algorithm}","kid":"{id}","x":"{x}"}}"#);
        }
        let curve = if self.curve == Curve::P384 { "P-384" } else { "P-256" };
        // The tag is one byte and the two coordinates follow it, each `coordinate_bytes` long. Written
        // with `get` rather than a range index because `indexing_slicing` is denied in this crate, and
        // an empty coordinate is a JWK a verifier refuses - which is a better failure than a panic.
        let half = self.curve.coordinate_bytes();
        let x = point.get(1..=half).map_or_else(String::new, base64url);
        let y = point.get(half + 1..).map_or_else(String::new, base64url);
        format!(r#"{{"kty":"EC","crv":"{curve}","use":"sig","alg":"{algorithm}","kid":"{id}","x":"{x}","y":"{y}"}}"#)
    }

    /// The private half, in the shape the JWT library's DER constructors expect.
    fn encoding(&self) -> jsonwebtoken::EncodingKey {
        let der = self.pair.serialize_der();
        if self.curve == Curve::Ed25519 {
            return jsonwebtoken::EncodingKey::from_ed_der(&der);
        }
        jsonwebtoken::EncodingKey::from_ec_der(&der)
    }
}

/// base64url with no padding, which is what every field of a JOSE document is.
fn base64url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// An authorization server that exists for the length of a test.
///
/// It holds an issuer identifier, the audience its tokens are for, and one or more signing keys. Both
/// names are held rather than passed per token because they are what a deployment is *configured*
/// with: a test about a wrong issuer should have to say so, and every other test should not have to
/// repeat the right one.
pub struct MockIssuer {
    issuer: String,
    audience: String,
    keys: Vec<SigningKey>,
}

impl MockIssuer {
    /// An issuer with one `P-256` key under `key_id`.
    ///
    /// The ordinary constructor. A test that wants a second key, or another curve, adds one with
    /// [`MockIssuer::also_holding`].
    ///
    /// # Errors
    ///
    /// [`IssuerDefect::KeyPairUngeneratable`] if the crypto backend will not generate a key pair.
    pub fn generating(issuer: &str, audience: &str, key_id: &str) -> Result<Self, IssuerDefect> {
        Self {
            issuer: String::from(issuer),
            audience: String::from(audience),
            keys: Vec::new(),
        }
        .also_holding(key_id, Curve::P256)
    }

    /// The same issuer, holding one more key.
    ///
    /// # Errors
    ///
    /// [`IssuerDefect::KeyPairUngeneratable`] if the crypto backend will not generate a key pair.
    pub fn also_holding(mut self, key_id: &str, curve: Curve) -> Result<Self, IssuerDefect> {
        let pair = rcgen::KeyPair::generate_for(curve.parameters()).map_err(|_backend| IssuerDefect::KeyPairUngeneratable)?;
        self.keys.push(SigningKey {
            id: String::from(key_id),
            curve,
            pair,
        });
        Ok(self)
    }

    /// What this issuer calls itself - the value an `iss` claim carries.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Who its tokens are for - the value an `aud` claim carries.
    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }

    /// The ids of every key it publishes, in the order they were added.
    #[must_use]
    pub fn key_ids(&self) -> Vec<&str> {
        self.keys.iter().map(|key| key.id.as_str()).collect()
    }

    /// The JWK set holding every key.
    #[must_use]
    pub fn key_set(&self) -> String {
        set_of(self.keys.iter().map(SigningKey::jwk))
    }

    /// The JWK set with one key **removed**, which is what a rotation looks like from outside.
    ///
    /// The whole reason this hands back a document rather than a struct: revocation is bounded only if
    /// a verifier re-reads its *source*, so the assertion has to be against a source whose content
    /// changed. An in-memory key set would test the cache and not the bound.
    #[must_use]
    pub fn key_set_without(&self, key_id: &str) -> String {
        set_of(self.keys.iter().filter(|key| key.id != key_id).map(SigningKey::jwk))
    }

    /// The JWK set holding one key twice under one id, which a verifier must refuse rather than
    /// resolve by document order.
    #[must_use]
    pub fn key_set_naming_one_key_twice(&self, key_id: &str) -> String {
        set_of(
            self.keys
                .iter()
                .take(1)
                .flat_map(|key| [key.jwk_named(key_id), key.jwk_named(key_id)]),
        )
    }

    /// A JWK set holding one **symmetric** key, which is what must never be accepted.
    ///
    /// An associated function rather than a method: no signing key is involved, and the value of the
    /// fixture is that the document is otherwise well formed. Accepting it would make algorithm
    /// confusion reachable - the holder of a *published* key could sign with it.
    #[must_use]
    pub fn key_set_of_symmetric_keys() -> String {
        String::from(r#"{"keys":[{"kty":"oct","kid":"shared","alg":"HS256","k":"c2VjcmV0LXNoYXJlZC13aXRoLWV2ZXJ5Ym9keQ"}]}"#)
    }

    /// A JWK set holding one RSA key, for the family mismatch a deployment pinning `ES*` must refuse.
    ///
    /// **The modulus is not a real key and does not need to be.** What is asserted with it is that a
    /// key set of the wrong family is refused *at load*, which happens before anything verifies a
    /// signature - so the exponent and the modulus only have to be base64url a decoder will accept.
    /// This is also why `RS*` is not mintable here: the refusal is the coverage.
    #[must_use]
    pub fn key_set_of_rsa_keys(key_id: &str) -> String {
        let modulus = base64url(&[0xC5_u8; 256]);
        let exponent = base64url(&[0x01_u8, 0x00, 0x01]);
        format!(r#"{{"keys":[{{"kty":"RSA","use":"sig","alg":"RS256","kid":"{key_id}","n":"{modulus}","e":"{exponent}"}}]}}"#)
    }

    /// Publishes the whole key set at `path`, replacing whatever was there.
    ///
    /// # Errors
    ///
    /// [`IssuerDefect::Unpublishable`] if the write fails.
    pub fn publish(&self, path: impl AsRef<Path>) -> Result<(), IssuerDefect> {
        Self::publish_document(path, &self.key_set())
    }

    /// Publishes an arbitrary document at `path`, which is how a rotation is performed.
    ///
    /// # Errors
    ///
    /// [`IssuerDefect::Unpublishable`] if the write fails.
    pub fn publish_document(path: impl AsRef<Path>, document: &str) -> Result<(), IssuerDefect> {
        let path = path.as_ref();
        std::fs::write(path, document).map_err(|cause| IssuerDefect::Unpublishable {
            path: path.to_path_buf(),
            cause,
        })
    }

    /// Signs `token` with the key it names, or with the first key if it names none.
    ///
    /// # Errors
    ///
    /// [`IssuerDefect::NoSuchKey`] if the named key is not held, and [`IssuerDefect::Unsignable`] if
    /// the library refuses the claim set - which for a generated key means a library defect.
    pub fn mint(&self, token: &Token) -> Result<String, IssuerDefect> {
        let key = self.key_for(token)?;
        let mut header = jsonwebtoken::Header::new(key.curve.signing());
        header.kid = Some(key.id.clone());
        header.typ.clone_from(&token.class);
        jsonwebtoken::encode(&header, &self.claims(token), &key.encoding()).map_err(|_library| IssuerDefect::Unsignable {
            algorithm: key.curve.algorithm(),
        })
    }

    /// The same claim set, signed by a key this issuer does **not** publish.
    ///
    /// The forgery fixture, and the point is that it is correct in every other respect: the `kid` names
    /// a key the verifier holds, the issuer and the audience are right, and only the signature is
    /// somebody else's. A fixture that changed the `kid` as well would be asserting the unknown-key
    /// path instead.
    ///
    /// # Errors
    ///
    /// As [`MockIssuer::mint`], plus [`IssuerDefect::KeyPairUngeneratable`] for the stranger's key.
    pub fn mint_signed_by_a_stranger(&self, token: &Token) -> Result<String, IssuerDefect> {
        let named = self.key_for(token)?;
        let stranger = Self {
            issuer: self.issuer.clone(),
            audience: self.audience.clone(),
            keys: Vec::new(),
        }
        .also_holding(&named.id, named.curve)?;
        stranger.mint(token)
    }

    /// The same claim set with an `alg` of `none` and no signature at all.
    ///
    /// Hand-assembled, because the library will not encode it - which is itself the reassuring part.
    /// What this provokes is the oldest JWT defect there is: a verifier that reads the algorithm out of
    /// the header it was handed instead of out of what the deployment pinned.
    ///
    /// # Errors
    ///
    /// [`IssuerDefect::NoSuchKey`] if the token names a key this issuer does not hold. The `kid` is
    /// still resolved, so an unsigned token is refused for its algorithm rather than for its key id.
    pub fn mint_unsigned(&self, token: &Token) -> Result<String, IssuerDefect> {
        let key = self.key_for(token)?;
        let class = token
            .class
            .as_ref()
            .map_or_else(String::new, |typ| format!(r#","typ":"{typ}""#));
        let header = format!(r#"{{"alg":"none","kid":"{}"{class}}}"#, key.id);
        Ok(format!(
            "{}.{}.",
            base64url(header.as_bytes()),
            base64url(self.claims(token).to_string().as_bytes())
        ))
    }

    /// The key a token names, or the first one.
    fn key_for(&self, token: &Token) -> Result<&SigningKey, IssuerDefect> {
        token.key_id.as_ref().map_or_else(
            || {
                self.keys.first().ok_or_else(|| IssuerDefect::NoSuchKey {
                    id: String::from("<this issuer holds no key at all>"),
                })
            },
            |id| {
                self.keys
                    .iter()
                    .find(|key| key.id == *id)
                    .ok_or_else(|| IssuerDefect::NoSuchKey { id: id.clone() })
            },
        )
    }

    /// The claim set, with this issuer's own names filled in wherever the token left them open.
    fn claims(&self, token: &Token) -> serde_json::Value {
        let issued = token.issued_at.unwrap_or_else(now_seconds);
        let mut object = serde_json::Map::new();
        drop(object.insert(String::from("sub"), token.subject.clone().into()));
        drop(object.insert(
            String::from("iss"),
            token.issuer.clone().unwrap_or_else(|| self.issuer.clone()).into(),
        ));
        drop(object.insert(String::from("exp"), token.expires_at.unwrap_or(issued + 3600).into()));
        if let Some(audience) = token.audience.claim(&self.audience) {
            drop(object.insert(String::from("aud"), audience));
        }
        if token.state_the_issued_at {
            drop(object.insert(String::from("iat"), issued.into()));
        }
        if let Some(before) = token.not_before {
            drop(object.insert(String::from("nbf"), before.into()));
        }
        if let Some(ref scope) = token.scope {
            drop(object.insert(String::from("scope"), scope.clone().into()));
        }
        for (name, value) in &token.extra {
            drop(object.insert(name.clone(), value.clone()));
        }
        serde_json::Value::Object(object)
    }
}

/// Wraps JWKs into the one document shape an issuer publishes.
fn set_of(keys: impl Iterator<Item = String>) -> String {
    format!("{{\"keys\":[{}]}}", keys.collect::<Vec<String>>().join(","))
}

/// The seconds since the epoch, as a JWT timestamp.
///
/// Saturating rather than fallible: a clock before 1970, or past the year 292 277 026 596, is not a
/// failure this module has anything useful to say about, and a `Result` here would reach every builder.
fn now_seconds() -> i64 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    i64::try_from(seconds).unwrap_or(i64::MAX)
}

/// What audience a token claims, including none at all.
///
/// Four variants and not an `Option<Vec<String>>`, because they are four different tests: the ordinary
/// one, a named wrong one, the array form RFC 7519 permits, and the token that carries no `aud` for a
/// verifier that must refuse rather than pass a check with nothing to compare.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Audience {
    /// This issuer's own audience.
    AsConfigured,
    /// One named audience, which is how a wrong one is written.
    Named(String),
    /// An array, which is what a token for several resources carries.
    Several(Vec<String>),
    /// No `aud` claim at all.
    Unstated,
}

impl Audience {
    /// The claim value, or `None` when the token states no audience.
    fn claim(&self, configured: &str) -> Option<serde_json::Value> {
        match *self {
            Self::AsConfigured => Some(configured.into()),
            Self::Named(ref one) => Some(one.clone().into()),
            Self::Several(ref many) => Some(many.clone().into()),
            Self::Unstated => None,
        }
    }
}

/// One token to mint, with every claim a negative test needs to be able to move.
///
/// A builder rather than a struct literal, so the ORDINARY token is one call and each negative is one
/// call plus the one thing it is about. That is what keeps such a suite readable: a reader can see what
/// a test varies without diffing it against the good case.
#[derive(Debug, Clone)]
pub struct Token {
    subject: String,
    key_id: Option<String>,
    issuer: Option<String>,
    audience: Audience,
    class: Option<String>,
    issued_at: Option<i64>,
    state_the_issued_at: bool,
    expires_at: Option<i64>,
    not_before: Option<i64>,
    scope: Option<String>,
    extra: Vec<(String, serde_json::Value)>,
}

impl Token {
    /// RFC 9068's media type for an access token, which is what a resource server should require.
    pub const ACCESS_TOKEN: &'static str = "at+jwt";
    /// The class an `OpenID` Connect ID token carries. Present so the substitution test can name it.
    pub const ID_TOKEN: &'static str = "JWT";

    /// The ordinary token for `subject`: this issuer, this audience, an access token, valid for an hour.
    ///
    /// It states an `iat`, because the mode that needs one requires it and the mode that does not
    /// ignores it - so the default that is right in both places is to state it, and
    /// [`Token::stating_no_issued_at`] is the negative.
    #[must_use]
    pub fn for_subject(subject: &str) -> Self {
        Self {
            subject: String::from(subject),
            key_id: None,
            issuer: None,
            audience: Audience::AsConfigured,
            class: Some(String::from(Self::ACCESS_TOKEN)),
            issued_at: None,
            state_the_issued_at: true,
            expires_at: None,
            not_before: None,
            scope: None,
            extra: Vec::new(),
        }
    }

    /// Signs with the named key rather than the issuer's first.
    #[must_use]
    pub fn signed_by(mut self, key_id: &str) -> Self {
        self.key_id = Some(String::from(key_id));
        self
    }

    /// Claims an issuer of its own, which is how the wrong-issuer refusal is provoked.
    ///
    /// Named `claiming_issuer` and not `from_issuer` because a `from_*` method that takes `self` reads
    /// as a conversion and is not one - `clippy::wrong_self_convention` says so, and it is right.
    #[must_use]
    pub fn claiming_issuer(mut self, issuer: &str) -> Self {
        self.issuer = Some(String::from(issuer));
        self
    }

    /// Claims one named audience rather than the issuer's own.
    #[must_use]
    pub fn for_audience(mut self, audience: &str) -> Self {
        self.audience = Audience::Named(String::from(audience));
        self
    }

    /// Claims an array of audiences, the form RFC 7519 permits.
    #[must_use]
    pub fn for_audiences(mut self, audiences: &[&str]) -> Self {
        self.audience = Audience::Several(audiences.iter().map(|one| String::from(*one)).collect());
        self
    }

    /// Claims no audience at all, so there is nothing for a verifier to compare.
    #[must_use]
    pub fn for_nobody_in_particular(mut self) -> Self {
        self.audience = Audience::Unstated;
        self
    }

    /// Sets the `typ` header, which is what decides a token's CLASS.
    #[must_use]
    pub fn classed(mut self, class: &str) -> Self {
        self.class = Some(String::from(class));
        self
    }

    /// Carries no `typ` at all, which a class check must not be satisfiable by.
    #[must_use]
    pub fn unclassed(mut self) -> Self {
        self.class = None;
        self
    }

    /// Issued `seconds` ago. A negative value dates it forward, which is how a component would buy a
    /// longer replay window than the one this deployment chose.
    #[must_use]
    pub fn issued_ago(mut self, seconds: i64) -> Self {
        self.issued_at = Some(now_seconds() - seconds);
        self.state_the_issued_at = true;
        self
    }

    /// States no `iat`, which the gateway mode must refuse because its ceiling is `exp - iat`.
    #[must_use]
    pub const fn stating_no_issued_at(mut self) -> Self {
        self.state_the_issued_at = false;
        self
    }

    /// Lives for `seconds` from its `iat`, which is what a lifetime ceiling is compared against.
    #[must_use]
    pub fn living_for(mut self, seconds: i64) -> Self {
        self.expires_at = Some(self.issued_at.unwrap_or_else(now_seconds) + seconds);
        self
    }

    /// Expired `seconds` ago.
    #[must_use]
    pub fn expired_since(mut self, seconds: i64) -> Self {
        self.expires_at = Some(now_seconds() - seconds);
        self
    }

    /// Not valid until `seconds` from now.
    #[must_use]
    pub fn not_before_in(mut self, seconds: i64) -> Self {
        self.not_before = Some(now_seconds() + seconds);
        self
    }

    /// Carries a space-delimited `scope` claim, per RFC 6749.
    #[must_use]
    pub fn granting(mut self, scope: &str) -> Self {
        self.scope = Some(String::from(scope));
        self
    }

    /// Carries one more claim, for anything this builder has no name for.
    #[must_use]
    pub fn claiming(mut self, name: &str, value: serde_json::Value) -> Self {
        self.extra.push((String::from(name), value));
        self
    }
}

/// A key set on disk, removed when it goes out of scope.
///
/// **The seam a rotation test needs.** The one key set source that ships reads a *file*, so an
/// assertion about a revoked key stopping verifying has to change a file - and a test that left one
/// behind in the temporary directory would be a test that passes on its second run for the wrong
/// reason. [`PublishedKeySet::rotate_to`] is the whole vocabulary: publish a new document at the same
/// path and let the verifier notice.
pub struct PublishedKeySet {
    path: PathBuf,
}

impl PublishedKeySet {
    /// Publishes `issuer`'s key set at a path named after `label` and this process.
    ///
    /// The process id is in the name because the suite runs test binaries concurrently and the
    /// temporary directory is shared; the label is in it because a failure naming the file should say
    /// which test wrote it.
    ///
    /// # Errors
    ///
    /// [`IssuerDefect::Unpublishable`] if the write fails.
    pub fn of(issuer: &MockIssuer, label: &str) -> Result<Self, IssuerDefect> {
        let path = std::env::temp_dir().join(format!("sutura-mock-issuer-{label}-{}.json", std::process::id()));
        issuer.publish(&path)?;
        Ok(Self { path })
    }

    /// Where it is, which is what a deployment's `key_set_file` is set to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replaces the published document, which is what a rotation is.
    ///
    /// # Errors
    ///
    /// [`IssuerDefect::Unpublishable`] if the write fails.
    pub fn rotate_to(&self, document: &str) -> Result<(), IssuerDefect> {
        MockIssuer::publish_document(&self.path, document)
    }
}

impl Drop for PublishedKeySet {
    /// Removes the file, ignoring a failure: a test that has already asserted must not fail on tidying
    /// up, and a leftover file in the temporary directory is not a correctness problem.
    fn drop(&mut self) {
        drop(std::fs::remove_file(&self.path));
    }
}
