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
use crate::model::{Aggregate, ColumnName, Grain, JoinType, MetricName, RelationshipName, SourceName, TableName};
use crate::warehouse::ParamValue;

pub mod leg;

#[cfg(test)]
mod anchor_tests;

pub use crate::plan::leg::{Executable, LegPlan, LegTerm};

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
    table: TableName,
    joins: Vec<PlanJoin>,
    bucket: PlanBucket,
    keys: Vec<PlanKey>,
    measure: PlanMeasure,
    measure_label: String,
    filters: Vec<PlanFilter>,
    params: Vec<ParamValue>,
    range: TimeRange,
    max_rows: u32,
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

/// The one thing [`Warehouse::verify_anchor`](crate::warehouse::Warehouse::verify_anchor) accepts:
/// a declared anchor's own plan.
///
/// **This type exists because the sentence "no signature in this workspace can execute a question as
/// this process" was false by one method, and a review caught it.**
/// [`Warehouse::execute`](crate::warehouse::Warehouse::execute) cannot be called without a
/// [`Presented`](crate::identity::Presented). `verify_anchor` deliberately takes no credential -
/// there is no caller at boot - and while it took a bare [`QueryPlan`] it would execute *any* plan
/// under whatever identity the deployment configured that adapter with, including a plan compiled
/// from a caller's question. Placement kept the request path off it; placement is not a mechanism.
///
/// So the boot path gets an input a question's plan cannot be: [`Self::of`] refuses a plan that is
/// grouped, a plan carrying a predicate the question asked for, a plan for a different metric, and a
/// plan whose range is not the anchor's. An anchor is asked with no dimensions and no filters over
/// the range its author certified, so those four checks accept exactly what the boot path builds and
/// reject every shape a caller can reach.
///
/// # The limit, stated with the claim
///
/// [`Self::of`] is `pub`, because the boot path lives in `sutura-app` and this type lives here - the
/// same reason [`LegCredentials::minted`](crate::identity::LegCredentials::minted) is. So this is a
/// **narrowing and not a closure**: a caller that already holds the bundle can still ask for a metric
/// at its coarsest grain, with no dimensions and no filters, over exactly the range that metric's
/// anchor declares, and construct one. What that plan returns is the number the bundle certifies in
/// its own catalog document and that provenance already publishes, so nothing reaches a caller
/// through this door that the definitions did not already state. What is no longer reachable is a
/// *question* - a grouped plan, a filtered one, a different range, a different metric - which is what
/// the claim is about.
///
/// A genuinely closed constructor would need the domain to compile the plan itself, and compilation
/// is `sutura-semantic`'s: the domain may not depend on it. `docs/adr/0008`'s own correction 2 is the
/// precedent for saying this rather than implying more - a `pub` constructor asserted as unreachable
/// is exactly what that correction found wrong with the record's first attempt at this method.
#[derive(Debug)]
pub struct AnchorPlan<'bundle> {
    plan: &'bundle QueryPlan,
}

/// A plan that is not a declared anchor's own, so nothing may execute it with no credential.
///
/// **An error and not a refusal**: reaching it means the boot path compiled something other than the
/// anchor's question, which is a defect here rather than anything about a caller.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotAnAnchorsPlan {
    /// The plan computes a different metric from the one whose anchor it would be checked against.
    #[error("this plan computes `{plan}` and the anchor certifies `{anchor}`")]
    NotThatMetric { plan: MetricName, anchor: MetricName },
    /// The plan groups by something. An anchor is a metric's own number, not a slice of it.
    #[error("an anchor's plan groups by nothing, and this one groups by {keys}")]
    Grouped { keys: usize },
    /// The plan carries a predicate a question asked for, which an anchor's plan never does.
    #[error("an anchor's plan carries only the metric's own predicates, and this one carries a requested one")]
    Requested,
    /// The plan's range is not the range the anchor's author certified.
    #[error("the anchor certifies {anchor} and this plan covers {plan}")]
    NotTheAnchorsRange { plan: TimeRange, anchor: TimeRange },
}

impl<'bundle> AnchorPlan<'bundle> {
    /// Parses a plan as one anchor's, refusing every shape a question could be.
    ///
    /// Takes the metric's name as well as the [`Anchor`], because an anchor carries a range and a
    /// value and does not know which metric declared it - so without the name the metric check would
    /// have nothing to compare against.
    pub fn of(plan: &'bundle QueryPlan, metric: &MetricName, anchor: &Anchor) -> Result<Self, NotAnAnchorsPlan> {
        if plan.metric() != metric {
            return Err(NotAnAnchorsPlan::NotThatMetric {
                plan: plan.metric().clone(),
                anchor: metric.clone(),
            });
        }
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
