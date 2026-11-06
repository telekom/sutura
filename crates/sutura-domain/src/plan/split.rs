//! Whether a leg's root is read whole or split before it is read, and into how many partitions -
//! `github.com/telekom/sutura#828`.
//!
//! [`decide`] takes a source's own estimate of its leg and answers [`SplitDecision`]: at or under
//! the deployment's across-node threshold, [`SplitDecision::Whole`] - the same behaviour as before
//! this type existed, so an estimate of zero (no catalog statistics yet, or a genuinely empty
//! table) reads whole without a separate case for it. Over the threshold, the fewest partitions
//! whose own row share sits at or under the per-stream bound, never more than the reading workers
//! ready for one - [`SplitDecision::Split`] - or, where even that many still leave one over the
//! bound, [`SplitDecision::Refused`] naming the shortfall rather than splitting further.
//!
//! **The limits**:
//!
//! - This is the bare decision, over newtypes a caller already measured. Nothing here reads a
//!   catalog or opens a connection - `sutura_exec_postgres`'s adapter is the first caller.
//! - "At or under the bound" assumes `rows` sits even across `per_stream_bound`'s partitions - the
//!   ADR's own row calls for "well under" the bound, not merely at or under it, for exactly this
//!   reason. A root whose key is skewed can still hand one partition a share over the bound despite
//!   [`decide`] calling the split fit; that partition is refused like any other stream past the
//!   bound, read, not split again. There is no margin knob here to widen against skew - a caller
//!   wanting one states its own bound narrower before calling [`decide`].
//! - [`PartitionReader`] reads one partition after another on the thread that asked for them.
//!   [`Granted`] hands the partitions to the nodes that read the leg's source instead, made by
//!   [`Grantee::grant`] for the asking caller; only a `sutura serve` deployment's
//!   PostgreSQL source makes one, and only of two or more partitions (`github.com/telekom/sutura#828`).
//! - A grant read with the caller's own token ([`ReadAs::CallerToken`]) takes only a token whose
//!   [`SourceToken::exchanged_from`] is the fingerprint of the subject token the request presented
//!   for the leg's source, and ends no later than the token. Only [`Grantee::grant`] adds a token
//!   to a grant - [`Granted::new`] is public and takes any caller, but reads as
//!   [`ReadAs::ServiceUser`]. That fingerprint is what the exchanging adapter states, and no source
//!   makes such a grant yet.

use core::fmt;
use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use core::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use arrow_schema::SchemaRef;

use crate::identity::{AssertionDigest, BoundToTheRequest, Presented, SourceToken, SubjectKey};
use crate::nonempty::NonEmpty;
use crate::plan::leg::LegPlan;
use crate::plan::remote::{NotSendable, PartitionLeg};
use crate::warehouse::ResultBatches;
use crate::warehouse::estimate::EstimatedBytes;

/// A dry estimate of the rows a leg's root would read, before any is - the catalog's `reltuples`,
/// or a source's equivalent.
///
/// **Zero is a legitimate estimate, not a stand-in for "unknown"**, the same contrast
/// [`EstimatedBytes`] states for its own unit: a table with no rows yet is zero rows, not an absent
/// estimate. What a source cannot estimate at all - PostgreSQL's `reltuples = -1` before the first
/// `ANALYZE` - is read as zero by the caller that measured it, which [`decide`] then reads as
/// "at or under every threshold", not as a hidden sentinel inside this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EstimatedRows(u64);

impl EstimatedRows {
    /// Wraps a row count a source's own statistics reported.
    ///
    /// Infallible: every `u64` is a row count some table could hold.
    #[must_use]
    #[inline]
    pub const fn parse(rows: u64) -> Self {
        Self(rows)
    }

    /// The estimate, in rows.
    #[must_use]
    #[inline]
    pub const fn rows(self) -> u64 {
        self.0
    }
}

/// Why a leg was not split: not even `ready_workers` partitions keep each one's row share at or
/// under `per_stream_bound`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "{rows} estimated rows split {ready_workers} ways still leaves a partition over the {per_stream_bound}-row \
     per-stream bound"
)]
pub struct SplitRefusal {
    rows: u64,
    per_stream_bound: NonZeroUsize,
    ready_workers: NonZeroUsize,
}

impl SplitRefusal {
    /// The estimate that would not fit, split as many ways as there were workers for it.
    #[must_use]
    #[inline]
    pub const fn rows(self) -> EstimatedRows {
        EstimatedRows(self.rows)
    }

    /// The per-stream bound a partition is refused for crossing.
    #[must_use]
    #[inline]
    pub const fn per_stream_bound(self) -> NonZeroUsize {
        self.per_stream_bound
    }

    /// The most partitions [`decide`] was allowed to try.
    #[must_use]
    #[inline]
    pub const fn ready_workers(self) -> NonZeroUsize {
        self.ready_workers
    }
}

/// [`decide`]'s one output: a leg's root read whole, split into this many partitions, or refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDecision {
    /// At or under the threshold, or no usable estimate: the leg's own statement, read whole - as
    /// every leg reads today.
    Whole,
    /// Over the threshold: this many partitions, each estimated at or under the per-stream bound
    /// and never more than the reading workers ready for this leg.
    Split(NonZeroUsize),
    /// Over the threshold, but even the most partitions allowed still leave one over the bound.
    Refused(SplitRefusal),
}

/// Decides [`SplitDecision`] for one leg's root.
///
/// `bytes` against `above_bytes` decides whether to split at all - at or under it,
/// [`SplitDecision::Whole`]. Over it, the fewest partitions in `1..=ready_workers` whose own share
/// of `rows` sits at or under `per_stream_bound` (by the ceiling-division identity: `n` partitions
/// of `rows` have none over `bound` exactly when `n >= rows.div_ceil(bound)`); none of them do,
/// [`SplitDecision::Refused`] names the shortfall rather than splitting past `ready_workers`.
///
/// Checked throughout: every width change is `try_from` clamped to the target's maximum, so `rows`,
/// `per_stream_bound` or `ready_workers` at their type's maximum decide rather than panic.
#[must_use]
pub fn decide(
    bytes: EstimatedBytes,
    rows: EstimatedRows,
    above_bytes: NonZeroU64,
    per_stream_bound: NonZeroUsize,
    ready_workers: NonZeroUsize,
) -> SplitDecision {
    if bytes.bytes() <= above_bytes.get() {
        return SplitDecision::Whole;
    }
    let bound = u64::try_from(per_stream_bound.get()).unwrap_or(u64::MAX);
    let workers = u64::try_from(ready_workers.get()).unwrap_or(u64::MAX);
    let required = rows.rows().div_ceil(bound).max(1);
    if required > workers {
        return SplitDecision::Refused(SplitRefusal {
            rows: rows.rows(),
            per_stream_bound,
            ready_workers,
        });
    }
    let partitions = usize::try_from(required).unwrap_or(usize::MAX);
    SplitDecision::Split(NonZeroUsize::new(partitions).unwrap_or(NonZeroUsize::MIN))
}

/// A split leg's partitions, read in order: each item is the next partition's rows, or why it was
/// not read.
///
/// **The port is [`Iterator`]**, boxed because the source's own reader does not cross it. Each item
/// is read when it is pulled, under the deadline the reader was built with, and dropping the reader
/// ends the read: what a source holds open across its partitions - PostgreSQL's exporting
/// transaction - is released then, every partition read or not.
pub type PartitionReader<'r, E> = Box<dyn Iterator<Item = Result<ResultBatches, E>> + 'r>;

/// What [`Warehouse::split_lookup`](crate::warehouse::Warehouse::split_lookup) answers for one leg.
pub enum Partitioned<'r, E> {
    /// Not split: the leg is read through [`Warehouse::execute`](crate::warehouse::Warehouse::execute),
    /// as every leg was before a source could split one.
    Whole,
    /// Split at the source: one item per partition.
    Split(PartitionReader<'r, E>),
    /// Over the threshold, and [`decide`] refused the split.
    Refused(SplitRefusal),
    /// Split at the source, and granted to the nodes that read it: none of it is read here.
    Granted(Granted<'r>),
    /// Split at the source for a [`Grantee`], and refused rather than read here.
    NotGranted(NotGranted),
}

/// [`Partitioned`], or the source's failure to estimate or split the leg.
pub type LookupSplit<'r, E> = Result<Partitioned<'r, E>, E>;

/// How a node reading a granted partition reaches its source.
#[derive(Debug)]
pub enum ReadAs {
    /// As the identity the deployment holds for the source, which is never the caller's.
    ServiceUser,
    /// With a token of the caller's own principal, forwarded to the reading node.
    CallerToken(Box<SourceToken>),
}

/// A lookup leg its source split and granted to the nodes that read it, rather than read here.
///
/// Every partition is read by a node reading the leg's source, for `caller`, until `until`, at most
/// `stream_rows` rows a stream, as `reads` says. `wire` is the leg as [`PartitionLeg::encode`] writes it from a
/// [`Grantee`], and each partition the granting adapter's own descriptor, opaque here. `held` is
/// what the source keeps open across the partitions - an exporting transaction - and lives as long
/// as this value, which [`Legs`](crate::plan::Legs) borrows past the combine. It is `Send + Sync`
/// because the combine's scoped join moves the borrowed legs across threads.
///
/// **The limit:** only [`Grantee::grant`] builds one in production, for a `sutura serve` deployment's
/// PostgreSQL source (`github.com/telekom/sutura#828`).
pub struct Granted<'r> {
    wire: Vec<u8>,
    schema: SchemaRef,
    partitions: NonEmpty<Vec<u8>>,
    caller: SubjectKey,
    until: SystemTime,
    stream_rows: NonZeroU32,
    estimate: EstimatedBytes,
    reads: ReadAs,
    _held: Box<dyn Send + Sync + 'r>,
}

/// [`Granted::new`] was handed a grant that ended at or before the `now` it was checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the grant ended at {until:?}, at or before the instant it was checked against")]
pub struct GrantExpired {
    until: SystemTime,
}

impl<'r> Granted<'r> {
    /// `partitions` of the leg `wire` encodes, answering `schema`, granted to `caller` until `until`
    /// at most `stream_rows` rows a stream, estimated at `estimate`, with `held` kept open as long,
    /// read as [`ReadAs::ServiceUser`].
    ///
    /// # Errors
    ///
    /// [`GrantExpired`] when `until` is not after `now`. Checked once, here: a grant that ends while
    /// its partitions are read is refused only by each reading worker, as it admits a partition.
    pub fn new(
        wire: Vec<u8>,
        schema: SchemaRef,
        partitions: NonEmpty<Vec<u8>>,
        caller: SubjectKey,
        until: SystemTime,
        now: SystemTime,
        stream_rows: NonZeroU32,
        estimate: EstimatedBytes,
        held: impl Send + Sync + 'r,
    ) -> Result<Self, GrantExpired> {
        if until <= now {
            return Err(GrantExpired { until });
        }
        Ok(Self {
            wire,
            schema,
            partitions,
            caller,
            until,
            stream_rows,
            estimate,
            reads: ReadAs::ServiceUser,
            _held: Box::new(held),
        })
    }

    /// This grant, read with the caller's forwarded `token`, ending no later than `token` does and
    /// checked against `now` again, so no grant outlives the token it is read with.
    ///
    /// # Errors
    ///
    /// [`GrantExpired`] when the earlier end is not after `now`.
    pub(crate) fn forwarding(mut self, token: SourceToken, now: SystemTime) -> Result<Self, GrantExpired> {
        self.until = self.until.min(token.not_after());
        if self.until <= now {
            return Err(GrantExpired { until: self.until });
        }
        self.reads = ReadAs::CallerToken(Box::new(token));
        Ok(self)
    }

    /// The leg, as the granting adapter encoded it for its readers.
    #[must_use]
    pub const fn wire(&self) -> &[u8] {
        self.wire.as_slice()
    }

    /// The columns every partition answers.
    #[must_use]
    pub const fn schema(&self) -> &SchemaRef {
        &self.schema
    }

    /// One descriptor per partition, in the granting adapter's encoding.
    #[must_use]
    pub const fn partitions(&self) -> &NonEmpty<Vec<u8>> {
        &self.partitions
    }

    /// Whom every partition is read for.
    #[must_use]
    pub const fn caller(&self) -> &SubjectKey {
        &self.caller
    }

    /// When every partition's grant ends.
    #[must_use]
    pub const fn until(&self) -> SystemTime {
        self.until
    }

    /// The most rows one partition's stream may carry.
    #[must_use]
    pub const fn stream_rows(&self) -> NonZeroU32 {
        self.stream_rows
    }

    /// The source's own estimate of the whole leg.
    #[must_use]
    pub const fn estimate(&self) -> EstimatedBytes {
        self.estimate
    }

    /// How every partition is read.
    #[must_use]
    pub const fn reads(&self) -> &ReadAs {
        &self.reads
    }
}

/// Whom a split lookup leg is granted for, and until when, read off the request's own grant.
///
/// Only [`Self::new`] builds one, from a [`BoundToTheRequest`]: the caller is the one the broker
/// answered for and the request agreed with, never a value a caller of this type supplies, and the
/// grant ends no later than that answer does. It keeps the fingerprint of the subject token that
/// request presented for the leg's source, so [`Self::forwarding`] takes only a token exchanged from
/// that credential.
pub struct Grantee {
    wire: Vec<u8>,
    caller: SubjectKey,
    until: SystemTime,
    presented: Option<AssertionDigest>,
    token: Option<SourceToken>,
}

/// Why [`Grantee::new`] offered no grant.
#[derive(Debug, thiserror::Error)]
pub enum NotOffered {
    /// The request has no verified caller - the deployment itself - and a grant names one.
    #[error("a grant names the caller it is read for, and this question has no verified caller")]
    NoCaller,
    /// The leg does not encode for its readers, so it is not granted.
    #[error(transparent)]
    NotSendable(NotSendable),
}

/// Why [`Grantee::grant`] made no grant. Each is a refusal of the leg, never a read here instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotGranted {
    /// No partition, or one only this node can read - PostgreSQL's when it exported no snapshot.
    #[error("a split leg has a partition no other node can read")]
    NotPortable,
    /// The grant ends at or before the instant the source handed back its partitions.
    #[error(transparent)]
    Expired(#[from] GrantExpired),
}

/// Why [`Grantee::forwarding`] refused a token: it was exchanged from a credential other than the
/// one the grantee's caller presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a grant forwards only a token exchanged from its own caller's credential")]
pub struct NotTheCallersToken;

impl Grantee {
    /// `leg`, encoded as its readers re-certify it, granted for the caller `asked` names, until the
    /// earlier of `question_ends` and `asked`'s own expiry.
    ///
    /// # Errors
    ///
    /// [`NotOffered::NoCaller`] for a request with no verified caller, decided before the leg is
    /// encoded; [`NotOffered::NotSendable`] for a leg [`PartitionLeg::encode`] cannot write.
    pub fn new(leg: &LegPlan, asked: &BoundToTheRequest, question_ends: SystemTime) -> Result<Self, NotOffered> {
        let caller = asked.asked_by().key().cloned().ok_or(NotOffered::NoCaller)?;
        let until = asked
            .not_after()
            .unix_seconds()
            .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
            .map_or(question_ends, |expires| expires.min(question_ends));
        let presented = match asked.presented_for(leg.source()) {
            Ok(Presented::SubjectToken { material }) => Some(AssertionDigest::of(material)),
            Ok(Presented::SubjectPrincipal { .. } | Presented::SubjectSignIn { .. } | Presented::SharedServiceUser { .. })
            | Err(_) => None,
        };
        Ok(Self {
            wire: PartitionLeg::encode(leg).map_err(NotOffered::NotSendable)?,
            caller,
            until,
            presented,
            token: None,
        })
    }

    /// When the grant ends.
    #[must_use]
    pub const fn until(&self) -> SystemTime {
        self.until
    }

    /// This grantee, its partitions read with `token` ([`ReadAs::CallerToken`]), when `token` was
    /// exchanged from the credential the request presented for this leg's source.
    ///
    /// # Errors
    ///
    /// [`NotTheCallersToken`] for a token exchanged from any other credential, and for every token
    /// where the request presented no subject token for the leg's source.
    pub fn forwarding(mut self, token: SourceToken) -> Result<Self, NotTheCallersToken> {
        if self.presented.as_ref() != Some(token.exchanged_from()) {
            return Err(NotTheCallersToken);
        }
        self.token = Some(token);
        Ok(self)
    }

    /// `partitions`, each a descriptor another node can read or `None` for one only this node can,
    /// granted as [`Granted::new`] grants them, checked against `now` - the instant the source
    /// handed them back, after its own round trips - and read with the token [`Self::forwarding`]
    /// took, if it took one.
    ///
    /// # Errors
    ///
    /// [`NotGranted::NotPortable`] first, then [`NotGranted::Expired`], for the forwarded token's
    /// end too.
    pub fn grant<'r>(
        self,
        schema: SchemaRef,
        partitions: impl IntoIterator<Item = Option<Vec<u8>>>,
        stream_rows: NonZeroU32,
        estimate: EstimatedBytes,
        held: impl Send + Sync + 'r,
        now: SystemTime,
    ) -> Result<Granted<'r>, NotGranted> {
        let portable = partitions
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or(NotGranted::NotPortable)?;
        let partitions = NonEmpty::parse(portable).map_err(|_none| NotGranted::NotPortable)?;
        let granted = Granted::new(
            self.wire,
            schema,
            partitions,
            self.caller,
            self.until,
            now,
            stream_rows,
            estimate,
            held,
        )?;
        Ok(match self.token {
            Some(token) => granted.forwarding(token, now)?,
            None => granted,
        })
    }
}

impl fmt::Debug for Grantee {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grantee")
            .field("caller", &self.caller)
            .field("until", &self.until)
            .field("forwarding", &self.token.is_some())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Granted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Granted")
            .field("partitions", &self.partitions.len())
            .field("estimate", &self.estimate)
            .finish_non_exhaustive()
    }
}
