//! A lookup leg granted to the nodes that read its source (`github.com/telekom/sutura#828`): its
//! partitions registered as one [`SplitScan`](crate::SplitScan), so the ambiguous-link check and the
//! join read each of them only on a node [`Nodes::reading`](crate::Nodes::reading) names for the
//! leg's source, and never on this node.
//!
//! # Limits
//!
//! - Nothing in production grants a leg yet (SERVE-2): every served lookup leg still arrives as rows.
//! - The ambiguous-link check and the join each read every partition, so each partition is read
//!   twice, under the one snapshot its source holds open across both. Accepted.
//! - A granted leg is planned across whatever its size and never tried on this node. A combiner
//!   with no other node, or a stage no resolved node reads the leg's source for, refuses it as
//!   [`CombineError::NoReadingNode`], which `sutura_app` does not ask about yet: it reaches a caller as
//!   this workspace's own defect.
//! - A granted leg whose link the relationship casts is refused as a registration failure: the cast
//!   applies to rows this node holds, and a granted leg's are not here.
//! - Each partition's stream is bounded at the combine's whole working-set ceiling in bytes, clamped
//!   to `u32`, and at the grant's own row bound. No worker enforces either yet, so this node's count
//!   of the answer stays the check.

#[cfg(feature = "distributed")]
use std::sync::Arc;

#[cfg(feature = "distributed")]
use datafusion::common::TableReference;
use datafusion::error::DataFusionError;
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

/// The source of the first lookup leg of `legs` granted rather than read here.
pub(super) fn granted<'a>(legs: &Legs<'a>) -> Option<&'a SourceName> {
    legs.lookups().iter().find_map(|&(leg, read)| match read {
        LookupRead::Granted(_) => Some(leg.leg().source()),
        LookupRead::Rows(_) => None,
    })
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
    let bound = StreamBound::new(granted.stream_rows(), u32::try_from(ceiling_bytes).unwrap_or(u32::MAX));
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

/// One combine by `combiner` with a lookup leg of `source` granted, which `run` answers across the
/// declared nodes, bounded to `ceiling_bytes`, with `legs_bytes` its legs' size. Never on this node
/// alone.
///
/// # Errors
///
/// [`CombineError::NoReadingNode`] for a combiner with no other node, asking none, and every failure
/// of a combine across them.
#[cfg_attr(
    not(feature = "distributed"),
    expect(unused_variables, reason = "reads `combiner.nodes`, which only `distributed` declares")
)]
pub(super) fn combined(
    combiner: &DataFusionCombiner,
    source: &SourceName,
    ceiling_bytes: u64,
    legs_bytes: u64,
    run: impl Fn(&SessionContext, WorkingSet) -> Result<ResultBatches, CombineError>,
) -> Result<ResultBatches, CombineError> {
    #[cfg(feature = "distributed")]
    if let Some(nodes) = &combiner.nodes {
        let working_set = working_set_of(ceiling_bytes);
        let _wanted = nodes.hold(legs_bytes, ceiling_bytes);
        return run(&nodes.session_for(working_set, legs_bytes, ceiling_bytes)?, working_set)
            .map_err(|error| across::relayed(across::lost(error)));
    }
    Err(CombineError::NoReadingNode {
        source_name: source.clone(),
    })
}
