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

use super::{Column, Dimension, MAX_DEFINITIONS_BYTES, MAX_VALUES_PER_DIMENSION, Metric, Model, Relationship, TIME_BUCKET_LABEL};
use crate::measure::Term;
use crate::model::{
    Aggregate, ColumnName, DimensionName, IdentifierCase, MetricName, ModelName, RelationshipName, SourceName, TableName,
};

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
    /// A model's primary-key evidence names a column the model does not declare.
    #[error("model {model} names {column} in its primary key, which it does not declare as a column")]
    UnknownPrimaryKeyColumn { model: ModelName, column: ColumnName },
    /// One metric declaring the same dimension twice.
    ///
    /// Raised by [`Metric::new`], which is the only place the pair is still visible; that
    /// constructor's own note is where the argument lives.
    #[error("metric {metric} declares dimension {dimension} twice")]
    DuplicateDimension { metric: MetricName, dimension: DimensionName },
    #[error("metric {metric} names model {model}, which is not declared")]
    UnknownModel { metric: MetricName, model: ModelName },
    /// A ratio term names a model - `telekom/sutura#780`'s vocabulary - that this catalog does not
    /// declare at all. The same dangling reference [`Self::UnknownModel`] is for the metric's own
    /// model, one field further into the measure.
    #[error("metric {metric} measures a term on model {model}, which is not declared")]
    UnknownTermModel { metric: MetricName, model: ModelName },
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
        /// Which hop of the chain the relationship is - 1 for the single-hop case, so an author
        /// reading the report finds the line to fix.
        hop: usize,
    },
    #[error(
        "dimension {dimension} of metric {metric} chains {relationship} after {previous}, but {relationship} starts at a model {previous} does not end at, so the chain is a set of relationships rather than a path"
    )]
    ChainDoesNotJoinUp {
        metric: MetricName,
        dimension: DimensionName,
        previous: RelationshipName,
        relationship: RelationshipName,
    },
    /// A chained join that leaves the data system the chain crossed to: back onto the metric's own,
    /// or on to a third.
    ///
    /// **A chain crosses one boundary, at any hop, and stays where it crossed to.** The hops before
    /// the crossing join on the metric's own system, the crossing hop is the one link the federated
    /// plan carries, and the hops after it join inside the lookup leg's statement. A hop that lands
    /// anywhere else has no place in that shape.
    ///
    /// **Each hop's target is compared with where the walk crossed to, and comparing it with the
    /// metric's own source alone was a wrong answer.** A chain that crossed and came back has a local
    /// target on its last hop, so a check reading the target against the metric's source accepted it.
    /// Then `sutura_semantic`'s `is_remote` reads such a chain off its LAST hop, calls it local, and
    /// the whole-answer plan renders the other system's table into one statement under a certified
    /// metric name.
    #[error(
        "dimension {dimension} of metric {metric} chains along {relationship}, which leaves {target_source}, the data system the chain crossed to from {own}, and a chain crosses one data system boundary and stays there"
    )]
    HopCrossesSource {
        metric: MetricName,
        dimension: DimensionName,
        relationship: RelationshipName,
        /// The field is `own` rather than `source`, because `thiserror` reads a field called
        /// `source` as the `Error::source` chain and a `SourceName` there does not compile. Every
        /// error that names a source field does so under a name that is not exactly `source`.
        own: SourceName,
        /// The data system the chain crossed to, which is where it has to stay: the one this hop
        /// leaves, whether it goes back to the metric's own or on to a third.
        target_source: SourceName,
    },
    #[error(
        "dimension {dimension} of metric {metric} declares an empty value allowlist, so it permits filtering and permits no value"
    )]
    EmptyAllowlist { metric: MetricName, dimension: DimensionName },
    /// More declared values than [`MAX_VALUES_PER_DIMENSION`].
    ///
    /// Checked here rather than at
    /// [`DimensionValue::parse`](crate::catalog::DimensionValue::parse), because a count is not a
    /// fact about one value: each of ten thousand values can be inside every per-value bound and
    /// the list of them is still the whole of one dimension's line in a rendered prompt. Same
    /// reason [`crate::knowledge::MAX_KNOWLEDGE_BYTES`] is checked over a bundle rather than over a
    /// note.
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
    /// should be maintaining per target - and the alternative outcomes across five dialects are a
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
    /// The aggregate cap over every column, required filter, dimension value and description this
    /// bundle carries. What [`MAX_DEFINITIONS_BYTES`] bounds.
    ///
    /// A model's own column count and a metric's own required-filter count are both uncapped, so a
    /// catalog of many small, individually-legal declarations is not a catalog any per-item cap can
    /// see - the same argument [`crate::knowledge::MAX_KNOWLEDGE_BYTES`] makes over a bundle of
    /// notes.
    #[error("this catalog's definitions carry {bytes} authored bytes, and the limit is {limit}")]
    DefinitionsTooLarge { bytes: usize, limit: usize },
    /// A cross-model ratio's shared calendar is not reachable from one of its fact models
    /// (`telekom/sutura#780`). The metric declares a `shared_calendar` model, and every model a
    /// ratio term names - including the metric's own - must declare a relationship FROM itself TO
    /// that calendar, the direction each fact leg joins it in. A model that does not is a
    /// definition that loads but whose ratio can never be bucketed through the shared calendar,
    /// so it is refused at declaration time rather than at query time.
    #[error("metric {metric} declares shared calendar {calendar}, which model {model} has no relationship to")]
    SharedCalendarNotReachable {
        metric: MetricName,
        calendar: ModelName,
        model: ModelName,
    },
    /// A cross-model ratio term whose aggregate has no value over no rows (`telekom/sutura#780`).
    ///
    /// The two facts are combined drill-across, and a group one fact never reached reads that
    /// fact's side as its aggregate over the empty set - 0 for a sum, a count or a conditional
    /// count, the owner's rule. An average, a minimum or a maximum has no such value, so a ratio
    /// using one on either side cannot be answered for every group; refused at declaration.
    #[error("metric {metric} is a cross-model ratio with a {aggregate} term, which has no value over no rows")]
    CrossModelTermHasNoEmptyValue { metric: MetricName, aggregate: Aggregate },
    /// A cross-model ratio term counting distinct values (`telekom/sutura#780`). A cross-model ratio
    /// is always two fact legs combined above, and a distinct count cannot be re-aggregated across
    /// legs, so the definition could never be answered; refused at declaration rather than when asked.
    #[error("metric {metric} is a cross-model ratio with a {aggregate} term, which cannot be re-aggregated across two facts")]
    CrossModelTermDoesNotReaggregate { metric: MetricName, aggregate: Aggregate },
    /// A cross-model ratio declaring an anchor (`telekom/sutura#780`, `docs/adr/0002`'s fourth
    /// amendment). The boot check executes an anchor as one statement over the metric's own model,
    /// and a cross-model ratio is two fact statements combined above the port, so no anchor on one
    /// is certified. Refused at load, naming the metric, rather than failing the boot check for the
    /// whole bundle.
    #[error("metric {metric} declares an anchor, which is not certified on a cross-model ratio")]
    AnchorOnCrossModelRatio { metric: MetricName },
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
        // No primary-key cross-check here: `Model::with_primary_key` already refused a dangling
        // key at construction, against the same model's own columns, so every `Model` this map
        // holds already has one that exists - unrepresentable rather than checked twice.

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

        let assembled = Self {
            models: model_map,
            relationships: relationship_map,
            metrics: metric_map,
        };
        let bytes = assembled.authored_bytes();
        if bytes > MAX_DEFINITIONS_BYTES {
            return Err(InconsistentDefinitions::DefinitionsTooLarge {
                bytes,
                limit: MAX_DEFINITIONS_BYTES,
            });
        }
        Ok(assembled)
    }

    /// Builds [`Definitions`] from pre-assembled maps without cross-reference checking.
    ///
    /// Behind the `test-fakes` feature, so no shipped artefact contains it: a broken bundle built
    /// this way is a fake that simulates what a buggy catalog adapter could produce, letting a
    /// downstream test provoke a `BundleInconsistent` variant through `sutura_semantic::compile`.
    /// `assemble` is the only constructor a shipped build reaches; this one exists so a test does
    /// not have to monkeypatch the assembler.
    #[cfg(any(test, feature = "test-fakes"))]
    pub const fn from_parts(
        models: BTreeMap<ModelName, Model>,
        relationships: BTreeMap<RelationshipName, Relationship>,
        metrics: BTreeMap<MetricName, Metric>,
    ) -> Self {
        Self {
            models,
            relationships,
            metrics,
        }
    }

    /// The authored bytes across the whole bundle. What [`MAX_DEFINITIONS_BYTES`] bounds.
    fn authored_bytes(&self) -> usize {
        let models = sum_bytes(self.models.values().map(model_bytes));
        let relationships = sum_bytes(self.relationships.values().map(relationship_bytes));
        let metrics = sum_bytes(self.metrics.values().map(metric_bytes));
        models.saturating_add(relationships).saturating_add(metrics)
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
        // Every key's origin must be a column of the origin model and every key's target a column
        // of the target model. A compound key is refused if any one of its columns is undeclared:
        // the join is an `AND` over the whole set, so a missing column is a statement that cannot
        // render, and the whole set is what promises the target unique.
        for key in relationship.keys().iter() {
            let (from_column, to_column) = (key.origin(), key.target());
            for (model, column) in [(from, from_column), (to, to_column)] {
                if !model.has_column(column) {
                    return Err(InconsistentDefinitions::RelationshipUnknownColumn {
                        relationship: relationship.name.clone(),
                        model: model.name.clone(),
                        column: column.clone(),
                    });
                }
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
        // Every term the measure reads, checked against the model THAT TERM names - the metric's own
        // when a term names none, or a `telekom/sutura#780` ratio side's own model when it does.
        // `Measure::columns`/`Measure::models` are the two places that know, positionally paired, so
        // a shape added to either cannot be forgotten here - the failure this loop replaces, from
        // when a measure was one column and the check read it directly.
        if let Some(measure) = metric.computation.measure() {
            for (column, term_model) in measure.columns().into_iter().zip(measure.models()) {
                let owning = match term_model {
                    None => model,
                    Some(name) => models.get(name).ok_or_else(|| InconsistentDefinitions::UnknownTermModel {
                        metric: metric.name.clone(),
                        model: name.clone(),
                    })?,
                };
                if !owning.has_column(column) {
                    return Err(InconsistentDefinitions::UnknownMeasureColumn {
                        metric: metric.name.clone(),
                        model: owning.name.clone(),
                        column: column.clone(),
                    });
                }
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
        Self::check_cross_model(models, relationships, metric)
    }

    /// One cross-model ratio term's aggregate: it must have a value over no rows (the empty-set rule
    /// reads an absent fact's leaves as it) and re-aggregate across the two fact legs.
    fn check_cross_model_aggregate(metric: &Metric, aggregate: Aggregate) -> Result<(), InconsistentDefinitions> {
        match aggregate {
            Aggregate::Avg | Aggregate::Min | Aggregate::Max => Err(InconsistentDefinitions::CrossModelTermHasNoEmptyValue {
                metric: metric.name.clone(),
                aggregate,
            }),
            Aggregate::CountDistinct => Err(InconsistentDefinitions::CrossModelTermDoesNotReaggregate {
                metric: metric.name.clone(),
                aggregate,
            }),
            Aggregate::Sum | Aggregate::Count => Ok(()),
        }
    }

    /// `telekom/sutura#780`'s load checks for a cross-model ratio: it declares no anchor, every term
    /// has a value over no rows, and the shared calendar is one each fact can join. Split from
    /// [`Self::check_metric`] to keep that function under the CRAP threshold.
    fn check_cross_model(
        models: &BTreeMap<ModelName, Model>,
        relationships: &BTreeMap<RelationshipName, Relationship>,
        metric: &Metric,
    ) -> Result<(), InconsistentDefinitions> {
        // `telekom/sutura#780`: a cross-model ratio buckets both facts through a shared calendar
        // model, reached via a relationship from each fact model. Checked here, at load, because a
        // calendar one fact cannot reach is a definition whose ratio can never be bucketed - the
        // refusal is at declaration time, which is where this repo puts them. Only the models a
        // ratio term actually names are checked: a one-model metric has no cross-model term, so its
        // `shared_calendar` (if set) is checked against just its own model.
        if let Some(measure) = metric.computation.measure()
            && measure.models().into_iter().flatten().any(|named| named != &metric.model)
        {
            if metric.anchor.is_some() {
                return Err(InconsistentDefinitions::AnchorOnCrossModelRatio {
                    metric: metric.name.clone(),
                });
            }
            for term in measure.terms() {
                if let Term::Aggregate(inner) = term {
                    Self::check_cross_model_aggregate(metric, inner.aggregate())?;
                }
            }
        }
        if let Some(calendar) = metric.shared_calendar.as_ref() {
            // The calendar model must exist.
            if !models.contains_key(calendar) {
                return Err(InconsistentDefinitions::SharedCalendarNotReachable {
                    metric: metric.name.clone(),
                    calendar: calendar.clone(),
                    model: calendar.clone(),
                });
            }
            // Every model a ratio term names must reach the calendar - the metric's own first,
            // then each cross-model term's - through a relationship from the model to the calendar,
            // which is the hop each fact leg joins.
            let mut models_to_check = vec![&metric.model];
            if let Some(measure) = metric.computation.measure() {
                for term_model in measure.models().into_iter().flatten() {
                    if term_model != &metric.model {
                        models_to_check.push(term_model);
                    }
                }
            }
            for &named in &models_to_check {
                let reaches = relationships
                    .values()
                    .any(|rel| rel.origin_model() == named && rel.target_model() == calendar);
                if !reaches {
                    return Err(InconsistentDefinitions::SharedCalendarNotReachable {
                        metric: metric.name.clone(),
                        calendar: calendar.clone(),
                        model: named.clone(),
                    });
                }
            }
            // The column both legs bucket and bound by is the metric's time column on the calendar.
            if models
                .get(calendar)
                .is_some_and(|model| !model.has_column(&metric.time_column))
            {
                return Err(InconsistentDefinitions::UnknownTimeColumn {
                    metric: metric.name.clone(),
                    model: calendar.clone(),
                    column: metric.time_column.clone(),
                });
            }
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

        // A chain walks hop by hop, and `owning` tracks where the walk stands: the metric's model
        // before the first hop, each hop's target after it. That makes the link-up check the same
        // comparison for every hop - hop N's origin must be where the walk stands - while the error
        // it reports differs: hop 1 failing is a relationship that does not start at the metric's
        // model, hop N failing is a chain that is a set of relationships rather than a path.
        let mut owning = model;
        let mut crossed: Option<&SourceName> = None;
        if let Some(chain) = dimension.via.as_ref() {
            for (hop, name) in chain.as_slice().iter().enumerate() {
                let relationship = relationships
                    .get(name)
                    .ok_or_else(|| InconsistentDefinitions::UnknownRelationship {
                        metric: metric.name.clone(),
                        dimension: dimension.name.clone(),
                        relationship: name.clone(),
                    })?;
                if relationship.origin_model != owning.name {
                    if hop == 0 {
                        return Err(InconsistentDefinitions::RelationshipNotFromMetricModel {
                            metric: metric.name.clone(),
                            dimension: dimension.name.clone(),
                            relationship: name.clone(),
                            model: metric.model.clone(),
                        });
                    }
                    // No indexing: the previous hop is the last name before this one.
                    let previous = chain
                        .as_slice()
                        .split_at(hop)
                        .0
                        .last()
                        .cloned()
                        .unwrap_or_else(|| name.clone());
                    return Err(InconsistentDefinitions::ChainDoesNotJoinUp {
                        metric: metric.name.clone(),
                        dimension: dimension.name.clone(),
                        previous,
                        relationship: name.clone(),
                    });
                }
                if relationship.join_type.may_duplicate_rows() {
                    return Err(InconsistentDefinitions::JoinWouldDuplicateRows {
                        metric: metric.name.clone(),
                        dimension: dimension.name.clone(),
                        relationship: name.clone(),
                        hop: hop.saturating_add(1),
                    });
                }
                // A chain crosses ONE data system boundary, at any hop, and stays on the system it
                // crossed to: the hops before the crossing join on the metric's own source, the
                // crossing hop is the federated plan's link, and the hops after it join inside the
                // lookup leg. `crossed` is where the walk landed, so each later hop's target is
                // compared with that - not with the metric's source, which accepted a chain that
                // crossed and came back (the variant's own note carries the report). Every hop's
                // origin is where the previous hop's target is, so the target is the one end to read.
                let target = models.get(&relationship.target_model).ok_or_else(|| {
                    InconsistentDefinitions::RelationshipToUnknownModel {
                        relationship: name.clone(),
                        model: relationship.target_model.clone(),
                    }
                })?;
                match crossed {
                    Some(crossed_to) if target.source() != crossed_to => {
                        return Err(InconsistentDefinitions::HopCrossesSource {
                            metric: metric.name.clone(),
                            dimension: dimension.name.clone(),
                            relationship: name.clone(),
                            own: model.source().clone(),
                            target_source: crossed_to.clone(),
                        });
                    }
                    None if target.source() != model.source() => crossed = Some(target.source()),
                    Some(_) | None => {}
                }
                owning = target;
            }
        }

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
/// The bytes a physical-schema listing can expose for one model.
fn model_bytes(model: &Model) -> usize {
    model
        .name()
        .as_str()
        .len()
        .saturating_add(model.table().to_string().len())
        .saturating_add(sum_bytes(model.columns().map(column_bytes)))
        .saturating_add(model.description().len())
}

/// The authored bytes behind one column: its name, its data type if declared, and its description.
///
/// The name counted here too, unchanged from before this type existed - a model with many columns
/// still adds up, whether or not any of them carries a type or a description.
fn column_bytes(column: &Column) -> usize {
    let name = column.name().as_str().len();
    let data_type = column.data_type().map_or(0, |data_type| data_type.as_str().len());
    let description = column.description().len();
    name.saturating_add(data_type).saturating_add(description)
}

/// The authored bytes behind one relationship beyond its own name: every column of every key it
/// joins on.
fn relationship_bytes(relationship: &Relationship) -> usize {
    relationship.keys().iter().fold(0, |acc, key| {
        acc.saturating_add(key.origin().as_str().len())
            .saturating_add(key.target().as_str().len())
    })
}

/// The authored bytes behind one metric beyond its own name: every required filter as it would
/// render, every dimension it declares, and its description.
fn metric_bytes(metric: &Metric) -> usize {
    let filters = sum_bytes(metric.required_filters().iter().map(|filter| filter.to_string().len()));
    let dimensions = sum_bytes(metric.dimensions().values().map(dimension_bytes));
    filters.saturating_add(dimensions).saturating_add(metric.description().len())
}

/// The authored bytes behind one dimension beyond its own name: every allowed value, and its
/// description.
fn dimension_bytes(dimension: &Dimension) -> usize {
    let values = dimension
        .allowed_values()
        .map_or(0, |values| sum_bytes(values.iter().map(|value| value.as_str().len())));
    values.saturating_add(dimension.description().len())
}

/// Saturating, because a count of authored bytes must not be able to wrap into a small number and
/// pass the cap it exists to fail. The same rule [`crate::knowledge`]'s own `sum_bytes` holds, kept
/// as a second copy rather than a shared one: that one is `knowledge`'s private helper, and a public
/// seam for three lines of arithmetic is a bigger change than the duplication it would remove.
fn sum_bytes(counts: impl Iterator<Item = usize>) -> usize {
    counts.fold(0, usize::saturating_add)
}
