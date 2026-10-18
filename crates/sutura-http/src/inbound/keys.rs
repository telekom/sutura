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
//! The second row holds **while the source answers**, and not otherwise - see *the revocation bound
//! excludes a failing refresh*, below.
//!
//! The age trigger fires from two places, both through [`KeySetCache::poll_once`]: the timer
//! [`KeySetCache::watch_until_shutdown`] bounds revocation *whether or not this deployment is serving
//! traffic*, and [`KeySetCache::key_for`] checks the age itself, so a root that never armed the timer
//! still cannot serve a stale key set indefinitely - the first request past the horizon pays one read.
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
//! single path, the timer cannot race a caller into two reads either.
//!
//! # Why `now` is a parameter everywhere
//!
//! [`KeySetCache::key_for`] and [`KeySetCache::poll_once`] take the instant, so a forged key id inside
//! the window, a stale set and two callers at one instant are assertable without a sleep. **A
//! concurrency bound proved by a sleep being long enough is worse than none.**
//!
//! # Where the read runs, and the three separate things that bound it
//!
//! [`KeySetSource::read`] is synchronous and the parse behind it is CPU work over a foreign
//! document, and both used to run inline in [`KeySetCache::poll_once`] - on the async worker thread
//! that was serving requests, reached from the timer as well as from a caller.
//! `sutura_runtime::spawn_carrying_span` moves both onto the blocking pool. **That is not by itself
//! a bound**, and the helper's own documentation says why: a started blocking task cannot be
//! aborted, so a caller that gave up does not stop the read and runtime shutdown waits for it.
//! Three different things bound three different growths:
//!
//! | What could grow | What bounds it | What that does not reach |
//! | --- | --- | --- |
//! | the bytes read, and the parse over them | [`MAX_KEY_SET_BYTES`], inside the source | a second implementor's read - the port hands back a `String`, so by then the allocation happened |
//! | how many looks start | one per window by `Cached::last_attempt`, and none while an [`InFlight`] exists - both decided in the acquisition that reserves | a read whose awaiter left keeps its pool thread until the source answers; nothing installs what it returns |
//! | how long a caller waits for one | the deployment's own request timeout | nothing here: a second deadline beside a documented one is the defect, not the fix |
//!
//! # A failing refresh keeps verifying up to a declared ceiling, and not past it
//!
//! [`MAX_KEY_SET_AGE`] bounds revocation *while the source answers with a document this deployment
//! can use*. An unreadable source and a refused document both leave the previous keys verifying, and
//! [`KeySetCache::reserve`] stamps the **attempt**, which says nothing about freshness. So the
//! declared `security.inbound.key_set_max_stale_seconds` is compared against `Cached::last_success`,
//! when a look last came back with a usable document and never the last attempt: past it no held
//! key verifies, known `kid` or unknown, until a look succeeds, and the answer is
//! [`KeyUnavailable::Unconfirmed`] - `503 identity_unavailable` at the gate. A look that returns to its
//! caller without confirming the keys logs `stale_for_ms` beside `max_stale_ms`, so an operator can
//! alert before it: a `WARN` when its read fails, an `ERROR` when it reads a refused document, the
//! same refused bytes read again included. A look whose caller left, or whose read has not returned,
//! logs nothing.
//!
//! The bound, whole: **a removed key stops verifying within [`MAX_KEY_SET_AGE`] plus one read while
//! refresh works - plus the rotated-out grace behind a gateway, the lifetime ceiling and the
//! clock-skew leeway - and within the declared ceiling while it does not.** Its limits: it measures
//! the last usable read and not whether the source is current, so a file a sidecar stopped updating
//! never trips it; it is per process and on the monotonic clock, which a suspend does not advance; and
//! past it every caller of this process is refused until the source answers.
//!
//! # What a key set is read from
//!
//! [`FileKeySet`] or [`UrlKeySet`], behind [`KeySetSource`], which hands back the document's
//! **bytes**. A file is how a process with no egress rotates; a URL makes the issuer a runtime
//! dependency, and `docs/adr/0014` requires its outage to stay *distinguishable from a dead data
//! system*: an unknown key id while the last look failed is [`KeyUnavailable::SourceUnavailable`],
//! which the gate answers `503 identity_unavailable`, and the keys already held keep verifying.
//! Neither source honours a cache header - what bounds staleness is [`MAX_KEY_SET_AGE`].
//!
//! The document is a JWK set, or one PEM public key or certificate - the `tls.crt` cluster
//! certificate tooling writes - named by its RFC 7638 thumbprint. `document` holds that parse; the
//! source hands back bytes and knows neither format.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use jsonwebtoken::DecodingKey;
use sutura_config::{KeyFamily, KeySetMaxStale};
use sutura_runtime::Shutdown;

/// Where a key set comes from, and the bound on the document it hands back.
///
/// Private and re-exported, the way `docs/adr/0014`'s own seam runs: this module owns *the
/// source* and nothing in it reads a clock or holds a lock. It was split off here at the
/// thousand-line cap rather than being a new public path, so every caller's import is unchanged.
mod source;

pub use crate::inbound::keys::source::{FileKeySet, KeySetSource, KeySetUnavailable, MAX_KEY_SET_BYTES};

mod url;
pub use crate::inbound::keys::url::{FetchFailed, KeySetClientUnusable, UrlKeySet};

/// What a key set document is, and the parse from its bytes to [`KeySet`].
mod document;
/// The key set a rotation replaced, and its grace behind a gateway.
mod retired;
use crate::inbound::keys::retired::Retired;
/// What one look's document does to the cache, and the line it logs.
mod look;
use crate::inbound::keys::look::{adopt, announce};

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
///
/// **What it bounds is the age of a SUCCESSFUL re-read, and the argument a reviewer should have with
/// it is that one.** A read that fails and a document that is rejected both keep the previous keys
/// verifying, so while refresh is unavailable this number bounds nothing and the declared
/// `key_set_max_stale_seconds` is what bounds it - see the module documentation.
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
    #[error("the key set opens as PEM and is not well-formed PEM")]
    NotPem,
    /// A private key block anywhere in the document, refused on its `BEGIN` label: never decoded.
    #[error(
        "the key set holds a private key. A verifier holds public keys only: point \
         security.inbound.key_set_file at the certificate or the public key"
    )]
    PrivateKeyMaterial,
    #[error("the PEM key set holds more than one key: one PUBLIC KEY, or one certificate and its chain")]
    MoreThanOneKey,
    #[error("the PEM key set holds a block that is neither a CERTIFICATE nor a PUBLIC KEY")]
    UnsupportedPemBlock,
    #[error("the PEM key set's certificate could not be read")]
    UnreadableCertificate,
    #[error("the PEM key set's public key could not be read")]
    UnreadablePublicKey,
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
    /// Parses a key set document: a JWK set, or one PEM public key or certificate - see `document`.
    ///
    /// **Every refusal here is a refusal to start, not a key that gets skipped.** A key set is
    /// operator-supplied configuration and a deployment that silently dropped half of it would
    /// authenticate an arbitrary subset of its callers - which reads exactly like an intermittent
    /// outage. `docs/adr/0014`'s posture is fail-closed on the query path and this is that.
    pub fn parse(document: &str) -> Result<Self, InvalidKeySet> {
        document::parse(document)
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
    /// The id is not in the set and the last look at the source failed: an outage to retry, not a
    /// bad token. The cause is that look's, shared by every caller refused until the next one.
    #[error("the key set source did not answer, and the key set does not hold the key this token names")]
    SourceUnavailable {
        #[source]
        cause: Arc<dyn core::error::Error + Send + Sync>,
    },
    /// No look has confirmed the key set for the declared ceiling, so no held key verifies - an
    /// outage to retry, like [`Self::SourceUnavailable`], and not a bad token.
    #[error(
        "no look has confirmed the key set for {stale_for_ms}ms, past the declared {ceiling_ms}ms; no held key \
         verifies until a look succeeds"
    )]
    Unconfirmed { stale_for_ms: u128, ceiling_ms: u128 },
}

/// What one look at the source did.
///
/// Close to the outcomes `crate::tls::Renewed` has, and for the same reasons: a candidate that was
/// examined and rejected is recorded as examined, so identical bytes on the next look are
/// [`Self::Unchanged`] rather than a second rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refreshed {
    /// The document is byte-for-byte the one the last look examined.
    ///
    /// **It does not mean the keys in use came from it.** A rejected candidate is recorded as
    /// examined, so the look after one is `Unchanged` over bytes this deployment refused - which is
    /// why the freshness stamp is decided by the candidate's own parse and not by this variant. See
    /// [`Self::Rejected`].
    Unchanged,
    /// **The source could not be looked at**, or the look did not finish.
    ///
    /// Not [`Self::Unchanged`], which it once was: *"these are still the keys"* and *"the source said
    /// nothing"* are opposite facts about freshness, and only the first stamps `Cached::last_success`.
    Unavailable,
    /// A new document parsed, held a key of the pinned family, and is now in use.
    Rotated,
    /// A new document was read and is NOT usable. The previous key set keeps verifying.
    Rejected,
    /// **The source was not looked at**, because the window has not opened or another caller already
    /// reserved this look.
    ///
    /// Not [`Self::Unchanged`]: "nobody looked" is a fact about this deployment, and folding it in
    /// would make the bound [`KeySetCache::reserve`] enforces unobservable, so untestable.
    NotDue,
}

/// The bytes one look returned, and what they parsed to.
///
/// The parse travels with the read because both run on the same pool thread - see
/// [`KeySetCache::look`] - and because [`adopt`] wants the candidate whether or not it adopts it.
type Looked = Result<(String, Result<KeySet, InvalidKeySet>), LookFailed>;

/// Why one look produced no document at all.
///
/// Two reasons and not one, because the second is not the source's fault: a blocking task that did
/// not finish says nothing about whether the key set is readable, and folding it into
/// [`KeySetUnavailable`] would have needed a path this cache does not have. Private - what a caller
/// sees is [`Refreshed::Unavailable`] and a log line.
#[derive(Debug, thiserror::Error)]
enum LookFailed {
    #[error(transparent)]
    Source(#[from] KeySetUnavailable),
    /// The pool never handed an answer back - the task panicked, or the runtime is shutting down.
    #[error("the look at the key set source did not finish")]
    DidNotFinish(#[source] tokio::task::JoinError),
}

/// What the cache holds between reads.
struct Cached {
    keys: KeySet,
    /// The bytes the last look examined, so a re-read can be compared rather than reparsed into a
    /// value nothing can compare. [`Cached::keys`] was parsed from them unless they were refused.
    document: String,
    /// The bytes [`Cached::keys`] was parsed from, while [`Cached::document`] is a refused one: the
    /// same set read again after a refusal is no rotation, so it does not end a grace.
    adopted: Option<String>,
    /// When the source was last **attempted**, whether or not it answered.
    ///
    /// Attempted and not succeeded, and that is the whole rate limit: an issuer that is down would
    /// otherwise mean an outbound call per request, which is the same primitive the forged key id
    /// opens, arriving from the other direction.
    last_attempt: Instant,
    /// When a look last came back with a document this deployment could use.
    ///
    /// **Distinct from [`Self::last_attempt`], and the distinction is the finding.** A reservation
    /// stamps the attempt *before* the read, so a source failing every time keeps `last_attempt` one
    /// window old for as long as the process lives while nothing has confirmed the keys since this
    /// instant. The declared ceiling is compared against this, which `last_attempt` cannot be.
    last_success: Instant,
    /// The set the last rotation replaced, while it has a grace. Always `None` in `direct`.
    retired: Option<Retired>,
    /// Why the last finished look produced no document, so a miss is an outage and not a refusal.
    source_failed: Option<Arc<dyn core::error::Error + Send + Sync>>,
}

impl Cached {
    /// The verifier for `id`: the current set first, then the retired one while its grace is open.
    fn get(&self, id: &KeyId, now: Instant) -> Option<DecodingKey> {
        self.keys.get(id).or_else(|| self.retired.as_ref()?.get(id, now))
    }

    /// A miss while the source is failing is that outage; otherwise it is `refused`.
    fn outage_or(&self, refused: KeyUnavailable) -> KeyUnavailable {
        self.source_failed
            .as_ref()
            .map_or(refused, |cause| KeyUnavailable::SourceUnavailable {
                cause: Arc::clone(cause),
            })
    }
}

/// The one look in flight, held by whoever awaits it and released when that value is dropped.
///
/// It exists because the read runs on the blocking pool and can outlast its own window. Without it
/// a slow source accumulates one detached read per window, and two of them completing out of order
/// install the OLDER document - [`adopt`] compares against the last bytes examined, so a late arrival
/// looks like a change. One look at a time makes that ordering unrepresentable rather than checked.
///
/// **A value with a `Drop` and not a `bool`, because the `bool` leaked.** Set in
/// [`KeySetCache::reserve`] and cleared in [`KeySetCache::record`], it stayed set for the life of the
/// process when a request timeout or a disconnect dropped the awaiting future between the two: no
/// further look, so no rotation and no revocation, from one disconnect. Now only this drop clears it,
/// so an awaiter that leaves frees the slot and the next window may look again.
///
/// **The limit, stated because it is the cost of the choice:** this follows the awaiter, not the
/// read. A read whose awaiter left keeps its pool thread until the source answers, and nothing
/// installs what it returns - the only path that installs is the awaiter's `record`, which is gone.
/// So a source that hangs costs one pool thread per abandoned reservation, at most one per window,
/// and holds refresh for as long as it hangs under an awaiter that stays - the timer's does.
#[must_use]
struct InFlight<'cache>(&'cache AtomicBool);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
    /// `Arc` and not `Box`, for one reason: [`Self::look`] hands the source to the blocking pool,
    /// which needs an owned `'static` handle. Shared across tasks and immutable once built, which is
    /// what an `Arc` is for here.
    source: Arc<dyn KeySetSource>,
    refetch_interval: Duration,
    max_age: Duration,
    /// How long a rotated-out set keeps verifying. `None` in `direct` - see `retired`.
    grace: Option<Duration>,
    /// How long after `Cached::last_success` any key verifies.
    max_stale: Duration,
    /// The family the pinned algorithms need, so a swap cannot adopt a key set that verifies nothing.
    family: KeyFamily,
    /// What the pinned algorithms are, for the refusal's message alone.
    pinned: String,
    state: tokio::sync::RwLock<Cached>,
    /// Whether an [`InFlight`] exists. Beside the lock and not inside it, because the value that
    /// clears it does so from a `Drop`, which cannot await the lock - and it is only ever set under
    /// the lock, so a reservation is still decided in one acquisition.
    reading: AtomicBool,
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
        // `Box` at the boundary, `Arc` for the blocking pool. The boot read is inline: there is no
        // executor yet to block, and an unreadable key set is a refusal to start.
        let source = Arc::<dyn KeySetSource>::from(source);
        let document = source.read()?;
        let keys = parse_for(&document, family, &pinned).map_err(|cause| KeySetUnavailable::Invalid {
            path: PathBuf::from("the configured key set"),
            cause,
        })?;
        Ok(Self {
            source,
            refetch_interval,
            max_age,
            grace: None,
            max_stale: Duration::from_secs(KeySetMaxStale::DEFAULT_SECONDS),
            family,
            pinned,
            state: tokio::sync::RwLock::new(Cached {
                keys,
                document,
                last_attempt: now,
                // The boot read IS a success, so a deployment that never refreshes again reports a
                // staleness measured from when it started rather than from nothing.
                last_success: now,
                adopted: None,
                retired: None,
                source_failed: None,
            }),
            reading: AtomicBool::new(false),
        })
    }

    /// The same cache, keeping a rotated-out set verifying for `grace`. Set by the gate from the
    /// declaration, so the mode decides it and no caller can.
    #[must_use]
    pub(crate) const fn with_grace(mut self, grace: Option<Duration>) -> Self {
        self.grace = grace;
        self
    }

    /// The same cache, refusing every key once no look has confirmed the set for `max_stale`. Set by
    /// the gate from the declaration, like the grace.
    #[must_use]
    pub(crate) const fn with_max_stale(mut self, max_stale: Duration) -> Self {
        self.max_stale = max_stale;
        self
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
    /// [`MAX_KEY_SET_AGE`] plus the duration of one source read **while the source keeps answering
    /// with a usable document, and by the declared ceiling while refresh is failing** - past it both
    /// fast paths fall through and [`Self::after_look`] refuses. A rotation can
    /// cost a concurrent caller one `401` it can retry. Making it wait instead would put N request
    /// tasks behind one file read, which is the primitive this whole file is arranged against.
    pub async fn key_for(&self, id: &KeyId, now: Instant) -> Result<DecodingKey, KeyUnavailable> {
        // One read guard, dropped inside this statement: `clippy` flags a guard outliving its last use.
        let (found, since, confirmed) = {
            let cached = self.state.read().await;
            (
                cached.get(id, now),
                now.saturating_duration_since(cached.last_attempt),
                now.saturating_duration_since(cached.last_success) < self.max_stale,
            )
        };
        if let Some(key) = found {
            // A HIT is still subject to the age bound, and that is the revocation fix: the caller
            // presenting a revoked key presents an id this deployment holds, so nothing else here
            // would ever trigger a re-read. The read is skipped while the set is fresh, so the
            // ordinary request pays a lock and a map lookup.
            if since < self.max_age && confirmed {
                return Ok(key);
            }
            let _refreshed = self.poll_once(now).await;
            // Re-read after the refresh: the key may have been REVOKED by it.
            return self.after_look(id, now).await;
        }
        if since < self.refetch_interval && confirmed {
            let limited = KeyUnavailable::RefetchRateLimited {
                ago_ms: since.as_millis(),
            };
            return Err(self.state.read().await.outage_or(limited));
        }
        let _refreshed = self.poll_once(now).await;
        self.after_look(id, now).await
    }

    /// The key after a look, or why there is none - first among them a set past the ceiling.
    async fn after_look(&self, id: &KeyId, now: Instant) -> Result<DecodingKey, KeyUnavailable> {
        let cached = self.state.read().await;
        let unconfirmed = now.saturating_duration_since(cached.last_success);
        if unconfirmed >= self.max_stale {
            return Err(KeyUnavailable::Unconfirmed {
                stale_for_ms: unconfirmed.as_millis(),
                ceiling_ms: self.max_stale.as_millis(),
            });
        }
        cached
            .get(id, now)
            .ok_or_else(|| cached.outage_or(KeyUnavailable::UnknownKeyId))
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
    ///
    /// **A look already in flight loses too, whatever the windows say.** The [`InFlight`] this hands
    /// back is that reservation: its flag is set here, under the same acquisition, because
    /// [`Self::look`] runs on the blocking pool and can outlast its own window - so the window alone
    /// stopped bounding how many reads exist. See that type for the ordering hazard it removes, for
    /// the leak its first shape had, and for what a read whose awaiter left costs.
    async fn reserve(&self, now: Instant) -> Option<InFlight<'_>> {
        let window = self.refetch_interval.min(self.max_age);
        let mut cached = self.state.write().await;
        if now.saturating_duration_since(cached.last_attempt) < window {
            return None;
        }
        // Set only here and only under the write lock, so two reservers cannot both see it clear;
        // cleared only by the value's drop. An in-flight look loses BEFORE the stamp below, so a
        // caller that lost to one has not consumed the window either.
        if self.reading.swap(true, Ordering::AcqRel) {
            return None;
        }
        // Stamped here, under the same acquisition as the comparison above, and BEFORE the read: a
        // source that fails, or one that hangs and then fails, has still consumed the window - which is
        // the same primitive arriving from the other direction. What it is NOT is a record that the
        // keys are fresh; that is `Cached::last_success`, stamped by `Self::record`.
        cached.last_attempt = now;
        drop(cached);
        Some(InFlight(&self.reading))
    }

    /// One look at the source, off the executor and outside every lock.
    ///
    /// **The first half of this file's finding.** [`KeySetSource::read`] is synchronous by design and
    /// the parse behind it is CPU work over a foreign document, and both ran inline in an `async fn`,
    /// so a slow read blocked the worker thread that was serving requests - reached from the timer as
    /// much as from a caller. `sutura_runtime::spawn_carrying_span` is the one spawn `clippy.toml`
    /// permits for this, and it carries the request's span onto the pool thread so the lines a look
    /// emits stay attributable.
    ///
    /// **The parse travels with the read** rather than staying behind: it is the only other work in
    /// a look worth moving, it has to happen on every look anyway - see [`adopt`] - and doing it
    /// here is what keeps it out of the write lock.
    ///
    /// **What this is not**, because the helper's own documentation says so: it neither bounds nor
    /// cancels. A started blocking task cannot be aborted, so a caller that gave up does not stop
    /// the read and runtime shutdown waits for it. What bounds the work is [`MAX_KEY_SET_BYTES`],
    /// and what bounds how many looks start is [`InFlight`].
    async fn look(&self) -> Looked {
        let source = Arc::clone(&self.source);
        let family = self.family;
        // Cloned because the closure has to be `'static`, and it is a handful of bytes at most once
        // per window - it exists for a refusal's message alone.
        let pinned = self.pinned.clone();
        let looked = sutura_runtime::spawn_carrying_span(move || {
            let document = source.read()?;
            let candidate = parse_for(&document, family, &pinned);
            Ok((document, candidate))
        })
        .await;
        looked.unwrap_or_else(|cause| Err(LookFailed::DidNotFinish(cause)))
    }

    /// Records what one look did, and says how stale the keys in use now are.
    ///
    /// One write-lock acquisition, taken here and released with the return, so nothing awaits or
    /// expands a `tracing` macro while it is held - which is why [`announce`] is a separate function
    /// and why this hands the staleness back rather than logging it.
    ///
    /// **The reservation ends here, under the lock, after the result is in** - so a reservation
    /// taken next cannot start a look before this one's document is installed. That is the ordering
    /// [`InFlight`] exists for; an awaiter that never reaches this line releases it by dropping it.
    async fn record(&self, now: Instant, looked: Looked, in_flight: InFlight<'_>) -> (Refreshed, Duration) {
        let mut cached = self.state.write().await;
        cached.source_failed = None;
        let outcome = match looked {
            Err(failed) => {
                cached.source_failed = Some(Arc::new(failed));
                Refreshed::Unavailable
            }
            Ok((read, candidate)) => {
                // **The freshness stamp, and the second half of this file's finding.** The source
                // answered with a document this deployment can use, so the keys in use are as fresh
                // as the source is - whether or not they changed. Neither a failed read nor a
                // rejected candidate reaches this line, which is what stops `last_attempt` from
                // being mistaken for freshness. It is decided by the candidate's own parse rather
                // than by the outcome, because `Refreshed::Unchanged` is also what a look at bytes
                // this deployment already rejected returns.
                if candidate.is_ok() {
                    cached.last_success = now;
                }
                adopt(&mut cached, read, candidate, self.grace, now)
            }
        };
        drop(in_flight);
        (outcome, now.saturating_duration_since(cached.last_success))
    }

    /// How long since a look at the source last came back with a document this deployment could use.
    ///
    /// The number the declared ceiling is compared against, and deliberately not the last *attempt* -
    /// a reservation stamps the attempt before the read, so a source that has failed every time for an
    /// hour reports an attempt one window old and a success an hour old.
    pub async fn stale_for(&self, now: Instant) -> Duration {
        let cached = self.state.read().await;
        now.saturating_duration_since(cached.last_success)
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
        let Some(in_flight) = self.reserve(now).await else {
            return Refreshed::NotDue;
        };
        // The read and the parse both happen here, on the blocking pool, with no lock held across
        // either, and `in_flight` is held across both awaits - a future dropped at either one
        // releases it. Unreadable is deliberately not "changed": a mounted secret being swapped can
        // make a path briefly absent, and treating that as a rotation would reject on every swap.
        let looked = self.look().await;
        // The cause's text before the value is consumed, and only on the path that has one: an error
        // type is not a log line, and `announce` is the one place the two meet.
        let unavailable = looked.as_ref().err().map(ToString::to_string);
        let confirmed = matches!(looked, Ok((_, Ok(_))));
        let (outcome, stale_for) = self.record(now, looked, in_flight).await;
        announce(outcome, confirmed, (stale_for, self.max_stale), unavailable.as_deref());
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
