//! Whether what a catalog said holds together: the assembled [`Definitions`] and every
//! cross-reference check that had to pass before one could exist.
//!
//! [`super`] is what a catalog SAYS - a model, a join, a dimension, a metric, each of them a
//! declaration that means nothing about its neighbours. This is the other half of that sentence: the
//! one place those declarations are read against each other, and the type that exists only where
//! they agreed. Splitting the two is what lets the resolver assume a metric's model exists and a
//! dimension's column is real, because there is exactly one gate between an adapter's reading and a
//! [`Definitions`].
//!
//! # Why a separate file
//!
//! `cargo xtask max-lines` fails at a thousand lines under `crates/` and cannot be exempted, and
//! [`super`] plus this reached it. The seam is the one [`super`]'s own header already named - *the
//! declarations and the cross-reference checks* - so this is that sentence's second half becoming a
//! file rather than a place the file happened to be cut. Same arrangement, and the same reason, as
//! `super::authored`: the names stay where they were, because the module is the unit of API and the
//! files are not.

use std::collections::{BTreeMap, BTreeSet};

use super::{Dimension, MAX_VALUES_PER_DIMENSION, Metric, Model, Relationship, TIME_BUCKET_LABEL};
use crate::model::{ColumnName, DimensionName, IdentifierCase, MetricName, ModelName, RelationshipName, TableName};

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
/// Most variants are a dangling reference of some kind, and the rest are two declarations that
/// cannot both stand. Catching them here, once, is what lets the resolver assume that a metric's
/// model exists and that a dimension's column is real: without it each of those becomes a runtime
/// branch on the query path, and the failure surfaces as a data system error rather than as a
/// refusal.
///
/// **Two variants are raised by [`Metric::new`] and not by [`Definitions::assemble`]** -
/// [`Self::DuplicateDimension`] and [`Self::TwoDimensionsOneLabel`], both about a pair the
/// constructor is the last place that can see. They are in this enum anyway, so an adapter maps one
/// type from both seams.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InconsistentDefinitions {
    #[error("model {model} is declared twice")]
    DuplicateModel { model: ModelName },
    #[error("metric {metric} is declared twice")]
    DuplicateMetric { metric: MetricName },
    #[error("relationship {relationship} is declared twice")]
    DuplicateRelationship { relationship: RelationshipName },
    /// One metric declaring the same dimension twice.
    ///
    /// Raised by [`Metric::new`], which is the only place the pair is still visible; that
    /// constructor's own note is where the argument lives.
    #[error("metric {metric} declares dimension {dimension} twice")]
    DuplicateDimension { metric: MetricName, dimension: DimensionName },
    #[error("metric {metric} names model {model}, which is not declared")]
    UnknownModel { metric: MetricName, model: ModelName },
    #[error("metric {metric} measures column {column}, which model {model} does not declare")]
    UnknownMeasureColumn {
        metric: MetricName,
        model: ModelName,
        column: ColumnName,
    },
    #[error("metric {metric} has a required filter on column {column}, which model {model} does not declare")]
    UnknownRequiredFilterColumn {
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
    /// More declared values than [`MAX_VALUES_PER_DIMENSION`].
    ///
    /// Checked here rather than at [`DimensionValue::parse`], because a count is not a fact about one
    /// value: each of ten thousand values can be inside every per-value bound and the list of them is
    /// still the whole of one dimension's line in a rendered prompt. Same reason
    /// [`crate::knowledge::MAX_KNOWLEDGE_BYTES`] is checked over a bundle rather than over a note.
    #[error("dimension {dimension} of metric {metric} declares {count} values, and at most {limit} may be declared")]
    TooManyValues {
        metric: MetricName,
        dimension: DimensionName,
        count: usize,
        limit: usize,
    },
    #[error(
        "dimension {dimension} of metric {metric} is named {TIME_BUCKET_LABEL}, which is the label the time bucket is projected under"
    )]
    DimensionShadowsTimeBucket { metric: MetricName, dimension: DimensionName },
    #[error(
        "dimension {dimension} of metric {metric} has the metric's own name, which is the label the measure is projected under"
    )]
    DimensionShadowsMeasure { metric: MetricName, dimension: DimensionName },
    /// Two dimensions of one metric whose labels fold together.
    ///
    /// **The pair the two variants above could not see**, and the last of the label collisions to be
    /// closed: a dimension was folded against the time bucket and against the measure, and every
    /// label was folded against every table, while two dimensions were compared by nothing at all.
    ///
    /// Raised by [`Metric::new`], which is where the measured cost of a folded pair and the argument
    /// for asking before the keying both live. `first` and `second` are in the order the document
    /// declared them, which is what makes the message tell an author which of the two to rename.
    /// Both are the DECLARED spellings and neither is normalised: the folded form is what collided,
    /// and the unfolded ones are what is written in the file.
    #[error("metric {metric} declares dimensions {first} and {second}, which are one label once case is folded")]
    TwoDimensionsOneLabel {
        metric: MetricName,
        first: DimensionName,
        second: DimensionName,
    },
    /// A label this metric projects is spelled the same as a table its statement reads.
    ///
    /// **This one is a wrong-answer report rather than a hypothetical, and it was found by a live
    /// run.** A `BigQuery` submission came back `400 invalidQuery` - *"Cannot access field day on a
    /// value with type INT64"* - because the metric's label equalled the table name, and `GoogleSQL`
    /// resolved the qualifier in `table.column` to the **select-list alias** instead of to the table.
    /// Same root as the unqualified table itself: a physical name that nothing checked against the
    /// labels beside it.
    ///
    /// It is refused for **every** dialect rather than for the one that reported it, because a rule
    /// about which of two things a qualifier binds to is precisely the kind of difference nobody
    /// should be maintaining per target - and the alternative outcomes across four dialects are a
    /// wrong number, a rejected statement and silence.
    ///
    /// **The comparison folds case, and it did not until a review reproduced the hole.** `GoogleSQL`'s
    /// lexical reference lists *aliases within a query* and *column names* as NOT case-sensitive
    /// (checked 2026-08-30), so a table named `Orders` beside a projected label `orders` passed an
    /// equality check here and then collided in the generated statement exactly like the live
    /// same-case failure above. [`IdentifierCase`] is the vocabulary,
    /// `sutura_sql::Dialect::identifier_case` is the per-target declaration, and
    /// [`IdentifierCase::COARSEST`] is what this check compares under - see that type for why a
    /// dialect-agnostic bundle has to be held to the coarsest rule rather than to the serving
    /// target's.
    ///
    /// `label` is a `String` and not one of the three name types, because the three labels a
    /// statement projects are a metric name, a dimension name and
    /// [`TIME_BUCKET_LABEL`] - which is a `&str` constant. The variant says which text collided; the
    /// three sources of it are not what a reader needs to branch on.
    #[error("metric {metric} projects the label {label}, which is also the name of the table {table} its statement reads")]
    LabelShadowsTable {
        metric: MetricName,
        label: String,
        table: TableName,
    },
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
        // Every column the measure reads, whichever shape it is. `Measure::columns` is the single
        // place that knows, so a shape added there cannot be forgotten here - which is the failure
        // this loop replaces, from when a measure was one column and the check read it directly.
        for column in metric.measure.columns() {
            if !model.has_column(column) {
                return Err(InconsistentDefinitions::UnknownMeasureColumn {
                    metric: metric.name.clone(),
                    model: model.name.clone(),
                    column: column.clone(),
                });
            }
        }
        // A required filter is applied to every question about the metric, so a column it names that
        // does not exist is a metric that can never be answered - and the error has to say that
        // rather than surfacing later as a rejected statement.
        for filter in &metric.required_filters {
            if !model.has_column(filter.column()) {
                return Err(InconsistentDefinitions::UnknownRequiredFilterColumn {
                    metric: metric.name.clone(),
                    model: model.name.clone(),
                    column: filter.column().clone(),
                });
            }
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
        // The metric's own model, which every statement about it reads. Each dimension's owning model
        // is checked in `check_dimension`, where the relationship has already been resolved.
        Self::check_labels_against_table(metric, model.table_name())?;
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
        //
        // **Compared under `IdentifierCase::COARSEST` and not by equality**, because `GoogleSQL`
        // documents a result column's name as case-insensitive, so `Period` beside `period` is one
        // column there and two here. That type's own note is where the argument for using the
        // coarsest rule at load time lives, and `sutura_sql::Dialect::identifier_case` is where each
        // target declares its own.
        if IdentifierCase::COARSEST.names_one_thing(dimension.name.as_str(), TIME_BUCKET_LABEL) {
            return Err(InconsistentDefinitions::DimensionShadowsTimeBucket {
                metric: metric.name.clone(),
                dimension: dimension.name.clone(),
            });
        }
        if IdentifierCase::COARSEST.names_one_thing(dimension.name.as_str(), metric.name.as_str()) {
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
        // The count, which no per-value parse can see. Read from the assembled set rather than from
        // what the adapter handed over, so two adapters that spell the same allowlist differently -
        // a list with a repeat in it, a mapping - are held to the same number of DISTINCT values.
        if let Some(count) = dimension
            .allowed_values
            .as_ref()
            .map(BTreeSet::len)
            .filter(|count| *count > MAX_VALUES_PER_DIMENSION)
        {
            return Err(InconsistentDefinitions::TooManyValues {
                metric: metric.name.clone(),
                dimension: dimension.name.clone(),
                count,
                limit: MAX_VALUES_PER_DIMENSION,
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
        // The joined table, now that the relationship has been resolved. `check_metric` covers the
        // metric's own model; between them every table a statement about this metric can read is
        // checked against every label it can project.
        Self::check_labels_against_table(metric, owning.table_name())?;
        Ok(())
    }

    /// Every label this metric projects, against one table name its statement reads.
    ///
    /// **Every label against every table, which is stricter than the collision that has to bite and
    /// deliberately so.** A joined table is in the `FROM` only when a dimension reached through it is
    /// asked for, but any *other* dimension can be asked for in the same question - so which pairs
    /// can meet at query time is a function of the question, and a load-time check that tried to
    /// predict it would be a check that sometimes let one through. Refusing the whole cross product
    /// costs a catalog author one rename and cannot be wrong in the direction that returns a number.
    ///
    /// [`TIME_BUCKET_LABEL`] is in the list because it is projected for every question, so a table
    /// literally named `period` collides with every one of them.
    ///
    /// Compared under [`IdentifierCase::COARSEST`] rather than by equality: a table named `Period`
    /// collides too. The variant's own note carries the report and the doc reference.
    fn check_labels_against_table(metric: &Metric, table: &TableName) -> Result<(), InconsistentDefinitions> {
        let projected = [metric.name.as_str(), TIME_BUCKET_LABEL]
            .into_iter()
            .chain(metric.dimensions.values().map(|dimension| dimension.name.as_str()));
        for label in projected {
            if IdentifierCase::COARSEST.names_one_thing(label, table.as_str()) {
                return Err(InconsistentDefinitions::LabelShadowsTable {
                    metric: metric.name.clone(),
                    label: String::from(label),
                    table: table.clone(),
                });
            }
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
