//! The tool surface: what a caller may ask, and what comes back.
//!
//! **This module is the governance boundary, and it is defined by what it does not contain.**
//! [`Query`] has no field for SQL, a table name, a filter expression or a list of row ids. An
//! uncertified question is therefore unrepresentable rather than refused, which is a stronger
//! property than it sounds: a refusal can be retried until something succeeds, and an absent field
//! cannot.
//!
//! Widening this is a governance change. `AGENTS.md` says which mechanism has to still hold.

use std::collections::BTreeSet;

use crate::calendar::TimeRange;
use crate::model::{DimensionName, Grain, MetricName, SourceName};
use crate::pinned::Provenance;
use crate::warehouse::RowSet;

/// The most dimensions one question may group by.
///
/// A bound for the same reason the time range is bounded: a group-by over every column is a table
/// scan with a plausible name, and the cost lands on a shared data system. Four covers the questions
/// a person asks and refuses the ones a loop generates.
pub const MAX_DIMENSIONS: usize = 4;

/// One equality filter: a dimension, and a value the pinned bundle declares.
///
/// The value is a `String` here and a bind parameter by the time it reaches a statement. It is
/// checked against the metric's allowlist first, so the parameterisation is the second line of
/// defence rather than the only one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    dimension: DimensionName,
    value: String,
}

impl Filter {
    pub const fn new(dimension: DimensionName, value: String) -> Self {
        Self { dimension, value }
    }

    #[inline]
    pub const fn dimension(&self) -> &DimensionName {
        &self.dimension
    }

    #[inline]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A modelled question.
///
/// `deny_unknown_fields` is load-bearing rather than strict-for-its-own-sake. Without it a question
/// carrying `sql:` or `table:` deserializes cleanly with the extra field dropped on the floor, and a
/// caller who believes they sent SQL gets an answer to a different question. With it, the attempt is
/// an error naming the field.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    metric: MetricName,
    grain: Grain,
    range: TimeRange,
    #[serde(default)]
    dimensions: Vec<DimensionName>,
    #[serde(default)]
    filters: Vec<Filter>,
}

impl Query {
    pub const fn new(
        metric: MetricName,
        grain: Grain,
        range: TimeRange,
        dimensions: Vec<DimensionName>,
        filters: Vec<Filter>,
    ) -> Self {
        Self {
            metric,
            grain,
            range,
            dimensions,
            filters,
        }
    }

    #[inline]
    pub const fn metric(&self) -> &MetricName {
        &self.metric
    }

    #[inline]
    pub const fn grain(&self) -> Grain {
        self.grain
    }

    #[inline]
    pub const fn range(&self) -> TimeRange {
        self.range
    }

    #[inline]
    pub fn dimensions(&self) -> &[DimensionName] {
        &self.dimensions
    }

    #[inline]
    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    /// The literal text this question carries, for the assertion that none of it reaches the SQL.
    ///
    /// It exists so the no-injection golden can be written as "no value from the question appears in
    /// the statement" rather than as a list of places to look, which is the form that goes stale the
    /// first time a field is added.
    pub fn literals(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::from([self.range.start().to_iso(), self.range.end().to_iso()]);
        out.extend(self.filters.iter().map(|f| String::from(f.value())));
        out
    }
}

/// Why a question was not answered.
///
/// Typed rather than prose, because the variant is the contract and the message is not. Every
/// variant has a test that provokes it: a refusal nobody has seen happen is a refusal nobody knows
/// works.
///
/// **There is no `TimeRangeUnbounded` variant, deliberately.** [`TimeRange`] has no unbounded form,
/// so such a refusal could never be provoked, and a variant with no test that can reach it looks
/// like coverage while being dead code. The type does that job instead.
///
/// Note what these variants do *not* carry: a rejected filter value is never echoed back.
/// `DimensionValueNotAllowed` names the dimension and stops there. Reflecting caller-supplied text
/// into a message that reaches a log, a UI and an agent's context is how a rejected value becomes
/// somebody else's input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum RefusalReason {
    /// No metric of that name is in the pinned bundle.
    MetricUnknown { metric: MetricName },
    /// The metric exists and does not declare that grain. Not a narrower question: a grain the
    /// author did not render is a number nobody certified.
    GrainNotSupported { metric: MetricName, grain: Grain },
    /// The metric does not declare that dimension. A dimension a metric did not declare is a name
    /// that does not resolve, not a filter to apply anyway.
    DimensionNotPermitted { metric: MetricName, dimension: DimensionName },
    /// The dimension exists but declares no value allowlist, so it can be grouped by and not
    /// filtered.
    DimensionNotFilterable { metric: MetricName, dimension: DimensionName },
    /// The dimension is filterable and the value is not one the bundle declares.
    DimensionValueNotAllowed { metric: MetricName, dimension: DimensionName },
    /// The same dimension appears twice in one question. Refused rather than deduplicated: a
    /// caller who sent it twice believes something we do not.
    DuplicateDimension { dimension: DimensionName },
    /// More group-by keys than [`MAX_DIMENSIONS`].
    TooManyDimensions { requested: usize, limit: usize },
    /// The plan would need to read from more than one data system.
    ///
    /// Refused rather than run in parts, because a second data system is a second identity to
    /// satisfy, and a plan that runs partly as somebody else is the failure this design exists to
    /// prevent.
    PlanSpansTwoSources { sources: usize },
    /// The one data system the plan resolved to could not be reached as the calling subject.
    ///
    /// A refusal rather than a fallback. Running as the service's own identity instead would turn
    /// "you may not see these rows" into "here are the rows".
    SourceUnavailable { source: SourceName },
}

/// What a tool call produced.
///
/// A refusal is a *variant of the result*, not an `Err`. A caller cannot mistake it for a transport
/// hiccup and retry until something works, which is what an error would invite.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ToolOutcome {
    Answer { provenance: Provenance, rows: RowSet },
    Refusal { reason: RefusalReason },
}

impl ToolOutcome {
    #[inline]
    pub const fn is_refusal(&self) -> bool {
        matches!(*self, Self::Refusal { .. })
    }

    /// The refusal reason, if this is one. Convenience for tests and for an audit sink.
    #[inline]
    pub const fn refusal(&self) -> Option<&RefusalReason> {
        match *self {
            Self::Refusal { ref reason } => Some(reason),
            Self::Answer { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Filter, Query, RefusalReason, ToolOutcome};
    use crate::calendar::{Date, TimeRange};
    use crate::model::{DimensionName, Grain, MetricName};

    fn june() -> TimeRange {
        TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range")
    }

    fn query_with_filter(value: &str) -> Query {
        Query::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            Grain::Month,
            june(),
            vec![DimensionName::parse("region").expect("a test dimension is a dimension")],
            vec![Filter::new(
                DimensionName::parse("region").expect("a test dimension is a dimension"),
                String::from(value),
            )],
        )
    }

    #[test]
    fn a_question_reports_every_literal_it_carries() {
        // The no-injection golden is written against this, so a field added to `Query` without
        // being listed here would silently stop being checked. That is why it is a method on the
        // type rather than a list in the test.
        let literals = query_with_filter("north").literals();
        assert!(literals.contains("2026-06-01"), "{literals:?}");
        assert!(literals.contains("2026-07-01"), "{literals:?}");
        assert!(literals.contains("north"), "{literals:?}");
        assert_eq!(literals.len(), 3);
    }

    // The two governance properties of this type that need a real format parser to provoke -
    // `deny_unknown_fields` refusing a `sql:` field, and a range with no `end` failing to
    // deserialize at all - are asserted in `sutura-catalog-local`, which has one. They are not
    // asserted here because `serde_json` would have to join `ALLOWED_IN_DOMAIN` in
    // `xtask/src/boundaries.rs` to do it, and widening that allowlist to reach a test is exactly
    // the trade the boundary gate exists to make visible.

    #[test]
    fn a_refusal_is_a_result_and_not_an_error() {
        // The invariant in one assertion: a refusal is reachable by matching on the outcome, so a
        // caller cannot treat it as a transport failure and retry until it succeeds.
        let outcome = ToolOutcome::Refusal {
            reason: RefusalReason::MetricUnknown {
                metric: MetricName::parse("clv").expect("a test metric is a metric"),
            },
        };
        assert!(outcome.is_refusal());
        assert!(matches!(outcome.refusal(), Some(&RefusalReason::MetricUnknown { .. })));
    }

    #[test]
    fn a_rejected_filter_value_is_not_echoed_back() {
        // Deliberate: a refusal message reaches a log, a UI and an agent's context. Reflecting the
        // caller's text into all three turns a rejected value into somebody else's input, so the
        // variant names the dimension and stops.
        let rejected = query_with_filter("north");
        let reason = RefusalReason::DimensionValueNotAllowed {
            metric: rejected.metric().clone(),
            dimension: DimensionName::parse("region").expect("a test dimension is a dimension"),
        };
        // `Debug` is the rendering that reaches a log by accident, so it is the one to assert on.
        let rendered = format!("{reason:?}");
        assert!(!rendered.contains("north"), "{rendered}");
        assert!(rendered.contains("region"), "{rendered}");
    }
}
