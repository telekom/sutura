//! A cube document: one model, one time column and one dimension list, shared by several measures.
//!
//! Authoring sugar and nothing more (`telekom/sutura#1148`). [`CubeDoc::into_domain`] expands it
//! into one [`MetricDoc`] per measure and converts each through [`MetricDoc::into_domain`], so a
//! cube's metric is checked by exactly the code a hand-written metric document is, and the domain
//! never sees a cube. A cube and its hand-flattened equivalent are the same bundle, digest included.
//!
//! What a measure cannot say is held by `deny_unknown_fields` on [`MeasureDoc`]: no `dimensions:`
//! of its own (the cube's list is the only one), no `model:`, `time_column:` or `grains:`, and no
//! `shared_calendar:` - a measure needing one is a metric document for now. `hierarchies:` is
//! refused by name on [`CubeDoc`] the same way: a roll-up order has no domain representation.

use std::collections::BTreeSet;

use sutura_domain::catalog::{Description, Metric};
use sutura_domain::expression::AuthoredSql;
use sutura_domain::measure::{Measure, RequiredFilter};
use sutura_domain::model::{ColumnName, Grain, InvalidIdentifier, MetricName, ModelName};

use super::{AnchorDoc, AudienceDoc, DimensionDoc, DocumentKind, InvalidMetricDocument, MetricDoc};

/// The document. Its `name` and each measure's `name` use a metric name's grammar because the two
/// join into one: a measure's metric is `<cube>_<measure>`.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CubeDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: MetricName,
    model: ModelName,
    time_column: ColumnName,
    grains: BTreeSet<Grain>,
    #[serde(default)]
    dimensions: Vec<DimensionDoc>,
    /// A list with a `name` per entry rather than a map, for [`DimensionDoc`]'s reason: a YAML map
    /// keeps the last of two equal keys in silence, and a list lets the repeat reach a refusal.
    measures: Vec<MeasureDoc>,
}

/// One measure: the per-metric fields of [`MetricDoc`], parsed with the same adapters.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MeasureDoc {
    name: MetricName,
    #[serde(default, with = "serde_norway::with::singleton_map")]
    measure: Option<Measure>,
    #[serde(default)]
    authored_sql: Option<AuthoredSql>,
    #[serde(default, with = "serde_norway::with::singleton_map_recursive")]
    required_filters: Vec<RequiredFilter>,
    #[serde(default)]
    anchor: Option<AnchorDoc>,
    #[serde(with = "serde_norway::with::singleton_map")]
    audience: AudienceDoc,
    /// This measure's own prose. Absent means the cube document's body.
    #[serde(default)]
    description: Option<Description>,
}

/// Why a cube document cannot become metrics.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidCubeDocument {
    /// `measures: []`, which would otherwise load as nothing and say so nowhere.
    #[error("cube {cube} declares no measure")]
    NoMeasures { cube: MetricName },
    /// Held here rather than left to `InconsistentDefinitions::DuplicateMetric`, which would fire
    /// too but name neither the cube nor its file.
    #[error("cube {cube} declares measure {measure} more than once")]
    DuplicateMeasure { cube: MetricName, measure: MetricName },
    /// `<cube>_<measure>` is not a metric name. Both halves already parsed, so the one fault left is
    /// length.
    #[error("cube {cube}'s measure {measure} does not join into a metric name")]
    MetricName {
        cube: MetricName,
        measure: MetricName,
        #[source]
        cause: InvalidIdentifier,
    },
    /// A measure's metric refused as a metric document would be; the message names the metric.
    #[error(transparent)]
    Metric(InvalidMetricDocument),
}

impl CubeDoc {
    /// One metric per measure, in the order the measures are written.
    pub fn into_domain(self, prose: &Description) -> Result<Vec<Metric>, InvalidCubeDocument> {
        let mut seen = BTreeSet::new();
        if let Some(repeat) = self.measures.iter().find(|measure| !seen.insert(&measure.name)) {
            return Err(InvalidCubeDocument::DuplicateMeasure {
                cube: self.name,
                measure: repeat.name.clone(),
            });
        }
        if self.measures.is_empty() {
            return Err(InvalidCubeDocument::NoMeasures { cube: self.name });
        }
        let mut metrics = Vec::with_capacity(self.measures.len());
        for measure in self.measures {
            let name = MetricName::parse(format!("{}_{}", self.name, measure.name)).map_err(|cause| {
                InvalidCubeDocument::MetricName {
                    cube: self.name.clone(),
                    measure: measure.name.clone(),
                    cause,
                }
            })?;
            let description = measure.description.unwrap_or_else(|| prose.clone());
            let doc = MetricDoc {
                kind: DocumentKind::Metric,
                name,
                model: self.model.clone(),
                measure: measure.measure,
                authored_sql: measure.authored_sql,
                required_filters: measure.required_filters,
                time_column: self.time_column.clone(),
                grains: self.grains.clone(),
                dimensions: self.dimensions.clone(),
                anchor: measure.anchor,
                audience: measure.audience,
                shared_calendar: None,
            };
            metrics.push(doc.into_domain(description).map_err(InvalidCubeDocument::Metric)?);
        }
        Ok(metrics)
    }
}
