//! The signing keys, the cache in front of them, and the two things that make it re-read.
//!
//! `docs/adr/0014` names key rotation as one of three things a directly validating deployment newly
//! owns, and it names the standard way to get it wrong: *"Cache the key set, honour its cache
//! headers, refetch on an unknown key id - and **rate-limit that refetch**. Without the limit, a
//! forged key id turns every request into an outbound call to the authorization server, which is a
//! denial-of-service primitive pointed at our own dependency."*
//!
//! # Two triggers, and the second one is a REVOCATION bound rather than a rotation one
//!
//! The rate limit was the whole mechanism here once, and review found what that left open: a key
//! **removed** from the set kept verifying until an unrelated unknown key id happened to arrive. A
//! caller-driven refetch cannot bound revocation, because the caller presenting a revoked key
//! presents a `kid` this deployment *has* - so nothing triggers.
//!
//! So there are two triggers and they answer different questions:
//!
//! | Trigger | Answers | Bounded by |
//! | --- | --- | --- |
//! | an unknown key id | "has a key been ADDED that I have not seen" | [`MIN_REFETCH_INTERVAL`], because the trigger is caller-controlled |
//! | age | "has a key been REMOVED" | [`MAX_KEY_SET_AGE`], because the trigger is the clock and a caller cannot make it fire faster |
//!
//! The age trigger fires from two places, deliberately. [`KeySetCache::watch_until_shutdown`] is a
//! timer - the same shape `crate::tls::Renewal::watch_until_shutdown` already uses, spawned from the
//! composition root inside the runtime - so revocation latency is bounded *whether or not this
//! deployment is serving traffic*. And [`KeySetCache::key_for`] checks the age itself, so a
//! composition root that never armed the timer still cannot serve a stale key set indefinitely: the
//! first request past the horizon pays one file read. Neither is a second code path - both call
//! [`KeySetCache::poll_once`].
//!
//! # The bound is one read per window WHATEVER THE CONCURRENCY, and it was not
//!
//! Review measured three reads where two were required, from two concurrent misses. The cause was
//! double-checked locking with the second check missing: [`KeySetCache::key_for`] decided a look was
//! due from a value read under a *read* lock, and the write lock was taken only to stamp - so two
//! callers observing the same `last_attempt` both went on to read the source, and an attacker
//! amplified source I/O by the number of in-flight forged key ids.
//!
//! [`KeySetCache::reserve`] is the fix: **the check and the stamp are one lock acquisition**, the
//! source read stays outside the lock, and it is the only place either window is compared. The two
//! comparisons in `key_for` remain as a cheap fast path and decide nothing. Because `poll_once` is the
//! single path, the timer cannot race a caller into two reads either - which is a question worth
//! asking of a design with two triggers and is answered by there being one gate.
//!
//! # Why `now` is a parameter everywhere
//!
//! [`KeySetCache::key_for`] and [`KeySetCache::poll_once`] take the current instant rather than
//! reading the clock. That is what makes the interesting cases - a forged key id arriving inside the
//! window, a key set going stale, and two callers arriving at the same instant - assertable without a
//! sleep, which is the same reason `Renewal::poll_once` is public. **A concurrency bound proved by a
//! sleep being long enough is worse than none**, and this file is the second attempt at this bound.
//!
//! # What a key set is read from, and the gap that is named rather than hidden
//!
//! [`FileKeySet`] is the only source that ships. **There is no HTTPS fetcher**, and that is stated
//! here rather than left to be discovered: an outbound HTTP client is a supply-chain change with its
//! own review, and `docs/adr/0014` says plainly that the authorization server then becomes a hard
//! runtime dependency whose outage must stay *distinguishable from a dead data system*. None of that
//! is built.
//!
//! What is built is everything that a URL source would need anyway - the cache, both triggers, and
//! the limit on the caller-driven one - behind [`KeySetSource`], which is one method returning the
//! document's **bytes**. A JWKS endpoint arrives as a second implementor and changes nothing else in
//! this file. A file is also a real deployment shape rather than a placeholder: a sidecar that
//! rewrites a mounted key set is how a process with no egress gets rotation.
//!
//! **The honest cost of the file source:** it does not honour a cache header, because a file has
//! none. What bounds staleness is [`MAX_KEY_SET_AGE`] and nothing the issuer says.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use jsonwebtoken::DecodingKey;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, JwkSet};
use sutura_config::KeyFamily;
use sutura_runtime::Shutdown;

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

/// How stale a cached key set may be before it is re-read whatever a caller asks for.
///
/// **This is the revocation bound**, and it is the number a reviewer should argue with if they argue
/// with anything here: a key removed from the set keeps verifying for at most this long. A constant
/// rather than a key for the same reason the limit above is one - and unlike that limit, this one
/// only ever wants to be *smaller*, so the cost is what sets it. One minute is one small read per
/// minute per process, which is the same order as
/// `crate::middleware::REAP_INTERVAL` and is nothing next to a signature verification.
pub const MAX_KEY_SET_AGE: Duration = Duration::from_secs(60);

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
    /// Two keys under one id.
    ///
    /// **Refused rather than last-wins, which is what review found here.** A map insert made the last
    /// entry silently displace the first, so a key set holding two keys under one `kid` decided which
    /// one verifies by its position in a JSON array - and a rotation performed by *appending* the new
    /// key under the old id would then work, while the same document written the other way round
    /// would not. There is no reading of a JWK set in which two keys share an id on purpose.
    #[error(
        "the key set holds two keys under the id {id}, so which one verifies would be decided by their order in the document"
    )]
    DuplicateKeyId { id: KeyId },
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
    #[error("a key in the set uses a key family this deployment does not support")]
    UnsupportedKeyFamily,
    #[error("a key in the set has parameters this deployment cannot build a verifier from")]
    UnusableKey {
        #[source]
        cause: jsonwebtoken::errors::Error,
    },
    /// Not one key of the family the pinned algorithms need.
    ///
    /// **The refusal review asked for, and the failure it prevents is a deployment that starts and
    /// answers `401` to everybody.** The library verifies a token with one key and refuses a
    /// permitted-algorithm list whose family disagrees with that key, so a key set of RSA keys under
    /// `algorithms: ["ES256"]` cannot verify anything - and every request would be a `401` with
    /// nothing in the log connecting the two. `sutura_config::InvalidAlgorithms::MixedFamilies`
    /// refuses the same shape from the configuration side; this is the half that reads the keys.
    #[error(
        "the key set holds no {family} key, and this deployment pinned {pinned} - so no token could \
         ever verify. The key set and security.inbound.algorithms have to agree about the kind of \
         key"
    )]
    NoKeyOfThePinnedFamily { family: &'static str, pinned: String },
}

/// The verifying keys this deployment holds, by id.
///
/// A `BTreeMap` rather than a `HashMap`: a key set holds a handful of entries, the ordering makes a
/// log line and a test deterministic, and there is no hash-collision surface on a caller-supplied
/// lookup key at all.
#[derive(Clone)]
pub struct KeySet {
    keys: BTreeMap<KeyId, Verifier>,
}

/// One verifying key, and the kind of key it is.
///
/// The family is carried alongside rather than read back out of the library's own key, whose
/// equivalent field is private. It exists for [`KeySet::holds`], which is what makes
/// [`InvalidKeySet::NoKeyOfThePinnedFamily`] checkable at load rather than discoverable from a `401`.
#[derive(Clone)]
struct Verifier {
    key: DecodingKey,
    family: KeyFamily,
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
        let mut keys: BTreeMap<KeyId, Verifier> = BTreeMap::new();
        for jwk in &set.keys {
            let id = jwk
                .common
                .key_id
                .as_deref()
                .ok_or(InvalidKeySet::KeyWithoutAnId)
                .and_then(|raw| KeyId::parse(raw).map_err(|cause| InvalidKeySet::UnusableKeyId { cause }))?;
            if keys.contains_key(&id) {
                return Err(InvalidKeySet::DuplicateKeyId { id });
            }
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
        self.keys.get(id).map(|held| held.key.clone())
    }

    /// Does this set hold a key of `family`?
    ///
    /// The question [`InvalidKeySet::NoKeyOfThePinnedFamily`] is asked of. **At least one** rather
    /// than all of them, deliberately: an issuer legitimately publishes RSA and elliptic-curve keys in
    /// one document, and what makes a deployment unable to authenticate anybody is holding *none* of
    /// the kind it pinned.
    #[must_use]
    pub fn holds(&self, family: KeyFamily) -> bool {
        self.keys.values().any(|held| held.family == family)
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
/// inside a loop inside a fold. The family comes from the JWK's own parameters rather than from its
/// `alg`, which is optional and advisory.
fn verifier_for(jwk: &Jwk) -> Result<Verifier, InvalidKeySet> {
    let family = match jwk.algorithm {
        // See `InvalidKeySet::SymmetricKey`. This arm exists before the `from_jwk` call and not
        // after, because `from_jwk` would happily build an HMAC verifier out of it.
        AlgorithmParameters::OctetKey(_) => return Err(InvalidKeySet::SymmetricKey),
        AlgorithmParameters::RSA(_) => KeyFamily::Rsa,
        AlgorithmParameters::EllipticCurve(_) => KeyFamily::EllipticCurve,
        AlgorithmParameters::OctetKeyPair(_) => KeyFamily::EdwardsCurve,
        _ => return Err(InvalidKeySet::UnsupportedKeyFamily),
    };
    let key = DecodingKey::from_jwk(jwk).map_err(|cause| InvalidKeySet::UnusableKey { cause })?;
    Ok(Verifier { key, family })
}

/// Where a key set is read from.
///
/// One method, so a JWKS endpoint is a second implementor and nothing else in this file moves. See
/// the module documentation for why the only implementor today reads a file.
///
/// **It returns the document's BYTES rather than a parsed key set**, and that is what lets
/// [`KeySetCache::poll_once`] tell "changed" from "unchanged" the way `crate::tls::Renewal` does. A
/// comparison of parsed keys could not: the library's key type implements no equality, so the
/// alternative was comparing key *ids*, which would miss a key whose material rotated under the same
/// id.
///
/// **Synchronous, deliberately.** The one implementor reads a small local file, at most once per
/// [`MAX_KEY_SET_AGE`], and making the trait `async` would either need a boxed future in the
/// signature or force the file source to pretend. A URL source arrives with a real decision about
/// where its I/O runs, and that decision belongs in the same change as the client.
pub trait KeySetSource: Send + Sync + 'static {
    /// Reads the key set document as it is now.
    fn read(&self) -> Result<String, KeySetUnavailable>;
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
    /// Names the file. Does not read it: [`Self::read`] is the read, and the composition root reads
    /// once before the listener opens so an unreadable key set is a refusal to start.
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
    fn read(&self) -> Result<String, KeySetUnavailable> {
        std::fs::read_to_string(&self.path).map_err(|cause| KeySetUnavailable::Unreadable {
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

/// What one look at the source did.
///
/// The same three outcomes `crate::tls::Renewed` has, and for the same reasons: an unreadable source
/// is not a change, and a candidate that was examined and rejected is recorded as examined so
/// identical bytes on the next tick are silent rather than logging a rejection once per interval
/// forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refreshed {
    /// The document is byte-for-byte what is already in use, or it could not be read.
    Unchanged,
    /// A new document parsed, held a key of the pinned family, and is now in use.
    Rotated,
    /// A new document was read and is NOT usable. The previous key set keeps verifying.
    Rejected,
    /// **The source was not looked at**, because the window has not opened or another caller already
    /// reserved this look.
    ///
    /// A fourth variant rather than folding into [`Self::Unchanged`], and the distinction is the whole
    /// point of the guard it reports: "the document did not change" is a fact about the source, and
    /// "nobody looked" is a fact about this deployment. Collapsing them would make the bound that
    /// [`KeySetCache::reserve`] enforces unobservable, and a bound nothing can observe is one nothing
    /// can test - which is how the concurrent case got past the first round.
    NotDue,
}

/// What the cache holds between reads.
struct Cached {
    keys: KeySet,
    /// The bytes [`Cached::keys`] was parsed from, so a re-read can be compared rather than reparsed
    /// into a value nothing can compare.
    document: String,
    /// When the source was last **attempted**, whether or not it answered.
    ///
    /// Attempted and not succeeded, and that is the whole rate limit: an issuer that is down would
    /// otherwise mean an outbound call per request, which is the same primitive the forged key id
    /// opens, arriving from the other direction.
    last_attempt: Instant,
}

/// The key set, cached, with a rate-limited refetch on an unknown key id and an age bound on the
/// whole set.
///
/// `tokio::sync::RwLock` rather than `std::sync::RwLock`, which `clippy.toml` bans: this is held
/// across an `await` in an async middleware, which is exactly the deadlock that ban is for.
///
/// **No read of the source happens while the write lock is held**, which review asked for: the lock
/// is taken to stamp the attempt, released, the document read, and taken again to swap. The stamp
/// under the first lock is what keeps two concurrent misses from becoming two reads.
pub struct KeySetCache {
    source: Box<dyn KeySetSource>,
    refetch_interval: Duration,
    max_age: Duration,
    /// The family the pinned algorithms need, so a swap cannot adopt a key set that verifies nothing.
    family: KeyFamily,
    /// What the pinned algorithms are, for the refusal's message alone.
    pinned: String,
    state: tokio::sync::RwLock<Cached>,
}

impl KeySetCache {
    /// Reads the source once and caches what it returned.
    ///
    /// **Fails rather than starting empty**, which is what makes an unreadable key set a refusal to
    /// start: a cache that began empty would answer every request `401` while looking healthy, and the
    /// rate limit would keep it that way for thirty seconds at a time. It also fails when the document
    /// holds no key of the pinned family - see [`InvalidKeySet::NoKeyOfThePinnedFamily`].
    pub fn primed(
        source: Box<dyn KeySetSource>,
        family: KeyFamily,
        pinned: String,
        now: Instant,
    ) -> Result<Self, KeySetUnavailable> {
        Self::primed_with_window(source, family, pinned, MIN_REFETCH_INTERVAL, MAX_KEY_SET_AGE, now)
    }

    /// The same, with the two windows a test can shorten.
    ///
    /// `pub(crate)` and `cfg(test)`-free because the two windows are not a deployment's choice - see
    /// [`MIN_REFETCH_INTERVAL`] and [`MAX_KEY_SET_AGE`] - and the only reason they are parameters at
    /// all is that a test asserting a window *reopens* would otherwise have to sleep for it. It is not
    /// `cfg(test)` because [`Self::primed`] is written in terms of it.
    pub(crate) fn primed_with_window(
        source: Box<dyn KeySetSource>,
        family: KeyFamily,
        pinned: String,
        refetch_interval: Duration,
        max_age: Duration,
        now: Instant,
    ) -> Result<Self, KeySetUnavailable> {
        let document = source.read()?;
        let keys = parse_for(&document, family, &pinned).map_err(|cause| KeySetUnavailable::Invalid {
            path: PathBuf::from("the configured key set"),
            cause,
        })?;
        Ok(Self {
            source,
            refetch_interval,
            max_age,
            family,
            pinned,
            state: tokio::sync::RwLock::new(Cached {
                keys,
                document,
                last_attempt: now,
            }),
        })
    }

    /// The verifier for a key id, re-reading the source when the set is stale or the id is unknown.
    ///
    /// The order is: read, then age or window, then [`Self::poll_once`]. **The two comparisons here are
    /// a cheap FAST PATH and not the decision**, which is the correction review forced: they run under a
    /// read lock, so two callers can both observe the same `last_attempt` and both fall through. Whether
    /// a look at the source actually happens is decided once, atomically, inside `poll_once` - see
    /// [`Self::reserve`].
    ///
    /// **What a caller that loses the reservation gets, stated because it is a real outcome:** it does
    /// not wait. It answers from whatever is cached, which during a rotation may be an
    /// [`KeyUnavailable::UnknownKeyId`] for a key the winner is about to install, or - on the age path -
    /// one more use of a key the winner is about to remove. So revocation is bounded by
    /// [`MAX_KEY_SET_AGE`] plus the duration of one source read, and a rotation can cost a concurrent
    /// caller one `401` it can retry. Making it wait instead would put N request tasks behind one file
    /// read, which is the primitive this whole file is arranged against.
    pub async fn key_for(&self, id: &KeyId, now: Instant) -> Result<DecodingKey, KeyUnavailable> {
        // The read guard is taken, read from twice, and dropped inside this statement. Written as one
        // expression rather than as a block with early returns because `clippy` flags a lock guard
        // that outlives its last use, and here that lint and the right shape agree.
        let (found, since) = {
            let cached = self.state.read().await;
            (cached.keys.get(id), now.saturating_duration_since(cached.last_attempt))
        };
        if let Some(key) = found {
            // A HIT is still subject to the age bound, and that is the revocation fix: the caller
            // presenting a revoked key presents an id this deployment holds, so nothing else here
            // would ever trigger a re-read. The read is skipped while the set is fresh, so the
            // ordinary request pays a lock and a map lookup.
            if since < self.max_age {
                return Ok(key);
            }
            let _refreshed = self.poll_once(now).await;
            // Re-read after the refresh: the key may have been REVOKED by it, and then falling through
            // to the unknown-id refusal is exactly right. The guard is taken and dropped inside this
            // statement, for the reason the one above is.
            let current = { self.state.read().await.keys.get(id) };
            return current.ok_or(KeyUnavailable::UnknownKeyId);
        }
        if since < self.refetch_interval {
            return Err(KeyUnavailable::RefetchRateLimited {
                ago_ms: since.as_millis(),
            });
        }
        let _refreshed = self.poll_once(now).await;
        let current = { self.state.read().await.keys.get(id) };
        current.ok_or(KeyUnavailable::UnknownKeyId)
    }

    /// Atomically reserves the next look at the source, or says that somebody else has it.
    ///
    /// **THE fix for the finding this file's own bound was reported for.** The shape it replaces was
    /// classic double-checked locking with the second check missing: [`Self::key_for`] decided a look
    /// was due from a value it had read under a *read* lock, and the write lock was then taken only to
    /// stamp. Two callers both observing the old `last_attempt` therefore both went on to read the
    /// source - measured at three reads where two were required - so the "one read per window" bound
    /// that this module, a test name and `docs/adr/0014` all state held only when nothing was
    /// concurrent.
    ///
    /// The check and the stamp are now one lock acquisition, so exactly one caller wins whatever the
    /// interleaving. The source read stays *outside* the lock, which is the other half of what review
    /// asked for: a reservation is cheap and a read is not.
    ///
    /// **The window is the SHORTER of the two**, and that is not arbitrary. This function answers "may
    /// anybody look right now", while its callers answer "is a look due"; a reservation window longer
    /// than a trigger's own horizon would neuter that trigger - an age bound of one second under a
    /// thirty-second reservation would never fire. In a shipped deployment the shorter one is
    /// [`MIN_REFETCH_INTERVAL`], because [`MAX_KEY_SET_AGE`] is twice it.
    async fn reserve(&self, now: Instant) -> bool {
        let window = self.refetch_interval.min(self.max_age);
        let mut cached = self.state.write().await;
        if now.saturating_duration_since(cached.last_attempt) < window {
            return false;
        }
        // Stamped here, under the same acquisition as the comparison above, and BEFORE the read: a
        // source that fails, or one that hangs and then fails, has still consumed the window - which is
        // the same primitive arriving from the other direction.
        cached.last_attempt = now;
        true
    }

    /// Looks at the source if a look is due, and swaps the key set if what came back is usable and
    /// different.
    ///
    /// Public and taking `now`, for the reason `crate::tls::Renewal::poll_once` is public: a test
    /// rotating a key set must not have to wait for a timer, and asserting that a timer fires is a
    /// different assertion from asserting that a rotation works.
    ///
    /// **It is DUE-CHECKED, which is a deliberate departure from `Renewal::poll_once`.** That one looks
    /// unconditionally, because a TLS renewal watch is the only thing that calls it. This one is called
    /// by the timer *and* by two paths in [`Self::key_for`], one of which a caller triggers - so there
    /// has to be exactly one place the reservation is taken, or the paths are three chances to get the
    /// bound wrong. **That is also the answer to whether the timer can race a caller into two reads: it
    /// cannot, because they are the same path.**
    ///
    /// **Loud, and then carry on with what works.** A document that will not parse, or that holds no
    /// key of the pinned family, is [`Refreshed::Rejected`] and the previous set keeps verifying -
    /// adopting a broken set would turn a rotation mistake into a total outage, which is the trade
    /// `crate::tls` already makes for the same reason.
    pub async fn poll_once(&self, now: Instant) -> Refreshed {
        if !self.reserve(now).await {
            return Refreshed::NotDue;
        }
        let read = match self.source.read() {
            Ok(document) => document,
            Err(cause) => {
                // Unreadable is deliberately not "changed": a mounted secret being swapped can make a
                // path briefly absent, and treating that as a rotation would reject on every swap.
                tracing::warn!(error = %cause, "could not re-read the key set; keeping the keys in use");
                return Refreshed::Unchanged;
            }
        };
        let candidate = parse_for(&read, self.family, &self.pinned);
        // The guard is taken after the parse and dropped inside this statement, so nothing holds it
        // across work it does not need held.
        let outcome = {
            let mut cached = self.state.write().await;
            adopt(&mut cached, read, candidate)
        };
        announce(outcome);
        outcome
    }

    /// Polls on an interval until shutdown is asked for.
    ///
    /// A `tokio` task rather than the OS thread `crate::middleware::spawn_reaper` uses, and the
    /// difference is where it is started from - the same difference `crate::tls::Renewal` records: the
    /// reaper is spawned while the router is assembled, before a runtime exists, and this is spawned
    /// from inside the served future.
    ///
    /// **Forgetting to arm it does not leave revocation unbounded**, which is why it is not the only
    /// trigger: [`Self::key_for`] checks the age itself, so a deployment serving traffic re-reads
    /// anyway. What the timer adds is a bound that holds while nothing is being asked.
    pub fn watch_until_shutdown(cache: &Arc<Self>, interval: Duration, shutdown: Shutdown) {
        let watched = Arc::downgrade(cache);
        drop(tokio::spawn(poll_until_shutdown(watched, interval, shutdown)));
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

/// Decides what a freshly read document does to the cache, under the write lock.
///
/// Split out of [`KeySetCache::poll_once`] because the three outcomes plus two log sites were over the
/// cognitive-complexity threshold in `clippy.toml` - and the split is where the reasoning is anyway:
/// this function is *what a candidate does to the state*, and nothing in it awaits or reads a source.
///
/// **The document is recorded whichever way it went**, including when it was rejected. That is the
/// distinction `crate::tls::Renewal::seen` already carries: a candidate that was examined and refused
/// is still examined, so identical bytes on the next tick are silent rather than logging a rejection
/// once per interval forever. The first look at it said everything, loudly.
fn adopt(cached: &mut Cached, read: String, candidate: Result<KeySet, InvalidKeySet>) -> Refreshed {
    if read == cached.document {
        return Refreshed::Unchanged;
    }
    cached.document = read;
    match candidate {
        Ok(keys) => {
            cached.keys = keys;
            Refreshed::Rotated
        }
        Err(_refused) => Refreshed::Rejected,
    }
}

/// Says what one look did, at the level the outcome deserves.
///
/// A function of its own so [`KeySetCache::poll_once`] holds no lock while a `tracing` macro expands,
/// and so the levels are decided in one place: a rotation is `info`, a refused candidate is `error`
/// because the deployment is now serving keys that disagree with what is on disk, and no change says
/// nothing at all - a quiet deployment must not emit a line a minute saying so.
fn announce(outcome: Refreshed) {
    match outcome {
        // Both silent, and for one reason: a quiet deployment must not emit a line a minute saying
        // that nothing happened. `NotDue` is reached on every request past the age horizon that loses
        // the reservation, so it is the noisiest of the four and says the least.
        Refreshed::Unchanged | Refreshed::NotDue => {}
        Refreshed::Rotated => tracing::info!("the key set was re-read and replaced"),
        Refreshed::Rejected => tracing::error!(
            "the key set at the configured path changed and is NOT usable; still verifying with the \
             previous keys. Nothing will rotate until it parses and holds a key of the pinned family"
        ),
    }
}

/// Parses a document and refuses one that cannot verify anything this deployment would accept.
///
/// The family check is here rather than in [`KeySet::parse`] so that every path which adopts a key
/// set - priming and every refresh - goes through the same refusal. A swap that skipped it would turn
/// a startup refusal into a `401` for everybody an hour later.
fn parse_for(document: &str, family: KeyFamily, pinned: &str) -> Result<KeySet, InvalidKeySet> {
    let keys = KeySet::parse(document)?;
    if keys.holds(family) {
        return Ok(keys);
    }
    Err(InvalidKeySet::NoKeyOfThePinnedFamily {
        family: family.as_str(),
        pinned: String::from(pinned),
    })
}

/// The refresh watch's body. Stops when shutdown is asked for, or when the gate is dropped.
///
/// `Weak` and not `Arc`, so the watch cannot be the reason a gate stays alive - the same argument
/// `crate::middleware::LimiterHandle::watch` makes: a test that builds a gate per test must not
/// accumulate one task per test.
#[expect(
    clippy::integer_division_remainder_used,
    reason = "the `select!` macro expands through remainder arithmetic to pick a poll order; nothing here does"
)]
async fn poll_until_shutdown(watched: Weak<KeySetCache>, interval: Duration, shutdown: Shutdown) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(interval) => {
                let Some(cache) = watched.upgrade() else {
                    tracing::debug!("the inbound identity gate was dropped; the key set watch is stopping");
                    return;
                };
                let _outcome = cache.poll_once(Instant::now()).await;
            }
            reason = shutdown.requested() => {
                tracing::debug!(%reason, "the key set watch is stopping");
                return;
            }
        }
    }
}

impl core::fmt::Debug for KeySetCache {
    /// Hand-written because the source is a trait object and the state is behind a lock this must not
    /// take to render a log line.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("KeySetCache")
            .field("refetch_interval", &self.refetch_interval)
            .field("max_age", &self.max_age)
            .field("family", &self.family)
            .finish_non_exhaustive()
    }
}
