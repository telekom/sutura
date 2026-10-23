//! What a leg may name on this node: the joins, dimensions and links its metric declares.
//!
//! A fact leg rooted on its metric's own model may join the hops of the metric's dimension chains
//! that stay on its source, and key or filter on a dimension whose whole chain stays there. A lookup
//! leg names its metric too: it may join the hops one of that metric's dimension chains makes after
//! it crosses into the leg's root, and key or filter on a dimension whose chain ends there. A fact
//! leg bucketed on a shared calendar may also join that calendar, through the one
//! relationship the planner picks for its root.

use std::collections::BTreeSet;

use super::Statement;
use crate::catalog::{Definitions, Dimension, Metric, Model, Relationship};
use crate::model::{DimensionName, RelationshipName, SourceName};
use crate::plan::PlanColumn;

/// A dimension a leg may name: the column it reads and the joins its chain reaches it through.
struct Reach<'d> {
    dimension: &'d Dimension,
    column: PlanColumn,
    through: Vec<&'d RelationshipName>,
}

/// The joins and dimensions one leg may name.
pub(super) struct Surface<'d> {
    joins: BTreeSet<&'d RelationshipName>,
    reaches: Vec<Reach<'d>>,
}

/// One hop of a dimension chain and the model it lands on.
type Hop<'d> = (&'d RelationshipName, &'d Model);

/// Each hop of `chain` with the model it lands on, or `None` when a hop is not declared here.
fn hops<'d>(definitions: &'d Definitions, chain: &'d [RelationshipName]) -> Option<Vec<Hop<'d>>> {
    chain
        .iter()
        .map(|name| {
            let relationship = definitions.relationship(name)?;
            Some((name, definitions.model(relationship.target_model())?))
        })
        .collect()
}

impl<'d> Surface<'d> {
    /// A fact leg's surface: `calendar` only, unless the leg is rooted on `metric`'s own model.
    pub(super) fn of_fact(
        definitions: &'d Definitions,
        metric: &'d Metric,
        root: &'d Model,
        calendar: Option<&'d RelationshipName>,
    ) -> Self {
        let mut surface = Self {
            joins: calendar.into_iter().collect(),
            reaches: Vec::new(),
        };
        if root.name() != metric.model() {
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
            surface.joins.extend(hops.iter().take(local).map(|&(name, _)| name));
            if local == hops.len() {
                let at = hops.last().map_or(root, |&(_, target)| target);
                surface.reaches.push(Reach {
                    dimension,
                    column: PlanColumn::new(at.table_name().clone(), dimension.column().clone()),
                    through: hops.iter().map(|&(name, _)| name).collect(),
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
            for &(name, target) in &hops {
                if target.source() != at.source() {
                    through = (target.name() == root.name()).then(Vec::new);
                } else if let Some(joined) = through.as_mut() {
                    joined.push(name);
                    surface.joins.insert(name);
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
}

/// Whether `column` is key part `part` of a declared relationship between two sources: its origin
/// on a model of `statement`, or, for a lookup leg, its target on the leg's root.
pub(super) fn links(
    (definitions, source): (&Definitions, &SourceName),
    statement: &Statement<'_>,
    lookup: bool,
    part: usize,
    column: &PlanColumn,
) -> bool {
    definitions.relationships().values().any(|relationship| {
        let (Some(origin), Some(target), Some(key)) = (
            definitions.model(relationship.origin_model()),
            definitions.model(relationship.target_model()),
            relationship.keys().iter().nth(part),
        ) else {
            return false;
        };
        let leaves = origin.source() == source
            && target.source() != source
            && statement.models.iter().any(|model| model.name() == origin.name())
            && column.table() == origin.table_name()
            && column.column() == key.origin();
        let enters = lookup
            && target.source() == source
            && origin.source() != source
            && statement.root.name() == target.name()
            && column.table() == target.table_name()
            && column.column() == key.target();
        leaves || enters
    })
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
