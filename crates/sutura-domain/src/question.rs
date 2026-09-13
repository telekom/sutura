//! Parsing a caller's raw question fields into a certified [`crate::query::Query`].
//!
//! **The one conversion two transports used to duplicate.** `sutura-http`'s `QuestionBody` and
//! `sutura-mcp`'s `AskArgs` carry the same five fields, for the reason every wire shape here does:
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
//! stack and which therefore has no analogue here. [`parse_query`] takes the five fields already
//! extracted, as borrowed strings, so it carries no serde of its own and no framework.

use crate::calendar::{Date, InvalidDate, InvalidTimeRange, TimeRange};
use crate::catalog::DimensionValue;
use crate::model::{DimensionName, Grain, InvalidIdentifier, MetricName};
use crate::query::{Filter, Query};

/// One filter, before parsing: a caller's raw dimension name and value, borrowed out of whichever
/// wire struct a transport deserialized.
///
/// Fields are private - a `pub` field on a `pub struct` fails `cargo xtask check-boundaries` in
/// this crate - even though nothing here is validated yet: the two strings are exactly what a
/// transport extracted, unchanged, and [`new`](Self::new) is the only way to pair them.
#[derive(Debug, Clone, Copy)]
pub struct RawFilter<'a> {
    dimension: &'a str,
    value: &'a str,
}

impl<'a> RawFilter<'a> {
    /// Pairs a caller's raw dimension name and value, as a transport extracted them.
    #[inline]
    pub const fn new(dimension: &'a str, value: &'a str) -> Self {
        Self { dimension, value }
    }
}

/// Why a caller's raw fields did not become a [`Query`].
///
/// Every variant names the field, and none of them echoes the caller's value back except where the
/// value is the thing that failed to parse as an identifier - which is a bounded character set, not
/// free text.
#[derive(Debug, thiserror::Error)]
pub enum MalformedQuestion {
    #[error("`metric` is not a metric name")]
    Metric {
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("`grain` is not one of: day, week, month, quarter, year")]
    Grain { found: String },
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
    /// The value is not one a catalog could have declared: nothing, more than one line, a control
    /// character, an invisible or direction-changing code point, spacing a reader cannot see, or
    /// longer than `crate::catalog::MAX_DIMENSION_VALUE_CHARS`.
    ///
    /// **The one variant with no `#[source]`, and the omission is the point.** Every other cause in
    /// this enum either carries no caller text or carries text that already failed an identifier
    /// parse, which is a few dozen ASCII bytes. `crate::catalog::DimensionValue`'s own parse error
    /// carries the offending input, because it exists for the author of a catalog - and a transport
    /// error reaches a log, a UI and an agent's context, which is the one place
    /// [`crate::query::RefusalReason`] is explicit that a caller's own text must not arrive. So the
    /// field and the index are reported and the cause is dropped: the same answer
    /// `DimensionValueNotAllowed` gives, at the boundary that now catches it earlier.
    #[error("`filters[{index}].value` is not a value this catalog could declare")]
    FilterValue { index: usize },
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
        // The discard IS the control, so it is spelled out rather than lint-silenced by accident:
        // `DimensionValue`'s own parse error carries the offending text because it exists for the
        // author of a catalog, and this error becomes a transport error that reaches a log, a UI
        // and an agent's context. `RefusalReason` is explicit that a caller's own text must not
        // arrive there.
        #[expect(
            clippy::map_err_ignore,
            reason = "the parse error carries the caller's own text, and a transport error must not \
                      reflect it back - see MalformedQuestion::FilterValue"
        )]
        let value = DimensionValue::parse(raw.value).map_err(|_| MalformedQuestion::FilterValue { index })?;
        parsed_filters.push(Filter::new(dimension, value));
    }
    Ok(Query::new(metric, grain, range, parsed_dimensions, parsed_filters))
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
        other => Err(MalformedQuestion::Grain {
            found: String::from(other),
        }),
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
            &[RawFilter::new("region", "north")],
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
    fn an_unknown_grain_names_the_field_and_the_accepted_set() {
        let error =
            parse_query("revenue", "fortnight", "2026-06-01", "2026-07-01", &[], &[]).expect_err("`fortnight` is not a grain");
        assert!(matches!(error, MalformedQuestion::Grain { .. }), "{error:?}");
        assert!(error.to_string().contains("quarter"), "{error}");
    }

    #[test]
    fn a_malformed_date_names_which_end_of_the_range() {
        let error = parse_query("revenue", "month", "nope", "2026-07-01", &[], &[]).expect_err("`nope` is not a date");
        let MalformedQuestion::Date { field, .. } = error else {
            panic!("{error:?} is not a Date error");
        };
        assert_eq!(field, "start");
    }

    #[test]
    fn a_filter_value_this_catalog_could_not_declare_names_no_caller_text() {
        let error = parse_query(
            "revenue",
            "month",
            "2026-06-01",
            "2026-07-01",
            &[],
            &[RawFilter::new("region", "line one\nline two")],
        )
        .expect_err("a multi-line value is not one this catalog could declare");
        assert!(matches!(error, MalformedQuestion::FilterValue { index: 0 }), "{error:?}");
        assert!(!error.to_string().contains("line one"), "{error}");
    }
}
