//! A lookup leg granted to the nodes that read its source (`github.com/telekom/sutura#828`): its
//! partitions registered as one [`SplitScan`](crate::SplitScan), so the ambiguous-link check and the
//! join read each of them only on a node [`Nodes::reading`](crate::Nodes::reading) names for the
//! leg's source, and never on this node.
//!
//! # Limits
//!
//! - A served question is granted a leg only by a mixed-kind `sutura serve` deployment's PostgreSQL
//!   source that `server.nodes` declares readers for; every other served lookup leg arrives as rows.
//! - The ambiguous-link check is part of the plan that joins the leg, so each partition is read once,
//!   for every granted leg of an arm: each is the build side of a RIGHT join, which reads the
//!   relation holding every earlier granted leg's check whole (`across::joined`).
//! - That plan keeps the join order it builds only because the session's statistics-driven join
//!   reordering is off for the whole combine, every other join in it included.
//! - An INNER granted leg is that RIGHT join and a filter above it, so each relation row the leg has
//!   no row for is carried through the join to the filter, unpruned at its scan. Not measured.
//! - Reading once moves the leg from the join's streamed side to its held side: the whole leg is held
//!   as the count's group state and then as the join's build side, on workers that spill nowhere,
//!   where the leg read twice streamed through the join and held only its link values and counts.
//!   A large granted leg the twice-read plan answered may now be refused for its working set. Not
//!   measured.
//! - A granted leg is planned across whatever its size and never tried on this node. A combiner
//!   with no other node, or a stage no resolved node reads the leg's source for, refuses it as
//!   [`CombineError::NoReadingNode`], which `sutura_app` answers as `SourceUnavailable` naming that
//!   source. A granted source no other node is declared as reading is refused
//!   before any granted partition is read; one whose declared readers do not resolve only when its
//!   stage is routed, after an earlier leg's partitions may have been read.
//! - A granted leg whose link the relationship casts is refused as a registration failure: the cast
//!   applies to rows this node holds, and a granted leg's are not here. `sutura_app` grants no leg
//!   whose link the plan declares a comparison for (`LinkCast::Declared`), so only a runner that
//!   ignores that reaches it.
//! - A granted leg's provenance records the posture this node declares for its source, while each
//!   reading worker reads under its own declared credential for that source. Nothing compares the
//!   two: that they agree is the operator's configuration.
//! - Each partition's stream is bounded at the combine's whole working-set ceiling in bytes
//!   (`runtime.working_set_max_bytes`), clamped to `u32`, and at the grant's own row bound - at that
//!   key's default of 1 GiB, looser than the 256 MiB a PostgreSQL read on this node is bounded at
//!   (`sutura_exec_postgres`'s `MOST_RESULT_BYTES`, a constant, not a setting). No worker enforces
//!   either yet, so this node's count of the answer stays the check.

#[cfg(feature = "distributed")]
use std::sync::Arc;

#[cfg(feature = "distributed")]
use datafusion::common::TableReference;
use datafusion::error::DataFusionError;
use datafusion::logical_expr::{Expr, LogicalPlan};
use datafusion::prelude::SessionContext;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{Granted, InternalLabel, Legs, LookupLeg, LookupRead};
use sutura_domain::warehouse::ResultBatches;

use super::cast::Cast;
use super::schema::LegSchema;
use super::{CombineError, DataFusionCombiner, held, missing_lookup, register_link};
#[cfg(feature = "distributed")]
use super::{SplitScan, StreamBound, across, working_set_of};
use crate::pool::WorkingSet;

/// The source of each lookup leg of `legs` granted rather than read here, in order.
pub(super) fn granted<'a>(legs: &Legs<'a>) -> impl Iterator<Item = &'a SourceName> {
    legs.lookups()
        .iter()
        .filter(|&&(_, read)| matches!(read, LookupRead::Granted(_)))
        .map(|&(leg, _)| leg.leg().source())
}

/// The bytes a lookup leg holds here, or its source's estimate of a granted one.
pub(super) fn lookup_held(read: LookupRead<'_>) -> u64 {
    match read {
        LookupRead::Rows(parts) => parts.iter().map(held).fold(0, u64::saturating_add),
        LookupRead::Granted(granted) => granted.estimate().bytes(),
    }
}

/// The schema of the lookup leg at `position`, registered as `table`.
pub(super) fn lookup_schema(read: LookupRead<'_>, table: &'static str, position: usize) -> Result<LegSchema, CombineError> {
    let schema = match read {
        LookupRead::Rows(parts) => parts
            .first()
            .map(ResultBatches::schema)
            .ok_or_else(|| missing_lookup(&InternalLabel::link(position).label()))?,
        LookupRead::Granted(granted) => granted.schema(),
    };
    LegSchema::of(table, schema)
}

/// `leg`'s `read` registered as `table`, its link column `label` cast first when `cast` is one.
pub(super) fn register_lookup(
    context: &SessionContext,
    (leg, table, read): (&LookupLeg, &'static str, LookupRead<'_>),
    label: &str,
    cast: Option<&Cast>,
    ceiling_bytes: u64,
) -> Result<(), CombineError> {
    match read {
        LookupRead::Rows(parts) => register_link(context, table, table, parts, label, cast),
        LookupRead::Granted(_) if cast.is_some() => Err(CombineError::Register {
            side: table,
            cause: DataFusionError::Plan(format!(
                "a granted leg of `{}` has its link cast, which reads rows this node does not hold",
                leg.leg().source()
            )),
        }),
        LookupRead::Granted(granted) => registered_split(context, table, leg.leg().source(), granted, ceiling_bytes),
    }
}

/// `granted` as one [`SplitScan`](crate::SplitScan) table, each partition's stream bounded at
/// `ceiling_bytes` clamped to `u32` and at the grant's row bound.
#[cfg(feature = "distributed")]
fn registered_split(
    context: &SessionContext,
    table: &'static str,
    source: &SourceName,
    granted: &Granted<'_>,
    ceiling_bytes: u64,
) -> Result<(), CombineError> {
    let unregistered = |cause| CombineError::Register { side: table, cause };
    let bound = StreamBound::new(granted.stream_rows().get(), u32::try_from(ceiling_bytes).unwrap_or(u32::MAX));
    let scan = SplitScan::granted(
        granted.wire().to_vec(),
        source.clone(),
        Arc::clone(granted.schema()),
        granted.partitions().iter().cloned(),
        granted.caller(),
        granted.until(),
        bound,
    )
    .map_err(unregistered)?;
    context.deregister_table(TableReference::bare(table)).map_err(unregistered)?;
    context
        .register_table(TableReference::bare(table), Arc::new(scan))
        .map(|_replaced| ())
        .map_err(unregistered)
}

/// No split leg is read in a build without `distributed`.
#[cfg(not(feature = "distributed"))]
fn registered_split(
    _context: &SessionContext,
    _table: &'static str,
    source: &SourceName,
    _granted: &Granted<'_>,
    _ceiling_bytes: u64,
) -> Result<(), CombineError> {
    Err(CombineError::NoReadingNode {
        source_name: source.clone(),
    })
}

/// Whether each lookup leg of `reads`, by position, has its link check inside the plan that joins it:
/// every granted leg.
///
/// Every granted leg is the build side of a RIGHT join (`across::joined`): a hash join reads its
/// build side whole, and a RIGHT one reads its probe side, the relation every earlier leg built,
/// even when the build side has no joinable row. Every other join keeps the relation as its build
/// side, so no join skips a granted leg's count. The limit: a LEFT leg no column of which is read
/// above its join is removed with its count by the engine's `EliminateJoin` rule, and no planned
/// question has that shape today (the limit in `across::once`); nothing holds that.
pub(super) fn counted_in_plan(reads: &[LookupRead<'_>]) -> Vec<bool> {
    reads.iter().map(|read| matches!(read, LookupRead::Granted(_))).collect()
}

/// `scanned`, the granted leg registered as `table`, its link values on `links` checked inside the
/// plan that joins it onto `relation` on `on` (`across::joined`), `context` keeping the join order
/// the plan builds.
#[cfg(feature = "distributed")]
pub(super) fn joined(
    context: &SessionContext,
    relation: LogicalPlan,
    scanned: LogicalPlan,
    table: &str,
    links: &[String],
    on: Vec<Expr>,
    include_unmatched: bool,
) -> Result<LogicalPlan, CombineError> {
    context
        .state_ref()
        .write()
        .config_mut()
        .options_mut()
        .optimizer
        .join_reordering = false;
    across::joined(relation, scanned, table, links, on, include_unmatched).map_err(|cause| CombineError::Build { cause })
}

/// No granted leg is registered in a build without `distributed`, so none is joined.
#[cfg(not(feature = "distributed"))]
pub(super) fn joined(
    _context: &SessionContext,
    _relation: LogicalPlan,
    _scanned: LogicalPlan,
    table: &str,
    _links: &[String],
    _on: Vec<Expr>,
    _include_unmatched: bool,
) -> Result<LogicalPlan, CombineError> {
    Err(CombineError::Build {
        cause: DataFusionError::Plan(format!("the granted leg `{table}` is joined only under `distributed`")),
    })
}

/// One combine by `combiner` of `legs`, whose first granted lookup leg is of `source`, which `run`
/// answers across the declared nodes, bounded to `ceiling_bytes`. Never on this node alone.
///
/// # Errors
///
/// [`CombineError::NoReadingNode`] for a combiner with no other node, or for the first granted
/// source no other node is declared as reading, asking none, and every failure of a combine across
/// them.
#[cfg_attr(
    not(feature = "distributed"),
    expect(unused_variables, reason = "reads `combiner.nodes`, which only `distributed` declares")
)]
pub(super) fn combined(
    combiner: &DataFusionCombiner,
    source: &SourceName,
    legs: &Legs<'_>,
    ceiling_bytes: u64,
    run: impl Fn(&SessionContext, WorkingSet) -> Result<ResultBatches, CombineError>,
) -> Result<ResultBatches, CombineError> {
    #[cfg(feature = "distributed")]
    if let Some(nodes) = &combiner.nodes {
        if let Some(unread) = granted(legs).find(|&leg_source| !nodes.reads(leg_source)) {
            return Err(CombineError::NoReadingNode {
                source_name: unread.clone(),
            });
        }
        let legs_bytes = super::legs_held(legs);
        let working_set = working_set_of(ceiling_bytes);
        let _wanted = nodes.hold(legs_bytes, ceiling_bytes);
        return run(&nodes.session_for(working_set, legs_bytes, ceiling_bytes)?, working_set)
            .map_err(|error| across::relayed(across::lost(error)));
    }
    Err(CombineError::NoReadingNode {
        source_name: source.clone(),
    })
}
