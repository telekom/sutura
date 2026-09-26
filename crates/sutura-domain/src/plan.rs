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
//!
//! **Two shapes, not one, and [`leg`] holds the second.** A [`QueryPlan`] is a whole answer from one
//! data system. A [`LegPlan`] is one data system's share of an answer assembled above it, and it is
//! its own type rather than a `QueryPlan` with three fields made optional - `leg` says at length
//! why. [`Executable`] is what the port takes, so an adapter's match over what it can be handed is
//! exhaustive.

use crate::calendar::TimeRange;
use crate::measure::{Measure, RequiredFilter, Term, ZeroDenominator};
use crate::model::{Aggregate, ColumnName, Grain, JoinType, MetricName, QualifiedTable, RelationshipName, SourceName, TableName};
use crate::query::Top;
use crate::warehouse::ParamValue;

use crate::nonempty::NonEmpty;
mod anchor;
pub mod bindings;
pub mod federated;
pub mod label;
pub mod leg;
pub mod tables;

#[cfg(test)]
mod anchor_tests;

pub use crate::plan::anchor::{AnchorPlan, NotAnAnchorsPlan};
pub use crate::plan::bindings::{IncoherentBindings, PlanBindings};
pub use crate::plan::federated::{
    AnswerKey, FederatedAnswerRefusal, FederatedPlan, FederatedPlanError, FederationCombiner, InternalLabel, LegResult, LegSide,
    Legs, LegsAreNotOneOfEach, labels,
};
#[cfg(any(test, feature = "fixtures"))]
pub use crate::plan::federated::{NothingCombined, RefusingCombiner};
pub use crate::plan::label::ResultLabel;
pub use crate::plan::leg::{Executable, LegPlan, LegTerm};
pub use crate::plan::tables::{AmbiguousTables, StatementTables};

/// Why [`QueryPlan::resolve_tables`] could not produce a valid plan.
///
/// Either the `resolve` closure refused a table path, or the resolved tables - taken together -
/// answer to one identifier, which is [`AmbiguousTables`]'s refusal re-validated through
/// [`StatementTables::parse`].
#[derive(Debug, thiserror::Error)]
pub enum ResolveTablesError<E> {
    /// The `resolve` closure refused this table path.
    #[error("a table path could not be resolved")]
    Resolve(#[source] E),
    /// The resolved tables share an identifier, so the statement cannot tell them apart.
    #[error(transparent)]
    Ambiguous(#[from] AmbiguousTables),
}

/// The most rows any plan may return.
///
/// A hard cap rather than a budget, for now. A bounded range and a bounded set of group-by keys
/// still permit a large result, and the cost of that lands on a shared data system. When there is a
/// real budget this becomes its floor.
///
/// **It is a refusal and not a truncation, and that is the correction a review forced.** This used
/// to be the `LIMIT` on the statement and nothing else: nothing compared the rows that came back
/// against it. So a question at `day` grain over a year, grouped by up to
/// [`MAX_DIMENSIONS`](crate::query::MAX_DIMENSIONS) keys, answered with the first ten thousand
/// groups by group key, carried a provenance digest, and said nowhere that it was partial. Summing
/// those rows gives a wrong number under a certified name, arrived at by omission - which is the
/// failure mode this repository exists to prevent, and the one a caller has no way to detect.
///
/// What holds it up is two things that have to be read together. [`QueryPlan::row_limit`] is one
/// MORE than this, so a result that reached the cap is distinguishable from a result the cap cut
/// short; and a row count above this is
/// [`RefusalReason::ResultTooLarge`](crate::query::RefusalReason::ResultTooLarge). A refusal is the
/// honest outcome: "your question is too wide to certify" is a governance answer, not an error, and
/// the caller's move is to narrow the range or drop a dimension.
pub const MAX_ROWS: u32 = 10_000;

/// How many rows a `top` answer is certified over, before it is refused.
///
/// **A configured value, defaulting to [`MAX_ROWS`] - `github.com/telekom/sutura#777`.** Unlike
/// `MAX_ROWS` itself, which stays the compiled constant for an ordinary question, this is the
/// ceiling [`RefusalReason::ResultTooLarge`](crate::query::RefusalReason::ResultTooLarge) and
/// [`RefusalReason::TopOverUncertifiedRows`](crate::query::RefusalReason::TopOverUncertifiedRows)
/// name to a caller for a `top` question - which is the one place a message telling the caller to
/// *"ask your operator to raise this"* has to be true rather than aspirational. A deployment
/// configures one in its settings; absent, [`Self::DEFAULT`] is what every deployment already got.
///
/// **The limit, next to the claim.** Nothing here makes the ORDINARY row cap configurable - a
/// question with no `top` is still refused against the compiled [`MAX_ROWS`]. Only the two `top`
/// refusals this type feeds read a configured value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowCeiling(u32);

/// Why a row ceiling did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRowCeiling {
    /// A zero ceiling reads as "unlimited" to whoever wrote it, not "refuse every `top`".
    #[error("a row ceiling of zero would refuse every `top` rather than mean unlimited")]
    Zero,
}

impl RowCeiling {
    /// [`MAX_ROWS`], restated as a ceiling - what every deployment had before this type existed.
    pub const DEFAULT: Self = Self(MAX_ROWS);

    /// Parses an operator-chosen ceiling, refusing zero for [`InvalidRowCeiling::Zero`]'s reason.
    pub const fn parse(rows: u32) -> Result<Self, InvalidRowCeiling> {
        if rows == 0 {
            return Err(InvalidRowCeiling::Zero);
        }
        Ok(Self(rows))
    }

    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

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
    table: QualifiedTable,
    join_type: JoinType,
    keys: crate::nonempty::NonEmpty<PlanJoinKey>,
}

impl PlanJoin {
    /// One join to a table, wherever that table lives.
    ///
    /// **A joined table carries its own qualifier, and that is the whole point of the feature rather
    /// than completeness:** a fact table in one dataset joined to a dimension table in another is
    /// what a multi-project estate looks like, and it is one statement, one job and one credential -
    /// a native join the data system pushes down, not a second source. `sutura_semantic::plan` says
    /// so where a source count decides between one statement, a split and
    /// `PlanSpansTooManySources`.
    ///
    /// **A join with no keys cannot be built**, and that is what makes `keys` a `NonEmpty` rather
    /// than a `Vec` the renderers have to fail closed on: every key contributes one `=` term, so an
    /// empty list would be a `JOIN ... ON` with no predicate - a shape that cannot be minted here.
    /// The catalog's `JoinKeys` is non-empty by the same construction, so the plan's list, derived
    /// from it, can never be empty either.
    ///
    /// `impl Into<QualifiedTable>` for the reason `Model::new` gives.
    #[inline]
    pub fn new(
        relationship: RelationshipName,
        table: impl Into<QualifiedTable>,
        join_type: JoinType,
        keys: crate::nonempty::NonEmpty<PlanJoinKey>,
    ) -> Self {
        Self {
            relationship,
            table: table.into(),
            join_type,
            keys,
        }
    }

    #[inline]
    pub const fn relationship(&self) -> &RelationshipName {
        &self.relationship
    }

    /// Where the joined table lives: the whole path, which is what a `JOIN` clause names.
    #[inline]
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }

    /// The joined table's own name, which is what its columns are qualified by.
    #[inline]
    pub const fn table_name(&self) -> &TableName {
        self.table.name()
    }

    #[inline]
    pub const fn join_type(&self) -> JoinType {
        self.join_type
    }

    /// The keys this join links on, each qualified by the tables it reads. Never empty.
    #[inline]
    #[must_use]
    pub const fn keys(&self) -> &crate::nonempty::NonEmpty<PlanJoinKey> {
        &self.keys
    }
}

/// One term of a planned join, as the renderer will make it.
///
/// The same two shapes the catalog's [`crate::catalog::JoinKey`] declares, but with each column
/// resolved to the table-qualified [`PlanColumn`] the statement will read - the origin from the
/// table the join starts at, the target from the joined table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum PlanJoinKey {
    /// The origin column equals the target column.
    Equal { origin: PlanColumn, target: PlanColumn },
    /// The origin column, truncated to a grain, equals the target column.
    TruncatedEqual {
        origin: PlanColumn,
        grain: Grain,
        target: PlanColumn,
    },
}

impl PlanJoinKey {
    /// The origin column, truncated to its grain when this is [`PlanJoinKey::TruncatedEqual`].
    #[inline]
    pub const fn origin(&self) -> &PlanColumn {
        match self {
            Self::Equal { origin, .. } | Self::TruncatedEqual { origin, .. } => origin,
        }
    }

    /// The target column the origin (or its truncation) is compared against.
    #[inline]
    pub const fn target(&self) -> &PlanColumn {
        match self {
            Self::Equal { target, .. } | Self::TruncatedEqual { target, .. } => target,
        }
    }

    /// The grain a [`PlanJoinKey::TruncatedEqual`] truncates its origin to.
    #[inline]
    pub const fn grain(&self) -> Option<Grain> {
        match self {
            Self::Equal { .. } => None,
            Self::TruncatedEqual { grain, .. } => Some(*grain),
        }
    }
}

/// The truncated time column, and the label it is projected under.
///
/// **The label is a [`ResultLabel`] and not a `String`, which is the third carrier
/// `telekom/sutura#337` names.** Every producer in this workspace passes [`ResultLabel::bucket`],
/// which takes no argument because there is nothing to choose:
/// [`TIME_BUCKET_LABEL`](crate::catalog::TIME_BUCKET_LABEL) is the one spelling.
///
/// **The limit, next to the claim:** the type says the text came from something already parsed, not
/// WHICH of the four constructors produced it - so a bucket labelled with a dimension's own name is
/// still representable here, and what refuses that particular collision is
/// [`Definitions::assemble`](crate::catalog::Definitions), which will not accept a dimension named
/// `period` in the first place.
///
/// A computed bucket label is a compile error, which is the pair [`ResultLabel`] carries for a key
/// applied to the carrier it did not reach:
///
/// ```compile_fail
/// use sutura_domain::model::{ColumnName, Grain, TableName};
/// use sutura_domain::plan::{PlanBucket, PlanColumn};
///
/// fn _computed(table: TableName, column: ColumnName, leaf: usize) -> PlanBucket {
///     PlanBucket::new(format!("0_leaf_{leaf}"), Grain::Month, PlanColumn::new(table, column))
/// }
/// ```
///
/// And the twin, so a rename cannot make that block pass vacuously:
///
/// ```
/// use sutura_domain::model::{ColumnName, Grain, TableName};
/// use sutura_domain::plan::{PlanBucket, PlanColumn, ResultLabel};
///
/// fn _parsed(table: TableName, column: ColumnName) -> PlanBucket {
///     PlanBucket::new(ResultLabel::bucket(), Grain::Month, PlanColumn::new(table, column))
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanBucket {
    label: ResultLabel,
    grain: Grain,
    column: PlanColumn,
}

impl PlanBucket {
    #[inline]
    pub const fn new(label: ResultLabel, grain: Grain, column: PlanColumn) -> Self {
        Self { label, grain, column }
    }

    /// The text the bucket is projected under.
    #[inline]
    pub fn label(&self) -> &str {
        self.label.as_str()
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
///
/// **The label is a [`ResultLabel`] and not a `String`, which is `telekom/sutura#337`.** A key used
/// to be labelled with whatever text its producer computed, and what kept that out of the internal
/// namespace a federated leg also projects into was a derivation held by review. The type is the
/// mechanism now: `ResultLabel` carries the compile-fail pair that says so.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanKey {
    label: ResultLabel,
    column: PlanColumn,
}

impl PlanKey {
    #[inline]
    pub const fn new(label: ResultLabel, column: PlanColumn) -> Self {
        Self { label, column }
    }

    /// The text this key is projected under.
    ///
    /// Text rather than the [`ResultLabel`], because every reader of a label renders it: the
    /// generator quotes it as an alias and the combiner looks a column up by it. What the type
    /// holds is the way IN.
    #[inline]
    pub fn label(&self) -> &str {
        self.label.as_str()
    }

    #[inline]
    pub const fn column(&self) -> &PlanColumn {
        &self.column
    }
}

/// One term of a measure, with its column resolved to a table.
///
/// [`Term`] restated over [`PlanColumn`] rather than [`ColumnName`]. Mirrored at the same level the
/// domain names it, so an adapter that renders one half of a ratio and one that renders a whole
/// measure reach for the same function instead of each flattening two levels its own way.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanTerm {
    Aggregate { aggregate: Aggregate, column: PlanColumn },
    CountIf { column: PlanColumn },
}

impl PlanTerm {
    #[inline]
    pub const fn column(&self) -> &PlanColumn {
        match *self {
            Self::Aggregate { ref column, .. } | Self::CountIf { ref column } => column,
        }
    }
}

/// What is measured, under what label, with every column resolved to a table.
///
/// The shape is [`Measure`]'s, restated over [`PlanTerm`]: the plan knows which table each column
/// comes from and the catalog does not have to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanMeasure {
    Simple {
        term: PlanTerm,
    },
    Ratio {
        numerator: PlanTerm,
        denominator: PlanTerm,
        zero_denominator: ZeroDenominator,
    },
}

/// One metric's share of a query's select list, and its own definitional guard.
///
/// A multi-metric question answers one grouped statement with one certified column per metric.
/// Each column is that metric's measure, computed only over the rows its OWN required filters
/// admit - and that restriction is folded into the measure's conditional aggregation rather than
/// into the shared `WHERE`, because a `WHERE` applies to every column at once and would let one
/// [`PlanPredicate`]s - its own required filters - that renderers fold into the aggregate, so
/// none of them leaks into another metric's column. [`QueryPlan::measures`] holds one of these
/// per named metric, in the order the question gave them.
///
/// The label is a [`ResultLabel`], so a metric name cannot arrive here as text - the same carrier
/// argument `telekom/sutura#337` makes for a dimension key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlannedMeasure {
    metric: MetricName,
    label: ResultLabel,
    measure: PlanMeasure,
    guard: Vec<PlanPredicate>,
}

impl PlannedMeasure {
    /// One metric's measure, labelled and bound by its own definitional guard.
    ///
    /// `guard` holds this metric's own REQUIRED filters - the predicates that say what this metric
    /// counts. The shared time range, being the same for every metric, stays in the plan's shared
    /// `WHERE`; only the per-metric required filters are folded into the conditional aggregate, so
    /// one metric's filter cannot constrain another's column. A metric with no required filters
    /// carries an empty guard and computes over every row in range.
    #[must_use]
    pub const fn new(metric: MetricName, label: ResultLabel, measure: PlanMeasure, guard: Vec<PlanPredicate>) -> Self {
        Self {
            metric,
            label,
            measure,
            guard,
        }
    }
    #[inline]
    pub const fn metric(&self) -> &MetricName {
        &self.metric
    }

    #[inline]
    pub fn label(&self) -> &str {
        self.label.as_str()
    }

    #[inline]
    pub const fn measure(&self) -> &PlanMeasure {
        &self.measure
    }

    /// This metric's own definitional guard predicates, in render order.
    #[inline]
    pub fn guard(&self) -> &[PlanPredicate] {
        &self.guard
    }
}
/// One predicate in the plan's filter, and which parameter carries its value.
///
/// The parameter index is recorded rather than implied by position, so a reader of a plan can see
/// which value goes where without reconstructing the generator's ordering in their head - and so an
/// adapter that binds by index cannot disagree with one that binds by order.
///
/// **The index on its own is unconstrained, and what bounds it is [`PlanBindings`].** Whether an
/// index resolves is a relation between this predicate and a parameter LIST, so it is parsed over
/// the pair rather than wrapped around the number; a predicate outside a parsed set reaches no
/// renderer and no executor. [`crate::plan::bindings`] carries the argument and the limit.
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
    /// `column IN (param, param, ..)` - one or more values, `github.com/telekom/sutura#968`.
    /// [`crate::nonempty::NonEmpty`] rather than a plain `Vec`: an empty `IN ()` is either a
    /// syntax error or, rendered as `NOT IN ()`, a silently vanished filter (fail-open), and a
    /// producer cannot reach this variant with zero placeholders to fill.
    In {
        column: PlanColumn,
        params: crate::nonempty::NonEmpty<usize>,
    },
    /// `column NOT IN (param, param, ..)` - [`Self::In`]'s negation, same reason for `NonEmpty`.
    NotIn {
        column: PlanColumn,
        params: crate::nonempty::NonEmpty<usize>,
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
            | Self::In { ref column, .. }
            | Self::NotIn { ref column, .. }
            | Self::IsTrue { ref column }
            | Self::IsNotNull { ref column } => column,
        }
    }

    /// The one parameter a single-valued predicate binds. `None` for `In`/`NotIn` too - see
    /// [`Self::bound_params`] for the shape that covers every variant.
    #[inline]
    pub const fn param(&self) -> Option<usize> {
        match *self {
            Self::AtOrAfter { param, .. }
            | Self::Before { param, .. }
            | Self::Equals { param, .. }
            | Self::NotEquals { param, .. } => Some(param),
            Self::In { .. } | Self::NotIn { .. } | Self::IsTrue { .. } | Self::IsNotNull { .. } => None,
        }
    }

    /// Every parameter index this predicate binds, in placeholder order - zero for `IsTrue`/
    /// `IsNotNull`, one for a comparing predicate, one per value for `In`/`NotIn`. The one place
    /// [`bindings::PlanBindings::parse`] walks all eight variants without matching on which one.
    #[inline]
    pub(crate) fn bound_params(&self) -> BoundParams<'_> {
        match *self {
            Self::AtOrAfter { param, .. }
            | Self::Before { param, .. }
            | Self::Equals { param, .. }
            | Self::NotEquals { param, .. } => BoundParams::One(Some(param)),
            Self::In { ref params, .. } | Self::NotIn { ref params, .. } => BoundParams::Many(params.into_iter()),
            Self::IsTrue { .. } | Self::IsNotNull { .. } => BoundParams::None,
        }
    }
}

mod bound_params;
pub(crate) use bound_params::BoundParams;

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
    measures: NonEmpty<PlannedMeasure>,
    table: QualifiedTable,
    joins: Vec<PlanJoin>,
    bucket: PlanBucket,
    keys: Vec<PlanKey>,
    filters: Vec<PlanFilter>,
    params: Vec<ParamValue>,
    range: TimeRange,
    max_rows: u32,
    top: Option<Top>,
}

impl QueryPlan {
    /// One statement's worth of decisions.
    ///
    /// **Two of the arguments are parsed sets rather than loose fields, and that is the whole of what
    /// keeps this constructor infallible.** Each carries a relation the plan would otherwise have to
    /// be trusted to have got right, checked where the set is parsed so the incoherent plan does not
    /// exist to be rendered:
    ///
    /// - the tables arrive as a [`StatementTables`] and not as a table plus a vector of joins, so no
    ///   plan holds two tables one statement could not tell apart. [`crate::plan::tables`] argues the
    ///   defect, the measurement and the choice of a refusal over an alias.
    /// - the filters arrive as [`PlanBindings`] and not as a filter list plus a parameter list, so no
    ///   plan holds a predicate that binds a parameter it does not carry, or binds one out of the
    ///   order a positional placeholder gives it. [`crate::plan::bindings`] argues what each adapter
    ///   does with the incoherent pair, and why the check cannot live on the index.
    ///
    /// A single-metric convenience over [`Self::with_measures`], so every existing caller (and the
    /// serialized form of the overwhelmingly common one metric) keeps its shape. The wrapped
    /// [`PlannedMeasure`] carries an empty guard - this metric's required filters stay in the shared
    /// `WHERE`, which is correct when there is exactly one column for them to constrain.
    pub fn new(
        source: SourceName,
        metric: MetricName,
        tables: StatementTables,
        bucket: PlanBucket,
        keys: Vec<PlanKey>,
        measure: PlanMeasure,
        measure_label: ResultLabel,
        bindings: PlanBindings,
        range: TimeRange,
    ) -> Self {
        let primary = PlannedMeasure::new(metric, measure_label, measure, Vec::new());
        Self::with_measures(source, NonEmpty::one(primary), tables, bucket, keys, bindings, range)
    }

    /// One statement's worth of decisions over several metrics.
    ///
    /// [`Self::new`]'s general form: `measures` holds one [`PlannedMeasure`] per named metric, in
    /// the order the question gave them. Each metric's own required filters live in that measure's
    /// guard (folded into its conditional aggregate), so none of them constrains a column that is
    /// not its own.
    ///
    /// **`bindings` carries the whole statement's parameters, guards first.** A metric's guard
    /// predicates render inside its measure column - which appears in the `SELECT` before the
    /// `WHERE` - so on a positional dialect their placeholders come before the range and requested
    /// ones. The caller therefore hands this constructor `PlanBindings` whose filter list is the
    /// union `[every metric's guards][range, requested]` and whose parameter list matches; this
    /// constructor validates the union through [`PlanBindings::parse`] machinery the caller already
    /// ran, then stores only the shared tail as the plan's `filters` (what a `WHERE` emits) while
    /// keeping the full `params`. The guard predicates themselves stay on their [`PlannedMeasure`]s,
    /// which is where a renderer reads them for the `SELECT`.
    pub fn with_measures(
        source: SourceName,
        measures: NonEmpty<PlannedMeasure>,
        tables: StatementTables,
        bucket: PlanBucket,
        keys: Vec<PlanKey>,
        bindings: PlanBindings,
        range: TimeRange,
    ) -> Self {
        // Both sets are taken apart rather than stored whole, so the serialized form a golden pins is
        // unchanged by either guard existing. What the arguments buy is that there is no way in here
        // for a set either one refuses.
        let (guard_count, table, joins) = {
            let (table, joins) = tables.into_parts();
            let guard_count: usize = measures.iter().map(|m| m.guard().len()).sum();
            (guard_count, table, joins)
        };
        let (mut filters, params) = bindings.into_parts();
        // The shared tail is everything after every guard. The union's contiguity is what
        // `PlanBindings::parse` held when the caller built it, so splitting by the guard count and
        // no more is the same split the renderers' placeholder order assumes.
        //
        // **Held by this assertion, not by recall alone.** A caller that miscounted (or misordered)
        // the guards-first union would otherwise either panic here (`split_off` past the end) or,
        // worse, silently misplace a required filter into the shared tail - exactly the leak this
        // whole type exists to prevent. `debug_assert!` rather than a `Result`: the two producers of
        // this union are both in this workspace (`sutura_semantic::plan::guards_and_shared` is the
        // only one today), so a violation is a defect in OUR OWN code, not a caller's, and a
        // panicking constructor for that class is this crate's own established shape (`QueryPlan::new`
        // stays infallible over caller-facing input; this is a debug-only self-check on top of it).
        debug_assert!(
            guard_count <= filters.len(),
            "with_measures: the union carries {} filter(s), fewer than the {guard_count} the measures' own guards need",
            filters.len()
        );
        let shared = filters.split_off(guard_count.min(filters.len()));
        Self {
            source,
            measures,
            table,
            joins,
            bucket,
            keys,
            filters: shared,
            params,
            range,
            max_rows: MAX_ROWS,
            top: None,
        }
    }

    /// Attaches a caller-chosen order and row limit, so the generator renders it instead of the
    /// plan's own tie-break-only order and probe-by-one limit.
    ///
    /// A builder rather than a constructor argument, for the reason [`Query::with_top`](crate::query::Query::with_top)
    /// gives: every existing caller of [`Self::new`] keeps its argument list, and a plan built
    /// without it is byte-for-byte one built before this field existed.
    #[inline]
    #[must_use]
    pub const fn with_top(mut self, top: Top) -> Self {
        self.top = Some(top);
        self
    }

    #[inline]
    pub const fn top(&self) -> Option<Top> {
        self.top
    }

    #[inline]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }

    pub const fn measures(&self) -> &NonEmpty<PlannedMeasure> {
        &self.measures
    }

    /// Where the table lives: the whole path, which is what the `FROM` clause names.
    ///
    /// Read [`Self::table_name`] instead where what is wanted is the name a column is qualified by.
    #[inline]
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }

    /// The table's own name, which is what this plan's columns are qualified by.
    #[inline]
    pub const fn table_name(&self) -> &TableName {
        self.table.name()
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

    /// The most rows this plan's result may carry before it is refused.
    ///
    /// The cap itself, which is what a row count is compared against. Read it with
    /// [`row_limit`](QueryPlan::row_limit): the two differ by one, deliberately, and neither is
    /// useful without the other.
    #[inline]
    pub const fn max_rows(&self) -> u32 {
        self.max_rows
    }

    /// How many rows an adapter asks for: one more than [`max_rows`](QueryPlan::max_rows).
    ///
    /// **The extra row is the whole mechanism.** Fetching exactly the cap makes a result AT the cap
    /// indistinguishable from a result the cap cut short, and the second of those is a partial total
    /// under a certified name. Asking for one more makes "there is more" observable at no cost - the
    /// extra row is never returned to a caller, because a result carrying it is refused as
    /// [`RefusalReason::ResultTooLarge`](crate::query::RefusalReason::ResultTooLarge).
    ///
    /// Saturating, so a cap of [`u32::MAX`] stays a number rather than wrapping to zero and asking
    /// a data system for nothing.
    #[inline]
    pub const fn row_limit(&self) -> u32 {
        self.max_rows.saturating_add(1)
    }

    /// The labels this plan's result will carry, in order.
    ///
    /// One definition, so an adapter that builds a schema and an adapter that renders a projection
    /// cannot disagree about it - which is the whole reason two adapters can be compared against
    /// each other at all.
    pub fn result_labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self.keys.iter().map(|k| String::from(k.label())).collect();
        labels.push(String::from(self.bucket.label()));
        labels.extend(self.measures.iter().map(|m| String::from(m.label())));
        labels
    }

    /// This plan with every table path passed through `resolve`, and nothing else changed.
    ///
    /// **For an adapter that can close a gap in a path before the statement renders, and has to
    /// do it here rather than inside the renderer** - a path with less than its full qualifier is
    /// exactly what `sutura_sql::generate::table_path` renders as literal text, so what it
    /// resolves to is a decision about this plan, made once, not a rule every dialect's renderer
    /// would otherwise need. Runs over the `FROM` table and then every joined one; the first
    /// failure stops the walk.
    ///
    /// **Re-validates through [`StatementTables::parse`]** after resolving, so a `resolve` closure
    /// that rewrites a table name into one already in the statement is refused rather than shipped
    /// to the renderer. The invariant [`QueryPlan::new`] holds - that no plan holds two tables one
    /// statement could not tell apart - is preserved here rather than assumed: the resolved tables
    /// go back through the same parser that built the plan in the first place.
    pub fn resolve_tables<E>(
        mut self,
        mut resolve: impl FnMut(&QualifiedTable) -> Result<QualifiedTable, E>,
    ) -> Result<Self, ResolveTablesError<E>> {
        self.table = resolve(&self.table).map_err(ResolveTablesError::Resolve)?;
        for join in &mut self.joins {
            join.table = resolve(&join.table).map_err(ResolveTablesError::Resolve)?;
        }
        let tables = StatementTables::parse(self.table, self.joins).map_err(ResolveTablesError::Ambiguous)?;
        let (table, joins) = tables.into_parts();
        self.table = table;
        self.joins = joins;
        Ok(self)
    }

    /// Every parameter a definitional predicate binds - the shared `WHERE`'s AND, for a
    /// multi-metric plan, every measure's own guard.
    ///
    /// Used by the golden that asserts a required filter is bound rather than written into the
    /// statement. A single-metric plan's required filters are the shared `WHERE` half alone,
    /// exactly as before this method's second half existed; a multi-metric plan's are folded into
    /// each measure's guard instead (`PlannedMeasure::guard`) and would otherwise never be checked
    /// - the same "a shorter list passes" gap this method's own history already records once.
    pub fn definitional_params(&self) -> Vec<&ParamValue> {
        let shared = self
            .filters
            .iter()
            .filter(|f| matches!(f.origin(), PredicateOrigin::Definition))
            .filter_map(|f| f.predicate().param());
        let guards = self
            .measures
            .iter()
            .flat_map(|m| m.guard().iter())
            .filter_map(PlanPredicate::param);
        shared
            .chain(guards)
            // `get` rather than an index because `indexing_slicing` is denied, and it drops nothing:
            // every index a filter of this plan carries resolves, because the pair was parsed as
            // `PlanBindings` before the plan existed. It used to drop, and the golden that reads this
            // list passed on the shorter one - `crate::plan::bindings` is where that is argued.
            .filter_map(|index| self.params.get(index))
            .collect()
    }
}

/// Restated over plan columns, so an adapter does not need the catalog to know which table a
/// measure's column comes from.
///
/// Resolves one term at a time rather than one shape at a time, which is why a term added to the
/// vocabulary is one arm here instead of one arm per shape.
///
/// `resolve` reads only the column, never the term's `model`: a term naming a model other than the
/// metric's own is refused before this is called - `sutura_semantic::plan::plan`'s own
/// `telekom/sutura#780` check - so every column this function resolves belongs to the same table
/// `resolve`'s caller already qualified everything else by.
pub fn plan_measure(measure: &Measure, resolve: impl Fn(&ColumnName) -> PlanColumn) -> PlanMeasure {
    let resolve_term = |term: &Term| match *term {
        Term::Aggregate(ref inner) => PlanTerm::Aggregate {
            aggregate: inner.aggregate(),
            column: resolve(inner.column()),
        },
        Term::CountIf { ref column, .. } => PlanTerm::CountIf { column: resolve(column) },
    };
    match *measure {
        Measure::Simple(ref term) => PlanMeasure::Simple {
            term: resolve_term(term),
        },
        Measure::Ratio {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => PlanMeasure::Ratio {
            numerator: resolve_term(numerator),
            denominator: resolve_term(denominator),
            zero_denominator,
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
            param: bind(String::from(value.as_str())),
        },
        RequiredFilter::NotEquals { ref value, .. } => PlanPredicate::NotEquals {
            column,
            param: bind(String::from(value.as_str())),
        },
        RequiredFilter::IsTrue { .. } => PlanPredicate::IsTrue { column },
        RequiredFilter::IsNotNull { .. } => PlanPredicate::IsNotNull { column },
    }
}

#[cfg(test)]
mod resolve_tests;
