//! What we decided to execute, and the two things nothing else may decide.
//!
//! **A plan lives in the domain rather than in the compiler, and that is what let a second kind of
//! adapter exist.** The `Warehouse` port used to take a rendered statement, which quietly said that
//! every data system speaks SQL. One does not: an in-process engine executes a logical plan over
//! Arrow and generates no SQL at all. So the port takes a [`QueryPlan`] and *how* to execute it is
//! the adapter's business - render a statement, or build a plan of its own.
//!
//! That is worth more than the tidiness. Rendering SQL for a local file was where every dialect bug
//! lived: a truncated date coming back as a timestamp, an alias emitted unquoted, a `GROUP BY` given
//! an aliased expression, a placeholder in the wrong syntax. An adapter that never renders SQL
//! cannot have any of them.
//!
//! A plan holds no SQL. Its serialized form is what a golden snapshot pins, so a change to what we
//! decided shows up as a reviewable diff rather than as a different number.

use crate::calendar::TimeRange;
use crate::measure::{Measure, RequiredFilter};
use crate::model::{ColumnName, Grain, JoinType, MetricName, RelationshipName, SourceName, TableName};
use crate::warehouse::ParamValue;

/// The most rows any plan may return.
///
/// A hard cap rather than a budget, for now. A bounded range and a bounded set of group-by keys
/// still permit a large result, and the cost of that lands on a shared data system. When there is a
/// real budget this becomes its floor.
pub const MAX_ROWS: u32 = 10_000;

/// A column, qualified by the table it is read from.
///
/// Qualified always, even when there is only one table. An unqualified column in a statement that
/// later grows a join binds to whichever table happens to have it, and that is a wrong number rather
/// than an error.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanColumn {
    table: TableName,
    column: ColumnName,
}

impl PlanColumn {
    #[inline]
    pub const fn new(table: TableName, column: ColumnName) -> Self {
        Self { table, column }
    }

    #[inline]
    pub const fn table(&self) -> &TableName {
        &self.table
    }

    #[inline]
    pub const fn column(&self) -> &ColumnName {
        &self.column
    }
}

/// One join, as the plan will make it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanJoin {
    relationship: RelationshipName,
    table: TableName,
    join_type: JoinType,
    origin: PlanColumn,
    target: PlanColumn,
}

impl PlanJoin {
    #[inline]
    pub const fn new(
        relationship: RelationshipName,
        table: TableName,
        join_type: JoinType,
        origin: PlanColumn,
        target: PlanColumn,
    ) -> Self {
        Self {
            relationship,
            table,
            join_type,
            origin,
            target,
        }
    }

    #[inline]
    pub const fn relationship(&self) -> &RelationshipName {
        &self.relationship
    }

    #[inline]
    pub const fn table(&self) -> &TableName {
        &self.table
    }

    #[inline]
    pub const fn join_type(&self) -> JoinType {
        self.join_type
    }

    #[inline]
    pub const fn origin(&self) -> &PlanColumn {
        &self.origin
    }

    #[inline]
    pub const fn target(&self) -> &PlanColumn {
        &self.target
    }
}

/// The truncated time column, and the label it is projected under.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanBucket {
    label: String,
    grain: Grain,
    column: PlanColumn,
}

impl PlanBucket {
    #[inline]
    pub const fn new(label: String, grain: Grain, column: PlanColumn) -> Self {
        Self { label, grain, column }
    }

    #[inline]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[inline]
    pub const fn grain(&self) -> Grain {
        self.grain
    }

    #[inline]
    pub const fn column(&self) -> &PlanColumn {
        &self.column
    }
}

/// One group-by key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanKey {
    label: String,
    column: PlanColumn,
}

impl PlanKey {
    #[inline]
    pub const fn new(label: String, column: PlanColumn) -> Self {
        Self { label, column }
    }

    #[inline]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[inline]
    pub const fn column(&self) -> &PlanColumn {
        &self.column
    }
}

/// What is measured, under what label, with every column resolved to a table.
///
/// The shape is [`Measure`]'s, restated over [`PlanColumn`] rather than [`ColumnName`]: the plan
/// knows which table each column comes from and the catalog does not have to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanMeasure {
    Simple {
        aggregate: crate::model::Aggregate,
        column: PlanColumn,
    },
    CountIf {
        column: PlanColumn,
    },
    Ratio {
        numerator_aggregate: crate::model::Aggregate,
        numerator: PlanColumn,
        denominator_aggregate: crate::model::Aggregate,
        denominator: PlanColumn,
        zero_safe: bool,
    },
}

/// One predicate in the plan's filter, and which parameter carries its value.
///
/// The parameter index is recorded rather than implied by position, so a reader of a plan can see
/// which value goes where without reconstructing the generator's ordering in their head - and so an
/// adapter that binds by index cannot disagree with one that binds by order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanPredicate {
    /// `column >= param`, the inclusive start of the range.
    AtOrAfter {
        column: PlanColumn,
        param: usize,
    },
    /// `column < param`, the exclusive end.
    Before {
        column: PlanColumn,
        param: usize,
    },
    Equals {
        column: PlanColumn,
        param: usize,
    },
    NotEquals {
        column: PlanColumn,
        param: usize,
    },
    IsTrue {
        column: PlanColumn,
    },
    IsNotNull {
        column: PlanColumn,
    },
}

impl PlanPredicate {
    #[inline]
    pub const fn column(&self) -> &PlanColumn {
        match *self {
            Self::AtOrAfter { ref column, .. }
            | Self::Before { ref column, .. }
            | Self::Equals { ref column, .. }
            | Self::NotEquals { ref column, .. }
            | Self::IsTrue { ref column }
            | Self::IsNotNull { ref column } => column,
        }
    }

    #[inline]
    pub const fn param(&self) -> Option<usize> {
        match *self {
            Self::AtOrAfter { param, .. }
            | Self::Before { param, .. }
            | Self::Equals { param, .. }
            | Self::NotEquals { param, .. } => Some(param),
            Self::IsTrue { .. } | Self::IsNotNull { .. } => None,
        }
    }
}

/// Where a predicate came from.
///
/// Recorded because it is the difference between a number being wrong and a caller being refused. A
/// definitional predicate is part of what the metric means and a caller cannot see or remove it; a
/// requested one came from the question and was checked against an allowlist. Keeping the two
/// distinguishable in the plan is what lets a golden assert that the definitional ones are always
/// present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateOrigin {
    /// From the metric's own definition: a required filter, or the bounded time range.
    Definition,
    /// From the question, having passed the pinned allowlist.
    Requested,
}

/// A predicate and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanFilter {
    origin: PredicateOrigin,
    predicate: PlanPredicate,
}

impl PlanFilter {
    #[inline]
    pub const fn new(origin: PredicateOrigin, predicate: PlanPredicate) -> Self {
        Self { origin, predicate }
    }

    #[inline]
    pub const fn origin(&self) -> PredicateOrigin {
        self.origin
    }

    #[inline]
    pub const fn predicate(&self) -> &PlanPredicate {
        &self.predicate
    }
}

/// One statement's worth of decisions, and no SQL.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QueryPlan {
    source: SourceName,
    metric: MetricName,
    table: TableName,
    joins: Vec<PlanJoin>,
    bucket: PlanBucket,
    keys: Vec<PlanKey>,
    measure: PlanMeasure,
    measure_label: String,
    filters: Vec<PlanFilter>,
    params: Vec<ParamValue>,
    range: TimeRange,
    row_limit: u32,
}

impl QueryPlan {
    #[expect(
        clippy::too_many_arguments,
        reason = "a plan is what the compiler decided, and every field is decided in one place; a \
                  builder would add a half-built state that this type cannot currently have"
    )]
    pub const fn new(
        source: SourceName,
        metric: MetricName,
        table: TableName,
        joins: Vec<PlanJoin>,
        bucket: PlanBucket,
        keys: Vec<PlanKey>,
        measure: PlanMeasure,
        measure_label: String,
        filters: Vec<PlanFilter>,
        params: Vec<ParamValue>,
        range: TimeRange,
    ) -> Self {
        Self {
            source,
            metric,
            table,
            joins,
            bucket,
            keys,
            measure,
            measure_label,
            filters,
            params,
            range,
            row_limit: MAX_ROWS,
        }
    }

    #[inline]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }

    #[inline]
    pub const fn metric(&self) -> &MetricName {
        &self.metric
    }

    #[inline]
    pub const fn table(&self) -> &TableName {
        &self.table
    }

    #[inline]
    pub fn joins(&self) -> &[PlanJoin] {
        &self.joins
    }

    #[inline]
    pub const fn bucket(&self) -> &PlanBucket {
        &self.bucket
    }

    #[inline]
    pub fn keys(&self) -> &[PlanKey] {
        &self.keys
    }

    #[inline]
    pub const fn measure(&self) -> &PlanMeasure {
        &self.measure
    }

    #[inline]
    pub fn measure_label(&self) -> &str {
        &self.measure_label
    }

    #[inline]
    pub fn filters(&self) -> &[PlanFilter] {
        &self.filters
    }

    #[inline]
    pub fn params(&self) -> &[ParamValue] {
        &self.params
    }

    #[inline]
    pub const fn range(&self) -> TimeRange {
        self.range
    }

    #[inline]
    pub const fn row_limit(&self) -> u32 {
        self.row_limit
    }

    /// The labels this plan's result will carry, in order.
    ///
    /// One definition, so an adapter that builds a schema and an adapter that renders a projection
    /// cannot disagree about it - which is the whole reason two adapters can be compared against
    /// each other at all.
    pub fn result_labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self.keys.iter().map(|k| String::from(k.label())).collect();
        labels.push(String::from(self.bucket.label()));
        labels.push(self.measure_label.clone());
        labels
    }

    /// Every parameter a definitional predicate binds.
    ///
    /// Used by the golden that asserts a required filter is bound rather than written into the
    /// statement.
    pub fn definitional_params(&self) -> Vec<&ParamValue> {
        self.filters
            .iter()
            .filter(|f| matches!(f.origin(), PredicateOrigin::Definition))
            .filter_map(|f| f.predicate().param())
            .filter_map(|index| self.params.get(index))
            .collect()
    }
}

/// Restated over plan columns, so an adapter does not need the catalog to know which table a
/// measure's column comes from.
pub fn plan_measure(measure: &Measure, resolve: impl Fn(&ColumnName) -> PlanColumn) -> PlanMeasure {
    match *measure {
        Measure::Simple(ref term) => PlanMeasure::Simple {
            aggregate: term.aggregate(),
            column: resolve(term.column()),
        },
        Measure::CountIf { ref column } => PlanMeasure::CountIf { column: resolve(column) },
        Measure::Ratio {
            ref numerator,
            ref denominator,
            zero_safe,
        } => PlanMeasure::Ratio {
            numerator_aggregate: numerator.aggregate(),
            numerator: resolve(numerator.column()),
            denominator_aggregate: denominator.aggregate(),
            denominator: resolve(denominator.column()),
            zero_safe,
        },
    }
}

/// A required filter as a plan predicate, binding a parameter when it needs one.
///
/// `bind` is called only for the operators that compare against a value, and returns the index it
/// was stored at. Passing the binding in rather than returning a value keeps the parameter list in
/// one place: the caller owns the order, which is what the placeholder-position contract depends on.
pub fn plan_required_filter(filter: &RequiredFilter, column: PlanColumn, bind: impl FnOnce(String) -> usize) -> PlanPredicate {
    match *filter {
        RequiredFilter::Equals { ref value, .. } => PlanPredicate::Equals {
            column,
            param: bind(value.clone()),
        },
        RequiredFilter::NotEquals { ref value, .. } => PlanPredicate::NotEquals {
            column,
            param: bind(value.clone()),
        },
        RequiredFilter::IsTrue { .. } => PlanPredicate::IsTrue { column },
        RequiredFilter::IsNotNull { .. } => PlanPredicate::IsNotNull { column },
    }
}
