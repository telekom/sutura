//! What a dimension chain IS, read off a resolution: which data system it ends on, which table
//! qualifies each hop's origin column, and which hop - if any - leaves the metric's own source.
//!
//! **A module rather than six functions in `plan.rs`, and the seam is the concept.** Every plan
//! shape asks the same three questions of a chain, and the whole-answer path and the fact leg each
//! answered the second one with their own copy of the loop - which is how one of them came to
//! qualify every hop by the metric's table. One reader, two callers.
//!
//! Nothing here renders and nothing here refuses: [`chain_leaving_its_source`] reports what it
//! found and `super::plan` decides what that is. The module is private and sits under `plan`, so
//! the dependency runs inward only - it reads `crate::resolve` and `sutura_domain`, and neither
//! reads it.

use sutura_domain::catalog::JoinKey;
use sutura_domain::model::{DimensionName, SourceName, TableName};
use sutura_domain::plan::{PlanColumn, PlanJoin, PlanJoinKey};

use crate::resolve::{Resolution, ResolvedDimension};

/// The first chain that leaves the metric's data system after its first hop, and which hop it is.
///
/// Hop 1 may cross - that is the federated case, one link into one lookup table. Every later hop
/// must join two tables on the metric's own source, so BOTH of its ends are compared: a hop whose
/// origin sits elsewhere is a chain that crossed earlier and came back, which neither plan shape
/// can render and which [`is_remote`] reads as local. The hop number is 1-based, so it matches the
/// load-time report a catalog author reads.
///
/// `windows(2)` rather than an index, which is `sutura_domain::catalog::ViaChain`'s own rule: the
/// pair is (where the walk stands, where the hop after it goes), and the hop under test is the
/// second of the two.
pub(super) fn chain_leaving_its_source<'a>(resolution: &Resolution<'a>) -> Option<(&'a DimensionName, usize)> {
    let own = resolution.model.source();
    every_dimension(resolution).find_map(|resolved| {
        let hops = resolved.join.as_ref()?;
        let leaving = hops
            .windows(2)
            .position(|pair| pair.iter().any(|end| end.model.source() != own))?;
        Some((resolved.dimension.name(), leaving.saturating_add(2)))
    })
}

/// Every dimension that sits on a data system other than the metric's own.
pub(super) fn every_remote_dimension<'a, 'r>(resolution: &'r Resolution<'a>) -> impl Iterator<Item = &'r ResolvedDimension<'a>> {
    let own = resolution.model.source();
    every_dimension(resolution).filter(move |dim| is_remote(dim, own))
}

/// Whether a dimension sits on a data system other than `own`.
///
/// The LAST hop decides: hop 1 may cross a source boundary - that is the federated case - but from
/// the second hop on a chain stays on the data system its previous hop ended at, so a chain whose
/// last hop is local is a local dimension, not a federated one.
pub(super) fn is_remote(dimension: &ResolvedDimension<'_>, own: &SourceName) -> bool {
    dimension
        .join
        .as_ref()
        .and_then(|hops| hops.last())
        .is_some_and(|hop| hop.model.source() != own)
}

/// One [`PlanJoin`] per hop of every chain the question reaches, for both plan shapes.
///
/// **Hop N's origin column is qualified by hop N-1's target, and qualifying it by the metric's own
/// table was a wrong answer.** `origin` walks the chain the way
/// `sutura_domain::catalog::Definitions::assemble` walks it - the metric's table before the first
/// hop, each hop's target after it. Before that walk existed, `own_table` was used for every hop, so
/// a three-model chain rendered `ON fact.region_code = regions.code` when the origin column belongs
/// to `customers`: a binder error where the fact table has no such column, and a SILENTLY wrong
/// grouping where it happens to have one by the same name. Reproduced both ways against `DuckDB`.
///
/// **Chains are sorted as units, which is what makes the statement a function of the plan rather
/// than of the order the caller listed their dimensions.** The per-join sort that used to do this
/// was dropped when `via` became a chain, for a real reason - sorting flattened hops renders a
/// chain `[b, a]` as `a` then `b`, a join order that is a function of the alphabet rather than of
/// the path - but the comment that replaced it claimed determinism still held, and it did not: two
/// questions differing only in the order of `dimensions:` produced different join order. Sorting
/// whole chains removes the caller's order without reordering anything inside a chain. LEFT joins
/// commute, so what was at stake is the rendered TEXT the goldens pin, not a number.
///
/// Deduplicated by relationship AFTER the sort, so two dimensions reached through one hop produce
/// one join and which chain contributed it is decided by the sort rather than by arrival order.
///
/// A chain stops at its first hop that leaves `own_source`. On the fact leg that hop is the link
/// the lookup leg carries instead; on the whole-answer path no such hop exists, because `plan`
/// dispatches there only for a question with no remote dimension. Either way no hop may FOLLOW a
/// crossing one - [`super::PlanError::ChainLeavesItsSource`] is the refusal - so this stops rather than
/// skipping, and a chain that crossed can never contribute a join to a statement it has left.
pub(super) fn chain_joins(resolution: &Resolution<'_>, own_table: &TableName, own_source: &SourceName) -> Vec<PlanJoin> {
    let mut chains: Vec<Vec<PlanJoin>> = Vec::new();
    for resolved in every_dimension(resolution) {
        let Some(hops) = resolved.join.as_ref() else {
            continue;
        };
        let mut origin = own_table;
        let mut chain: Vec<PlanJoin> = Vec::with_capacity(hops.len());
        for hop in hops {
            if hop.model.source() != own_source {
                break;
            }
            let target = hop.model.table_name();
            let keys = hop
                .relationship
                .keys()
                .iter()
                .map(|key| plan_join_key(key, origin, target))
                .collect();
            chain.push(PlanJoin::new(
                hop.relationship.name().clone(),
                // The joined model's own path: a dimension table in another dataset is still one
                // statement. Its columns are qualified by the bare name beside it, for the reason
                // `mono_plan`'s `own_table` gives.
                hop.model.table().clone(),
                hop.relationship.join_type(),
                keys,
            ));
            origin = target;
        }
        chains.push(chain);
    }
    chains.sort_by(|left, right| {
        left.iter()
            .map(PlanJoin::relationship)
            .cmp(right.iter().map(PlanJoin::relationship))
    });
    let mut joins: Vec<PlanJoin> = Vec::new();
    for join in chains.into_iter().flatten() {
        if !joins.iter().any(|existing| existing.relationship() == join.relationship()) {
            joins.push(join);
        }
    }
    joins
}

/// Every dimension the question mentions, grouped by or filtered on.
///
/// One iterator, so the source check and the join collection walk the same set. They read it for
/// different reasons, and a dimension missing from one of them would be a join that is planned and
/// not counted, or counted and not planned.
pub(super) fn every_dimension<'a, 'r>(resolution: &'r Resolution<'a>) -> impl Iterator<Item = &'r ResolvedDimension<'a>> {
    resolution
        .keys
        .iter()
        .chain(resolution.filters.iter().map(|filter| &filter.dimension))
}

/// One [`JoinKey`] term, qualified by the tables the two ends of a hop read from.
fn plan_join_key(key: &JoinKey, origin_table: &TableName, target_table: &TableName) -> PlanJoinKey {
    let target = PlanColumn::new(target_table.clone(), key.target().clone());
    match key {
        JoinKey::Equal { origin, .. } => PlanJoinKey::Equal {
            origin: PlanColumn::new(origin_table.clone(), origin.clone()),
            target,
        },
        JoinKey::TruncatedEqual { origin, grain, .. } => PlanJoinKey::TruncatedEqual {
            origin: PlanColumn::new(origin_table.clone(), origin.clone()),
            grain: *grain,
            target,
        },
    }
}

/// Which table a dimension's column is read from: the last hop's when there is a chain, the metric's
/// own otherwise.
pub(super) fn column_of(resolved: &ResolvedDimension<'_>, own_table: &TableName) -> PlanColumn {
    let table = resolved
        .join
        .as_ref()
        .and_then(|hops| hops.last())
        .map_or_else(|| own_table.clone(), |hop| hop.model.table_name().clone());
    PlanColumn::new(table, resolved.dimension.column().clone())
}
