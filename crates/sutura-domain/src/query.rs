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
use crate::catalog::DimensionValue;
use crate::model::{DimensionName, Grain, MetricName, SourceName};
use crate::pinned::Provenance;
use crate::warehouse::RowSet;

/// The most dimensions one question may group by.
///
/// A bound for the same reason the time range is bounded: a group-by over every column is a table
/// scan with a plausible name, and the cost lands on a shared data system. Four covers the questions
/// a person asks and refuses the ones a loop generates.
pub const MAX_DIMENSIONS: usize = 4;

/// The longest span of history one question may ask about, in days.
///
/// **This is the bound the [`TimeRange`] newtype does not provide.** That type refuses an *absent*
/// endpoint; it accepts `[0001-01-01, 9999-12-31)`, which is over three and a half million days and, on
/// both execution paths, a full scan. `plan::MAX_ROWS` does not help: it bounds the rows *returned*
/// after the aggregate - refusing a result that exceeds them - so a question that scans everything and
/// groups it into one bucket is inside it. The span is what rows-read is a function of, so the span is
/// where the cap goes.
///
/// **3653 days is ten calendar years, counted at its longest.** Ten consecutive Gregorian years hold
/// 3652 or 3653 days depending on where the leap days fall, so this number is the one that lets
/// *any* ten-year window through rather than most of them. Ten years is chosen because it covers the
/// reporting a person actually does - a decade of annual figures, five years of quarters, three years
/// of months - and the longest range anywhere in this repository's example corpus, in its questions
/// and in its anchors alike, is 181 days - so nothing authored today is anywhere near it.
///
/// It also stays under `plan::MAX_ROWS`, and that is not a coincidence worth losing: at `day` grain
/// the time axis of a permitted question is at most 3653 buckets, so the row cap can only ever be
/// reached by dimension cardinality and never by the range alone. Raising this past the row cap would
/// make [`RefusalReason::ResultTooLarge`] the normal outcome of a wide range - a refusal nobody could
/// act on, because narrowing the range would not be what got them there.
///
/// A *span*, not a bucket count, and the difference matters. A bucket count would let `year` grain
/// through with a thousand years of scanning for a thousand rows, which is precisely the request this
/// exists to refuse; the span bounds the scan at every grain and bounds the buckets as a consequence.
///
/// **What it does not bound, said plainly rather than left for someone to discover.** It bounds ONE
/// question: three permitted ten-year questions cover thirty years, and nothing here correlates two
/// requests, because a per-caller budget needs a clock, a subject and somewhere to keep a counter and
/// this crate has none of the three. And inside a permitted span the *groups* are still the span times
/// the cardinality of up to [`MAX_DIMENSIONS`] dimensions - a dimension declared without a value list
/// has whatever cardinality the column has - so `plan::MAX_ROWS` refuses that result rather than
/// bounding the work that produced it: the groups are built, and then the answer is declined. A
/// refusal is not a budget. A day count is also only a proxy for rows: ten years of a small table and
/// ten years of a large one are the same number here. A real budget is expressed in rows or bytes
/// scanned, which needs something from the data system that no port asks for yet.
pub const MAX_RANGE_DAYS: i32 = 3653;

/// One equality filter: a dimension, and a value the pinned bundle declares.
///
/// The value is a [`DimensionValue`] here and a bind parameter by the time it reaches a statement. It
/// is checked against the metric's allowlist first, so the parameterisation is the second line of
/// defence rather than the only one.
///
/// # Why a caller's value is parsed by the type a catalog author's value is parsed by
///
/// It was a `String`, and the review that gave `DimensionValue` to the catalog side asked whether the
/// request side wanted it too. It does, for four reasons, and the last one is the decisive one:
///
/// * **It refuses nothing a request could have been answered.** The two are compared for equality
///   against the metric's allowlist, and every entry in that allowlist is a `DimensionValue`. Text
///   that cannot be one cannot be in there, so parsing here turns a `DimensionValueNotAllowed`
///   refusal into a `400` naming the field and loses no answerable question.
/// * **The precedent is already here and is older than this type.** A caller's `metric` and
///   `dimension` arrive as text and are parsed by [`MetricName`] and [`DimensionName`] - the same
///   types the catalog loader uses, at the same boundary, by the same constructor. A value being the
///   one field held to a laxer rule was the asymmetry, not the fix.
/// * **It bounds what a request may carry before anything allocates it.** A ten-megabyte filter value
///   used to be compared against the allowlist and refused, having been read, cloned into
///   [`Self::literals`] and rendered into whatever an audit sink keeps.
/// * **A second character rule is a rule nothing compares against the first.** [`crate::text`] exists
///   because one such rule was written down twice and the copies drifted. A request-side value type
///   with its own idea of what a value may hold would be that mistake, deliberately, in a place where
///   one side of the comparison is content and the other is a caller.
///
/// **What does NOT follow is that a refusal may name the text.** `sutura_http::wire` parses the value
/// and reports `filters[i].value` without the parse error underneath it, because
/// [`InvalidDimensionValue`](crate::catalog::InvalidDimensionValue) carries the offending input and
/// [`RefusalReason`]'s own rule is that caller-supplied text is never reflected into a message that
/// reaches a log, a UI and an agent's context.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    dimension: DimensionName,
    value: DimensionValue,
}

impl Filter {
    pub const fn new(dimension: DimensionName, value: DimensionValue) -> Self {
        Self { dimension, value }
    }

    #[inline]
    pub const fn dimension(&self) -> &DimensionName {
        &self.dimension
    }

    #[inline]
    pub const fn value(&self) -> &DimensionValue {
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
        out.extend(self.filters.iter().map(|f| String::from(f.value().as_str())));
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
/// [`TimeRangeTooLong`](RefusalReason::TimeRangeTooLong) is the variant that exists for the half the
/// type does *not* do, and the pair is worth reading together: an absent bound is unrepresentable, a
/// bound that is present and enormous is refused. The second has to be a refusal rather than a parse
/// error because the same [`TimeRange`] is also a catalog author's anchor range, and a maximum on the
/// type would govern authorship in order to govern requests.
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
    /// The result would carry more rows than `plan::MAX_ROWS`.
    ///
    /// **The refusal that replaced a silent truncation, and it was a wrong-number bug.** The cap
    /// used to be the `LIMIT` on the statement and nothing compared the rows that came back against
    /// it, so a question wide enough to exceed it was answered with the first `MAX_ROWS` groups by
    /// group key - with provenance attached and no indication it was partial. Summing them gives a
    /// wrong number under a certified name, and nothing downstream can tell.
    ///
    /// A governance outcome rather than an error, for the same reason
    /// [`TimeRangeTooLong`](RefusalReason::TimeRangeTooLong) is: the question is well formed, the
    /// metric permits it, and the answer is still no. Refused rather than narrowed, because a total
    /// over part of the groups is not a smaller answer to the question asked - it is a different
    /// number wearing the same name.
    ///
    /// Carries the limit and **not** how many rows there would have been, because nobody knows: the
    /// plan asks for one row more than the cap and stops there, so what is known is "more than
    /// this". That is the difference from [`TooManyDimensions`](RefusalReason::TooManyDimensions),
    /// which can name what was requested because the caller sent it.
    ResultTooLarge { limit: u32 },
    /// A span of history longer than [`MAX_RANGE_DAYS`].
    ///
    /// The availability boundary, and a governance outcome rather than a malformed question: the
    /// range parsed, both endpoints are real dates, and the answer is still no. Refused rather than
    /// silently narrowed to the last permitted day, because an answer about a different period than
    /// the one asked about is a wrong number nothing downstream can detect.
    ///
    /// Carries the two day counts and nothing from the caller's text, which is what makes it safe to
    /// log: a day count is derived from parsed dates, so there is no caller-controlled string to
    /// reflect into a message that reaches a log, a UI and an agent's context.
    TimeRangeTooLong { days: i32, limit: i32 },
    /// The plan would need to read from more than one data system.
    ///
    /// Refused rather than run in parts, because a second data system is a second identity to
    /// satisfy, and a plan that runs partly as somebody else is the failure this design exists to
    /// prevent.
    PlanSpansTwoSources { sources: usize },
    /// The plan named a data system this process did not open.
    ///
    /// **What raises it today is a name comparison, not an identity check**, and the doc comment
    /// used to claim otherwise. `sutura_app::answer` compares the plan's source against the
    /// adapter's own and refuses when they differ, which catches a bundle pointed at one data
    /// system being answered from another - a real hole, and the reason the check exists.
    ///
    /// It is deliberately the variant an identity failure will also use, because both are the same
    /// answer to a caller: this question cannot be answered here, and it will not be answered
    /// somewhere else instead. A refusal rather than a fallback - running as the service's own
    /// identity would turn "you may not see these rows" into "here are the rows" - but nothing in
    /// this workspace can yet run as any identity, so that half is a design target and not a
    /// control. `AGENTS.md` records which is which.
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
    use crate::catalog::DimensionValue;
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
                DimensionValue::parse(value).expect("a test value is a value"),
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
    // deserialize at all - are asserted in `sutura-catalog-local`, against the YAML. They are
    // asserted there rather than here because that is the format a catalog is actually written in,
    // so the assertion covers the read path a typo arrives through; asserting them over a second
    // format would restate serde rather than the catalog. Not for want of a parser here:
    // `serde_json` is on `ALLOWED_IN_DOMAIN` in `xtask/src/boundaries.rs`, because the definition
    // digest is taken over the serialized form and has to be computed by code the domain trusts.

    #[test]
    fn a_value_a_catalog_could_not_declare_never_becomes_a_filter() {
        // The request side is held to the same character rule and the same length as the catalog
        // side, so text that could not be in an allowlist never reaches the comparison against one.
        // A caller sending either of these gets a `400` naming `filters[0].value` - raised by
        // `sutura_http::wire` WITHOUT the parse error underneath it, because that error carries the
        // caller's own text and this module's rule is that nothing reflects it back.
        drop(DimensionValue::parse("nor\u{200B}th").unwrap_err());
        drop(DimensionValue::parse("x".repeat(10_000)).unwrap_err());
        // And the value that would have been answered still is.
        assert_eq!(query_with_filter("north").filters()[0].value().as_str(), "north");
    }

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
