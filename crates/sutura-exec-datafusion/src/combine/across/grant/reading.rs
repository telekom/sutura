//! The source-reading grant a reading task carries, signed under a key drawn for that task alone
//! (Decision 5, `docs/adr/20261102082308-workers-read-sources.md`) - never the question's own key,
//! so a node that merely holds a question cannot sign a read of a partition it was not handed.
//!
//! # Limits
//!
//! No source-reading stage exists in this tree yet; a later slice plans one. This module mints and
//! verifies a [`ReadingGrant`](crate::combine::across::grant::reading::ReadingGrant) under a
//! [`ReadingKey`](crate::combine::across::grant::reading::ReadingKey) the caller draws per reading
//! task - never [`Key`](crate::combine::across::grant::Key), the per-question key `grant.rs` signs
//! task-install/execute calls under, so neither type can be passed for the other - and nothing here
//! is wired into `across.rs`: no stage asks for one today, so every item below is reached only from
//! this module's own tests until that slice lands.
//!
//! - How the grant reaches a reading task's worker is not built; the slice that wires a
//!   source-reading stage carries it.
//! - The worker side of "first key wins" - keeping the FIRST key seen for a given reading task, the
//!   way `grant::Keys` keeps the first key seen for a question - is **not built**. Nothing here is
//!   wired to a worker yet, so there is nothing today that could keep a second one; a later slice
//!   wiring a reading stage has to add that keeping, not assume
//!   [`verify`](crate::combine::across::grant::reading::verify) alone provides it.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "mint, verify and their types are exercised by this module's tests alone until a later slice wires a source-reading stage; see the module doc"
    )
)]

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
pub(super) struct ReadingKey([u8; 32]);

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
