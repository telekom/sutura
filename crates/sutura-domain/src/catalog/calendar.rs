use std::collections::{BTreeMap, BTreeSet};

use super::{InconsistentDefinitions, Metric, Model, Relationship};
use crate::model::{ModelName, RelationshipName, SourceName};
use crate::plan::federated::LinkKind;

/// The calendar models a cross-model ratio buckets its fact legs through: one per data system a
/// fact lives on (`docs/adr/0002`'s sixth amendment).
///
/// **Non-empty and repeat-free by construction.** That a listed model exists, sits on a source no
/// other listed model sits on, and is reached from each fact model are cross-references, so
/// [`Definitions::assemble`](super::Definitions::assemble) checks them - the type holds only what it
/// can see. Kept as a set, so the order an author lists them in does not move the definition digest.
/// There is no `Deserialize`: an adapter owns how a document spells the list and converts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SharedCalendars(BTreeSet<ModelName>);

/// Why a list of shared calendars could not be built.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidSharedCalendars {
    #[error("a shared calendar list must name at least one model")]
    Empty,
    #[error("the shared calendar list names model {0} more than once")]
    Repeated(ModelName),
}

impl SharedCalendars {
    /// The calendars listed, refusing an empty list and a model named twice.
    pub fn parse(models: impl IntoIterator<Item = ModelName>) -> Result<Self, InvalidSharedCalendars> {
        let mut calendars = BTreeSet::new();
        for model in models {
            if let Some(repeat) = calendars.replace(model) {
                return Err(InvalidSharedCalendars::Repeated(repeat));
            }
        }
        if calendars.is_empty() {
            return Err(InvalidSharedCalendars::Empty);
        }
        Ok(Self(calendars))
    }

    /// One calendar: the declaration of a ratio whose facts all sit on one source.
    pub fn single(model: ModelName) -> Self {
        Self(BTreeSet::from([model]))
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ModelName> {
        self.0.iter()
    }
}

/// The load checks for a metric's shared calendars, in the order a catalog author fixes them.
///
/// Every fact model - the metric's own and each cross-model term's - must have exactly one listed
/// calendar on its own source, reached by a relationship from the model, because each fact leg joins
/// the calendar inside its own statement and a statement reads one source. Every listed calendar
/// must declare the metric's time column, and two whose declared types name different kinds
/// (`LinkKind::declared`, case-folded: `date` and `DATE` agree) are refused.
///
/// **Limit:** sutura cannot check that two calendar tables hold the same rows. A type that is
/// undeclared, or of a kind `LinkKind::declared` does not know (a timestamp), is not compared; the
/// declaration is trusted.
pub(super) fn check(
    models: &BTreeMap<ModelName, Model>,
    relationships: &BTreeMap<RelationshipName, Relationship>,
    metric: &Metric,
    calendars: &SharedCalendars,
) -> Result<(), InconsistentDefinitions> {
    let mut on_source: BTreeMap<&SourceName, &Model> = BTreeMap::new();
    for name in calendars.iter() {
        let Some(calendar) = models.get(name) else {
            return Err(InconsistentDefinitions::SharedCalendarNotReachable {
                metric: metric.name.clone(),
                calendar: name.clone(),
                model: name.clone(),
            });
        };
        if let Some(first) = on_source.insert(calendar.source(), calendar) {
            return Err(InconsistentDefinitions::SharedCalendarsShareASource {
                metric: metric.name.clone(),
                data_system: calendar.source().clone(),
                first: first.name.clone(),
                second: calendar.name.clone(),
            });
        }
    }
    let term_models = metric
        .computation
        .measure()
        .into_iter()
        .flat_map(|measure| measure.models().into_iter().flatten());
    let facts: BTreeSet<&ModelName> = std::iter::once(&metric.model).chain(term_models).collect();
    for fact in facts.into_iter().filter_map(|name| models.get(name)) {
        let Some(calendar) = on_source.get(fact.source()) else {
            return Err(InconsistentDefinitions::NoSharedCalendarOnSource {
                metric: metric.name.clone(),
                model: fact.name.clone(),
                data_system: fact.source().clone(),
            });
        };
        let joins = relationships
            .values()
            .any(|rel| rel.origin_model() == &fact.name && rel.target_model() == &calendar.name);
        if !joins {
            return Err(InconsistentDefinitions::SharedCalendarNotReachable {
                metric: metric.name.clone(),
                calendar: calendar.name.clone(),
                model: fact.name.clone(),
            });
        }
    }
    let mut typed: Option<(&Model, LinkKind)> = None;
    for calendar in on_source.values() {
        let Some(column) = calendar.column(&metric.time_column) else {
            return Err(InconsistentDefinitions::UnknownTimeColumn {
                metric: metric.name.clone(),
                model: calendar.name.clone(),
                column: metric.time_column.clone(),
            });
        };
        match (typed, column.data_type().and_then(LinkKind::declared)) {
            (Some((first, seen)), Some(declared)) if seen != declared => {
                return Err(InconsistentDefinitions::SharedCalendarsDisagreeOnTimeType {
                    metric: metric.name.clone(),
                    column: metric.time_column.clone(),
                    first: first.name.clone(),
                    other: calendar.name.clone(),
                });
            }
            (None, Some(declared)) => typed = Some((calendar, declared)),
            _ => {}
        }
    }
    Ok(())
}
