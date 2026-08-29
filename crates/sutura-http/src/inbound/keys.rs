//! The signing keys, the cache in front of them, and the rate limit on refetching.
//!
//! `docs/adr/0014` names key rotation as one of three things a directly validating deployment newly
//! owns, and it names the standard way to get it wrong: *"Cache the key set, honour its cache
//! headers, refetch on an unknown key id - and **rate-limit that refetch**. Without the limit, a
//! forged key id turns every request into an outbound call to the authorization server, which is a
//! denial-of-service primitive pointed at our own dependency."*
//!
//! So the rate limit is the mechanism in this file, and it is the reason [`KeySetCache::key_for`]
//! takes the current instant as an argument rather than reading the clock itself: that is what makes
//! the interesting case - the second request with a forged key id, arriving inside the window -
//! assertable without a sleep.
//!
//! # What a key set is read from, and the gap that is named rather than hidden
//!
//! [`FileKeySet`] is the only source that ships. **There is no HTTPS fetcher**, and that is stated
//! here rather than left to be discovered: an outbound HTTP client is a supply-chain change with its
//! own review, and `docs/adr/0014` says plainly that the authorization server then becomes a hard
//! runtime dependency whose outage must stay *distinguishable from a dead data system*. None of that
//! is built.
//!
//! What is built is everything that a URL source would need anyway - the cache, the unknown-key
//! refetch, and the limit on it - behind [`KeySetSource`], which is one method. A JWKS endpoint
//! arrives as a second implementor and changes nothing else in this file. A file is also a real
//! deployment shape rather than a placeholder: a sidecar that refreshes a mounted key set is how a
//! process with no egress gets rotation.
//!
//! **The honest cost of the file source:** it does not honour a cache header, because a file has
//! none. Staleness is bounded by whatever rewrites the file, plus this cache's own refetch on an
//! unknown key id - so a key that rotated *without* its id changing is a key this deployment keeps
//! using until something asks for an id it has not got. Issuers do not do that, and nothing here
//! stops one that did.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use jsonwebtoken::DecodingKey;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, JwkSet};

/// The longest key id accepted.
///
/// The `kid` in a token header is **caller-supplied**, so it is bounded before anything is done with
/// it: it is compared against a map, it decides whether an outbound refetch happens, and it reaches a
/// log line. A hundred and twenty-eight characters is wider than any issuer's thumbprint and narrow
/// enough that it cannot carry a paragraph into a record.
const MAX_KEY_ID: usize = 128;

/// How long after one attempt to reach the source another may be made.
///
/// **The rate limit `docs/adr/0014` asks for**, as a constant rather than a configuration key. It is
/// not a posture decision - nothing a caller can do changes what the right answer is - and a knob
/// here would only ever be set wrong, in the direction that reopens the denial-of-service primitive.
/// Thirty seconds is far below any horizon at which a rotation is late and far above the cost of a
/// forged key id.
pub const MIN_REFETCH_INTERVAL: Duration = Duration::from_secs(30);

/// A key identifier, out of a token header or out of a key set.
///
/// A newtype rather than a `String` because the value arrives from a caller and is then used as a map
/// key, as the trigger for an outbound fetch, and as a log field. The field is private and
/// [`Self::parse`] is the only way in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(String);

/// Why a string is not a key identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotAKeyId {
    #[error("a key id must not be empty")]
    Empty,
    #[error("a key id may be at most {limit} characters and this one is {found}")]
    TooLong { found: usize, limit: usize },
    /// Anything outside printable ASCII.
    ///
    /// The position and never the value, for the reason every refusal in this crate names a position:
    /// a `kid` is caller-supplied, and echoing one into a message puts caller text into a log. A
    /// newline in particular would append a line nobody wrote, in the record whose job is attribution.
    #[error("a key id holds a character at position {position} that could not have come from a JWK")]
    NotPrintable { position: usize },
}

impl KeyId {
    /// Parses a key identifier.
    ///
    /// **No trim, deliberately**, unlike almost every other parse in this workspace. A `kid` is an
    /// opaque identifier an issuer chose and we compare byte for byte against a key set the same
    /// issuer published; trimming would make a token whose id has a trailing space match a key whose
    /// id does not, and the two are different strings to whoever wrote them.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, NotAKeyId> {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(NotAKeyId::Empty);
        }
        let found = raw.chars().count();
        if found > MAX_KEY_ID {
            return Err(NotAKeyId::TooLong {
                found,
                limit: MAX_KEY_ID,
            });
        }
        for (position, character) in raw.chars().enumerate() {
            // Printable ASCII and nothing else, which refuses every control character and every
            // invisible or direction-changing code point in one check - all of them are outside it.
            if !character.is_ascii_graphic() {
                return Err(NotAKeyId::NotPrintable { position });
            }
        }
        Ok(Self(String::from(raw)))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for KeyId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a document is not a usable key set.
#[derive(Debug, thiserror::Error)]
pub enum InvalidKeySet {
    #[error("the key set is not a JWK set")]
    NotAJwkSet {
        #[source]
        cause: serde_json::Error,
    },
    #[error("the key set holds no key this deployment could verify a signature with")]
    NoUsableKey,
    /// A key with no `kid`.
    ///
    /// Refused rather than skipped, and the difference matters: a key set whose keys have no ids
    /// cannot be looked up by the id a token carries, so a deployment reading one would fall back to
    /// "try every key" - which is what makes a rotation invisible and what makes an unknown key id
    /// indistinguishable from a wrong signature.
    #[error("a key in the set has no `kid`, so no token could name it")]
    KeyWithoutAnId,
    #[error("a key in the set has an id that could not have come from a JWK")]
    UnusableKeyId {
        #[source]
        cause: NotAKeyId,
    },
    /// A symmetric key.
    ///
    /// **The second place algorithm confusion dies, and it is here rather than only in the pinned
    /// algorithms because the two guard different halves.** A pinned list cannot name `HS256` - the
    /// enum has no such variant - and a key set holding an `oct` key would still hand this library a
    /// key of the HMAC family, whose *shared secret* is a value the issuer published. Both have to be
    /// closed for the attack to be closed.
    #[error(
        "a key in the set is a symmetric (`oct`) key. Its secret is published in the key set, so \
         anybody who can read the key set can mint a token: this is asymmetric validation and there \
         is no configuration in which that key belongs here"
    )]
    SymmetricKey,
    #[error("a key in the set has parameters this deployment cannot build a verifier from")]
    UnusableKey {
        #[source]
        cause: jsonwebtoken::errors::Error,
    },
}

/// The verifying keys this deployment holds, by id.
///
/// A `BTreeMap` rather than a `HashMap`: a key set holds a handful of entries, the ordering makes a
/// log line and a test deterministic, and there is no hash-collision surface on a caller-supplied
/// lookup key at all.
#[derive(Clone)]
pub struct KeySet {
    keys: BTreeMap<KeyId, DecodingKey>,
}

impl KeySet {
    /// Parses a JWK set document.
    ///
    /// **Every refusal here is a refusal to start, not a key that gets skipped.** A key set is
    /// operator-supplied configuration and a deployment that silently dropped half of it would
    /// authenticate an arbitrary subset of its callers - which reads exactly like an intermittent
    /// outage. `docs/adr/0014`'s posture is fail-closed on the query path and this is that.
    pub fn parse(document: &str) -> Result<Self, InvalidKeySet> {
        let set: JwkSet = serde_json::from_str(document).map_err(|cause| InvalidKeySet::NotAJwkSet { cause })?;
        let mut keys = BTreeMap::new();
        for jwk in &set.keys {
            let id = jwk
                .common
                .key_id
                .as_deref()
                .ok_or(InvalidKeySet::KeyWithoutAnId)
                .and_then(|raw| KeyId::parse(raw).map_err(|cause| InvalidKeySet::UnusableKeyId { cause }))?;
            drop(keys.insert(id, verifier_for(jwk)?));
        }
        if keys.is_empty() {
            return Err(InvalidKeySet::NoUsableKey);
        }
        Ok(Self { keys })
    }

    /// The verifier for one key id, if this set holds it.
    ///
    /// Cloned rather than borrowed, because the caller is about to await a lock release and then
    /// verify: a `DecodingKey` is a small owned value - a modulus and an exponent, or a point - and
    /// holding a read lock across the verification would serialise every request behind it.
    #[must_use]
    pub fn get(&self, id: &KeyId) -> Option<DecodingKey> {
        self.keys.get(id).cloned()
    }

    /// How many keys are held. For a startup log line, so an operator can see the set was read.
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.keys.len()
    }

    /// The ids, for a log line and for a test.
    #[must_use]
    pub fn ids(&self) -> Vec<&KeyId> {
        self.keys.keys().collect()
    }
}

impl core::fmt::Debug for KeySet {
    /// Hand-written, because `DecodingKey` is not `Debug` and because what a reader wants named is
    /// which ids are held. **A verifying key is public material and is still not printed**: it is
    /// large, it is not what anybody is looking for, and the habit of not printing key material is
    /// worth more than the one line where it would have been harmless.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("KeySet").field("ids", &self.ids()).finish()
    }
}

/// Builds a verifier for one JWK, refusing the kinds that must not be here.
///
/// Split out of [`KeySet::parse`] so the symmetric refusal is one named branch rather than an arm
/// inside a loop inside a fold.
fn verifier_for(jwk: &Jwk) -> Result<DecodingKey, InvalidKeySet> {
    match jwk.algorithm {
        // See `InvalidKeySet::SymmetricKey`. This arm exists before the `from_jwk` call and not
        // after, because `from_jwk` would happily build an HMAC verifier out of it.
        AlgorithmParameters::OctetKey(_) => Err(InvalidKeySet::SymmetricKey),
        AlgorithmParameters::RSA(_) | AlgorithmParameters::EllipticCurve(_) | AlgorithmParameters::OctetKeyPair(_) => {
            DecodingKey::from_jwk(jwk).map_err(|cause| InvalidKeySet::UnusableKey { cause })
        }
    }
}

/// Where a key set is read from.
///
/// One method, so a JWKS endpoint is a second implementor and nothing else in this file moves. See
/// the module documentation for why the only implementor today reads a file.
///
/// **Synchronous, deliberately.** The one implementor reads a small local file, at most once per
/// [`MIN_REFETCH_INTERVAL`], and making the trait `async` would either need a boxed future in the
/// signature or force the file source to pretend. A URL source arrives with a real decision about
/// where its I/O runs, and that decision belongs in the same change as the client.
pub trait KeySetSource: Send + Sync + 'static {
    /// Reads the current key set.
    fn fetch(&self) -> Result<KeySet, KeySetUnavailable>;
}

/// The source could not be read, or what it returned is not a key set.
#[derive(Debug, thiserror::Error)]
pub enum KeySetUnavailable {
    #[error("the key set at {path} could not be read")]
    Unreadable {
        path: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    #[error("the key set at {path} is not usable")]
    Invalid {
        path: PathBuf,
        #[source]
        cause: InvalidKeySet,
    },
}

/// A key set on the local filesystem.
#[derive(Debug, Clone)]
pub struct FileKeySet {
    path: PathBuf,
}

impl FileKeySet {
    /// Names the file. Does not read it: [`Self::fetch`] is the read, and the composition root calls
    /// it once before the listener opens so an unreadable key set is a refusal to start.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The path, for a startup log line.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl KeySetSource for FileKeySet {
    fn fetch(&self) -> Result<KeySet, KeySetUnavailable> {
        let document = std::fs::read_to_string(&self.path).map_err(|cause| KeySetUnavailable::Unreadable {
            path: self.path.clone(),
            cause,
        })?;
        KeySet::parse(&document).map_err(|cause| KeySetUnavailable::Invalid {
            path: self.path.clone(),
            cause,
        })
    }
}

/// Why a token could not be matched to a verifying key.
///
/// Separate from the token's own refusals because the two are different facts about a deployment: a
/// signature that does not verify is a bad token, and a key id nobody has heard of after a refetch is
/// either a rotation this deployment has not caught up with or a caller guessing.
#[derive(Debug, thiserror::Error)]
pub enum KeyUnavailable {
    /// The id is not in the set, and it is too soon to look again.
    ///
    /// **This is the rate limit firing, and it is a refusal rather than a wait.** Blocking the request
    /// until the window opens would make the denial-of-service primitive a slow one instead of
    /// removing it: N forged key ids would hold N request tasks. The caller is told it is
    /// unauthenticated, which is true, and the retry window is on the log line rather than in the
    /// response - a caller does not need to know when this deployment will next talk to its issuer.
    #[error("no key with that id, and the last attempt to read the key set was {ago_ms}ms ago")]
    RefetchRateLimited { ago_ms: u128 },
    /// The id is not in the set after a fresh read.
    #[error("the key set does not hold the key this token names")]
    UnknownKeyId,
    #[error("the key set could not be re-read")]
    SourceUnavailable {
        #[source]
        cause: KeySetUnavailable,
    },
}

/// What the cache holds between reads.
struct Cached {
    keys: KeySet,
    /// When the source was last **attempted**, whether or not it answered.
    ///
    /// Attempted and not succeeded, and that is the whole rate limit: an issuer that is down would
    /// otherwise mean an outbound call per request, which is the same primitive the forged key id
    /// opens, arriving from the other direction.
    last_attempt: Instant,
}

/// The key set, cached, with a rate-limited refetch on an unknown key id.
///
/// `tokio::sync::RwLock` rather than `std::sync::RwLock`, which `clippy.toml` bans: this is held
/// across an `await` in an async middleware, which is exactly the deadlock that ban is for. The write
/// side is taken only when a key id is missing *and* the window has opened, so the ordinary request
/// takes a read lock and nothing else.
pub struct KeySetCache {
    source: Box<dyn KeySetSource>,
    refetch_interval: Duration,
    state: tokio::sync::RwLock<Cached>,
}

impl KeySetCache {
    /// Reads the source once and caches what it returned.
    ///
    /// **Fails rather than starting empty**, which is what makes an unreadable key set a refusal to
    /// start: a cache that began empty would answer every request `401` while looking healthy, and the
    /// rate limit would keep it that way for thirty seconds at a time.
    pub fn primed(source: Box<dyn KeySetSource>, now: Instant) -> Result<Self, KeySetUnavailable> {
        let keys = source.fetch()?;
        Ok(Self {
            source,
            refetch_interval: MIN_REFETCH_INTERVAL,
            state: tokio::sync::RwLock::new(Cached { keys, last_attempt: now }),
        })
    }

    /// The same, with a refetch window a test can shorten.
    ///
    /// `#[cfg(test)]` and `pub(crate)`, which together are the point: the window is not a deployment's
    /// choice - see [`MIN_REFETCH_INTERVAL`] - and the only reason it is a parameter at all is that a
    /// test asserting the window *reopens* would otherwise have to sleep for thirty seconds. Compiling
    /// it out of a shipped build is what keeps it from becoming a second constructor somebody reaches
    /// for.
    #[cfg(test)]
    pub(crate) fn primed_with_window(
        source: Box<dyn KeySetSource>,
        refetch_interval: Duration,
        now: Instant,
    ) -> Result<Self, KeySetUnavailable> {
        let keys = source.fetch()?;
        Ok(Self {
            source,
            refetch_interval,
            state: tokio::sync::RwLock::new(Cached { keys, last_attempt: now }),
        })
    }

    /// The verifier for a key id, refetching at most once per window when the id is unknown.
    ///
    /// **`now` is a parameter and not `Instant::now()`**, so the case that matters - a second forged
    /// key id arriving inside the window - is assertable without a sleep. The middleware passes the
    /// real clock, once, at the top of the request.
    ///
    /// The order is: read, then window, then write-and-recheck. The recheck under the write lock is
    /// not belt-and-braces: two requests naming the same unknown id race here, and without it the
    /// second would fetch again immediately after the first - one refetch per concurrent request,
    /// which is the limit not holding.
    pub async fn key_for(&self, id: &KeyId, now: Instant) -> Result<DecodingKey, KeyUnavailable> {
        // The read guard is taken, read from twice, and dropped inside this statement. Written as one
        // expression rather than as a block with two early returns because `clippy` flags a lock guard
        // that outlives its last use, and here that lint and the right shape agree: nothing between
        // the read and the decision needs the lock held.
        let (found, since) = {
            let cached = self.state.read().await;
            (cached.keys.get(id), now.saturating_duration_since(cached.last_attempt))
        };
        if let Some(key) = found {
            return Ok(key);
        }
        if since < self.refetch_interval {
            return Err(KeyUnavailable::RefetchRateLimited {
                ago_ms: since.as_millis(),
            });
        }
        let mut cached = self.state.write().await;
        // Re-read under the write lock: another task may have refetched while this one waited for it,
        // and then this id is either present or was already looked for.
        if let Some(key) = cached.keys.get(id) {
            return Ok(key);
        }
        let since = now.saturating_duration_since(cached.last_attempt);
        if since < self.refetch_interval {
            return Err(KeyUnavailable::RefetchRateLimited {
                ago_ms: since.as_millis(),
            });
        }
        // Stamped BEFORE the fetch, so a source that fails - or one that hangs and then fails - has
        // still consumed the window. See `Cached::last_attempt`.
        cached.last_attempt = now;
        let fetched = self
            .source
            .fetch()
            .map_err(|cause| KeyUnavailable::SourceUnavailable { cause })?;
        cached.keys = fetched;
        cached.keys.get(id).ok_or(KeyUnavailable::UnknownKeyId)
    }

    /// How many keys are cached, and their ids. For a startup log line and for a test.
    pub async fn describe(&self) -> (usize, Vec<String>) {
        let cached = self.state.read().await;
        (
            cached.keys.count(),
            cached.keys.ids().into_iter().map(KeyId::to_string).collect(),
        )
    }
}

impl core::fmt::Debug for KeySetCache {
    /// Hand-written because the source is a trait object and the state is behind a lock this must not
    /// take to render a log line.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("KeySetCache")
            .field("refetch_interval", &self.refetch_interval)
            .finish_non_exhaustive()
    }
}
