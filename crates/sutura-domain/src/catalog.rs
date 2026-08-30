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

// Two files rather than one, because `cargo xtask max-lines` fails at a thousand lines under
// `crates/` and cannot be exempted. `authored` holds the character-level parse of the two fields
// in this module that are prose rather than structure; this file holds the declarations and the
// cross-reference checks. The names stay where they were - a caller still writes
// `sutura_domain::catalog::DimensionValue` - because the module is the unit of API and the files are
// not.
mod authored;

pub use authored::{
    Description, DimensionValue, InvalidDescription, InvalidDimensionValue, MAX_DESCRIPTION_BYTES, MAX_DESCRIPTION_LINES,
    MAX_DIMENSION_VALUE_CHARS,
};

use crate::calendar::TimeRange;
use crate::measure::{Measure, RequiredFilter};
use crate::model::{
    ColumnName, DimensionName, Grain, IdentifierCase, JoinType, MetricName, ModelName, QualifiedTable, RelationshipName,
    SourceName, TableName,
};

/// The label a generated projection gives the truncated time column.
///
/// It lives here rather than in the compiler because it is part of the result schema, which is a
/// contract, and because [`Definitions::assemble`] has to know it: a dimension by this name would
/// produce two columns with one label, and a caller reading a result by name would get whichever
/// the data system listed first.
pub const TIME_BUCKET_LABEL: &str = "period";

/// The most values one dimension may declare.
///
/// **Measured before it was chosen, and it is the count that was missing rather than the length.**
/// The largest allowlist in this repository's example catalog is `region`, with five values, and the
/// next largest is `product_family` with four. Nothing here declares more than five, and the
/// question this bound answers is what a REVIEWED allowlist can plausibly be: 64 is twelve times the
/// largest one written here and four times the sixteen German federal states, which is the largest
/// enumeration a person writes out by hand in one line of a document. A dimension needing two
/// hundred country codes is not an allowlist somebody read; it is a lookup table, and it wants a
/// mechanism that does not put every entry into an agent's prompt.
///
/// Argued the way [`crate::query::MAX_RANGE_DAYS`] is argued, including about what it does not bound.
/// **The number that matters is the product of this and [`MAX_DIMENSION_VALUE_CHARS`]**, because
/// `sutura_app::prompt` lists every declared value of a dimension on one line of the document an
/// agent reads: 64 values of 64 characters is 4 KiB, the same order as
/// [`crate::knowledge::MAX_NOTE_BODY_BYTES`], so one dimension's value list is bounded by about what
/// one note body is. Per-value caps alone let N conforming values do what one oversized value cannot,
/// which is the same argument [`crate::knowledge::MAX_KNOWLEDGE_BYTES`] makes for notes.
///
/// What it does not bound, said plainly. It bounds ONE dimension: nothing here caps how many
/// dimensions a metric declares or how many metrics a catalog holds, so the size of the whole
/// rendered document is still a function of how much a catalog says. Those are the same shape of hole
/// and want the same kind of fix; this is the one the review named, and the honest statement of what
/// holds is better than a bound nobody measured.
pub const MAX_VALUES_PER_DIMENSION: usize = 64;

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
    table: QualifiedTable,
    columns: BTreeSet<ColumnName>,
    description: Description,
}

impl Model {
    /// A model over one physical table, wherever that table lives.
    ///
    /// **`impl Into<QualifiedTable>` and not `QualifiedTable`, and that is the compatibility hinge
    /// rather than a convenience.** `From<TableName>` yields an unqualified path, so every existing
    /// caller - a catalog document naming only a table, and every fixture in this workspace - passes
    /// a [`TableName`] and compiles unchanged, meaning exactly what it used to. It costs the `const`
    /// this constructor used to be, which nothing depended on.
    pub fn new(
        name: ModelName,
        source: SourceName,
        table: impl Into<QualifiedTable>,
        columns: BTreeSet<ColumnName>,
        description: Description,
    ) -> Self {
        Self {
            name,
            source,
            table: table.into(),
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

    /// Where the table lives: the whole path, which is what a `FROM` clause names.
    ///
    /// Read [`Self::table_name`] instead wherever what is wanted is the name a column is qualified by
    /// or the name a file-registering engine registers under. Two accessors rather than one that
    /// guesses - `QualifiedTable::name` carries why both readings are real.
    #[inline]
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }

    /// The table's own name, without whatever sits above it.
    #[inline]
    pub const fn table_name(&self) -> &TableName {
        self.table.name()
    }

    #[inline]
    pub const fn columns(&self) -> &BTreeSet<ColumnName> {
        &self.columns
    }

    #[inline]
    pub fn description(&self) -> &str {
        self.description.as_str()
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
///
/// **Every entry is a [`DimensionValue`], and how many there may be is
/// [`MAX_VALUES_PER_DIMENSION`].** Both are new, and both close the same hole: this was
/// `Option<BTreeSet<String>>` read straight out of a YAML document, and `sutura_app::prompt`
/// interpolates the whole list into the line of an agent-facing document that tells an agent what it
/// may filter on. A value with an invisible code point in it made that line read as something other
/// than what it said; an unbounded count made it as long as an author liked. The character rule is
/// the type's and the count rule is [`Definitions::assemble`]'s, because a count is not a fact about
/// one value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Dimension {
    name: DimensionName,
    column: ColumnName,
    via: Option<RelationshipName>,
    allowed_values: Option<BTreeSet<DimensionValue>>,
    description: Description,
}

impl Dimension {
    pub const fn new(
        name: DimensionName,
        column: ColumnName,
        via: Option<RelationshipName>,
        allowed_values: Option<BTreeSet<DimensionValue>>,
        description: Description,
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
    pub const fn allowed_values(&self) -> Option<&BTreeSet<DimensionValue>> {
        self.allowed_values.as_ref()
    }

    #[inline]
    pub fn description(&self) -> &str {
        self.description.as_str()
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
    ///
    /// Takes a [`DimensionValue`] rather than a `&str`, so the two sides of the comparison are the
    /// same type: a caller's value is parsed by [`DimensionValue::parse`] at the wire boundary the way
    /// their metric name is parsed by [`MetricName::parse`](crate::model::MetricName::parse), and text
    /// that could not have been declared never reaches this comparison to be found absent from it.
    pub fn permits(&self, value: &DimensionValue) -> bool {
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
    /// Predicates that are part of what this metric MEANS, applied to every question about it.
    ///
    /// A caller cannot see, choose or remove one. `mrr` means revenue from active subscriptions, and
    /// a statement that omits that predicate returns a different number under the same name.
    required_filters: Vec<RequiredFilter>,
    time_column: ColumnName,
    grains: BTreeSet<Grain>,
    dimensions: BTreeMap<DimensionName, Dimension>,
    anchor: Option<Anchor>,
    description: Description,
}

impl Metric {
    #[expect(
        clippy::too_many_arguments,
        reason = "a metric is nine facts and all nine are decided together; a builder for a value \
                  this immutable would add a partially-built state this type cannot have"
    )]
    pub const fn new(
        name: MetricName,
        model: ModelName,
        measure: Measure,
        required_filters: Vec<RequiredFilter>,
        time_column: ColumnName,
        grains: BTreeSet<Grain>,
        dimensions: BTreeMap<DimensionName, Dimension>,
        anchor: Option<Anchor>,
        description: Description,
    ) -> Self {
        Self {
            name,
            model,
            measure,
            required_filters,
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

    /// The predicates every question about this metric carries, whether the caller asked or not.
    #[inline]
    pub fn required_filters(&self) -> &[RequiredFilter] {
        &self.required_filters
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
        self.description.as_str()
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

#[cfg(test)]
mod tests;
