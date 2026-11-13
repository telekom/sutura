//! The one per-kind split dispatch: which linked kinds split a lookup leg across the nodes that read
//! their source, and how a reading worker reads a partition of each.
//!
//! A kind that splits implements [`SplitKind`](crate::serve::kind::split::SplitKind) and takes one
//! arm in [`AnyWarehouse::splits`], and a single-kind deployment of it one arm in
//! [`worker_read`](crate::serve::kind::split::worker_read) and
//! [`worker_read_forwarded`](crate::serve::kind::split::worker_read_forwarded) or
//! [`worker_read_switched`](crate::serve::kind::split::worker_read_switched), plus the `cfg` of the
//! modules and methods that name the kind. Every other kind is read whole by
//! the planning node and refused by a reading worker, naming the source. `PostgreSQL` splits, read
//! as its service user (`postgres`); `BigQuery` splits at a pool source only, read with the caller's
//! forwarded token (`bigquery`); Oracle splits at a source declaring `split: flashback`, read as its
//! service user (`oracle`); ClickHouse hash buckets use the service user or the caller's switched
//! principal (`clickhouse`).
//!
//! **Limits.** This module and `grant` are built with `distributed` and at least one of
//! `postgres`, `clickhouse` or `oracle`. The `BigQuery` arm additionally needs `postgres`.

use core::num::{NonZeroU64, NonZeroUsize};
use std::time::Instant;

use sutura_domain::identity::{Presented, PrincipalName, Secret, WorkloadPool};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{Grantee, LegPlan, LookupSplit};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::{ResultBatches, Warehouse};
use sutura_exec_datafusion::{Forwarded, SchemaRef, TokenUnread, Unread};

use super::super::OpenedSources;
use super::{AnyWarehouse, AnyWarehouseError};

/// Whom a reading worker reads a granted partition as.
pub(super) enum PartitionReads {
    /// This node's own declared service user for the source.
    ServiceUser,
    /// The caller's own principal, which the source switches to for the read.
    #[cfg(feature = "clickhouse")]
    SwitchedPrincipal(PrincipalName),
}

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

    /// The granted `partition` of `leg`, which this node re-certified, read on this node as `reads`
    /// names, into `granted`, the columns the planning node granted it under.
    ///
    /// # Errors
    ///
    /// A partition past the source's per-stream bound, as [`Unread::DidNotFit`]; the source's other
    /// failures to read it, as [`Unread::Failed`].
    fn read_granted(
        &self,
        leg: &LegPlan,
        partition: &[u8],
        granted: &SchemaRef,
        reads: &PartitionReads,
        deadline: Deadline,
    ) -> Result<ResultBatches, Unread>;

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
            #[cfg(feature = "postgres")]
            Self::Postgres(postgres) => Some(&**postgres),
            #[cfg(all(feature = "bigquery", feature = "postgres"))]
            Self::BigQuery(bigquery) => Some(&**bigquery),
            #[cfg(feature = "clickhouse")]
            Self::ClickHouse(clickhouse) => Some(&**clickhouse),
            #[cfg(feature = "oracle")]
            Self::Oracle(oracle) => Some(&**oracle),
            _ => None,
        }
    }
}

/// `source`'s `partition` of the leg `wire` holds, into `granted`, read as this node's service user
/// only once `pinned` re-certified the leg and only for a kind that splits.
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
    granted: &SchemaRef,
    deadline: Deadline,
) -> Result<ResultBatches, Unread> {
    let not_split = || Unread::source_not_read(source);
    match sources {
        #[cfg(all(feature = "bigquery", feature = "postgres"))]
        OpenedSources::BigQuery(sources) => read_in(pinned, sources, source, wire, |bigquery, leg| {
            read_here(bigquery, leg, partition, granted, deadline)
        }),
        #[cfg(feature = "postgres")]
        OpenedSources::Postgres(sources) => read_in(pinned, sources, source, wire, |postgres, leg| {
            read_here(postgres, leg, partition, granted, deadline)
        }),
        #[cfg(feature = "clickhouse")]
        OpenedSources::ClickHouse(sources) => read_in(pinned, sources, source, wire, |clickhouse, leg| {
            read_here(clickhouse, leg, partition, granted, deadline)
        }),
        #[cfg(feature = "oracle")]
        OpenedSources::Oracle(sources) => read_in(pinned, sources, source, wire, |oracle, leg| {
            read_here(oracle, leg, partition, granted, deadline)
        }),
        OpenedSources::Mixed(mixed) => read_in(pinned, &mixed.engines, source, wire, |engine, leg| {
            engine
                .splits()
                .map_or_else(|| Err(not_split()), |kind| read_here(kind, leg, partition, granted, deadline))
        }),
        _ => Err(not_split()),
    }
}

/// Reads only a ClickHouse partition as the principal the admitted task grant named. Other kinds
/// refuse the identity rather than dropping it and reading as their service user.
pub(crate) fn worker_read_switched(
    pinned: &PinnedDefinitions,
    sources: &OpenedSources,
    source: &SourceName,
    wire: &[u8],
    partition: &[u8],
    granted: &SchemaRef,
    principal: PrincipalName,
    deadline: Deadline,
) -> Result<ResultBatches, Unread> {
    #[cfg(feature = "clickhouse")]
    #[expect(
        clippy::disallowed_methods,
        reason = "the admitted grant names the principal and partition; read_in re-certifies the leg before dispatch"
    )]
    let read = |kind: &dyn SplitKind, leg: &LegPlan| {
        kind.read_granted(
            leg,
            partition,
            granted,
            &PartitionReads::SwitchedPrincipal(principal),
            deadline,
        )
    };
    #[cfg(not(feature = "clickhouse"))]
    let _unused = (pinned, source, wire, partition, granted, principal, deadline);
    match sources {
        #[cfg(feature = "clickhouse")]
        OpenedSources::ClickHouse(opened) => read_in(pinned, opened, source, wire, |kind, leg| read(kind, leg)),
        #[cfg(feature = "clickhouse")]
        OpenedSources::Mixed(mixed) => read_in(pinned, &mixed.engines, source, wire, |kind, leg| match kind {
            AnyWarehouse::ClickHouse(clickhouse) => read(&**clickhouse, leg),
            _ => Err(Unread::LegNotCertified),
        }),
        _ => Err(Unread::LegNotCertified),
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
        #[cfg(all(feature = "bigquery", feature = "postgres"))]
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

/// `kind`'s read of the granted `partition` of `leg` as this node's service user, the one call site
/// of [`read_granted`](SplitKind::read_granted).
#[expect(
    clippy::disallowed_methods,
    reason = "only worker_read calls this, and only from inside read_recertified: the admit rule verified the task's reading grant, and the leg was re-certified against this node's catalog first"
)]
fn read_here(
    kind: &dyn SplitKind,
    leg: &LegPlan,
    partition: &[u8],
    granted: &SchemaRef,
    deadline: Deadline,
) -> Result<ResultBatches, Unread> {
    kind.read_granted(leg, partition, granted, &PartitionReads::ServiceUser, deadline)
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

#[cfg(all(feature = "bigquery", feature = "postgres"))]
mod bigquery;
#[cfg(feature = "oracle")]
mod oracle;
/// Visible to [`kind`](super) for its reader cells, which ask `postgres::unread`.
#[cfg(feature = "postgres")]
pub(super) mod postgres;

#[cfg(feature = "clickhouse")]
mod clickhouse;
