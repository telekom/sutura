//! Parsing a caller's raw question fields into a certified [`crate::query::Query`].
//!
//! **The one conversion two transports used to duplicate.** `sutura-http`'s `QuestionBody` and
//! `sutura-mcp`'s `AskArgs` carry the same six fields, for the reason every wire shape here does:
//! they are both the whole of what a caller may ask, extracted as plain strings so a parse failure
//! can name the field rather than quote a deserializer. Before this module existed, each transport
//! re-derived [`crate::query::Query`] from those fields with its own copy of this logic, its own
//! copy of [`MalformedQuestion`], and its own copy of the grain and range parsers - identical code,
//! kept equal only by review, because *an adapter may not depend on another adapter*. The shared
//! part moves here, inward of both, where nothing has to be kept equal by hand any more.
//!
//! What stays with each transport is genuinely transport-shaped: the `#[derive(serde::Deserialize)]`
//! / `schemars::JsonSchema` wire struct itself, and the one extra failure a transport's own
//! deserialization step can produce before this function is ever reached - MCP's arguments object
//! failing to deserialize as an object at all, which HTTP's extractor rejects earlier in its own
//! stack and which therefore has no analogue here. [`parse_query`] takes the six fields already
//! extracted, as borrowed strings and one optional [`RawTop`], so it carries no serde of its own
//! and no framework.

use crate::calendar::{Date, InvalidDate, InvalidTimeRange, TimeRange};
use crate::catalog::{DimensionValue, MAX_VALUES_PER_DIMENSION};
use crate::model::{DimensionName, Grain, InvalidIdentifier, MetricName};
use crate::query::{Filter, FilterOp, FilterValues, InvalidTopN, Query, Top, TopBy, TopDirection, TopN};

/// One filter, before parsing: a caller's raw dimension name, comparison and value list, borrowed
/// out of whichever wire struct a transport deserialized.
///
/// Fields are private - a `pub` field on a `pub struct` fails `cargo xtask check-boundaries` in
/// this crate - even though nothing here is validated yet: the three fields are exactly what a
/// transport extracted, unchanged, and [`new`](Self::new) is the only way to pair them.
#[derive(Debug, Clone, Copy)]
pub struct RawFilter<'a> {
    dimension: &'a str,
    op: &'a str,
    values: &'a [String],
}

impl<'a> RawFilter<'a> {
    /// Pairs a caller's raw dimension name, comparison and value list, as a transport extracted
    /// them.
    #[inline]
    pub const fn new(dimension: &'a str, op: &'a str, values: &'a [String]) -> Self {
        Self { dimension, op, values }
    }
}

/// A `top` clause, before parsing: a caller's raw row count, ranking key and direction, borrowed
/// out of whichever wire struct a transport deserialized.
#[derive(Debug, Clone, Copy)]
pub struct RawTop<'a> {
    n: u32,
    by: &'a str,
    direction: &'a str,
}

impl<'a> RawTop<'a> {
    #[inline]
    pub const fn new(n: u32, by: &'a str, direction: &'a str) -> Self {
        Self { n, by, direction }
    }
}

/// Why a caller's raw fields did not become a [`Query`].
///
/// **Every variant names the field, and neither a variant's own sentence nor any link of its cause
/// chain carries the caller's own text.** The chain is the half that has to be said out loud,
/// because both transports render this failure by walking `source()` to the end and handing the
/// result straight to whoever asked - the RFC 7807 `detail` on the HTTP surface, an
/// `invalid_params` message in a model's context on the agent one. So
/// [`crate::model::InvalidIdentifier`] and [`crate::calendar::InvalidDate`] report the rule that
/// was broken, the one offending character, and the measured length against the limit, and echo
/// nothing they rejected; `FilterValue` drops its cause outright, for the reason its own note
/// gives.
///
/// **The one value that does come back is one that already parsed.** `Range`'s cause is
/// [`crate::calendar::InvalidTimeRange::Empty`], which names both endpoints - and they are
/// [`crate::calendar::Date`]s by then, so what renders is ten characters of digits and hyphens in
/// the single layout that type accepts, not the text a caller sent. That is the distinction this
/// whole note turns on, and the one an earlier version of it got backwards: a parse's OUTPUT is
/// bounded by the type that produced it, and its INPUT is bounded by nothing whatever.
///
/// **The limit.** This is a property of `Display` and `source()` across these enums, held by their
/// carrying no field a renderer could reach for - not by a gate, and not by either transport, which
/// walk whatever chain they are given. `crates/sutura-mcp/tests/shared_question_conversion.rs`
/// asserts it over every caller-controlled field against a reproduction of both transports' chain
/// walk, not by calling either transport's own renderer, so it reddens if one of these sentences is
/// widened again but not if a transport's renderer alone started interpolating something this
/// parser never carried.
#[derive(Debug, thiserror::Error)]
pub enum MalformedQuestion {
    #[error("`metric` is not a metric name")]
    Metric {
        #[source]
        cause: InvalidIdentifier,
    },
    /// **Carries no field, and that is on purpose.** The accepted set is fixed and finite, so the
    /// sentence names all five instead of echoing back the one that did not match - the caller's
    /// text would otherwise sit in a `Debug` rendering unread by any transport, the shape
    /// `MalformedQuestion` is elsewhere careful never to carry.
    #[error("`grain` is not one of: day, week, month, quarter, year")]
    Grain,
    #[error("`range.{field}` is not a date in `YYYY-MM-DD` form")]
    Date {
        field: &'static str,
        #[source]
        cause: InvalidDate,
    },
    #[error("`range` is not a period")]
    Range {
        #[source]
        cause: InvalidTimeRange,
    },
    #[error("`dimensions[{index}]` is not a dimension name")]
    Dimension {
        index: usize,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("`filters[{index}].dimension` is not a dimension name")]
    FilterDimension {
        index: usize,
        #[source]
        cause: InvalidIdentifier,
    },
    /// Carries no field, for [`Self::Grain`]'s own reason: the accepted set is fixed and finite.
    #[error("`filters[{index}].op` is not one of: in, not_in")]
    FilterOp { index: usize },
    /// More values than [`crate::catalog::MAX_VALUES_PER_DIMENSION`] declared on one filter.
    ///
    /// Bounded before any of them is parsed, for the same reason a value's own length is: work
    /// proportional to a caller-chosen count must not run before a caller-chosen count is checked
    /// against something fixed. No filterable dimension can declare a larger allowlist than this,
    /// so a caller's set can never be usefully bigger either - every value past the limit would be
    /// refused as [`crate::query::RefusalReason::DimensionValueNotAllowed`] regardless.
    #[error("`filters[{index}].values` names {requested} values, and a filter may carry at most {limit}")]
    TooManyFilterValues { index: usize, requested: usize, limit: usize },
    /// The value is not one a catalog could have declared: nothing, more than one line, a control
    /// character, an invisible or direction-changing code point, spacing a reader cannot see, or
    /// longer than `crate::catalog::MAX_DIMENSION_VALUE_CHARS`.
    ///
    /// **The one variant with no `#[source]`, and the omission is the point.** Every other cause in
    /// this enum reports the rule it applied and not the input it rejected, so its chain is safe to
    /// walk. `crate::catalog::DimensionValue`'s own parse error is the exception: it carries the
    /// offending input, because it exists for the author of a catalog - and a transport error
    /// reaches a log, a UI and an agent's context, which is the one place
    /// [`crate::query::RefusalReason`] is explicit that a caller's own text must not arrive. So the
    /// field and the index are reported and the cause is dropped rather than reported and trusted:
    /// the same answer `DimensionValueNotAllowed` gives, at the boundary that now catches it
    /// earlier.
    #[error("`filters[{index}].values[{value_index}]` is not a value this catalog could declare")]
    FilterValue { index: usize, value_index: usize },
    /// `filters[{index}].values` deserialized to an empty list.
    ///
    /// Unreachable from a wire body whose `values` field is present at all with at least one
    /// entry, and reachable from an explicit `"values": []` - `serde`'s own array default is
    /// empty, so this is the field's normal shape being empty rather than missing.
    #[error("`filters[{index}].values` must carry at least one value")]
    EmptyFilterValues { index: usize },
    #[error("`top.n` is not a positive row count")]
    TopN {
        #[source]
        cause: InvalidTopN,
    },
    /// Carries no field, for [`Self::Grain`]'s own reason: the accepted set is fixed and finite.
    #[error("`top.by` is not one of: metric, period")]
    TopBy,
    /// Carries no field, for [`Self::Grain`]'s own reason: the accepted set is fixed and finite.
    #[error("`top.direction` is not one of: desc, asc")]
    TopDirection,
}

/// Parses a caller's raw question fields into a [`Query`].
///
/// **This is the whole translation a transport is allowed to do**: extract each field from its own
/// wire shape as a plain string, hand them here, get back a certified [`Query`] or a
/// [`MalformedQuestion`] naming the field. Nothing here decides what may be asked - that is
/// `sutura_app::compile`'s job, against the pinned catalog this function never sees and cannot
/// widen.
pub fn parse_query(
    metric: &str,
    grain: &str,
    range_start: &str,
    range_end: &str,
    dimensions: &[String],
    filters: &[RawFilter<'_>],
    top: Option<RawTop<'_>>,
) -> Result<Query, MalformedQuestion> {
    let metric = MetricName::parse(metric).map_err(|cause| MalformedQuestion::Metric { cause })?;
    let grain = grain_of(grain)?;
    let range = range_of(range_start, range_end)?;
    let mut parsed_dimensions = Vec::with_capacity(dimensions.len());
    for (index, raw) in dimensions.iter().enumerate() {
        parsed_dimensions.push(DimensionName::parse(raw).map_err(|cause| MalformedQuestion::Dimension { index, cause })?);
    }
    let mut parsed_filters = Vec::with_capacity(filters.len());
    for (index, raw) in filters.iter().enumerate() {
        let dimension =
            DimensionName::parse(raw.dimension).map_err(|cause| MalformedQuestion::FilterDimension { index, cause })?;
        let op = match raw.op {
            "in" => FilterOp::In,
            "not_in" => FilterOp::NotIn,
            _other => return Err(MalformedQuestion::FilterOp { index }),
        };
        // Bounded before any value is parsed - see the variant's own doc comment.
        if raw.values.len() > MAX_VALUES_PER_DIMENSION {
            return Err(MalformedQuestion::TooManyFilterValues {
                index,
                requested: raw.values.len(),
                limit: MAX_VALUES_PER_DIMENSION,
            });
        }
        let mut values = Vec::with_capacity(raw.values.len());
        for (value_index, value) in raw.values.iter().enumerate() {
            // The discard IS the control, so it is spelled out rather than lint-silenced by
            // accident: `DimensionValue`'s own parse error carries the offending text because it
            // exists for the author of a catalog, and this error becomes a transport error that
            // reaches a log, a UI and an agent's context. `RefusalReason` is explicit that a
            // caller's own text must not arrive there.
            #[expect(
                clippy::map_err_ignore,
                reason = "the parse error carries the caller's own text, and a transport error must not \
                          reflect it back - see MalformedQuestion::FilterValue"
            )]
            let value = DimensionValue::parse(value).map_err(|_| MalformedQuestion::FilterValue { index, value_index })?;
            values.push(value);
        }
        // `EmptyFilterValues` carries no field to discard - a unit struct naming the one way
        // `FilterValues::parse` can fail - so this is not the caller-text case
        // `clippy::map_err_ignore` exists to catch; the lint cannot tell the two apart, hence the
        // named-but-unused binding rather than `_`.
        let values = FilterValues::parse(values).map_err(|_empty| MalformedQuestion::EmptyFilterValues { index })?;
        parsed_filters.push(Filter::new(dimension, op, values));
    }
    let query = Query::new(metric, grain, range, parsed_dimensions, parsed_filters);
    match top {
        None => Ok(query),
        Some(raw) => Ok(query.with_top(top_of(raw)?)),
    }
}

fn top_of(raw: RawTop<'_>) -> Result<Top, MalformedQuestion> {
    let n = TopN::parse(raw.n).map_err(|cause| MalformedQuestion::TopN { cause })?;
    let by = match raw.by {
        "metric" => TopBy::Metric,
        "period" => TopBy::Period,
        _other => return Err(MalformedQuestion::TopBy),
    };
    let direction = match raw.direction {
        "desc" => TopDirection::Desc,
        "asc" => TopDirection::Asc,
        _other => return Err(MalformedQuestion::TopDirection),
    };
    Ok(Top::new(n, by, direction))
}

/// The grain, from its name.
///
/// A hand-written match rather than a derived parse, so the error names the accepted set instead
/// of quoting a deserializer at a caller.
fn grain_of(raw: &str) -> Result<Grain, MalformedQuestion> {
    match raw {
        "day" => Ok(Grain::Day),
        "week" => Ok(Grain::Week),
        "month" => Ok(Grain::Month),
        "quarter" => Ok(Grain::Quarter),
        "year" => Ok(Grain::Year),
        _other => Err(MalformedQuestion::Grain),
    }
}

fn range_of(start: &str, end: &str) -> Result<TimeRange, MalformedQuestion> {
    let start = Date::parse(start).map_err(|cause| MalformedQuestion::Date { field: "start", cause })?;
    let end = Date::parse(end).map_err(|cause| MalformedQuestion::Date { field: "end", cause })?;
    TimeRange::new(start, end).map_err(|cause| MalformedQuestion::Range { cause })
}

#[cfg(test)]
mod tests {
    use super::{MalformedQuestion, RawFilter, parse_query};
    use crate::model::{DimensionName, Grain, MetricName};

    #[test]
    fn a_well_formed_question_becomes_a_query() {
        let query = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[String::from("region")],
            &[RawFilter::new("region", "in", &[String::from("north")])],
            None,
        )
        .expect("a well formed question is a query");
        assert_eq!(query.metric(), &MetricName::parse("revenue").expect("a test metric"));
        assert_eq!(query.grain(), Grain::Month);
        assert_eq!(
            query.dimensions(),
            [DimensionName::parse("region").expect("a test dimension")].as_slice()
        );
        assert_eq!(query.filters().len(), 1);
    }

    #[test]
    fn a_not_in_filter_with_several_values_becomes_a_query() {
        use crate::query::FilterOp;

        let query = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[RawFilter::new(
                "region",
                "not_in",
                &[String::from("north"), String::from("south")],
            )],
            None,
        )
        .expect("a well formed not_in filter is a query");
        let filter = &query.filters()[0];
        assert_eq!(filter.op(), FilterOp::NotIn);
        assert_eq!(filter.values().len(), 2);
    }

    #[test]
    fn an_unknown_filter_op_names_the_field_and_the_accepted_set() {
        let error = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[RawFilter::new("region", "equals", &[String::from("north")])],
            None,
        )
        .expect_err("`equals` is not `in` or `not_in`");
        assert!(matches!(error, MalformedQuestion::FilterOp { index: 0 }), "{error:?}");
    }

    #[test]
    fn an_empty_filter_value_list_names_the_field() {
        let error = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[RawFilter::new("region", "in", &[])],
            None,
        )
        .expect_err("an empty values list is not a filter");
        assert!(
            matches!(error, MalformedQuestion::EmptyFilterValues { index: 0 }),
            "{error:?}"
        );
    }

    #[test]
    fn more_filter_values_than_the_limit_are_refused_before_any_is_parsed() {
        let too_many: Vec<String> = (0..=super::MAX_VALUES_PER_DIMENSION).map(|n| n.to_string()).collect();
        let error = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[RawFilter::new("region", "in", &too_many)],
            None,
        )
        .expect_err("more values than the limit is not a filter");
        assert!(
            matches!(error, MalformedQuestion::TooManyFilterValues { index: 0, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn an_unknown_grain_names_the_field_and_the_accepted_set() {
        let error = parse_query("revenue", "fortnight", "2026-06-01", "2026-07-01", &[], &[], None)
            .expect_err("`fortnight` is not a grain");
        assert!(matches!(error, MalformedQuestion::Grain), "{error:?}");
        assert!(error.to_string().contains("quarter"), "{error}");
    }

    #[test]
    fn a_malformed_date_names_which_end_of_the_range() {
        let error = parse_query("revenue", "month", "nope", "2026-07-01", &[], &[], None).expect_err("`nope` is not a date");
        let MalformedQuestion::Date { field, .. } = error else {
            panic!("{error:?} is not a Date error");
        };
        assert_eq!(field, "start");
    }

    #[test]
    fn a_reversed_range_is_refused_rather_than_reordered() {
        let error = parse_query("revenue", "month", "2026-07-01", "2026-06-01", &[], &[], None)
            .expect_err("an end before its start is not a period");
        assert!(matches!(error, MalformedQuestion::Range { .. }), "{error:?}");
    }

    #[test]
    fn a_filter_value_this_catalog_could_not_declare_names_no_caller_text() {
        let error = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[RawFilter::new("region", "in", &[String::from("line one\nline two")])],
            None,
        )
        .expect_err("a multi-line value is not one this catalog could declare");
        assert!(
            matches!(
                error,
                MalformedQuestion::FilterValue {
                    index: 0,
                    value_index: 0
                }
            ),
            "{error:?}"
        );
        assert!(!error.to_string().contains("line one"), "{error}");
    }

    #[test]
    fn a_well_formed_top_attaches_to_the_query() {
        let query = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[],
            Some(super::RawTop::new(10, "metric", "desc")),
        )
        .expect("a well formed top is a top");
        let top = query.top().expect("top was attached");
        assert_eq!(top.n().get(), 10);
    }

    #[test]
    fn a_zero_top_n_names_the_field() {
        let error = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[],
            Some(super::RawTop::new(0, "metric", "desc")),
        )
        .expect_err("zero rows is not a row count");
        assert!(matches!(error, MalformedQuestion::TopN { .. }), "{error:?}");
    }

    #[test]
    fn an_unknown_top_by_names_the_accepted_set() {
        let error = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[],
            Some(super::RawTop::new(10, "revenue", "desc")),
        )
        .expect_err("`revenue` is not `metric` or `period`");
        assert!(matches!(error, MalformedQuestion::TopBy), "{error:?}");
    }

    #[test]
    fn an_unknown_top_direction_names_the_accepted_set() {
        let error = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[],
            Some(super::RawTop::new(10, "metric", "sideways")),
        )
        .expect_err("`sideways` is not `desc` or `asc`");
        assert!(matches!(error, MalformedQuestion::TopDirection), "{error:?}");
    }
}
