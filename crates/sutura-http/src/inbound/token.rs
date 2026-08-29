//! Verifying one token, and turning its claims into a principal chain.
//!
//! # The three things `docs/adr/0014` says a direct deployment owns
//!
//! Key rotation is [`super::keys`]. The other two are here:
//!
//! **Algorithm pinning.** [`TokenValidator::new`] builds the validation from
//! `sutura_config::PinnedAlgorithms`, which is a non-empty list of a single key family with no
//! symmetric variant and no `none` variant *available to construct*. Nothing in this file reads the
//! `alg` of the token being validated in order to choose how to verify it: the library compares the
//! header's algorithm against the pinned list and refuses otherwise, and because the list can only
//! hold asymmetric algorithms, the classic confusion - a token signed `HS256` with the issuer's public
//! key as the HMAC secret - has no path. [`super::keys::InvalidKeySet::SymmetricKey`] closes the other
//! half, which is a key set that published a shared secret.
//!
//! **The audience.** `Validation::set_audience` is given exactly one value: this deployment's own
//! resource identifier, out of the configuration, and `aud` is in `required_spec_claims` - so a token
//! with *no* audience is refused rather than accepted for want of a claim to compare. That is the
//! security decision in the record: a client's resource indicator is welcome and is an optimisation,
//! and this check is ours, unconditional, and not skippable when the indicator is absent.
//!
//! # What is not `deny_unknown_fields`, and why that is right exactly here
//!
//! [`Claims`] deliberately accepts unknown fields, which is the opposite of every other wire shape in
//! this workspace. A token is not a document this deployment defines: an issuer puts `azp`, `jti`,
//! `email`, `groups` and a dozen vendor claims in one, and refusing a token because its issuer added a
//! claim would break every deployment on the issuer's next release. What keeps that from being a hole
//! is that the fields we *do* read are the only ones anything downstream can see - and one of them,
//! the actor chain, is bounded.
//!
//! # Where the caller's own text is bounded
//!
//! Before anything expensive: [`MAX_TOKEN_BYTES`] caps what is even looked at, and `SubjectId` and
//! `Actor` are parsed by `sutura_domain::identity`, which bounds their length and refuses a control
//! character or an invisible one - because those values are written into an audit record that is one
//! line per call.

use core::time::Duration;

use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use sutura_config::{PinnedAlgorithms, ResourceIdentifier, SigningAlgorithm, TokenRequirement};
use sutura_domain::identity::{Actor, ActorChain, InvalidPrincipalId, PrincipalChain, Subject, SubjectId};

use crate::inbound::caller::{InvalidScope, Scopes, VerifiedCaller};
use crate::inbound::keys::{KeyId, NotAKeyId};

/// The largest token this surface will look at.
///
/// **Bounded before the signature is checked, because everything before that point is work done on
/// behalf of an unauthenticated caller.** Eight kibibytes is generous for an access token carrying
/// groups and an actor chain, and far below the header limit the HTTP implementation would otherwise
/// be the only bound at. An unbounded input is a denial-of-service primitive whatever else it is.
pub const MAX_TOKEN_BYTES: usize = 8192;

/// How much clock skew is tolerated on `exp` and `nbf`, in seconds.
///
/// Thirty rather than the library's default sixty, and stated rather than inherited: a deployment and
/// its issuer are both on NTP, and the value is a window in which an expired token is still accepted.
/// Not a configuration key - an operator who needs more than half a minute has a clock problem that a
/// wider window hides rather than fixes.
const LEEWAY_SECONDS: u64 = 30;

/// The longest actor chain accepted.
///
/// The `act` claim nests, and each level is one more delegation. Eight is more than any real chain -
/// `docs/adr/0009`'s is human, then agent, then task - and the bound is here because the chain is
/// rendered into an audit line and because a deeply nested claim is work proportional to input.
const MAX_ACTORS: usize = 8;

/// Why a presented token did not establish a caller.
///
/// **Every variant is "this caller is not authenticated", and none of them is a
/// `sutura_domain::query::RefusalReason`.** That is the placement `docs/adr/0008` part 6 already gives
/// an expired assertion: a refusal is a governance answer to a question that was understood, and a
/// caller who has not proved who they are has not asked a question yet. `crate::problem::Failure` is
/// where this becomes a status.
///
/// **No variant carries the token, a claim value, or a key.** The `#[error]` text names what was
/// wrong; the value that was wrong stays out of it, because these render into a log an operator reads
/// and an error is not a place for credential material.
#[derive(Debug, thiserror::Error)]
pub enum TokenRejected {
    /// No token where this deployment reads one.
    ///
    /// Its own variant rather than folded into a malformed token, because the two mean different
    /// things to whoever is debugging: one is a client that has not been configured, the other is a
    /// client whose token is wrong.
    #[error("no token was presented in {location}")]
    Absent { location: String },
    #[error("the presented token is longer than {limit} bytes")]
    TooLong { limit: usize },
    /// The header is not a JWT header, or names no key.
    #[error("the presented token does not carry a readable header")]
    UnreadableHeader {
        #[source]
        cause: jsonwebtoken::errors::Error,
    },
    /// No `kid`.
    ///
    /// Required rather than optional, and that is a decision: a token with no key id can only be
    /// verified by trying every key in the set, which makes an unknown key indistinguishable from a
    /// bad signature and makes rotation invisible. Every authorization server that publishes a key set
    /// sets it.
    #[error("the presented token names no key id, so no key in the set can be selected for it")]
    NoKeyId,
    #[error("the key id in the presented token is not one a JWK could carry")]
    UnusableKeyId {
        #[source]
        cause: NotAKeyId,
    },
    #[error("no verifying key is available for the key this token names")]
    NoKey {
        #[source]
        cause: crate::inbound::keys::KeyUnavailable,
    },
    /// The signature, the expiry, the issuer or the audience.
    ///
    /// **One variant for all four, and that is deliberate rather than lazy.** The library reports
    /// which, and it goes to the log through `#[source]`; what must not happen is a *response* that
    /// distinguishes them, because "the signature is fine and the audience is wrong" tells a caller
    /// which half of a forgery to fix. A caller is told it is unauthenticated.
    #[error("the presented token did not verify")]
    NotVerified {
        #[source]
        cause: jsonwebtoken::errors::Error,
    },
    /// A `sub` this workspace will not write into a record.
    #[error("the subject claim in the presented token is not a principal identifier")]
    UnusableSubject {
        #[source]
        cause: InvalidPrincipalId,
    },
    #[error("an actor claim in the presented token is not a principal identifier")]
    UnusableActor {
        #[source]
        cause: InvalidPrincipalId,
    },
    /// More nesting in `act` than [`MAX_ACTORS`] allows.
    #[error("the presented token names more than {limit} actors")]
    TooManyActors { limit: usize },
    #[error("a scope in the presented token is not a scope")]
    UnusableScope {
        #[source]
        cause: InvalidScope,
    },
}

/// The `act` claim: who acted for the subject, nesting outward in time.
///
/// RFC 8693 §4.1: the **top level is the most recent actor** and prior actors are nested inside it. So
/// the nesting runs the opposite way from `sutura_domain::identity::ActorChain`, whose iteration order
/// is nearest-the-subject first - which is why [`chain_from`] folds from the innermost nesting
/// outward rather than pushing as it walks.
///
/// `Box` because the type is recursive. The nesting is bounded by [`MAX_ACTORS`] when it is read, and
/// by `serde_json`'s own recursion limit before that.
#[derive(Debug, serde::Deserialize)]
struct ActorClaim {
    sub: String,
    #[serde(default)]
    act: Option<Box<Self>>,
}

/// The claims this deployment reads. Everything else in the token is ignored - see the module
/// documentation for why this one shape is not `deny_unknown_fields`.
#[derive(Debug, serde::Deserialize)]
struct Claims {
    /// Who the token is about. Required: `required_spec_claims` includes it, so a token without one
    /// does not reach here.
    sub: String,
    /// RFC 8693's actor claim, if something acted for the subject.
    #[serde(default)]
    act: Option<ActorClaim>,
    /// RFC 6749's space-delimited scope string, if the issuer sent one.
    #[serde(default)]
    scope: Option<String>,
}

/// One deployment's whole token check, built once at startup.
///
/// Holds the built `Validation` rather than rebuilding it per request, which is not an optimisation:
/// building it per request would be a per-request opportunity for one of its fields to be set
/// differently, and every field on it is a control.
#[derive(Debug)]
pub struct TokenValidator {
    validation: Validation,
    audience: ResourceIdentifier,
}

impl TokenValidator {
    /// Builds the validation from a declaration `sutura-config` already accepted or refused.
    ///
    /// Every line here is a control, so each one says what it is for. What is **not** here is equally
    /// the point: no `insecure_disable_signature_validation`, no `validate_aud = false`, and no path
    /// that reads the algorithm out of the token in order to pick one.
    #[must_use]
    pub fn new(requirement: &TokenRequirement<'_>) -> Self {
        let algorithms = requirement.algorithms();
        // `Validation::new` seeds `algorithms` with the one it is given; the assignment below replaces
        // the whole list, so the seed cannot survive as an extra permitted algorithm.
        let mut validation = Validation::new(map_algorithm(first_of(algorithms)));
        validation.algorithms = algorithms.iter().map(map_algorithm).collect();
        // The audience check. One value - ours - whatever the client asked its issuer for.
        validation.set_audience(&[requirement.audience().as_str()]);
        validation.set_issuer(&[requirement.issuer().as_str()]);
        // `exp` and `aud` and `iss` and `sub` all REQUIRED. The library's default set is `{"exp"}`
        // alone, and the difference is not academic: `validate_aud` only compares an audience that is
        // there, so without `aud` here a token carrying none would pass the audience check by having
        // nothing to check. `sub` is required because a verified caller with no subject is not a
        // caller.
        validation.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);
        validation.validate_exp = true;
        // Off by default in the library, and a token that is not valid yet is not valid.
        validation.validate_nbf = true;
        validation.leeway = LEEWAY_SECONDS;
        Self {
            validation,
            audience: requirement.audience().clone(),
        }
    }

    /// The key id the token names, bounded and parsed, without verifying anything.
    ///
    /// **Separate from [`Self::verify`] because it happens before the signature is checked** and the
    /// caller has to know that: the header of a JWT is unauthenticated input by construction, which is
    /// why the only thing taken out of it is an identifier that gets bounded, parsed and used as a map
    /// key. The algorithm in that header is read by nothing here.
    ///
    /// An associated function rather than a method, and that is the same point stated by the signature:
    /// nothing configured applies yet. There is no `&self` because there is nothing on the validator
    /// this step is allowed to consult.
    pub fn key_id(token: &str) -> Result<KeyId, TokenRejected> {
        if token.len() > MAX_TOKEN_BYTES {
            return Err(TokenRejected::TooLong { limit: MAX_TOKEN_BYTES });
        }
        let header = jsonwebtoken::decode_header(token).map_err(|cause| TokenRejected::UnreadableHeader { cause })?;
        let named = header.kid.ok_or(TokenRejected::NoKeyId)?;
        KeyId::parse(named).map_err(|cause| TokenRejected::UnusableKeyId { cause })
    }

    /// Verifies the token with the key and turns its claims into a caller.
    ///
    /// The order is the library's and it is the right one: signature first, then the registered
    /// claims, and only then is the claim payload deserialized into [`Claims`] - so the nesting bound
    /// in [`chain_from`] is applied to a document an issuer signed rather than to one a caller wrote.
    pub fn verify(&self, token: &str, key: &DecodingKey) -> Result<VerifiedCaller, TokenRejected> {
        let decoded =
            jsonwebtoken::decode::<Claims>(token, key, &self.validation).map_err(|cause| TokenRejected::NotVerified { cause })?;
        let claims = decoded.claims;
        let subject = SubjectId::parse(&claims.sub).map_err(|cause| TokenRejected::UnusableSubject { cause })?;
        let mut chain = PrincipalChain::of(Subject::Verified { id: subject });
        if let Some(ref actor) = claims.act {
            chain = chain.acting(chain_from(actor)?);
        }
        let scopes = match claims.scope {
            None => Scopes::none(),
            Some(ref written) => Scopes::parse(written).map_err(|cause| TokenRejected::UnusableScope { cause })?,
        };
        Ok(VerifiedCaller::established(chain, scopes))
    }

    /// The audience this validator requires, for a challenge and for a log line.
    #[inline]
    #[must_use]
    pub const fn audience(&self) -> &ResourceIdentifier {
        &self.audience
    }

    /// The leeway in effect, so a test asserts the value rather than the constant.
    #[inline]
    #[must_use]
    pub const fn leeway() -> Duration {
        Duration::from_secs(LEEWAY_SECONDS)
    }
}

/// The first pinned algorithm. Infallible, because `PinnedAlgorithms` has no empty state.
fn first_of(algorithms: &PinnedAlgorithms) -> SigningAlgorithm {
    // `unwrap_or` and not `unwrap`: the type cannot be empty - its first element lives in a field of
    // its own - and `unwrap_used` is denied here, so the fallback is a value rather than a panic path.
    // Any fallback would be wrong if it were reachable, so it is the most restrictive one available.
    algorithms.iter().next().unwrap_or(SigningAlgorithm::Rs256)
}

/// Maps this workspace's algorithm vocabulary onto the library's.
///
/// **The only place the two meet**, which is what keeps `sutura-config` free of a JWT dependency and
/// keeps the pinned enum ours. Exhaustive with no wildcard arm, so an algorithm added to the
/// configuration fails to compile here until somebody says what it is.
const fn map_algorithm(algorithm: SigningAlgorithm) -> Algorithm {
    match algorithm {
        SigningAlgorithm::Rs256 => Algorithm::RS256,
        SigningAlgorithm::Rs384 => Algorithm::RS384,
        SigningAlgorithm::Rs512 => Algorithm::RS512,
        SigningAlgorithm::Ps256 => Algorithm::PS256,
        SigningAlgorithm::Ps384 => Algorithm::PS384,
        SigningAlgorithm::Ps512 => Algorithm::PS512,
        SigningAlgorithm::Es256 => Algorithm::ES256,
        SigningAlgorithm::Es384 => Algorithm::ES384,
        SigningAlgorithm::EdDsa => Algorithm::EdDSA,
    }
}

/// Turns a nested `act` claim into an ordered actor chain.
///
/// **The orders are opposite, which is the whole reason this is a function.** RFC 8693 nests backwards
/// in time - the top level acted most recently - and `ActorChain` iterates nearest-the-subject first
/// with the immediate actor last. So the claim is walked into a list, bounded, and then folded from
/// the deepest nesting outward: the deepest is the first thing that acted for the subject, and the top
/// level is the one that called this deployment.
/// Split the way `ActorChain` itself is split - the immediate link in a value of its own, the outer
/// ones behind it - so there is no branch here for "no actors": a claim that exists names one.
fn chain_from(claim: &ActorClaim) -> Result<ActorChain, TokenRejected> {
    let immediate = actor_of(&claim.sub)?;
    let mut outer = Vec::new();
    let mut current = claim.act.as_deref();
    while let Some(link) = current {
        // `+ 2` because the total is the immediate actor plus what is already collected plus this one.
        if outer.len().saturating_add(2) > MAX_ACTORS {
            return Err(TokenRejected::TooManyActors { limit: MAX_ACTORS });
        }
        outer.push(actor_of(&link.sub)?);
        current = link.act.as_deref();
    }
    // `outer` runs backwards in time, so reversed it is nearest-the-subject first - which is the order
    // `ActorChain::of` plus `acting_through` builds in. The immediate actor goes on last.
    let built = outer.into_iter().rev().fold(None::<ActorChain>, |chain, actor| {
        Some(match chain {
            None => ActorChain::of(actor),
            Some(existing) => existing.acting_through(actor),
        })
    });
    Ok(match built {
        // A single-level `act` claim, which is the ordinary case.
        None => ActorChain::of(immediate),
        Some(chain) => chain.acting_through(immediate),
    })
}

/// One actor identifier, parsed by the domain so a control character cannot reach an audit line.
fn actor_of(raw: &str) -> Result<Actor, TokenRejected> {
    Actor::parse(raw).map_err(|cause| TokenRejected::UnusableActor { cause })
}
