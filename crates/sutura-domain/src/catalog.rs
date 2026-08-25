//! What a catalog says: models, the relationships between them, and the metrics defined over them.
//!
//! These are the types every `SemanticCatalog` adapter produces, and [`Definitions::assemble`] is
//! the one place their cross-references are checked. That matters more than it looks: a directory of
//! files and a metadata service over HTTP disagree about almost everything except this, so a check
//! that lived in an adapter would be a check the other adapter did not have. Two adapters reading
//! the same content must produce the same [`Definitions`] or one of them is wrong, and the golden
//! suite asserts exactly that.
//!
//! Nothing here holds SQL. See `docs/adr/0001-first-party-semantic-models.md`.

use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::TimeRange;
use crate::model::{
    ColumnName, DimensionName, Grain, JoinType, Measure, MetricName, ModelName, RelationshipName, SourceName, TableName,
};

/// The label a generated projection gives the truncated time column.
///
/// It lives here rather than in the compiler because it is part of the result schema, which is a
/// contract, and because [`Definitions::assemble`] has to know it: a dimension by this name would
/// produce two columns with one label, and a caller reading a result by name would get whichever
/// the data system listed first.
pub const TIME_BUCKET_LABEL: &str = "period";

/// One physical table, and what the catalog knows about it.
///
/// `columns` is the whole set the model exposes, and it is a set rather than a list because it is
/// only ever asked "does this column exist?". Declaring it at all is what lets a dimension naming a
/// column that is not there be a refusal from the pinned bundle instead of an error from the data
/// system, which is the difference between a governed answer and a stack trace.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Model {
    name: ModelName,
    source: SourceName,
    table: TableName,
    columns: BTreeSet<ColumnName>,
    description: String,
}

impl Model {
    pub const fn new(
        name: ModelName,
        source: SourceName,
        table: TableName,
        columns: BTreeSet<ColumnName>,
        description: String,
    ) -> Self {
        Self {
            name,
            source,
            table,
            columns,
            description,
        }
    }

    #[inline]
    pub const fn name(&self) -> &ModelName {
        &self.name
    }

    #[inline]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }

    #[inline]
    pub const fn table(&self) -> &TableName {
        &self.table
    }

    #[inline]
    pub const fn columns(&self) -> &BTreeSet<ColumnName> {
        &self.columns
    }

    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[inline]
    pub fn has_column(&self, column: &ColumnName) -> bool {
        self.columns.contains(column)
    }
}

/// A declared join between two models: two columns and a cardinality.
///
/// A pair of columns rather than a condition string. The condition form is what the reference
/// modelling languages use, and it is an escape hatch: `a.x = b.y OR 1 = 1` is a valid condition.
/// Equality on one column each is the whole of what a model needs to say here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Relationship {
    name: RelationshipName,
    origin_model: ModelName,
    origin_column: ColumnName,
    target_model: ModelName,
    target_column: ColumnName,
    join_type: JoinType,
}

impl Relationship {
    #[expect(
        clippy::too_many_arguments,
        reason = "a relationship is six facts and all six are required together; a builder would \
                  add a half-built state that this type cannot currently have"
    )]
    pub const fn new(
        name: RelationshipName,
        origin_model: ModelName,
        origin_column: ColumnName,
        target_model: ModelName,
        target_column: ColumnName,
        join_type: JoinType,
    ) -> Self {
        Self {
            name,
            origin_model,
            origin_column,
            target_model,
            target_column,
            join_type,
        }
    }

    #[inline]
    pub const fn name(&self) -> &RelationshipName {
        &self.name
    }

    #[inline]
    pub const fn origin_model(&self) -> &ModelName {
        &self.origin_model
    }

    #[inline]
    pub const fn origin_column(&self) -> &ColumnName {
        &self.origin_column
    }

    #[inline]
    pub const fn target_model(&self) -> &ModelName {
        &self.target_model
    }

    #[inline]
    pub const fn target_column(&self) -> &ColumnName {
        &self.target_column
    }

    #[inline]
    pub const fn join_type(&self) -> JoinType {
        self.join_type
    }
}

/// An attribute a metric declares it can be broken down by.
///
/// `via` is `None` for a column on the metric's own model and `Some` for one reached through exactly
/// one declared relationship. **One hop, deliberately.** Two hops need a join order, join order
/// changes which rows a measure sees, and "the number changed because the planner chose differently"
/// is the failure this whole repository is arranged against. A second hop arrives with a plan type
/// that can represent it, not with a loop here.
///
/// `allowed_values` is what makes a dimension filterable. `None` means it can be grouped by and not
/// filtered: a filter needs an allowlist, because the alternative is comparing against a value the
/// caller supplied, and the pinned bundle is the only thing entitled to say which values exist.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Dimension {
    name: DimensionName,
    column: ColumnName,
    via: Option<RelationshipName>,
    allowed_values: Option<BTreeSet<String>>,
    description: String,
}

impl Dimension {
    pub const fn new(
        name: DimensionName,
        column: ColumnName,
        via: Option<RelationshipName>,
        allowed_values: Option<BTreeSet<String>>,
        description: String,
    ) -> Self {
        Self {
            name,
            column,
            via,
            allowed_values,
            description,
        }
    }

    #[inline]
    pub const fn name(&self) -> &DimensionName {
        &self.name
    }

    #[inline]
    pub const fn column(&self) -> &ColumnName {
        &self.column
    }

    #[inline]
    pub const fn via(&self) -> Option<&RelationshipName> {
        self.via.as_ref()
    }

    #[inline]
    pub const fn allowed_values(&self) -> Option<&BTreeSet<String>> {
        self.allowed_values.as_ref()
    }

    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// May this dimension be filtered on at all?
    ///
    /// Read from the presence of an allowlist rather than from a separate flag, so the two cannot
    /// disagree: a `filterable: true` beside an empty allowlist would be a dimension that permits
    /// filtering and permits no value.
    #[inline]
    pub const fn is_filterable(&self) -> bool {
        self.allowed_values.is_some()
    }

    /// Is `value` one the bundle declares?
    ///
    /// A dimension with no allowlist answers `false` for everything, which is the safe direction:
    /// the caller gets `DimensionNotFilterable` rather than a query.
    pub fn permits(&self, value: &str) -> bool {
        self.allowed_values.as_ref().is_some_and(|values| values.contains(value))
    }
}

/// A number a metric is expected to produce, so that "it still means what it claimed" is checkable.
///
/// The value is text rather than a float on purpose. It is compared against the canonical rendering
/// of what the data system returned, and a float would make the comparison depend on how two
/// languages happen to print the same bits.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    range: TimeRange,
    value: String,
}

impl Anchor {
    pub const fn new(range: TimeRange, value: String) -> Self {
        Self { range, value }
    }

    #[inline]
    pub const fn range(&self) -> TimeRange {
        self.range
    }

    #[inline]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A certified metric: one measure over one model, and the shapes of question it will answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Metric {
    name: MetricName,
    model: ModelName,
    measure: Measure,
    time_column: ColumnName,
    grains: BTreeSet<Grain>,
    dimensions: BTreeMap<DimensionName, Dimension>,
    anchor: Option<Anchor>,
    description: String,
}

impl Metric {
    #[expect(
        clippy::too_many_arguments,
        reason = "a metric is eight facts, and a builder for a value this immutable would be more \
                  code with a partially-built state that this type currently cannot have"
    )]
    pub const fn new(
        name: MetricName,
        model: ModelName,
        measure: Measure,
        time_column: ColumnName,
        grains: BTreeSet<Grain>,
        dimensions: BTreeMap<DimensionName, Dimension>,
        anchor: Option<Anchor>,
        description: String,
    ) -> Self {
        Self {
            name,
            model,
            measure,
            time_column,
            grains,
            dimensions,
            anchor,
            description,
        }
    }

    #[inline]
    pub const fn name(&self) -> &MetricName {
        &self.name
    }

    #[inline]
    pub const fn model(&self) -> &ModelName {
        &self.model
    }

    #[inline]
    pub const fn measure(&self) -> &Measure {
        &self.measure
    }

    #[inline]
    pub const fn time_column(&self) -> &ColumnName {
        &self.time_column
    }

    #[inline]
    pub const fn grains(&self) -> &BTreeSet<Grain> {
        &self.grains
    }

    #[inline]
    pub const fn dimensions(&self) -> &BTreeMap<DimensionName, Dimension> {
        &self.dimensions
    }

    #[inline]
    pub const fn anchor(&self) -> Option<&Anchor> {
        self.anchor.as_ref()
    }

    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[inline]
    pub fn supports_grain(&self, grain: Grain) -> bool {
        self.grains.contains(&grain)
    }

    #[inline]
    pub fn dimension(&self, name: &DimensionName) -> Option<&Dimension> {
        self.dimensions.get(name)
    }
}

/// Everything a catalog said, with its cross-references checked.
///
/// `BTreeMap` throughout rather than `HashMap`, and that is load-bearing: the digest is taken over
/// the serialized form of this value, and an unordered map serializes in whatever order its hasher
/// chose this run. A digest that moves without the content moving is a digest nobody trusts, and
/// then the pinning is decoration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Definitions {
    models: BTreeMap<ModelName, Model>,
    relationships: BTreeMap<RelationshipName, Relationship>,
    metrics: BTreeMap<MetricName, Metric>,
}

/// Why a set of definitions does not hold together.
///
/// Every variant is a dangling reference of some kind. Catching them here, once, is what lets the
/// resolver assume that a metric's model exists and that a dimension's column is real: without it
/// each of those becomes a runtime branch on the query path, and the failure surfaces as a data
/// system error rather than as a refusal.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InconsistentDefinitions {
    #[error("model {model} is declared twice")]
    DuplicateModel { model: ModelName },
    #[error("metric {metric} is declared twice")]
    DuplicateMetric { metric: MetricName },
    #[error("relationship {relationship} is declared twice")]
    DuplicateRelationship { relationship: RelationshipName },
    #[error("metric {metric} names model {model}, which is not declared")]
    UnknownModel { metric: MetricName, model: ModelName },
    #[error("metric {metric} measures column {column}, which model {model} does not declare")]
    UnknownMeasureColumn {
        metric: MetricName,
        model: ModelName,
        column: ColumnName,
    },
    #[error("metric {metric} uses time column {column}, which model {model} does not declare")]
    UnknownTimeColumn {
        metric: MetricName,
        model: ModelName,
        column: ColumnName,
    },
    #[error("metric {metric} declares no grain, so no question about it could resolve")]
    NoGrains { metric: MetricName },
    #[error("dimension {dimension} of metric {metric} is reached via relationship {relationship}, which is not declared")]
    UnknownRelationship {
        metric: MetricName,
        dimension: DimensionName,
        relationship: RelationshipName,
    },
    #[error("relationship {relationship} joins from model {model}, which is not declared")]
    RelationshipFromUnknownModel {
        relationship: RelationshipName,
        model: ModelName,
    },
    #[error("relationship {relationship} joins to model {model}, which is not declared")]
    RelationshipToUnknownModel {
        relationship: RelationshipName,
        model: ModelName,
    },
    #[error("relationship {relationship} joins on column {column}, which model {model} does not declare")]
    RelationshipUnknownColumn {
        relationship: RelationshipName,
        model: ModelName,
        column: ColumnName,
    },
    #[error("dimension {dimension} of metric {metric} names column {column}, which model {model} does not declare")]
    UnknownDimensionColumn {
        metric: MetricName,
        dimension: DimensionName,
        model: ModelName,
        column: ColumnName,
    },
    #[error(
        "dimension {dimension} of metric {metric} is reached via relationship {relationship}, which does not start at the metric's model {model}"
    )]
    RelationshipNotFromMetricModel {
        metric: MetricName,
        dimension: DimensionName,
        relationship: RelationshipName,
        model: ModelName,
    },
    #[error(
        "dimension {dimension} of metric {metric} joins along {relationship}, which may duplicate rows and so would change the measure"
    )]
    JoinWouldDuplicateRows {
        metric: MetricName,
        dimension: DimensionName,
        relationship: RelationshipName,
    },
    #[error(
        "dimension {dimension} of metric {metric} declares an empty value allowlist, so it permits filtering and permits no value"
    )]
    EmptyAllowlist { metric: MetricName, dimension: DimensionName },
    #[error(
        "dimension {dimension} of metric {metric} is named {TIME_BUCKET_LABEL}, which is the label the time bucket is projected under"
    )]
    DimensionShadowsTimeBucket { metric: MetricName, dimension: DimensionName },
    #[error(
        "dimension {dimension} of metric {metric} has the metric's own name, which is the label the measure is projected under"
    )]
    DimensionShadowsMeasure { metric: MetricName, dimension: DimensionName },
}

impl Definitions {
    /// Assembles definitions from what an adapter read, checking every cross-reference.
    ///
    /// Takes vectors rather than maps so the duplicate checks are ours: a caller that built a map
    /// first has already silently dropped one of a duplicated pair, and "the second declaration of
    /// revenue won" is not a thing to discover from a number.
    pub fn assemble(
        models: Vec<Model>,
        relationships: Vec<Relationship>,
        metrics: Vec<Metric>,
    ) -> Result<Self, InconsistentDefinitions> {
        let mut model_map: BTreeMap<ModelName, Model> = BTreeMap::new();
        for model in models {
            if let Some(existing) = model_map.insert(model.name.clone(), model) {
                return Err(InconsistentDefinitions::DuplicateModel { model: existing.name });
            }
        }

        let mut relationship_map: BTreeMap<RelationshipName, Relationship> = BTreeMap::new();
        for relationship in relationships {
            Self::check_relationship(&model_map, &relationship)?;
            if let Some(existing) = relationship_map.insert(relationship.name.clone(), relationship) {
                return Err(InconsistentDefinitions::DuplicateRelationship {
                    relationship: existing.name,
                });
            }
        }

        let mut metric_map: BTreeMap<MetricName, Metric> = BTreeMap::new();
        for metric in metrics {
            Self::check_metric(&model_map, &relationship_map, &metric)?;
            if let Some(existing) = metric_map.insert(metric.name.clone(), metric) {
                return Err(InconsistentDefinitions::DuplicateMetric { metric: existing.name });
            }
        }

        Ok(Self {
            models: model_map,
            relationships: relationship_map,
            metrics: metric_map,
        })
    }

    fn check_relationship(
        models: &BTreeMap<ModelName, Model>,
        relationship: &Relationship,
    ) -> Result<(), InconsistentDefinitions> {
        let from =
            models
                .get(&relationship.origin_model)
                .ok_or_else(|| InconsistentDefinitions::RelationshipFromUnknownModel {
                    relationship: relationship.name.clone(),
                    model: relationship.origin_model.clone(),
                })?;
        let to = models
            .get(&relationship.target_model)
            .ok_or_else(|| InconsistentDefinitions::RelationshipToUnknownModel {
                relationship: relationship.name.clone(),
                model: relationship.target_model.clone(),
            })?;
        for (model, column) in [(from, &relationship.origin_column), (to, &relationship.target_column)] {
            if !model.has_column(column) {
                return Err(InconsistentDefinitions::RelationshipUnknownColumn {
                    relationship: relationship.name.clone(),
                    model: model.name.clone(),
                    column: column.clone(),
                });
            }
        }
        Ok(())
    }

    fn check_metric(
        models: &BTreeMap<ModelName, Model>,
        relationships: &BTreeMap<RelationshipName, Relationship>,
        metric: &Metric,
    ) -> Result<(), InconsistentDefinitions> {
        let model = models
            .get(&metric.model)
            .ok_or_else(|| InconsistentDefinitions::UnknownModel {
                metric: metric.name.clone(),
                model: metric.model.clone(),
            })?;
        if !model.has_column(metric.measure.column()) {
            return Err(InconsistentDefinitions::UnknownMeasureColumn {
                metric: metric.name.clone(),
                model: model.name.clone(),
                column: metric.measure.column().clone(),
            });
        }
        if !model.has_column(&metric.time_column) {
            return Err(InconsistentDefinitions::UnknownTimeColumn {
                metric: metric.name.clone(),
                model: model.name.clone(),
                column: metric.time_column.clone(),
            });
        }
        if metric.grains.is_empty() {
            return Err(InconsistentDefinitions::NoGrains {
                metric: metric.name.clone(),
            });
        }
        for dimension in metric.dimensions.values() {
            Self::check_dimension(models, relationships, metric, model, dimension)?;
        }
        Ok(())
    }

    fn check_dimension(
        models: &BTreeMap<ModelName, Model>,
        relationships: &BTreeMap<RelationshipName, Relationship>,
        metric: &Metric,
        model: &Model,
        dimension: &Dimension,
    ) -> Result<(), InconsistentDefinitions> {
        // Two columns with one label is not a modelling opinion, it is a result set a caller cannot
        // read by name. Caught here, once, at load, rather than as a query-time refusal for
        // something the caller did not choose.
        if dimension.name.as_str() == TIME_BUCKET_LABEL {
            return Err(InconsistentDefinitions::DimensionShadowsTimeBucket {
                metric: metric.name.clone(),
                dimension: dimension.name.clone(),
            });
        }
        if dimension.name.as_str() == metric.name.as_str() {
            return Err(InconsistentDefinitions::DimensionShadowsMeasure {
                metric: metric.name.clone(),
                dimension: dimension.name.clone(),
            });
        }

        if dimension.allowed_values.as_ref().is_some_and(BTreeSet::is_empty) {
            return Err(InconsistentDefinitions::EmptyAllowlist {
                metric: metric.name.clone(),
                dimension: dimension.name.clone(),
            });
        }

        let owning = match dimension.via.as_ref() {
            None => model,
            Some(name) => {
                let relationship = relationships
                    .get(name)
                    .ok_or_else(|| InconsistentDefinitions::UnknownRelationship {
                        metric: metric.name.clone(),
                        dimension: dimension.name.clone(),
                        relationship: name.clone(),
                    })?;
                if relationship.origin_model != metric.model {
                    return Err(InconsistentDefinitions::RelationshipNotFromMetricModel {
                        metric: metric.name.clone(),
                        dimension: dimension.name.clone(),
                        relationship: name.clone(),
                        model: metric.model.clone(),
                    });
                }
                if relationship.join_type.may_duplicate_rows() {
                    return Err(InconsistentDefinitions::JoinWouldDuplicateRows {
                        metric: metric.name.clone(),
                        dimension: dimension.name.clone(),
                        relationship: name.clone(),
                    });
                }
                // `check_relationship` already proved this model exists, so the branch is not
                // reachable. It is still written as a lookup rather than an `expect`, because the
                // no-panic ban is not conditional on an argument being right.
                models
                    .get(&relationship.target_model)
                    .ok_or_else(|| InconsistentDefinitions::RelationshipToUnknownModel {
                        relationship: name.clone(),
                        model: relationship.target_model.clone(),
                    })?
            }
        };

        if !owning.has_column(&dimension.column) {
            return Err(InconsistentDefinitions::UnknownDimensionColumn {
                metric: metric.name.clone(),
                dimension: dimension.name.clone(),
                model: owning.name.clone(),
                column: dimension.column.clone(),
            });
        }
        Ok(())
    }

    #[inline]
    pub const fn models(&self) -> &BTreeMap<ModelName, Model> {
        &self.models
    }

    #[inline]
    pub const fn relationships(&self) -> &BTreeMap<RelationshipName, Relationship> {
        &self.relationships
    }

    #[inline]
    pub const fn metrics(&self) -> &BTreeMap<MetricName, Metric> {
        &self.metrics
    }

    #[inline]
    pub fn metric(&self, name: &MetricName) -> Option<&Metric> {
        self.metrics.get(name)
    }

    #[inline]
    pub fn model(&self, name: &ModelName) -> Option<&Model> {
        self.models.get(name)
    }

    #[inline]
    pub fn relationship(&self, name: &RelationshipName) -> Option<&Relationship> {
        self.relationships.get(name)
    }
}

#[cfg(test)]
mod tests;
