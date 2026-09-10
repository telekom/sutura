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
use crate::catalog::Anchor;
use crate::measure::{Measure, RequiredFilter, Term, ZeroDenominator};
use crate::model::{Aggregate, ColumnName, Grain, JoinType, MetricName, QualifiedTable, RelationshipName, SourceName, TableName};
use crate::pinned::PinnedDefinitions;
use crate::warehouse::ParamValue;

pub mod federated;
pub mod label;
pub mod leg;
pub mod tables;

#[cfg(test)]
mod anchor_tests;

pub use crate::plan::federated::{
    AnswerKey, FederatedFailure, FederatedPlan, FederatedPlanError, InternalLabel, LegSide, labels,
};
pub use crate::plan::label::ResultLabel;
pub use crate::plan::leg::{Executable, LegPlan, LegTerm};
pub use crate::plan::tables::{AmbiguousTables, StatementTables};

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
    origin: PlanColumn,
    target: PlanColumn,
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
    /// `impl Into<QualifiedTable>` for the reason `Model::new` gives.
    #[inline]
    pub fn new(
        relationship: RelationshipName,
        table: impl Into<QualifiedTable>,
        join_type: JoinType,
        origin: PlanColumn,
        target: PlanColumn,
    ) -> Self {
        Self {
            relationship,
            table: table.into(),
            join_type,
            origin,
            target,
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
    table: QualifiedTable,
    joins: Vec<PlanJoin>,
    bucket: PlanBucket,
    keys: Vec<PlanKey>,
    measure: PlanMeasure,
    measure_label: ResultLabel,
    filters: Vec<PlanFilter>,
    params: Vec<ParamValue>,
    range: TimeRange,
    max_rows: u32,
}

impl QueryPlan {
    /// One statement's worth of decisions.
    ///
    /// **The tables arrive as a [`StatementTables`] and not as a table plus a vector of joins**, and
    /// that argument is the whole of what keeps this constructor infallible: the check that two of
    /// them do not answer to one identifier happens where that set is parsed, so a plan holding the
    /// ambiguous pair does not exist to be rendered. [`crate::plan::tables`] is where the defect, the
    /// measurement and the choice of a refusal over an alias are argued.
    pub fn new(
        source: SourceName,
        metric: MetricName,
        tables: StatementTables,
        bucket: PlanBucket,
        keys: Vec<PlanKey>,
        measure: PlanMeasure,
        measure_label: ResultLabel,
        filters: Vec<PlanFilter>,
        params: Vec<ParamValue>,
        range: TimeRange,
    ) -> Self {
        // Taken apart rather than stored whole, so the serialized form a golden pins is unchanged by
        // the guard existing. What the argument buys is that there is no way in here for a set of
        // tables one statement could not tell apart - `plan::tables` is where that is argued.
        let (table, joins) = tables.into_parts();
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
            max_rows: MAX_ROWS,
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
    pub const fn measure(&self) -> &PlanMeasure {
        &self.measure
    }

    #[inline]
    pub fn measure_label(&self) -> &str {
        self.measure_label.as_str()
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
        labels.push(String::from(self.measure_label.as_str()));
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

/// The one thing [`Warehouse::verify_anchor`](crate::warehouse::Warehouse::verify_anchor) accepts:
/// a plan the pinned bundle itself agrees is one of its anchors' own.
///
/// # What this type is, and what it is not
///
/// **It is a self-check on the boot path, and it is NOT an authority.** That distinction is the whole
/// of what a second review corrected, and getting it wrong once put a false sentence in ten places
/// across seven files - `docs/adr/0008`'s second amendment to its correction 2 lists them. [`Warehouse::execute`](crate::warehouse::Warehouse::execute) cannot be called without a
/// [`Presented`](crate::identity::Presented); `verify_anchor` deliberately takes no credential,
/// because there is no caller at boot, and it therefore runs under whatever identity the deployment
/// configured that adapter with. So the question is what bounds its INPUT.
///
/// [`Self::of`] answers "did the boot path compile the question it meant to" and nothing stronger.
/// The plan has to compute a metric **this bundle** defines, that metric has to declare an anchor,
/// and the plan has to be that anchor's own question: the metric's coarsest declared grain, exactly
/// the range the anchor certifies, no group-by keys, and no predicate a question asked for. Every one
/// of those facts is read off the [`PinnedDefinitions`] rather than accepted as an argument, which is
/// what makes the check worth making - a caller no longer supplies the anchor it will be compared
/// against.
///
/// **What it cannot do is stop code that wants to.** Every value it reads is publicly constructible -
/// [`QueryPlan::new`], [`PinnedDefinitions::pin`], the metric and range types - and Rust has no
/// cross-crate friend visibility, so a constructor `sutura-app` can call is a constructor anything in
/// the workspace can call. A reviewer defeated the previous version of this type in one function by
/// fabricating the tuple it took, and the fix for that class is not a fifth guard: a shape check over
/// caller-constructible values can only ever be a shape check.
///
/// # So what makes the credential-free path boot-only
///
/// A lint, and it is named here rather than implied: `clippy.toml` bans
/// `sutura_domain::warehouse::Warehouse::verify_anchor`, verified to resolve by writing the call and
/// watching clippy reject it. `sutura_app::verify_anchors` holds the single `#[expect]`, so a second
/// call site is an error under `-D warnings` until somebody writes a second expectation a reviewer
/// sees in the diff. That is the same mechanism the ban on the panicking fragment API and the ban on a
/// bare `spawn_blocking` already rest on. **Its limit is that a lint is not a type:** it reaches this
/// workspace and not a crate outside it, and an `#[allow]` walks past it.
///
/// A genuinely closed constructor is not available. The domain cannot compile a plan - compilation is
/// `sutura-semantic`'s and dependencies point inward - and a token only `sutura-app`'s private `proof`
/// module could mint would have to be constructible from `sutura-domain`, which is the same public
/// door one level down. `docs/adr/0008`'s own corrections are the precedent for saying this rather
/// than implying more.
#[derive(Debug)]
pub struct AnchorPlan<'bundle> {
    plan: &'bundle QueryPlan,
}

/// A plan that is not a declared anchor's own, so the boot path did not compile what it meant to.
///
/// **An error and not a refusal**: reaching it means the boot path compiled something other than the
/// anchor's question, which is a defect here rather than anything about a caller.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotAnAnchorsPlan {
    /// The plan computes a different metric from the one whose anchor it would be checked against.
    #[error("this plan computes `{plan}` and the anchor certifies `{anchor}`")]
    NotThatMetric { plan: MetricName, anchor: MetricName },
    /// The bundle this plan is checked against does not define the metric at all.
    #[error("this bundle defines no metric `{metric}`, so it has no anchor to be the plan of")]
    MetricNotDefined { metric: MetricName },
    /// The metric is defined and declares no certified number, so there is no anchor to be a plan of.
    #[error("`{metric}` declares no anchor, so no plan of it is an anchor's")]
    DeclaresNoAnchor { metric: MetricName },
    /// The plan groups by something. An anchor is a metric's own number, not a slice of it.
    #[error("an anchor's plan groups by nothing, and this one groups by {keys}")]
    Grouped { keys: usize },
    /// The plan carries a predicate a question asked for, which an anchor's plan never does.
    #[error("an anchor's plan carries only the metric's own predicates, and this one carries a requested one")]
    Requested,
    /// The plan buckets at a finer grain than the metric's coarsest, so it returns a series.
    ///
    /// **The gap a second review found**, and the reason it is not cosmetic: an anchor certifies one
    /// number, and a plan at `Day` grain over the anchor's range comes back as one row per day. The
    /// comparison downstream insists on exactly one row, so this arrived as a mismatch that reads like
    /// a broken definition - and a plan that returns a series is strictly more than the number the
    /// bundle already publishes.
    ///
    /// `coarsest` is an [`Option`] because a set can be empty, and the empty case is folded in here
    /// rather than given a variant of its own: [`Definitions::assemble`](crate::catalog::Definitions)
    /// refuses a metric that declares no grain, so a separate variant would be one no test could
    /// provoke - and this crate's rule is that an enum does not carry one of those.
    #[error(
        "an anchor of `{metric}` is asked at {} and this plan buckets at {plan}",
        .coarsest.map_or("no grain it declares", Grain::as_str)
    )]
    NotTheCoarsestGrain {
        metric: MetricName,
        plan: Grain,
        coarsest: Option<Grain>,
    },
    /// The plan's range is not the range the anchor's author certified.
    #[error("the anchor certifies {anchor} and this plan covers {plan}")]
    NotTheAnchorsRange { plan: TimeRange, anchor: TimeRange },
}

impl<'bundle> AnchorPlan<'bundle> {
    /// Parses a plan as one of `pinned`'s own anchors', reading every fact it compares off the bundle.
    ///
    /// Takes the metric's name as well as the bundle, because the bundle holds many anchors and the
    /// caller is asserting *which* one this plan is of - so the first check is that the plan agrees.
    /// Everything after that is the bundle's own statement about that metric.
    ///
    /// **It does not take a `sutura_app::Validated` bundle, and it cannot:** validating a bundle is
    /// what this call is part of, so the proof does not exist yet. That is one more reason the type is
    /// a self-check rather than an authority.
    ///
    /// The order of the checks is chosen for the diagnostic rather than for cost - every input is
    /// already bounded and in memory. Which metric, then what the bundle says about that metric, then
    /// the two shapes only a question has, then the two values an anchor's own question pins.
    pub fn of(plan: &'bundle QueryPlan, pinned: &PinnedDefinitions, metric: &MetricName) -> Result<Self, NotAnAnchorsPlan> {
        if plan.metric() != metric {
            return Err(NotAnAnchorsPlan::NotThatMetric {
                plan: plan.metric().clone(),
                anchor: metric.clone(),
            });
        }
        let definition = pinned
            .definitions()
            .metric(metric)
            .ok_or_else(|| NotAnAnchorsPlan::MetricNotDefined { metric: metric.clone() })?;
        let anchor: &Anchor = definition
            .anchor()
            .ok_or_else(|| NotAnAnchorsPlan::DeclaresNoAnchor { metric: metric.clone() })?;
        // The coarsest grain the metric declares, because that is the one grain at which the anchor's
        // range yields a single number. Read here rather than passed in: a caller-supplied grain is a
        // caller-supplied answer to the question this check is asking.
        let coarsest = definition.grains().iter().copied().max();
        if !plan.keys().is_empty() {
            return Err(NotAnAnchorsPlan::Grouped { keys: plan.keys().len() });
        }
        if plan
            .filters()
            .iter()
            .any(|filter| matches!(filter.origin(), PredicateOrigin::Requested))
        {
            return Err(NotAnAnchorsPlan::Requested);
        }
        if coarsest != Some(plan.bucket().grain()) {
            return Err(NotAnAnchorsPlan::NotTheCoarsestGrain {
                metric: metric.clone(),
                plan: plan.bucket().grain(),
                coarsest,
            });
        }
        if plan.range() != anchor.range() {
            return Err(NotAnAnchorsPlan::NotTheAnchorsRange {
                plan: plan.range(),
                anchor: anchor.range(),
            });
        }
        Ok(Self { plan })
    }

    /// The plan, for the adapter that has to execute it.
    #[inline]
    #[must_use]
    pub const fn plan(&self) -> &QueryPlan {
        self.plan
    }
}

/// Restated over plan columns, so an adapter does not need the catalog to know which table a
/// measure's column comes from.
///
/// Resolves one term at a time rather than one shape at a time, which is why a term added to the
/// vocabulary is one arm here instead of one arm per shape.
pub fn plan_measure(measure: &Measure, resolve: impl Fn(&ColumnName) -> PlanColumn) -> PlanMeasure {
    let resolve_term = |term: &Term| match *term {
        Term::Aggregate(ref inner) => PlanTerm::Aggregate {
            aggregate: inner.aggregate(),
            column: resolve(inner.column()),
        },
        Term::CountIf { ref column } => PlanTerm::CountIf { column: resolve(column) },
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
