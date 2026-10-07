//! What a referent and a phrase resolve to in the definitions, for the checks [`super::Knowledge`] runs.
//!
//! Split out of `bundle.rs` for the line cap that file's own documentation names. The seam: this holds
//! the walks from a note's names to what the bundle declares under them, and `bundle` holds the
//! value they are collected into and the order the checks run in.

use std::collections::{BTreeMap, BTreeSet};

use super::{InconsistentKnowledge, NoteName, Phrase, Referent};
use crate::catalog::{Definitions, DimensionValue};
use crate::model::{ColumnName, DimensionName, MetricName, ModelName, RelationshipName};
use crate::pinned::view::ScopedView;

/// What a referent got wrong, before the note that wrote it dresses it as its own error.
///
/// One resolution and two dressings, rather than two copies of the resolution. The glossary and a
/// caveat name the offending document differently - a term, a note name - and that difference is the
/// whole reason their variants are not shared; the walk from a referent to the thing it refers to is
/// not different, so it is written once. Each variant carries every name it needs, so no site has to
/// re-derive one from the referent - which is where an `expect` on an unreachable branch would
/// otherwise appear.
pub(super) enum ReferentFault<'a> {
    UnknownMetric {
        metric: &'a MetricName,
    },
    UnknownDimension {
        metric: &'a MetricName,
        dimension: &'a DimensionName,
    },
    ValueNotAllowed {
        metric: &'a MetricName,
        dimension: &'a DimensionName,
        value: &'a DimensionValue,
    },
    UnknownModel {
        model: &'a ModelName,
    },
    UnknownColumn {
        model: &'a ModelName,
        column: &'a ColumnName,
    },
}

/// Does this referent name something the bundle declares?
pub(super) fn fault_in<'a>(definitions: &Definitions, referent: &'a Referent) -> Option<ReferentFault<'a>> {
    let metric = match *referent {
        Referent::Model { ref model } => {
            return definitions
                .model(model)
                .is_none()
                .then_some(ReferentFault::UnknownModel { model });
        }
        Referent::Column { ref model, ref column } => {
            let Some(declared) = definitions.model(model) else {
                return Some(ReferentFault::UnknownModel { model });
            };
            return (!declared.has_column(column)).then_some(ReferentFault::UnknownColumn { model, column });
        }
        Referent::Metric { ref metric } | Referent::Dimension { ref metric, .. } | Referent::Value { ref metric, .. } => metric,
    };
    let Some(declared) = definitions.metric(metric) else {
        return Some(ReferentFault::UnknownMetric { metric });
    };
    // `?` rather than `let ... else { return None }`, which the lint asks for: a referent that names
    // no dimension has nothing further to check, so the absence IS the answer.
    let dimension = referent.dimension()?;
    let Some(declared) = declared.dimension(dimension) else {
        return Some(ReferentFault::UnknownDimension { metric, dimension });
    };
    let value = referent.value()?;
    (!declared.permits(value)).then_some(ReferentFault::ValueNotAllowed {
        metric,
        dimension,
        value,
    })
}

/// Whether the caller behind `view` may see what `referent` names - `docs/adr/0028`.
///
/// A match over every variant, so a referent added later is a compile error here rather than a
/// note shown to every caller. A model or a column follows the MODEL: [`ScopedView::model`] holds a
/// model only when its audience is granted, so a model declared with no audience is withheld from
/// every caller-scoped view. The view scopes models and not columns, so a column is visible exactly
/// when its model is.
pub(super) fn visible(view: &ScopedView<'_>, referent: &Referent) -> bool {
    match *referent {
        Referent::Metric { ref metric } | Referent::Dimension { ref metric, .. } | Referent::Value { ref metric, .. } => {
            view.metric(metric).is_some()
        }
        Referent::Model { ref model } | Referent::Column { ref model, .. } => view.model(model).is_some(),
    }
}

/// The identifier a phrase would be, if somebody wrote it as one.
///
/// Lower-cased, with every run of characters that cannot appear in an identifier collapsed into one
/// underscore. It exists for exactly one comparison - an absence against a name the bundle declares -
/// and it is a free function rather than a method on [`Phrase`] because turning prose into an
/// identifier is not something a phrase should offer to do: the only legitimate use of the result is
/// to notice that two documents disagree.
pub(super) fn identifier_shape(phrase: &str) -> String {
    let mut out = String::new();
    for character in phrase.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    String::from(out.trim_matches('_'))
}

/// What the bundle declares under a phrase somebody recorded as undefined, as the absence's own error.
///
/// Compared through [`identifier_shape`] on both sides, so a phrase written the way a person writes it
/// is recognised as naming something written the way an identifier is written - and a declared value
/// like `business` is recognised in a note about "Business".
///
/// A metric, a dimension of one or a permitted value of one: the three things a metric block prints
/// as askable, which are what an absence would contradict. Three passes rather than one, so the
/// message is about the most important thing the phrase collides with: a phrase that names a metric
/// is reported as naming the metric even if some dimension somewhere shares the word.
pub(super) fn declared_as(definitions: &Definitions, phrase: &Phrase) -> Option<InconsistentKnowledge> {
    let shape = identifier_shape(phrase.as_str());
    for name in definitions.metrics().keys() {
        if identifier_shape(name.as_str()) == shape {
            return Some(InconsistentKnowledge::AbsenceNamesADefinedMetric {
                phrase: phrase.clone(),
                metric: name.clone(),
            });
        }
    }
    for (name, metric) in definitions.metrics() {
        for dimension in metric.dimensions().keys() {
            if identifier_shape(dimension.as_str()) == shape {
                return Some(InconsistentKnowledge::AbsenceNamesADeclaredDimension {
                    phrase: phrase.clone(),
                    metric: name.clone(),
                    dimension: dimension.clone(),
                });
            }
        }
    }
    for (name, metric) in definitions.metrics() {
        for (dimension, declared) in metric.dimensions() {
            for value in declared.allowed_values().into_iter().flatten() {
                if identifier_shape(value.as_str()) == shape {
                    return Some(InconsistentKnowledge::AbsenceNamesADeclaredValue {
                        phrase: phrase.clone(),
                        metric: name.clone(),
                        dimension: dimension.clone(),
                        value: value.clone(),
                    });
                }
            }
        }
    }
    None
}

/// One fault, dressed as the glossary's own error.
pub(super) fn glossary_fault(fault: &ReferentFault<'_>, term: &Phrase) -> InconsistentKnowledge {
    let term = term.clone();
    match *fault {
        ReferentFault::UnknownMetric { metric } => InconsistentKnowledge::GlossaryUnknownMetric {
            term,
            metric: metric.clone(),
        },
        ReferentFault::UnknownDimension { metric, dimension } => InconsistentKnowledge::GlossaryUnknownDimension {
            term,
            metric: metric.clone(),
            dimension: dimension.clone(),
        },
        ReferentFault::ValueNotAllowed {
            metric,
            dimension,
            value,
        } => InconsistentKnowledge::GlossaryValueNotAllowed {
            term,
            metric: metric.clone(),
            dimension: dimension.clone(),
            value: value.clone(),
        },
        ReferentFault::UnknownModel { model } => InconsistentKnowledge::GlossaryUnknownModel {
            term,
            model: model.clone(),
        },
        ReferentFault::UnknownColumn { model, column } => InconsistentKnowledge::GlossaryUnknownColumn {
            term,
            model: model.clone(),
            column: column.clone(),
        },
    }
}

/// One fault, dressed as a caveat's own error.
///
/// A caveat about a model or a column is refused before its referents are resolved, so the two model
/// faults are dressed as that refusal - which is what such a caveat is, declared model or not.
pub(super) fn caveat_fault(fault: &ReferentFault<'_>, name: &NoteName) -> InconsistentKnowledge {
    let name = name.clone();
    match *fault {
        ReferentFault::UnknownMetric { metric } => InconsistentKnowledge::CaveatUnknownMetric {
            name,
            metric: metric.clone(),
        },
        ReferentFault::UnknownDimension { metric, dimension } => InconsistentKnowledge::CaveatUnknownDimension {
            name,
            metric: metric.clone(),
            dimension: dimension.clone(),
        },
        ReferentFault::ValueNotAllowed {
            metric,
            dimension,
            value,
        } => InconsistentKnowledge::CaveatValueNotAllowed {
            name,
            metric: metric.clone(),
            dimension: dimension.clone(),
            value: value.clone(),
        },
        ReferentFault::UnknownModel { model } | ReferentFault::UnknownColumn { model, .. } => {
            InconsistentKnowledge::CaveatAboutAModel {
                name,
                model: model.clone(),
            }
        }
    }
}

/// The dimensions each metric reaches through any of these relationships: what a caveat written
/// about them is expanded into, one caveat per metric.
///
/// **"Reaches" is read off a dimension's `via` and nothing else.** A metric uses a relationship when
/// one of its dimensions names it anywhere in its chain, so the second hop of a two-hop chain counts.
/// A cross-model ratio's hop to its shared calendar does not: `sutura_semantic`'s resolver matches
/// that relationship by model rather than by a name in a `via`, so a caveat about it finds no metric
/// here and is refused rather than attached to the ratio.
///
/// Ordered maps, so the derived caveats and the dimensions each is about come out in one order
/// every load.
pub(super) fn reached_through<'d>(
    definitions: &'d Definitions,
    name: &NoteName,
    relationships: &[RelationshipName],
) -> Result<Reached<'d>, InconsistentKnowledge> {
    let mut reached = Reached::new();
    for relationship in relationships {
        if definitions.relationship(relationship).is_none() {
            return Err(InconsistentKnowledge::CaveatUnknownRelationship {
                name: name.clone(),
                relationship: relationship.clone(),
            });
        }
        let mut reaches_any = false;
        for (metric, declared) in definitions.metrics() {
            let through: Vec<&DimensionName> = declared
                .dimensions()
                .iter()
                .filter(|&(_, dimension)| dimension.via().is_some_and(|hops| hops.contains(relationship)))
                .map(|(dimension, _)| dimension)
                .collect();
            if !through.is_empty() {
                reaches_any = true;
                reached.entry(metric).or_default().extend(through);
            }
        }
        if !reaches_any {
            return Err(InconsistentKnowledge::CaveatRelationshipReachesNoMetric {
                name: name.clone(),
                relationship: relationship.clone(),
            });
        }
    }
    Ok(reached)
}

/// Each metric a relationship caveat reaches, and the dimensions it reaches through it.
pub(super) type Reached<'d> = BTreeMap<&'d MetricName, BTreeSet<&'d DimensionName>>;
