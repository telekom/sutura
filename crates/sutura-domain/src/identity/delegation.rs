//! The delegation exchange a `direct` deployment needs before a workload pool will accept its
//! caller (`docs/adr/0014` Decision 3 and its fourth amendment).
//!
//! In `direct` the inbound token's `aud` is this deployment's own resource identifier, which leg 1
//! requires, and the pool provider requires its own. One token cannot carry both, so the caller's
//! identity provider is asked - RFC 8693, subject token = the inbound token - for a token whose audience is the
//! pool provider's. The inbound token then serves leg 1 only, and the exchanged one is what the
//! credential document hands Google's token service.
//!
//! **Nothing here caches.** One exchange per source per request, and the result lives in that
//! request's [`LegCredentials`](crate::identity::LegCredentials) and nowhere else, so two subjects cannot
//! share an exchanged token through this module: there is no store for them to share.
//! `docs/adr/0014`'s *Caching exchanged tokens is where this gets dangerous* is why the first
//! version has none.
//!
//! # The limits, beside the claim
//!
//! - **The identity provider is a hard runtime dependency.** No exchange, no question - a failure is
//!   [`DelegationFailed`] and reaches a caller as `503 identity_unavailable`, never as an answer
//!   under the deployment.
//! - **The client credential is the most sensitive value in the deployment**: whoever holds it can
//!   obtain a pool-audience token for any subject whose inbound token they also hold. It is a
//!   [`Secret`] (redacted `Debug`, no `Display`, zeroized on drop - with the copy limits that type
//!   states).
//! - **`sutura serve` composes it** for a source declaring `workload_identity.delegation`, refused
//!   at boot unless the inbound mode is `direct`. No served-binary cell reaches it: a `bigquery`
//!   deployment needs the ADBC driver to boot and the default test venue carries none, so the
//!   composition is held in-process by `sutura-cli`'s `build_broker` cells.

use std::sync::Arc;

use crate::identity::Secret;

/// The audience the exchanged token must carry: the pool provider's client ID.
///
/// **Stored exactly as written**, as `ResourceIdentifier` is: an identity provider matches it byte for byte
/// against a client it knows, so a normalised spelling would ask for a different audience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedAudience(String);

/// Why a declared requested audience is not one an exchange can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnusableAudience {
    /// There was nothing there.
    #[error("the requested delegation audience is empty")]
    Empty,
    /// Longer than [`RequestedAudience::MOST`].
    #[error("the requested delegation audience is {found} characters and at most {most} are usable")]
    TooLong {
        /// How long it was.
        found: usize,
        /// The bound.
        most: usize,
    },
    /// A space or a character outside printable ASCII, at a byte offset.
    #[error("the requested delegation audience carries an unusable character at {at}")]
    Unprintable {
        /// Where, so an operator can find it without the refusal quoting it.
        at: usize,
    },
}

impl RequestedAudience {
    /// An OAuth client identifier has no standard bound; this one is generous and finite.
    pub const MOST: usize = 255;

    /// Parses the pool provider's client ID.
    ///
    /// # Errors
    ///
    /// [`UnusableAudience`], carrying a position and never the text.
    pub fn parse(raw: &str) -> Result<Self, UnusableAudience> {
        if raw.is_empty() {
            return Err(UnusableAudience::Empty);
        }
        if raw.len() > Self::MOST {
            return Err(UnusableAudience::TooLong {
                found: raw.len(),
                most: Self::MOST,
            });
        }
        if let Some(at) = raw.bytes().position(|byte| !byte.is_ascii_graphic()) {
            return Err(UnusableAudience::Unprintable { at });
        }
        Ok(Self(String::from(raw)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A token an identity provider issued for the requested audience, and the instant it stops being one.
///
/// The instant is a number and not an `Expiry`: a delegated token that never expires is not a
/// state an exchange can return, so the forever variant is unrepresentable here.
#[derive(Debug, Clone)]
pub struct Delegated {
    token: Secret,
    not_after_unix_seconds: u64,
}

impl Delegated {
    /// What an implementor of [`DelegationExchange`] returns after validating the identity provider's answer.
    #[must_use]
    pub const fn new(token: Secret, not_after_unix_seconds: u64) -> Self {
        Self {
            token,
            not_after_unix_seconds,
        }
    }

    /// The token, moved out: a leg presents it once and nothing else keeps a copy.
    #[inline]
    #[must_use]
    pub fn into_token(self) -> Secret {
        self.token
    }

    #[inline]
    #[must_use]
    pub const fn not_after_unix_seconds(&self) -> u64 {
        self.not_after_unix_seconds
    }
}

/// Why an exchange produced no usable token.
///
/// **No variant carries token material or the identity provider's free text.** `error_description` is dropped
/// because an identity provider may echo its input there; the RFC 6749 `error` code survives only when it is the
/// registered shape (`[a-z_]`, at most 64 bytes), which no JWT can be.
#[derive(Debug, thiserror::Error)]
pub enum DelegationFailed {
    /// The token endpoint could not be reached or its answer not read.
    #[error("the identity provider's token endpoint could not be reached")]
    Unreachable {
        #[source]
        cause: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The identity provider answered with a non-success status.
    #[error("the identity provider refused the delegation exchange ({status}, {})", error.as_deref().unwrap_or("no error code"))]
    Refused {
        /// The HTTP status.
        status: u16,
        /// The RFC 6749 `error` code, when it had the registered shape.
        error: Option<String>,
    },
    /// The answer was larger than the deployment's cap.
    #[error("the identity provider's answer is over the {cap}-byte cap")]
    TooLarge {
        /// The cap.
        cap: u64,
    },
    /// The answer lacked a field this exchange needs, or the token is not a JWT with an `exp`.
    #[error("the identity provider's answer is malformed: {what}")]
    Malformed {
        /// Which part, named by this crate rather than quoted from the answer.
        what: &'static str,
    },
    /// The identity provider issued something other than an access token.
    #[error("the identity provider issued a token of another type than the access token requested")]
    WrongTokenType,
    /// The issued token does not carry the requested audience.
    #[error("the identity provider issued a token whose `aud` does not carry the requested audience")]
    WrongAudience,
    /// The issued token's `exp` is not after the instant it was checked at.
    #[error("the identity provider issued a token that has already expired")]
    AlreadyExpired,
    /// One hop of a [`Delegation`] chain failed, so no later hop ran and nothing is presented.
    #[error("delegation hop {hop} failed")]
    AtHop {
        /// Which hop, counted from 1 in declaration order.
        hop: usize,
        #[source]
        cause: Box<Self>,
    },
}

/// The port: one RFC 8693 exchange at the caller's own identity provider.
///
/// **Synchronous**, because [`CredentialBroker::mint`](crate::identity::CredentialBroker::mint) is and every
/// served caller of it is already on the blocking pool (`sutura_runtime::spawn_carrying_span`).
pub trait DelegationExchange: core::fmt::Debug + Send + Sync {
    /// Exchanges `subject` - the caller's own inbound token - for one carrying `audience`.
    ///
    /// # Errors
    ///
    /// [`DelegationFailed`]: nothing usable came back, and the broker refuses to present anything.
    fn exchange(&self, subject: &Secret, audience: &RequestedAudience) -> Result<Delegated, DelegationFailed>;
}

/// What one impersonating source exchanges through: an ordered chain of hops.
///
/// Each hop exchanges the previous hop's token - the caller's own inbound token for the first -
/// for one carrying that hop's audience.
///
/// `Arc` because a cloned broker shares its source's identity provider clients - one TLS agent,
/// one credential each - rather than building more. A composition root builds one per source that
/// declares a delegation, never one per deployment.
#[derive(Debug, Clone)]
pub struct Delegation {
    first: Hop,
    then: Vec<Hop>,
}

#[derive(Debug, Clone)]
struct Hop {
    exchange: Arc<dyn DelegationExchange>,
    audience: RequestedAudience,
}

impl Hop {
    fn run(&self, subject: &Secret, hop: usize) -> Result<Delegated, DelegationFailed> {
        self.exchange
            .exchange(subject, &self.audience)
            .map_err(|cause| DelegationFailed::AtHop {
                hop,
                cause: Box::new(cause),
            })
    }
}

impl Delegation {
    /// A chain of one hop.
    #[must_use]
    pub fn through(exchange: Arc<dyn DelegationExchange>, audience: RequestedAudience) -> Self {
        Self {
            first: Hop { exchange, audience },
            then: Vec::new(),
        }
    }

    /// This chain, with one more hop that exchanges its last token.
    #[must_use]
    pub fn then(mut self, exchange: Arc<dyn DelegationExchange>, audience: RequestedAudience) -> Self {
        self.then.push(Hop { exchange, audience });
        self
    }

    /// Runs every hop in order and returns the last one's token. An earlier hop's token is dropped
    /// as the next one is issued, and a failed hop stops the chain.
    ///
    /// # Errors
    ///
    /// [`DelegationFailed::AtHop`], naming the hop that failed.
    pub fn exchange(&self, subject: &Secret) -> Result<Delegated, DelegationFailed> {
        let mut held = self.first.run(subject, 1)?;
        for (at, hop) in self.then.iter().enumerate() {
            held = hop.run(&held.token, at.saturating_add(2))?;
        }
        Ok(held)
    }
}

#[cfg(test)]
mod tests;
