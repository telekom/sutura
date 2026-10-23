//! What a leg may name on this node: the joins, dimensions and links its metric declares.
//!
//! A fact leg rooted on its metric's own model may join the hops of the metric's dimension chains
//! that stay on its source, key or filter on a dimension whose whole chain stays there, and link on
//! every key part of the hop where such a chain first crosses to another source. A fact leg rooted
//! on a second model of the measure links on a relationship from its root to where one of those
//! crossings lands, with one key for each of the crossing's keys, in the crossing's key order. A
//! lookup leg names its metric too, and is rooted only where one of that metric's
//! chains crosses into it: it links on that crossing hop and on the next crossing out of the part
//! of the chain it reads, may join the hops between them, and keys or filters on a dimension whose
//! chain ends there. A fact leg bucketed on a shared calendar may also join that calendar, through
//! the one relationship the planner picks for its root.

use std::collections::BTreeSet;

use super::Statement;
use crate::catalog::{Definitions, Dimension, JoinKey, Metric, Model, Relationship};
use crate::model::{ColumnName, DimensionName, ModelName, RelationshipName};
use crate::plan::PlanColumn;

/// A dimension a leg may name: the column it reads and the joins its chain reaches it through.
struct Reach<'d> {
    dimension: &'d Dimension,
    column: PlanColumn,
    through: Vec<&'d RelationshipName>,
}

/// The end of a crossing hop a link key reads: its origin on the leg it leaves, or its target on the
/// leg it enters.
#[derive(Clone, Copy)]
enum End {
    Origin,
    Target,
}

/// A link the leg may carry: the model it is read on, at which end, and its key columns by part.
struct Link<'d> {
    end: End,
    model: &'d ModelName,
    parts: Vec<&'d ColumnName>,
}

impl<'d> Link<'d> {
    /// One end of a crossing hop, its parts in the hop's key order.
    fn of(hop: &'d Relationship, end: End) -> Self {
        let (model, parts) = match end {
            End::Origin => (hop.origin_model(), hop.keys().iter().map(JoinKey::origin).collect()),
            End::Target => (hop.target_model(), hop.keys().iter().map(JoinKey::target).collect()),
        };
        Self { end, model, parts }
    }

    /// Whether the leg's `statement` holds the model this link is read on.
    fn held(&self, statement: &Statement<'_>) -> bool {
        match self.end {
            End::Origin => statement.models.iter().any(|at| at.name() == self.model),
            End::Target => statement.root.name() == self.model,
        }
    }

    /// Whether `column` is part `part` of this link.
    fn reads(&self, definitions: &Definitions, part: usize, column: &PlanColumn) -> bool {
        self.parts.get(part).is_some_and(|&at| at == column.column())
            && definitions
                .model(self.model)
                .is_some_and(|model| model.table_name() == column.table())
    }
}

/// The joins, dimensions and links one leg may name.
pub(super) struct Surface<'d> {
    joins: BTreeSet<&'d RelationshipName>,
    reaches: Vec<Reach<'d>>,
    links: Vec<Link<'d>>,
}

/// One hop of a dimension chain and the model it lands on.
type Hop<'d> = (&'d Relationship, &'d Model);

/// Each hop of `chain` with the model it lands on, or `None` when a hop is not declared here.
fn hops<'d>(definitions: &'d Definitions, chain: &'d [RelationshipName]) -> Option<Vec<Hop<'d>>> {
    chain
        .iter()
        .map(|name| {
            let relationship = definitions.relationship(name)?;
            Some((relationship, definitions.model(relationship.target_model())?))
        })
        .collect()
}

/// The hop where each of `metric`'s dimension chains first crosses to another source.
fn first_crossings<'d>(definitions: &'d Definitions, metric: &'d Metric) -> Vec<&'d Relationship> {
    let Some(own) = definitions.model(metric.model()) else {
        return Vec::new();
    };
    metric
        .dimensions()
        .values()
        .filter_map(|dimension| {
            let hops = hops(definitions, dimension.via()?)?;
            hops.into_iter()
                .find(|&(_, target)| target.source() != own.source())
                .map(|(relationship, _)| relationship)
        })
        .collect()
}

/// Whether one of `metric`'s dimension chains crosses from another source into `model`.
pub(super) fn crossed_into(definitions: &Definitions, metric: &Metric, model: &Model) -> bool {
    let Some(own) = definitions.model(metric.model()) else {
        return false;
    };
    metric.dimensions().values().any(|dimension| {
        let Some(hops) = dimension.via().and_then(|chain| hops(definitions, chain)) else {
            return false;
        };
        let mut at = own;
        hops.iter().any(|&(_, target)| {
            let crosses = target.source() != at.source() && target.name() == model.name();
            at = target;
            crosses
        })
    })
}

/// `second`'s link onto where `crossing` lands, as the planner builds it: the same number of keys,
/// each of `second`'s keys on a crossing key's target and grain, its parts in the crossing's key
/// order.
fn link_onto<'d>(second: &'d Relationship, crossing: &Relationship) -> Option<Link<'d>> {
    let same = |own: &JoinKey, other: &JoinKey| own.target() == other.target() && own.grain() == other.grain();
    let (ours, theirs) = (second.keys(), crossing.keys());
    if second.target_model() != crossing.target_model()
        || ours.len() != theirs.len()
        || !ours.iter().all(|key| theirs.iter().any(|own| same(own, key)))
    {
        return None;
    }
    let parts = theirs
        .iter()
        .map(|own| ours.iter().find(|key| same(own, key)).map(JoinKey::origin))
        .collect::<Option<Vec<_>>>()?;
    Some(Link {
        end: End::Origin,
        model: second.origin_model(),
        parts,
    })
}

impl<'d> Surface<'d> {
    /// A fact leg's surface: `calendar`, and on a second model of the measure the links onto
    /// `metric`'s first crossings, unless the leg is rooted on `metric`'s own model.
    pub(super) fn of_fact(
        definitions: &'d Definitions,
        metric: &'d Metric,
        root: &'d Model,
        calendar: Option<&'d RelationshipName>,
    ) -> Self {
        let mut surface = Self {
            joins: calendar.into_iter().collect(),
            reaches: Vec::new(),
            links: Vec::new(),
        };
        if root.name() != metric.model() {
            let crossings = first_crossings(definitions, metric);
            surface.links = definitions
                .relationships()
                .values()
                .filter(|second| second.origin_model() == root.name())
                .flat_map(|second| crossings.iter().filter_map(move |crossing| link_onto(second, crossing)))
                .collect();
            return surface;
        }
        for dimension in metric.dimensions().values() {
            let Some(hops) = hops(definitions, dimension.via().unwrap_or_default()) else {
                continue;
            };
            let local = hops
                .iter()
                .take_while(|&&(_, target)| target.source() == root.source())
                .count();
            surface.joins.extend(hops.iter().take(local).map(|&(hop, _)| hop.name()));
            if let Some(&(crossing, _)) = hops.get(local) {
                surface.links.push(Link::of(crossing, End::Origin));
            } else {
                let at = hops.last().map_or(root, |&(_, target)| target);
                surface.reaches.push(Reach {
                    dimension,
                    column: PlanColumn::new(at.table_name().clone(), dimension.column().clone()),
                    through: hops.iter().map(|&(hop, _)| hop.name()).collect(),
                });
            }
        }
        surface
    }

    /// A lookup leg's surface: what `metric`'s dimension chains name after crossing into `root`.
    pub(super) fn of_lookup(definitions: &'d Definitions, metric: &'d Metric, root: &'d Model) -> Self {
        let mut surface = Self {
            joins: BTreeSet::new(),
            reaches: Vec::new(),
            links: Vec::new(),
        };
        let Some(own) = definitions.model(metric.model()) else {
            return surface;
        };
        let dimensions = metric.dimensions().values();
        for dimension in dimensions {
            let Some(hops) = dimension.via().and_then(|chain| hops(definitions, chain)) else {
                continue;
            };
            let mut at = own;
            let mut through: Option<Vec<&'d RelationshipName>> = None;
            for &(hop, target) in &hops {
                if target.source() != at.source() {
                    if through.is_some() {
                        surface.links.push(Link::of(hop, End::Origin));
                    }
                    through = (target.name() == root.name()).then(Vec::new);
                    if through.is_some() {
                        surface.links.push(Link::of(hop, End::Target));
                    }
                } else if let Some(joined) = through.as_mut() {
                    joined.push(hop.name());
                    surface.joins.insert(hop.name());
                }
                at = target;
            }
            if let Some(through) = through {
                surface.reaches.push(Reach {
                    dimension,
                    column: PlanColumn::new(at.table_name().clone(), dimension.column().clone()),
                    through,
                });
            }
        }
        surface
    }

    /// Whether the leg may join `relationship`.
    pub(super) fn joins(&self, relationship: &RelationshipName) -> bool {
        self.joins.contains(relationship)
    }

    /// The dimensions on `column` whose whole chain the leg's `joins` hold.
    fn on<'s>(&'s self, column: &'s PlanColumn, joins: &'s [RelationshipName]) -> impl Iterator<Item = &'d Dimension> + 's {
        self.reaches
            .iter()
            .filter(move |reach| reach.column == *column && reach.through.iter().all(|hop| joins.contains(hop)))
            .map(|reach| reach.dimension)
    }

    /// Whether `dimension` is declared on `column`, reached through the leg's `joins`.
    pub(super) fn reaches(&self, dimension: &DimensionName, column: &PlanColumn, joins: &[RelationshipName]) -> bool {
        self.on(column, joins).any(|declared| declared.name() == dimension)
    }

    /// The filterable dimensions on `column`, reached through the leg's `joins`.
    pub(super) fn filterable<'s>(
        &'s self,
        column: &'s PlanColumn,
        joins: &'s [RelationshipName],
    ) -> impl Iterator<Item = &'d Dimension> + 's {
        self.on(column, joins).filter(|dimension| dimension.is_filterable())
    }

    /// Whether `column` is key part `part` of one of the leg's links, on a model the leg holds.
    pub(super) fn links(&self, definitions: &Definitions, statement: &Statement<'_>, part: usize, column: &PlanColumn) -> bool {
        self.links
            .iter()
            .any(|link| link.held(statement) && link.reads(definitions, part, column))
    }

    /// Whether `carried`, the parts one link label carries, are every part of one of the leg's
    /// links and nothing else, each part once.
    pub(super) fn completes(
        &self,
        definitions: &Definitions,
        statement: &Statement<'_>,
        carried: &[(usize, &PlanColumn)],
    ) -> bool {
        let parts: BTreeSet<usize> = carried.iter().map(|&(part, _)| part).collect();
        self.links.iter().any(|link| {
            link.held(statement)
                && parts.len() == carried.len()
                && parts.len() == link.parts.len()
                && carried.iter().all(|&(part, column)| link.reads(definitions, part, column))
        })
    }
}

/// The relationship the planner joins `root` to `metric`'s shared calendar on `root`'s source by:
/// the first declared one from `root` to it.
pub(super) fn calendar_hop<'d>(definitions: &'d Definitions, metric: &Metric, root: &Model) -> Option<&'d Relationship> {
    let calendar = metric
        .shared_calendars()?
        .iter()
        .filter_map(|name| definitions.model(name))
        .find(|calendar| calendar.source() == root.source())?;
    definitions
        .relationships()
        .values()
        .find(|relationship| relationship.origin_model() == root.name() && relationship.target_model() == calendar.name())
}
