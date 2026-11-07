//! The one per-kind split dispatch: which linked kinds split a lookup leg across the nodes that read
//! their source, and how a reading worker reads a partition of each.
//!
//! A kind that splits implements [`SplitKind`](crate::serve::kind::split::SplitKind) and takes one
//! arm in [`AnyWarehouse::splits`], and a single-kind deployment of it one arm in
//! [`worker_read`](crate::serve::kind::split::worker_read) and
//! [`worker_read_forwarded`](crate::serve::kind::split::worker_read_forwarded), plus the `cfg` of the
//! modules and methods that name the kind (see the PR's limits). Every other kind is read whole by
//! the planning node and refused by a reading worker, naming the source. `PostgreSQL` splits, read
//! as its service user; `BigQuery` splits at a pool source only, read with the caller's forwarded
//! token (`bigquery`).
//!
//! **Limit:** the `BigQuery` arm is built only with `postgres` too, as this module is.

use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::time::{Instant, SystemTime};

use sutura_domain::identity::{Presented, Secret, WorkloadPool};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{Grantee, LegPlan, LookupSplit, Partitioned};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::{ResultBatches, Warehouse};
use sutura_exec_datafusion::{Forwarded, TokenUnread, Unread};
use sutura_exec_postgres::PostgresError;
use sutura_exec_postgres::adbc::{MOST_RESULT_ROWS, Planned, SplitLeg};

use super::super::{OpenedSources, PostgresSource};
use super::grant::{self, Described, HeldSplit, Read};
use super::{AnyWarehouse, AnyWarehouseError};

/// A source kind whose lookup leg splits across the nodes that read its source.
///
/// Visible to [`kind`](super) alone, and [`read_granted`](SplitKind::read_granted) is banned by
/// `clippy.toml` beyond the one call in [`worker_read`], as [`read_forwarded`](SplitKind::read_forwarded)
/// is, so that its one call site is a visible `#[expect]`.
pub(super) trait SplitKind {
    /// `leg` split by this kind's own estimate: granted to `grantee`, read here, read whole, or
    /// refused - the port's [`Warehouse::split_lookup`] for this kind.
    ///
    /// # Errors
    ///
    /// The source's failure to estimate, split or describe the leg.
    fn split(
        &self,
        leg: &LegPlan,
        presented: &Presented,
        deadline: Deadline,
        above_bytes: NonZeroU64,
        readers: NonZeroUsize,
        grantee: Option<Grantee>,
        labels: &JobLabels,
    ) -> LookupSplit<'_, AnyWarehouseError>;

    /// The granted `partition` of `leg`, which this node re-certified, read on this node.
    ///
    /// # Errors
    ///
    /// A partition past the source's per-stream bound, as [`Unread::DidNotFit`]; the source's other
    /// failures to read it, as [`Unread::Failed`].
    fn read_granted(&self, leg: &LegPlan, partition: &[u8], deadline: Deadline) -> Result<ResultBatches, Unread>;

    /// The pool this source reads a partition with a caller's forwarded token at: `None` for a
    /// source that reads none, so a token is never handed to it.
    fn forwarded_at(&self) -> Option<&WorkloadPool> {
        None
    }

    /// [`Self::read_granted`] with `token`, the caller's own token the partition's grant named.
    ///
    /// # Errors
    ///
    /// By default every such read, as [`Unread::Failed`]; otherwise the source's failure to read.
    fn read_forwarded(
        &self,
        leg: &LegPlan,
        _partition: &[u8],
        _token: &Secret,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Unread> {
        Err(Unread::failed(TokenUnread::NotReadWithAToken {
            at: leg.source().clone(),
        }))
    }
}

impl AnyWarehouse {
    /// This adapter as a kind that splits, `None` for one read whole.
    pub(super) fn splits(&self) -> Option<&dyn SplitKind> {
        match self {
            Self::Postgres(postgres) => Some(&**postgres),
            #[cfg(feature = "bigquery")]
            Self::BigQuery(bigquery) => Some(&**bigquery),
            _ => None,
        }
    }
}

/// `source`'s `partition` of the leg `wire` holds, read only once `pinned` re-certified the leg and
/// only for a kind that splits.
///
/// # Errors
///
/// [`Unread::LegNotCertified`] for a leg `pinned` does not certify, in a deployment that holds a kind
/// that splits; a source of a kind that does not split, named - without certifying anything when the
/// deployment holds no kind that splits; then the source's own failure to read.
pub(crate) fn worker_read(
    pinned: &PinnedDefinitions,
    sources: &OpenedSources,
    source: &SourceName,
    wire: &[u8],
    partition: &[u8],
    deadline: Deadline,
) -> Result<ResultBatches, Unread> {
    let not_split = || Unread::source_not_read(source);
    match sources {
        #[cfg(feature = "bigquery")]
        OpenedSources::BigQuery(sources) => read_in(pinned, sources, source, wire, |bigquery, leg| {
            read_here(bigquery, leg, partition, deadline)
        }),
        OpenedSources::Postgres(sources) => read_in(pinned, sources, source, wire, |postgres, leg| {
            read_here(postgres, leg, partition, deadline)
        }),
        OpenedSources::Mixed(mixed) => read_in(pinned, &mixed.engines, source, wire, |engine, leg| {
            engine
                .splits()
                .map_or_else(|| Err(not_split()), |kind| read_here(kind, leg, partition, deadline))
        }),
        _ => Err(not_split()),
    }
}

/// `source`'s `partition` of the leg `wire` holds, read with `forwarded`, the caller's token the
/// admitted grant named: only at a source of that name this node federates at the token's own pool,
/// only while that grant lasts, and only once `pinned` re-certified the leg. The read ends no later
/// than this node's `budget` or the grant, whichever is first; the grant ends no later than the
/// token, as the planning node made it.
///
/// **Limit:** the token is opaque here, so its own expiry is read off the grant, which the planning
/// node clamped to it, and not off the token.
///
/// # Errors
///
/// [`Unread::LegNotCertified`] where this node declares no source of that name reading a token at
/// the pool it was forwarded for, whatever the kind; [`TokenUnread::Ended`] once the grant has
/// ended; then [`Unread::LegNotCertified`] for a leg `pinned` does not certify, and the source's own
/// failure to read.
pub(crate) fn worker_read_forwarded(
    pinned: &PinnedDefinitions,
    sources: &OpenedSources,
    source: &SourceName,
    wire: &[u8],
    partition: &[u8],
    forwarded: Forwarded,
    budget: Budget,
) -> Result<ResultBatches, Unread> {
    let asked = Asked {
        source,
        wire,
        partition,
        forwarded,
        budget,
    };
    match sources {
        #[cfg(feature = "bigquery")]
        OpenedSources::BigQuery(sources) => asked.read_in(pinned, sources, |bigquery| Some(bigquery as &dyn SplitKind)),
        OpenedSources::Mixed(mixed) => asked.read_in(pinned, &mixed.engines, AnyWarehouse::splits),
        _ => Err(Unread::LegNotCertified),
    }
}

/// A deployment's source as the kind that splits it, `None` for one that does not.
type KindOf<W> = fn(&W) -> Option<&dyn SplitKind>;

/// One read [`worker_read_forwarded`] was asked for.
struct Asked<'a> {
    source: &'a SourceName,
    wire: &'a [u8],
    partition: &'a [u8],
    forwarded: Forwarded,
    budget: Budget,
}

impl Asked<'_> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the reading worker's read: the token was forwarded for this source's own pool, its grant has not ended, and the leg is re-certified against this node's catalog first"
    )]
    fn read_in<W>(
        self,
        pinned: &PinnedDefinitions,
        opened: &sutura_app::Warehouses<W>,
        kind: KindOf<W>,
    ) -> Result<ResultBatches, Unread>
    where
        W: Warehouse,
    {
        let pool = opened.get(self.source).and_then(kind).and_then(SplitKind::forwarded_at);
        if !pool.is_some_and(|pool| self.forwarded.for_pool(pool)) {
            return Err(Unread::LegNotCertified);
        }
        let ended = || Unread::failed(TokenUnread::Ended { at: self.source.clone() });
        let left = self.forwarded.left().ok_or_else(ended)?;
        let budget = Budget::parse(self.budget.duration().min(left)).map_err(|_none| ended())?;
        let deadline = Deadline::opened_at(Instant::now(), budget);
        let token = self.forwarded.into_token();
        read_in(pinned, opened, self.source, self.wire, |engine, leg| {
            kind(engine).map_or(Err(Unread::LegNotCertified), |kind| {
                kind.read_forwarded(leg, self.partition, &token, deadline)
            })
        })
    }
}

/// `kind`'s read of the granted `partition` of `leg`, the one call site of
/// [`read_granted`](SplitKind::read_granted).
#[expect(
    clippy::disallowed_methods,
    reason = "only worker_read calls this, and only from inside read_recertified: the admit rule verified the task's reading grant, and the leg was re-certified against this node's catalog first"
)]
fn read_here(kind: &dyn SplitKind, leg: &LegPlan, partition: &[u8], deadline: Deadline) -> Result<ResultBatches, Unread> {
    kind.read_granted(leg, partition, deadline)
}

/// [`sutura_app::read_recertified`] over `opened`, a refusal answered as an uncertified leg.
fn read_in<W>(
    pinned: &PinnedDefinitions,
    opened: &sutura_app::Warehouses<W>,
    source: &SourceName,
    wire: &[u8],
    read: impl FnOnce(&W, &LegPlan) -> Result<ResultBatches, Unread>,
) -> Result<ResultBatches, Unread>
where
    W: Warehouse,
{
    sutura_app::read_recertified(pinned, opened, source, wire, |engine, leg| read(engine, &leg))
        .unwrap_or_else(|_refused| Err(Unread::LegNotCertified))
}

impl SplitKind for PostgresSource {
    /// Through `split_by_estimate`. A split of two or more partitions offered a `grantee` is granted
    /// to the nodes that read the source ([`grant::granted_or_read`]): its columns described on the
    /// exporting connection, each partition's descriptor, and the [`SplitLeg`] itself held by the
    /// grant, so the exporting transaction stays open as long as the grant does.
    /// [`Grantee::grant`]'s refusal is the answer, and no partition of a granted or refused leg is
    /// read here.
    ///
    /// Without a `grantee`, with one partition, or with a column whose type its cells decide
    /// (`NUMERIC`), the reader owns the split and reads by position until it holds no further
    /// partition, so each is read once, in page order, and the exporting transaction stays open until
    /// the reader is dropped.
    ///
    /// **Limits:** a leg answered [`Partitioned::Whole`] was estimated first, one round trip more
    /// than reading it whole outright; a granted one is described too, one more. And a link value two
    /// partitions both carry is one table's repeated tuple to the combine, refused as an ambiguous
    /// link - so a leg the whole read answered can be refused once split, never answered differently.
    fn split(
        &self,
        leg: &LegPlan,
        presented: &Presented,
        deadline: Deadline,
        above_bytes: NonZeroU64,
        readers: NonZeroUsize,
        grantee: Option<Grantee>,
        _labels: &JobLabels,
    ) -> LookupSplit<'_, AnyWarehouseError> {
        let stream_rows = NonZeroUsize::new(MOST_RESULT_ROWS).unwrap_or(NonZeroUsize::MIN);
        let split = match self.split_by_estimate(leg, presented, deadline, above_bytes, stream_rows, readers) {
            Ok(Planned::Whole) => return Ok(Partitioned::Whole),
            Ok(Planned::Refused(refusal)) => return Ok(Partitioned::Refused(refusal)),
            Ok(Planned::Split(split)) => split,
            Err(cause) => return Err(AnyWarehouseError::Postgres(cause)),
        };
        let rows = NonZeroU32::try_from(stream_rows).unwrap_or(NonZeroU32::MAX);
        grant::granted_or_read(split, grantee, deadline, rows, SystemTime::now)
    }

    /// Runs as this node's own declared service user for the source,
    /// `AdbcPostgres::read_partition`'s own limit; nothing checks it against the planning node's.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one production caller: the admit rule verified the task's reading grant, and the leg was re-certified against this node's catalog first"
    )]
    fn read_granted(&self, leg: &LegPlan, partition: &[u8], deadline: Deadline) -> Result<ResultBatches, Unread> {
        self.read_partition(leg, partition, deadline)
            .map_err(|cause| unread(leg.source(), cause))
    }
}

/// `cause` as this reader answers it: a partition past the per-stream bound as
/// [`Unread::DidNotFit`], which the planning node refuses as a result that did not fit, and anything
/// else as the engine's failure. Logged here, naming the source and never the cause's text, which
/// can quote what the server read.
pub(super) fn unread(source: &SourceName, cause: PostgresError) -> Unread {
    if cause.did_not_fit() {
        tracing::warn!(%source, "a granted partition did not fit this reader's per-stream bound, refused");
        Unread::DidNotFit
    } else {
        tracing::warn!(%source, "a granted partition could not be read");
        Unread::failed(cause)
    }
}

/// A split with a column whose type its cells decide (`NUMERIC`) describes no one schema, so it is
/// read here rather than granted.
impl HeldSplit for SplitLeg<'_> {
    type Error = AnyWarehouseError;

    fn descriptors(&self) -> Vec<Option<Vec<u8>>> {
        self.partitions()
            .map(|partition| partition.descriptor().map(|descriptor| descriptor.to_bytes()))
            .collect()
    }

    fn columns(&mut self, deadline: Deadline) -> Described<Self::Error> {
        self.schema(deadline).map_err(AnyWarehouseError::Postgres)
    }

    fn estimate(&self) -> EstimatedBytes {
        SplitLeg::estimate(self)
    }

    fn read(&self, index: usize, deadline: Deadline) -> Read<Self::Error> {
        self.partitions()
            .nth(index)
            .map(|partition| partition.read(deadline).map_err(AnyWarehouseError::Postgres))
    }
}

#[cfg(feature = "bigquery")]
mod bigquery;
