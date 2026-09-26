//! Turning a parsed wren `Manifest` into planned sutura documents and a [`Refusal`] for everything
//! else.
//!
//! **Every wren kind either becomes a planned document or a [`Refusal`] naming it - nothing is
//! silently dropped.** The one exception is a wren column that navigates a relationship
//! (`column.relationship` is set): it carries no data of its own, and the relationship it names is
//! already converted in full from the manifest's own `relationships:` list, so folding it in
//! produces a note in [`Converted`]'s own `notes` rather than a refusal - it is represented, just
//! not as a model column.
//!
//! Nothing here executes or validates against a live model the way `sutura_domain::catalog` does at
//! load - a metric naming a column its model does not have loads and is refused there, by the same
//! mechanism a hand-written document is held to. This module's job stops at the same line
//! `sutura-catalog-local`'s own document types stop at: parse what was written, refuse what does not
//! parse, leave consistency to the load a human then reviews.

use std::collections::BTreeMap;

use sutura_domain::model::{ColumnName, DimensionName, MetricName, ModelName, RelationshipName};

use super::plan::{
    PlannedColumn, PlannedComputation, PlannedDimension, PlannedMetric, PlannedModel, PlannedRelationship, PlannedTerm,
};
use super::wire::{JoinType, Manifest};
use super::{recognize, wire};

/// One wren item this converter could not carry into sutura, named rather than dropped.
pub(crate) struct Refusal {
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    pub(crate) reason: String,
}

impl Refusal {
    fn new(kind: &'static str, name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            reason: reason.into(),
        }
    }
}

/// Everything a wren project produced: what maps, what does not, and what a reviewer should know
/// that is neither.
#[derive(Default)]
pub(crate) struct Converted {
    pub(crate) models: Vec<PlannedModel>,
    pub(crate) relationships: Vec<PlannedRelationship>,
    pub(crate) metrics: Vec<PlannedMetric>,
    pub(crate) refusals: Vec<Refusal>,
    /// Something represented, but not the way its own wren entry looked - a relationship-navigation
    /// column, or a default this converter picked that wren declares no opinion about.
    pub(crate) notes: Vec<String>,
}

/// Records a parse failure as a named refusal and returns `None`, so every caller can fall through
/// with `let Some(x) = .. else { continue };` instead of repeating the push.
fn parsed<T, E>(result: Result<T, E>, describe: impl FnOnce() -> String, refusals: &mut Vec<Refusal>) -> Option<T>
where
    E: core::fmt::Display,
{
    match result {
        Ok(value) => Some(value),
        Err(cause) => {
            refusals.push(Refusal::new("unusable_identifier", describe(), cause.to_string()));
            None
        }
    }
}

/// The path a wren `tableReference` composes to, or `None` if it names nothing.
fn table_path(reference: Option<&wire::TableReference>) -> Option<String> {
    let reference = reference?;
    let parts: Vec<&str> = [
        reference.catalog.as_deref(),
        reference.schema.as_deref(),
        reference.table.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect();
    (!parts.is_empty()).then(|| parts.join("."))
}

fn convert_models(manifest: &Manifest, out: &mut Converted) -> BTreeMap<String, ModelName> {
    let mut by_wren_name = BTreeMap::new();
    for model in &manifest.models {
        if model.ref_sql.is_some() {
            out.refusals.push(Refusal::new(
                "ref_sql_model",
                &model.name,
                "a model with `refSql` set is a SQL view; conversion never parses foreign SQL",
            ));
            continue;
        }
        let Some(table) = table_path(model.table_reference.as_ref()) else {
            out.refusals.push(Refusal::new(
                "model_without_table",
                &model.name,
                "declares neither `refSql` nor `tableReference`",
            ));
            continue;
        };
        for rlac in &model.row_level_access_controls {
            out.refusals.push(Refusal::new(
                "row_access_control",
                format!("{}.{}", model.name, rlac.name),
                "a row-level access control has no sutura equivalent",
            ));
        }
        let mut columns = Vec::with_capacity(model.columns.len());
        for column in &model.columns {
            if let Some(relationship) = &column.relationship {
                out.notes.push(format!(
                    "{}.{} is a relationship-navigation column; represented by the `{relationship}` relationship document \
                     instead of a model column",
                    model.name, column.name
                ));
                continue;
            }
            if column.access_control.is_some() {
                out.refusals.push(Refusal::new(
                    "column_access_control",
                    format!("{}.{}", model.name, column.name),
                    "a column-level access control has no sutura equivalent",
                ));
                continue;
            }
            let is_computed = column.is_calculated || column.expression.as_deref().is_some_and(|e| !e.trim().is_empty());
            if is_computed {
                out.refusals.push(Refusal::new(
                    "calculated_column",
                    format!("{}.{}", model.name, column.name),
                    "a calculated column carries a SQL expression sutura's column metadata cannot hold",
                ));
                continue;
            }
            let Some(name) = parsed(
                ColumnName::parse(&column.name),
                || format!("{}.{}", model.name, column.name),
                &mut out.refusals,
            ) else {
                continue;
            };
            columns.push(PlannedColumn {
                name,
                data_type: column.r#type.clone(),
                nullable: !column.not_null,
            });
        }
        let Some(name) = parsed(ModelName::parse(&model.name), || model.name.clone(), &mut out.refusals) else {
            continue;
        };
        let primary_key = model
            .primary_key
            .as_deref()
            .and_then(|raw| ColumnName::parse(raw).ok())
            .filter(|key| columns.iter().any(|column| &column.name == key));
        drop(by_wren_name.insert(model.name.clone(), name.clone()));
        out.models.push(PlannedModel {
            name,
            table,
            columns,
            primary_key,
        });
    }
    by_wren_name
}

/// `sutura_domain::model::JoinType::as_str`'s own three spellings, or `None` for `MANY_TO_MANY` -
/// which has no variant to hold it. The one place that mapping is written, so the refusal above and
/// the field below can never disagree about which three wren join types survive.
const fn join_type_str(join_type: JoinType) -> Option<&'static str> {
    match join_type {
        JoinType::OneToOne => Some("one_to_one"),
        JoinType::OneToMany => Some("one_to_many"),
        JoinType::ManyToOne => Some("many_to_one"),
        JoinType::ManyToMany => None,
    }
}

fn convert_relationships(manifest: &Manifest, models: &BTreeMap<String, ModelName>, out: &mut Converted) {
    for relationship in &manifest.relationships {
        let [origin_raw, target_raw] = relationship.models.as_slice() else {
            out.refusals.push(Refusal::new(
                "unusable_relationship",
                &relationship.name,
                format!("declares {} models, not exactly two", relationship.models.len()),
            ));
            continue;
        };
        let Some(join_type) = join_type_str(relationship.join_type) else {
            out.refusals.push(Refusal::new(
                "many_to_many_relationship",
                &relationship.name,
                "MANY_TO_MANY has no JoinType in sutura's closed vocabulary",
            ));
            continue;
        };
        let Some((left, right)) = recognize::equi_join(&relationship.condition) else {
            out.refusals.push(Refusal::new(
                "unrecognised_join_condition",
                &relationship.name,
                "the condition is not a plain equality between two declared columns",
            ));
            continue;
        };
        if left.model != *origin_raw || right.model != *target_raw {
            out.refusals.push(Refusal::new(
                "unrecognised_join_condition",
                &relationship.name,
                "the condition's models are not in the declared `models:` order",
            ));
            continue;
        }
        let (Some(origin_model), Some(target_model)) = (models.get(origin_raw), models.get(target_raw)) else {
            out.refusals.push(Refusal::new(
                "relationship_on_unmapped_model",
                &relationship.name,
                "names a model that was refused or never declared",
            ));
            continue;
        };
        let Some(name) = parsed(
            RelationshipName::parse(&relationship.name),
            || relationship.name.clone(),
            &mut out.refusals,
        ) else {
            continue;
        };
        let Some(origin_column) = parsed(
            ColumnName::parse(&left.column),
            || format!("{origin_raw}.{}", left.column),
            &mut out.refusals,
        ) else {
            continue;
        };
        let Some(target_column) = parsed(
            ColumnName::parse(&right.column),
            || format!("{target_raw}.{}", right.column),
            &mut out.refusals,
        ) else {
            continue;
        };
        out.relationships.push(PlannedRelationship {
            name,
            origin_model: origin_model.clone(),
            origin_column,
            target_model: target_model.clone(),
            target_column,
            join_type,
        });
    }
}

fn convert_views(manifest: &Manifest, out: &mut Converted) {
    for view in &manifest.views {
        out.refusals.push(Refusal::new(
            "sql_view",
            &view.name,
            "a wren view is a named SQL statement; conversion never parses foreign SQL",
        ));
    }
}

/// The wren term this recognised, as a [`PlannedTerm`] - or `None`, with the refusal already
/// pushed.
fn planned_term(term: &recognize::Term, describe: impl FnOnce() -> String, refusals: &mut Vec<Refusal>) -> Option<PlannedTerm> {
    let column = parsed(ColumnName::parse(&term.column), describe, refusals)?;
    Some(PlannedTerm {
        aggregate: term.aggregate,
        column,
    })
}

fn convert_cubes(manifest: &Manifest, models: &BTreeMap<String, ModelName>, out: &mut Converted) {
    for cube in &manifest.cubes {
        let Some(base_model) = models.get(&cube.base_object) else {
            out.refusals.push(Refusal::new(
                "cube_on_unmapped_model",
                &cube.name,
                "the cube's base model was refused or never declared",
            ));
            continue;
        };
        if !cube.hierarchies.is_empty() {
            let keys: Vec<&str> = cube.hierarchies.keys().map(String::as_str).collect();
            out.refusals.push(Refusal::new(
                "cube_hierarchy",
                format!("{}: {}", cube.name, keys.join(", ")),
                "a cube hierarchy has no sutura equivalent",
            ));
        }
        let mut dimensions = Vec::with_capacity(cube.dimensions.len());
        for dimension in &cube.dimensions {
            let full_name = format!("{}.{}", cube.name, dimension.name);
            let Some(bare) = recognize::bare_column(&dimension.expression) else {
                out.refusals.push(Refusal::new(
                    "free_sql_dimension",
                    &full_name,
                    "the expression is not a bare column name",
                ));
                continue;
            };
            let Some(name) = parsed(DimensionName::parse(&dimension.name), || full_name.clone(), &mut out.refusals) else {
                continue;
            };
            let Some(column) = parsed(ColumnName::parse(bare), || full_name.clone(), &mut out.refusals) else {
                continue;
            };
            dimensions.push(PlannedDimension { name, column });
        }
        let mut time_columns = Vec::with_capacity(cube.time_dimensions.len());
        for time_dimension in &cube.time_dimensions {
            let full_name = format!("{}.{}", cube.name, time_dimension.name);
            let Some(bare) = recognize::bare_column(&time_dimension.expression) else {
                out.refusals.push(Refusal::new(
                    "free_sql_dimension",
                    &full_name,
                    "the expression is not a bare column name",
                ));
                continue;
            };
            let Some(column) = parsed(ColumnName::parse(bare), || full_name.clone(), &mut out.refusals) else {
                continue;
            };
            time_columns.push(column);
        }
        let Some(time_column) = time_columns.into_iter().next() else {
            out.refusals.push(Refusal::new(
                "cube_without_time_dimension",
                &cube.name,
                "no `timeDimensions` entry recognised as a bare column; a metric needs one `time_column`",
            ));
            continue;
        };
        for measure in &cube.measures {
            let full_name = format!("{}.{}", cube.name, measure.name);
            let Some(recognised) = recognize::measure(&measure.expression) else {
                out.refusals.push(Refusal::new(
                    "free_sql_measure",
                    &full_name,
                    "the expression is not one of the closed-vocabulary shapes",
                ));
                continue;
            };
            let computation = match recognised {
                recognize::Measure::Simple(term) => {
                    let Some(term) = planned_term(&term, || full_name.clone(), &mut out.refusals) else {
                        continue;
                    };
                    PlannedComputation::Simple(term)
                }
                recognize::Measure::Ratio { numerator, denominator } => {
                    let Some(numerator) = planned_term(&numerator, || full_name.clone(), &mut out.refusals) else {
                        continue;
                    };
                    let Some(denominator) = planned_term(&denominator, || full_name.clone(), &mut out.refusals) else {
                        continue;
                    };
                    PlannedComputation::Ratio { numerator, denominator }
                }
            };
            let Some(name) = parsed(
                MetricName::parse(format!("{}_{}", cube.name, measure.name)),
                || full_name.clone(),
                &mut out.refusals,
            ) else {
                continue;
            };
            out.metrics.push(PlannedMetric {
                name,
                model: base_model.clone(),
                computation,
                time_column: time_column.clone(),
                dimensions: dimensions.clone(),
                cube: cube.name.clone(),
                measure: measure.name.clone(),
            });
        }
    }
}

/// Converts a whole manifest. Every model is read first, because a relationship or a cube names one
/// by wren's own string and needs to resolve it to a parsed [`ModelName`].
pub(crate) fn convert(manifest: &Manifest) -> Converted {
    let mut out = Converted::default();
    let models = convert_models(manifest, &mut out);
    convert_relationships(manifest, &models, &mut out);
    convert_views(manifest, &mut out);
    convert_cubes(manifest, &models, &mut out);
    out.notes.push(String::from(
        "every mapped model was written with `source: wren`; rename it to match this deployment's own `sources:` \
         declaration before committing",
    ));
    out
}

#[cfg(test)]
mod tests;
