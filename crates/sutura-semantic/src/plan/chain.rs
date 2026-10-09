//! What a dimension chain IS, read off a resolution: which data system it ends on, which table
//! qualifies each hop's origin column, and which hop - if any - leaves the metric's own source.
//!
//! **A module rather than six functions in `plan.rs`, and the seam is the concept.** Every plan
//! shape asks the same questions of a chain, and the whole-answer path and the fact leg each
//! answered the second one with their own copy of the loop - which is how one of them came to
//! qualify every hop by the metric's table. One reader, two callers.
//!
//! **A chain crosses one data system boundary, at any hop.** The hops before the crossing join on
//! the metric's own source, the crossing hop is the federated plan's link, and the hops after it join
//! inside the lookup leg - [`crossing`] cuts a chain into those three.
//!
//! Nothing here renders and nothing here refuses: [`chain_leaving_its_crossing`] reports what it
//! found and `super::plan` decides what that is. The module is private and sits under `plan`, so
//! the dependency runs inward only - it reads `crate::resolve` and `sutura_domain`, and neither
//! reads it.

use sutura_domain::catalog::{JoinKey, Model, Relationship};
use sutura_domain::model::{DimensionName, SourceName, TableName};
use sutura_domain::plan::{PlanColumn, PlanJoin, PlanJoinKey};

use crate::resolve::{Resolution, ResolvedDimension, ResolvedJoin};

/// A chain cut at the hop that crosses onto another data system.
pub(super) struct Crossing<'r, 'a> {
    /// The hops on the metric's own source, which the fact leg joins.
    pub(super) before: &'r [ResolvedJoin<'a>],
    /// The hop that lands on the other system: the link the lookup leg carries.
    pub(super) hop: &'r ResolvedJoin<'a>,
    /// The hops after it, which join inside the lookup leg.
    pub(super) after: &'r [ResolvedJoin<'a>],
}

/// Where `hops` leaves `own`, or `None` for a chain that never does.
pub(super) fn crossing<'r, 'a>(hops: &'r [ResolvedJoin<'a>], own: &SourceName) -> Option<Crossing<'r, 'a>> {
    let at = hops.iter().position(|hop| hop.model.source() != own)?;
    let (before, rest) = hops.split_at(at);
    let (hop, after) = rest.split_first()?;
    Some(Crossing { before, hop, after })
}

/// The first chain that leaves the data system it crossed to, and which hop does it.
///
/// A chain may cross from the metric's source at any hop, and every hop after the crossing must land
/// on the system it crossed to. A later hop that lands anywhere else - back on the metric's source, or
/// on a third - is a chain neither plan shape can render, and [`is_remote`] reads the first of those
/// as local. The hop number is 1-based, so it matches the load-time report a catalog author reads.
pub(super) fn chain_leaving_its_crossing<'a>(resolution: &Resolution<'a>) -> Option<(&'a DimensionName, usize)> {
    let own = resolution.model.source();
    every_dimension(resolution).find_map(|resolved| {
        let hops = resolved.join.as_ref()?;
        let cut = crossing(hops, own)?;
        let landed = cut.hop.model.source();
        let (offset, _) = cut.after.iter().enumerate().find(|(_, hop)| hop.model.source() != landed)?;
        // Hops before the crossing, the crossing itself, the offset into what follows it, 1-based.
        let hop = cut.before.len().saturating_add(2).saturating_add(offset);
        Some((resolved.dimension.name(), hop))
    })
}

/// Every dimension that sits on a data system other than the metric's own.
pub(super) fn every_remote_dimension<'a, 'r>(resolution: &'r Resolution<'a>) -> impl Iterator<Item = &'r ResolvedDimension<'a>> {
    let own = resolution.model.source();
    every_dimension(resolution).filter(move |dim| is_remote(dim, own))
}

/// Whether a dimension sits on a data system other than `own`.
///
/// The LAST hop decides: a chain crosses at most once and stays where it crossed to, so a chain whose
/// last hop is on another system crossed, and one whose last hop is local never left - which is why a
/// chain that crossed and came back has to be refused before this is asked.
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
/// table was a wrong answer.** [`walk`] follows the chain the way
/// `sutura_domain::catalog::Definitions::assemble` walks it - the metric's table before the first
/// hop, each hop's target after it. Before that walk existed, `own_table` was used for every hop, so
/// a three-model chain rendered `ON fact.region_code = regions.code` when the origin column belongs
/// to `customers`: a binder error where the fact table has no such column, and a SILENTLY wrong
/// grouping where it happens to have one by the same name. Reproduced both ways against `DuckDB`.
///
/// A chain stops at its crossing hop. On the fact leg that hop is the link the lookup leg carries
/// instead, and the hops after it are [`lookup_joins`]; on the whole-answer path no such hop exists,
/// because `plan` dispatches there only for a question with no remote dimension. So a chain that
/// crossed never contributes a join to the statement it has left.
pub(super) fn chain_joins(resolution: &Resolution<'_>, own_table: &TableName, own_source: &SourceName) -> Vec<PlanJoin> {
    unique_chains(every_dimension(resolution).filter_map(|resolved| {
        let hops = resolved.join.as_ref()?;
        Some(walk(crossing(hops, own_source).map_or(hops, |cut| cut.before), own_table))
    }))
}

/// One [`PlanJoin`] per hop that follows the crossing, for the lookup leg's statement.
///
/// The first of them starts at the crossing hop's own table, which is the lookup leg's `FROM`. The
/// sort and the deduplication are [`chain_joins`]', through one function, so the two statements'
/// join order is a function of the plan in the same way.
pub(super) fn lookup_joins(resolution: &Resolution<'_>, own_source: &SourceName) -> Vec<PlanJoin> {
    unique_chains(every_dimension(resolution).filter_map(|resolved| {
        let cut = crossing(resolved.join.as_ref()?, own_source)?;
        Some(walk(cut.after, cut.hop.model.table_name()))
    }))
}

/// One join per hop, each starting at the table the previous hop ended on - `from` for the first.
fn walk(hops: &[ResolvedJoin<'_>], from: &TableName) -> Vec<PlanJoin> {
    let mut origin = from;
    hops.iter()
        .map(|hop| {
            let join = hop_join(hop.relationship, origin, hop.model);
            origin = hop.model.table_name();
            join
        })
        .collect()
}

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
fn unique_chains(chains: impl Iterator<Item = Vec<PlanJoin>>) -> Vec<PlanJoin> {
    let mut chains: Vec<Vec<PlanJoin>> = chains.collect();
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

/// One hop as a statement's `JOIN`: `relationship`, starting from the table `origin`, into `target`.
///
/// One join term per declared key, in the key's order. `origin` is where this hop's statement
/// starts - the metric's table for hop 1, each previous hop's target after - so every key's origin
/// column is qualified by it and its target by the joined table. `JoinKeys::map` holds the
/// relationship's own non-emptiness in its return type, so the plan's key list is `NonEmpty` by
/// construction too - no `expect` needed to say so. Also how a fact leg joins its shared calendar
/// (`telekom/sutura#780`).
pub(super) fn hop_join(relationship: &Relationship, origin: &TableName, target: &Model) -> PlanJoin {
    let target_table = target.table_name();
    let keys = relationship.keys().map(|key| match key {
        JoinKey::Equal { origin: o, target: t } => PlanJoinKey::Equal {
            origin: PlanColumn::new(origin.clone(), o.clone()),
            target: PlanColumn::new(target_table.clone(), t.clone()),
        },
        JoinKey::TruncatedEqual {
            origin: o,
            grain,
            target: t,
        } => PlanJoinKey::TruncatedEqual {
            origin: PlanColumn::new(origin.clone(), o.clone()),
            grain: *grain,
            target: PlanColumn::new(target_table.clone(), t.clone()),
        },
    });
    PlanJoin::new(
        relationship.name().clone(),
        // The joined model's own path: a dimension table in another dataset is still one
        // statement. Its columns are qualified by the bare name beside it, for the reason
        // `mono_plan`'s `own_table` gives.
        target.table().clone(),
        relationship.join_type(),
        keys,
    )
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
