//! The source-reading grant a reading task carries, signed under a key drawn for that task alone
//! (Decision 5, `docs/adr/20261102082308-workers-read-sources.md`) - never the question's own key,
//! so a node that merely holds a question cannot sign a read of a partition it was not handed.
//!
//! # Limits
//!
//! This module mints and verifies a
//! [`ReadingGrant`](crate::combine::across::grant::reading::ReadingGrant) under a
//! [`ReadingKey`](crate::combine::across::grant::reading::ReadingKey) drawn per partition - never
//! [`Key`](crate::combine::across::grant::Key), the per-question key `grant.rs` signs
//! task-install/execute calls under, so neither type can be passed for the other.
//!
//! - The planning node mints one grant per partition of a split leg (`grant::granted_read`, called
//!   by `combine::across::split`), and no served question builds a split leg yet.
//! - How the grant reaches a reading task's worker is `combine::across::partition`'s leaf and codec:
//!   it carries this module's [`encode`](crate::combine::across::grant::reading::encode)d
//!   grant/signature bytes, and the worker's admit rule
//!   [`decode`](crate::combine::across::grant::reading::decode)s and
//!   [`verify`](crate::combine::across::grant::reading::verify)s them.
//! - [`StreamBound`](crate::StreamBound) is signed into the grant and carried, but no worker
//!   enforces it yet.
//! - The worker side of "first key wins" - keeping the FIRST key seen for a given reading task, the
//!   way `grant::Keys` keeps the first key seen for a question - is `combine::across::grant::ReadingKeys`,
//!   beside `Keys`, not this module:
//!   [`verify`](crate::combine::across::grant::reading::verify) alone never provided it, and still
//!   does not.

use core::time::Duration;

use datafusion::error::DataFusionError;
use ring::digest;
use ring::hmac;
use sutura_domain::identity::SubjectKey;
use sutura_domain::model::SourceName;

use super::super::WorkerRefusal;

/// Domain tag, the first field fed into every [`ReadingGrant::signed`] message - so a signature
/// taken here can never collide with one taken over the same bytes by another signed message in
/// this workspace, and the version lets the encoding change without a stale signature verifying a
/// fresh message (`sutura_domain::identity::compute_context`'s `SCHEME` is the same idea for that
/// module's own digest).
const SCHEME: &[u8] = b"sutura.reading-grant.v1";

/// Feeds one field's own fixed-width digest into `message`, never the field's raw bytes.
///
/// A NUL-delimited encoding is ambiguous the moment a field may itself hold a NUL - [`Partition`] is
/// opaque, adapter-minted bytes, which may - and a length-prefixed one has to pick a byte order,
/// which `clippy::big_endian_bytes` and its two siblings (on in this workspace) exist precisely so
/// that choice does not have to be made. Hashing each field to a fixed width needs neither: no
/// concatenation of a different field sequence can reproduce the same message
/// (`sutura_domain::identity::compute_context::feed_field`'s doc states the same property for its
/// own fields; that function is `pub(super)` to `sutura-domain::identity` and unreachable from this
/// crate, so this is a local copy of the same construction rather than the same function).
fn feed_field(message: &mut Vec<u8>, field: &[u8]) {
    message.extend_from_slice(digest::digest(&digest::SHA256, field).as_ref());
}

/// The key one reading task's grant is signed under - distinct from [`super::Key`], the
/// per-question key, so a type that reads one can never be handed the other: a node that merely
/// holds a question's key must not be able to sign a read of a partition it was not handed a
/// reading task for. No `Debug`, so it is never printed, and no `Clone`: nothing here needs a
/// second copy of a key that exists to be handed to exactly one task.
pub(in crate::combine::across) struct ReadingKey([u8; 32]);

impl ReadingKey {
    pub(super) fn drawn() -> Result<Self, DataFusionError> {
        let mut key = [0; 32];
        #[expect(
            clippy::disallowed_methods,
            reason = "the key one reading task's grant is signed under; random so no node can mint a grant for \
                      a reading task it was not handed"
        )]
        let drawn = getrandom::fill(&mut key);
        drawn.map_err(|cause| DataFusionError::Execution(format!("no reading key could be drawn for the task: {cause}")))?;
        Ok(Self(key))
    }

    /// Reconstructs a reading task's key from the bytes a call presented for it - the worker's own
    /// admit rule, through `combine::across::grant::ReadingKeys`, is the only caller outside this
    /// module's tests: it never mints a key, only verifies a grant under one a call already carried.
    pub(super) const fn of(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The key's bytes, consumed, for the one partition scan that presents them to its worker.
    pub(super) const fn into_bytes(self) -> [u8; 32] {
        self.0
    }

    fn signing(&self) -> hmac::Key {
        hmac::Key::new(hmac::HMAC_SHA256, &self.0)
    }
}

/// A leg's own wire document, hashed - never a derived or redisplayed form of it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct LegDigest([u8; 32]);

impl LegDigest {
    /// Hashes `wire`, the leg's encoded wire document.
    pub(super) fn of(wire: &[u8]) -> Self {
        let hashed = digest::digest(&digest::SHA256, wire);
        let mut bytes = [0; 32];
        bytes.copy_from_slice(hashed.as_ref());
        Self(bytes)
    }
}

/// The partition a reading task reads: opaque bytes the adapter mints. This module asks nothing of
/// their shape - including whether they hold a NUL, which a NUL-delimited encoding could not have
/// said the same of.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Partition(Vec<u8>);

impl Partition {
    pub(super) fn of(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }
}

/// The question's deadline, read on the worker's own clock. Never read internally - every check
/// here takes `now` from its caller, so a test drives it deterministically.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Expiry(Duration);

impl Expiry {
    /// `deadline`, a duration since `UNIX_EPOCH`.
    pub(super) const fn at(deadline: Duration) -> Self {
        Self(deadline)
    }

    /// Whether `now` is strictly past the deadline - `now` equal to the deadline is not past it,
    /// matching "the worker refuses a grant past it"
    /// (`docs/adr/20261102082308-workers-read-sources.md`).
    fn passed(&self, now: Duration) -> bool {
        now > self.0
    }
}

/// The caller a reading grant names: the masked [`SubjectKey`] `Display` a record already renders -
/// never the full key.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct MaskedCaller(String);

impl MaskedCaller {
    /// `subject`'s masked rendering, never its unmasked form.
    pub(super) fn of(subject: &SubjectKey) -> Self {
        Self(subject.to_string())
    }

    /// Reconstructs a masked caller directly from its already-masked text - [`decode`]'s only
    /// source, since a reading grant crossing a link carries the masked form already, never the key
    /// it was taken from.
    pub(super) const fn of_masked(text: String) -> Self {
        Self(text)
    }
}

/// The per-stream bound share a reading grant carries - the only limit share this slice reads
/// (Decision 5): the federated ceiling's share is the whole ceiling (Decision 4), and rides outside
/// this grant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct StreamBound {
    pub(super) rows: u32,
    pub(super) bytes: u32,
}

/// A source-reading grant: what a reading task may read, until when, and the per-stream bound it
/// reads under.
pub(super) struct ReadingGrant {
    leg: LegDigest,
    source: SourceName,
    partition: Partition,
    expiry: Expiry,
    caller: MaskedCaller,
    bound: StreamBound,
}

impl ReadingGrant {
    /// Builds the grant's unsigned content. Minting and verifying it are this module's job; nothing
    /// else constructs one.
    pub(super) const fn new(
        leg: LegDigest,
        source: SourceName,
        partition: Partition,
        expiry: Expiry,
        caller: MaskedCaller,
        bound: StreamBound,
    ) -> Self {
        Self {
            leg,
            source,
            partition,
            expiry,
            caller,
            bound,
        }
    }

    /// The bytes an HMAC signs: [`SCHEME`] followed by every field's own fixed-width digest
    /// ([`feed_field`]), so every field - including [`Self::expiry`], [`Self::caller`] and
    /// [`Self::bound`] - is covered by the signature, not only the three [`verify`] matches by
    /// value.
    fn signed(&self) -> Vec<u8> {
        let mut message = Vec::new();
        feed_field(&mut message, SCHEME);
        feed_field(&mut message, &self.leg.0);
        feed_field(&mut message, self.source.as_str().as_bytes());
        feed_field(&mut message, &self.partition.0);
        feed_field(&mut message, self.expiry.0.as_secs().to_string().as_bytes());
        feed_field(&mut message, self.expiry.0.subsec_nanos().to_string().as_bytes());
        feed_field(&mut message, self.caller.0.as_bytes());
        feed_field(&mut message, self.bound.rows.to_string().as_bytes());
        feed_field(&mut message, self.bound.bytes.to_string().as_bytes());
        message
    }
}

/// A [`ReadingGrant`]'s signature over its own [`ReadingGrant::signed`] message - never a bare
/// `Vec<u8>` a caller could pass where a grant, a key or a leg digest's bytes were meant instead.
/// No `PartialEq`/`Eq`: a MAC tag is compared by [`hmac::verify`], never by `==`.
pub(super) struct Signature(Vec<u8>);

impl Signature {
    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// `grant`, signed under `key` - a key drawn for the one reading task `grant` is minted for, never
/// the question's own key, so holding it grants no other task.
pub(super) fn mint(key: &ReadingKey, grant: ReadingGrant) -> (ReadingGrant, Signature) {
    let signature = Signature(hmac::sign(&key.signing(), &grant.signed()).as_ref().to_vec());
    (grant, signature)
}

/// Identifies one reading task from the two values that never change across a retry of it: the
/// leg's own wire bytes and the partition descriptor, hashed together so a task is the same task
/// only when both match - never a value a caller supplies, so a hostile retry cannot pick its own
/// id and collide with another task's kept key (`combine::across::grant::ReadingKeys`).
#[must_use]
pub(super) fn task_id(wire: &[u8], partition: &[u8]) -> [u8; 32] {
    let mut message = Vec::new();
    feed_field(&mut message, b"sutura.reading-task.v1");
    feed_field(&mut message, wire);
    feed_field(&mut message, partition);
    let hashed = digest::digest(&digest::SHA256, &message);
    let mut bytes = [0; 32];
    bytes.copy_from_slice(hashed.as_ref());
    bytes
}

/// Writes `bytes` into `buf` as a decimal length, a `:`, then `bytes` itself - [`decode_field`]'s
/// counterpart. Not [`feed_field`]'s hash: a wire form must round-trip back to the same bytes, not
/// collapse them to a fixed-width digest. Not a fixed-width length prefix either -
/// `clippy::big_endian_bytes` and its two siblings (on in this workspace) exist so a byte order
/// never has to be picked; decimal text need not pick one.
fn encode_field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(bytes.len().to_string().as_bytes());
    buf.push(b':');
    buf.extend_from_slice(bytes);
}

/// Reads one [`encode_field`]-written field from the front of `*rest`, advancing `*rest` past it.
fn decode_field(rest: &mut &[u8]) -> Option<Vec<u8>> {
    let colon = rest.iter().position(|&byte| byte == b':')?;
    let (digits, tail) = rest.split_at(colon);
    let length: usize = core::str::from_utf8(digits).ok()?.parse().ok()?;
    let tail = tail.get(1..)?;
    if tail.len() < length {
        return None;
    }
    let (field, tail) = tail.split_at(length);
    *rest = tail;
    Some(field.to_vec())
}

/// Serializes `grant` and `signature` as the bytes a leaf carries across a link - the only form
/// outside this module that can stand for a [`ReadingGrant`], since none of its fields are
/// reachable from outside it. [`decode`] is the inverse.
#[must_use]
pub(super) fn encode(grant: &ReadingGrant, signature: &Signature) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_field(&mut buf, &grant.leg.0);
    encode_field(&mut buf, grant.source.as_str().as_bytes());
    encode_field(&mut buf, &grant.partition.0);
    encode_field(&mut buf, grant.expiry.0.as_secs().to_string().as_bytes());
    encode_field(&mut buf, grant.expiry.0.subsec_nanos().to_string().as_bytes());
    encode_field(&mut buf, grant.caller.0.as_bytes());
    encode_field(&mut buf, grant.bound.rows.to_string().as_bytes());
    encode_field(&mut buf, grant.bound.bytes.to_string().as_bytes());
    encode_field(&mut buf, signature.as_bytes());
    buf
}

/// The inverse of [`encode`], or `None` for anything that is not exactly nine well-formed fields -
/// a decode failure is never a panic: a worker calls this on bytes a call presented, and a
/// malformed presentation is refused the same way a merely wrong one is
/// ([`WorkerRefusal::GrantNotNaming`]).
#[must_use]
pub(super) fn decode(buf: &[u8]) -> Option<(ReadingGrant, Signature)> {
    let mut rest = buf;
    let leg = <[u8; 32]>::try_from(decode_field(&mut rest)?.as_slice()).ok()?;
    let source = SourceName::parse(String::from_utf8(decode_field(&mut rest)?).ok()?).ok()?;
    let partition = decode_field(&mut rest)?;
    let secs: u64 = core::str::from_utf8(&decode_field(&mut rest)?).ok()?.parse().ok()?;
    let nanos: u32 = core::str::from_utf8(&decode_field(&mut rest)?).ok()?.parse().ok()?;
    let caller = String::from_utf8(decode_field(&mut rest)?).ok()?;
    let rows: u32 = core::str::from_utf8(&decode_field(&mut rest)?).ok()?.parse().ok()?;
    let bytes: u32 = core::str::from_utf8(&decode_field(&mut rest)?).ok()?.parse().ok()?;
    let signature = decode_field(&mut rest)?;
    if !rest.is_empty() {
        return None;
    }
    let grant = ReadingGrant::new(
        LegDigest(leg),
        source,
        Partition(partition),
        Expiry::at(Duration::new(secs, nanos)),
        MaskedCaller::of_masked(caller),
        StreamBound { rows, bytes },
    );
    Some((grant, Signature(signature)))
}

/// What a worker asks [`verify`] to admit a reading task's call under: the grant and signature it
/// was sent, the key its question drew for this task, what the task itself reads, and the worker's
/// own clock - seven fields a positional call made easy to pass out of order, grouped here instead.
pub(super) struct Verification<'a> {
    pub(super) key: &'a ReadingKey,
    pub(super) grant: &'a ReadingGrant,
    pub(super) signature: &'a Signature,
    pub(super) wanted_leg: LegDigest,
    pub(super) wanted_source: &'a SourceName,
    pub(super) wanted_partition: &'a Partition,
    pub(super) now: Duration,
}

/// Admits `asked.grant`/`asked.signature` under `asked.key` for a read of `asked.wanted_leg`/
/// `asked.wanted_source`/`asked.wanted_partition` not yet past `asked.now`, or the refusal naming
/// why not.
pub(super) fn verify(asked: &Verification<'_>) -> Result<(), WorkerRefusal> {
    let names = asked.grant.leg == asked.wanted_leg
        && &asked.grant.source == asked.wanted_source
        && asked.grant.partition == *asked.wanted_partition;
    if !names || hmac::verify(&asked.key.signing(), &asked.grant.signed(), asked.signature.as_bytes()).is_err() {
        return Err(WorkerRefusal::GrantNotNaming);
    }
    if asked.grant.expiry.passed(asked.now) {
        return Err(WorkerRefusal::GrantExpired);
    }
    Ok(())
}
