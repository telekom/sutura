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

// Three files rather than one, because `cargo xtask max-lines` fails at a thousand lines under
// `crates/` and cannot be exempted. `authored` holds the character-level parse of the fields in this
// module that are prose rather than structure, `consistency` holds the cross-reference checks and
// the assembled `Definitions` they gate, and this file holds the declarations. The names stay where
// they were - a caller still writes `sutura_domain::catalog::DimensionValue` - because the module is
// the unit of API and the files are not.
mod authored;
mod consistency;

pub use authored::{
    Description, DimensionValue, InvalidDescription, InvalidDimensionValue, MAX_DESCRIPTION_BYTES, MAX_DESCRIPTION_LINES,
    MAX_DIMENSION_VALUE_CHARS,
};
pub use consistency::{Definitions, InconsistentDefinitions};

use crate::calendar::TimeRange;
use crate::measure::{Measure, RequiredFilter};
use crate::model::{
    ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, QualifiedTable, RelationshipName, SourceName, TableName,
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

#[cfg(test)]
mod tests;
