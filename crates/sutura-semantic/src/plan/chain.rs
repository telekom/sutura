//! What a dimension chain IS, read off a resolution: which data systems it crosses, which table
//! qualifies each hop's origin column, and which lookup legs a question needs.
//!
//! **A module rather than six functions in `plan.rs`, and the seam is the concept.** Every plan
//! shape asks the same questions of a chain, and the whole-answer path and the fact leg each
//! answered the second one with their own copy of the loop - which is how one of them came to
//! qualify every hop by the metric's table. One reader, two callers.
//!
//! **A chain crosses a data system boundary at every hop that lands on another system than the hop
//! before it.** [`cut`] splits a chain there. The hops before the first crossing join on the
//! metric's own source; each crossing hop is the link of one lookup leg, and the hops after it join
//! inside that leg until the next crossing. [`Lookups`] gathers those legs for the whole question.
//!
//! Nothing here renders and nothing here refuses: `super::plan` decides what a count of legs is.
//! The module is private and sits under `plan`, so the dependency runs inward only - it reads
//! `crate::resolve` and `sutura_domain`, and neither reads it.

use std::collections::BTreeMap;
use std::iter::once;
use std::slice::Iter;

use sutura_domain::catalog::Model;
use sutura_domain::model::{RelationshipName, TableName};
use sutura_domain::plan::{PlanColumn, PlanJoin};

use crate::resolve::{Resolution, ResolvedDimension, ResolvedJoin};

/// A crossing hop and the hops after it that stay on the data system it landed on.
pub(super) struct Segment<'r, 'a> {
    /// The model the crossing hop leaves.
    pub(super) origin: &'a Model,
    /// The hop that lands on another data system: the link of one lookup leg.
    pub(super) hop: &'r ResolvedJoin<'a>,
    /// The hops after it up to the next crossing, which join inside that lookup leg.
    pub(super) after: &'r [ResolvedJoin<'a>],
}

/// A chain cut at every hop that crosses onto another data system.
pub(super) struct Cut<'r, 'a> {
    /// The hops on the metric's own source, which the fact leg joins.
    pub(super) before: &'r [ResolvedJoin<'a>],
    /// One segment per crossing, in chain order.
    pub(super) segments: Vec<Segment<'r, 'a>>,
}

/// Where `hops` leaves each data system it reaches, or `None` for a chain that never leaves `own`'s.
///
/// `None` is also what an out-of-range slice would give, which no chain built from `hops` itself can
/// produce: the starts are indices into `hops` and each end is the next start or `hops.len()`.
pub(super) fn cut<'r, 'a>(hops: &'r [ResolvedJoin<'a>], own: &'a Model) -> Option<Cut<'r, 'a>> {
    let mut at = own;
    let mut starts: Vec<(usize, &'a Model)> = Vec::new();
    for (index, hop) in hops.iter().enumerate() {
        if hop.model.source() != at.source() {
            starts.push((index, at));
        }
        at = hop.model;
    }
    let &(first, _) = starts.first()?;
    let ends = starts.iter().skip(1).map(|&(next, _)| next).chain(once(hops.len()));
    let segments = starts
        .iter()
        .zip(ends)
        .map(|(&(start, origin), end)| {
            let (hop, after) = hops.get(start..end)?.split_first()?;
            Some(Segment { origin, hop, after })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Cut {
        before: hops.get(..first)?,
        segments,
    })
}

/// One lookup leg of a question: the crossing that opens it, and the joins its own statement makes.
pub(super) struct LookupShape<'r, 'a> {
    /// Every crossing relationship from the metric's own data system to this leg, this leg's own last.
    path: Vec<&'a RelationshipName>,
    /// The hop that lands on this leg's data system.
    pub(super) crossing: &'r ResolvedJoin<'a>,
    /// The model that hop leaves: it sits on the parent leg, and its column is the link's origin.
    pub(super) origin: &'a Model,
    /// The lookup leg this one joins onto, or `None` for the fact leg.
    pub(super) parent: Option<usize>,
    /// The joins after the crossing hop, for every chain that reaches this leg.
    pub(super) joins: Vec<PlanJoin>,
}

/// Every lookup leg a question needs, one per distinct path of crossing relationships.
///
/// **A leg is one crossing relationship reached one way, and two dimensions that reach one remote
/// model through two relationships read it in two legs.** The legs are sorted by path, and a path
/// sorts after its own prefix, so a leg's parent always comes before it - which is the order
/// `LookupLegs::parse` requires - and the order, with every link label it names, is a function of
/// the catalog and not of the order the caller listed their dimensions.
pub(super) struct Lookups<'r, 'a> {
    own: &'a Model,
    shapes: Vec<LookupShape<'r, 'a>>,
}

struct Draft<'r, 'a> {
    crossing: &'r ResolvedJoin<'a>,
    origin: &'a Model,
    joins: Vec<Vec<PlanJoin>>,
}

impl<'r, 'a> Lookups<'r, 'a> {
    pub(super) fn of(resolution: &'r Resolution<'a>) -> Self {
        let own = resolution.model;
        let mut drafts: BTreeMap<Vec<&'a RelationshipName>, Draft<'r, 'a>> = BTreeMap::new();
        for resolved in every_dimension(resolution) {
            let Some(cut) = resolved.join.as_deref().and_then(|hops| cut(hops, own)) else {
                continue;
            };
            let mut path: Vec<&'a RelationshipName> = Vec::new();
            for segment in &cut.segments {
                path.push(segment.hop.relationship.name());
                drafts
                    .entry(path.clone())
                    .or_insert_with(|| Draft {
                        crossing: segment.hop,
                        origin: segment.origin,
                        joins: Vec::new(),
                    })
                    .joins
                    .push(walk(segment.after, segment.hop.model.table_name()));
            }
        }
        let paths: Vec<Vec<&'a RelationshipName>> = drafts.keys().cloned().collect();
        let shapes = drafts
            .into_iter()
            .map(|(path, draft)| LookupShape {
                parent: path
                    .split_last()
                    .and_then(|(_, up)| paths.iter().position(|earlier| earlier.as_slice() == up)),
                path,
                crossing: draft.crossing,
                origin: draft.origin,
                joins: unique_chains(draft.joins.into_iter()),
            })
            .collect();
        Self { own, shapes }
    }

    pub(super) const fn len(&self) -> usize {
        self.shapes.len()
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    pub(super) fn first(&self) -> Option<&LookupShape<'r, 'a>> {
        self.shapes.first()
    }

    pub(super) fn iter(&self) -> Iter<'_, LookupShape<'r, 'a>> {
        self.shapes.iter()
    }

    /// The lookup leg a dimension is read from, or `None` for a dimension on the metric's own source.
    ///
    /// The LAST crossing decides: a chain that crossed and came back is read from the leg it came
    /// back through, and never from the fact leg.
    pub(super) fn index_of(&self, dimension: &ResolvedDimension<'_>) -> Option<usize> {
        let cut = dimension.join.as_deref().and_then(|hops| cut(hops, self.own))?;
        self.shapes.iter().position(|shape| {
            shape.path.len() == cut.segments.len()
                && shape
                    .path
                    .iter()
                    .zip(&cut.segments)
                    .all(|(name, segment)| *name == segment.hop.relationship.name())
        })
    }
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
/// instead, and the hops after it are the lookup leg's joins ([`Lookups`]); on the whole-answer path no such hop exists,
/// because `plan` dispatches there only for a question with no remote dimension. So a chain that
/// crossed never contributes a join to the statement it has left.
pub(super) fn chain_joins(resolution: &Resolution<'_>, own_table: &TableName) -> Vec<PlanJoin> {
    unique_chains(every_dimension(resolution).filter_map(|resolved| {
        let hops = resolved.join.as_deref()?;
        Some(walk(cut(hops, resolution.model).map_or(hops, |cut| cut.before), own_table))
    }))
}

/// One join per hop, each starting at the table the previous hop ended on - `from` for the first.
fn walk(hops: &[ResolvedJoin<'_>], from: &TableName) -> Vec<PlanJoin> {
    let mut origin = from;
    hops.iter()
        .map(|hop| {
            let join = PlanJoin::hop(hop.relationship, origin, hop.model);
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
