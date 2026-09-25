//! One filter: a dimension, which way it compares, and the values it compares against.
//!
//! Split out of `query.rs` when that file crossed the thousand-line gate `cargo xtask max-lines`
//! enforces and cannot exempt under `crates/`. Concept-named rather than `part2`: everything here
//! is the filter's own shape, and `query.rs` keeps [`super::Query`] and [`super::RefusalReason`].

use std::collections::BTreeSet;

use crate::catalog::DimensionValue;
use crate::model::DimensionName;

/// Which way a [`Filter`] compares: every requested value must be one the dimension declares
/// ([`Self::In`]), or none of them may be ([`Self::NotIn`]).
///
/// **There is no `Eq`, and that is not an omission.** `In` with one value already says "equals this
/// one declared value" - the shape every filter had before this type existed - so a third variant
/// spelling the same comparison would be a second way to ask the same question, kept equal to the
/// first only by review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// The dimension's value must be one of [`Filter::values`].
    In,
    /// The dimension's value must be none of [`Filter::values`].
    NotIn,
}

/// The values one [`Filter`] compares against: at least one, deduplicated, each already a
/// [`DimensionValue`] a metric's allowlist could declare.
///
/// A set rather than the [`Vec`] the wire carries, so a value repeated in the caller's own list -
/// `["north", "north"]` - is one entry here rather than two, and two requests that list the same
/// values in a different order compare equal. [`BTreeSet`] rather than a hash set for the same
/// reason every other collection in this crate that reaches a digest or a golden is ordered: the
/// iteration order is a function of the values and never of the order a caller happened to send
/// them in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct FilterValues(BTreeSet<DimensionValue>);

impl FilterValues {
    /// Parses a non-empty, deduplicated set of values, refusing an empty list.
    pub fn parse(values: Vec<DimensionValue>) -> Result<Self, EmptyFilterValues> {
        let values: BTreeSet<DimensionValue> = values.into_iter().collect();
        if values.is_empty() {
            return Err(EmptyFilterValues);
        }
        Ok(Self(values))
    }

    /// One value, which can never be an empty set - the shape every filter had before
    /// [`FilterOp`] existed, kept infallible so the many call sites that still only ever compare
    /// against one declared value do not have to handle a refusal that cannot happen.
    #[inline]
    pub fn one(value: DimensionValue) -> Self {
        Self(BTreeSet::from([value]))
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &DimensionValue> {
        self.0.iter()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The wire shape a [`Filter`] deserializes through, and the point [`EmptyFilterValues`] is
/// checked at for a `Filter` built straight from a document rather than from
/// `sutura_domain::question::parse_query`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FilterRepr {
    dimension: DimensionName,
    op: FilterOp,
    values: Vec<DimensionValue>,
}

impl TryFrom<FilterRepr> for Filter {
    type Error = EmptyFilterValues;

    fn try_from(repr: FilterRepr) -> Result<Self, Self::Error> {
        Ok(Self {
            dimension: repr.dimension,
            op: repr.op,
            values: FilterValues::parse(repr.values)?,
        })
    }
}

/// One filter: a dimension, which way it compares, and the values the pinned bundle declares.
///
/// Each value is a [`DimensionValue`] here and a bind parameter by the time it reaches a statement.
/// Every one is checked against the metric's allowlist first, so the parameterisation is the second
/// line of defence rather than the only one.
///
/// # Why a caller's value is parsed by the type a catalog author's value is parsed by
///
/// It was a `String`, and the review that gave `DimensionValue` to the catalog side asked whether the
/// request side wanted it too. It does, for four reasons, and the last one is the decisive one:
///
/// * **It refuses nothing a request could have been answered.** Every value is compared against the
///   metric's allowlist, and every entry in that allowlist is a `DimensionValue`. Text that cannot be
///   one cannot be in there, so parsing here turns a `DimensionValueNotAllowed` refusal into a `400`
///   naming the field and loses no answerable question.
/// * **The precedent is already here and is older than this type.** A caller's `metric` and
///   `dimension` arrive as text and are parsed by [`MetricName`](crate::model::MetricName) and
///   [`DimensionName`] - the same types the catalog loader uses, at the same boundary, by the same
///   constructor. A value being the one field held to a laxer rule was the asymmetry, not the fix.
/// * **It bounds what a request may carry before anything allocates it.** A ten-megabyte filter value
///   used to be compared against the allowlist and refused, having been read, cloned into
///   [`Query::literals`](super::Query::literals) and rendered into whatever an audit sink keeps.
/// * **A second character rule is a rule nothing compares against the first.** [`crate::text`] exists
///   because one such rule was written down twice and the copies drifted. A request-side value type
///   with its own idea of what a value may hold would be that mistake, deliberately, in a place where
///   one side of the comparison is content and the other is a caller.
///
/// **What does NOT follow is that a refusal may name the text.** `sutura_http::wire` parses each
/// value and reports `filters[i].values[j]` without the parse error underneath it, because
/// [`InvalidDimensionValue`](crate::catalog::InvalidDimensionValue) carries the offending input and
/// [`RefusalReason`](super::RefusalReason)'s own rule is that caller-supplied text is never
/// reflected into a message that reaches a log, a UI and an agent's context.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "FilterRepr")]
pub struct Filter {
    dimension: DimensionName,
    op: FilterOp,
    values: FilterValues,
}

impl Filter {
    #[inline]
    pub const fn new(dimension: DimensionName, op: FilterOp, values: FilterValues) -> Self {
        Self { dimension, op, values }
    }

    /// `dimension` [`FilterOp::In`] one `value` - what every filter was before this type gained
    /// [`FilterOp`] and a set. Infallible: [`FilterValues::one`] can never be an empty set.
    #[inline]
    pub fn equals(dimension: DimensionName, value: DimensionValue) -> Self {
        Self::new(dimension, FilterOp::In, FilterValues::one(value))
    }

    #[inline]
    pub const fn dimension(&self) -> &DimensionName {
        &self.dimension
    }

    #[inline]
    pub const fn op(&self) -> FilterOp {
        self.op
    }

    #[inline]
    pub const fn values(&self) -> &FilterValues {
        &self.values
    }
}

/// A filter whose value list was empty.
///
/// **Unrepresentable everywhere but the wire boundary itself.** Every production caller of
/// [`FilterValues::parse`] already holds at least one parsed [`DimensionValue`] by construction -
/// `sutura_domain::question::parse_query` refuses an empty `values` list before this is ever
/// reached, naming the field - so this variant exists for the one caller that cannot make that
/// promise: a `Filter` deserialized directly, which is what the example corpus's own fixtures are.
///
/// **Declared last in this file**, for `crate::warehouse::deadline::NoBudget`'s own reason:
/// `xtask check-boundaries`'s pub-field scan misreads a unit struct followed by an `impl` block
/// as the struct's own fields, a gate defect noted as a follow-up rather than worked around with
/// more prose here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a filter's values must not be empty")]
pub struct EmptyFilterValues;

#[cfg(test)]
mod tests {
    use super::{EmptyFilterValues, Filter, FilterOp, FilterValues};
    use crate::catalog::DimensionValue;
    use crate::model::DimensionName;

    fn filter(dimension: &str, op: FilterOp, values: &[&str]) -> Filter {
        Filter::new(
            DimensionName::parse(dimension).expect("a test dimension is a dimension"),
            op,
            FilterValues::parse(
                values
                    .iter()
                    .map(|value| DimensionValue::parse(*value).expect("a test value is a value"))
                    .collect(),
            )
            .expect("a test filter carries at least one value"),
        )
    }

    #[test]
    fn equals_is_in_with_one_value() {
        let dimension = DimensionName::parse("region").expect("a test dimension is a dimension");
        let value = DimensionValue::parse("north").expect("a test value is a value");
        assert_eq!(Filter::equals(dimension, value), filter("region", FilterOp::In, &["north"]));
    }

    #[test]
    fn an_empty_filter_value_list_is_refused() {
        // The one caller of `FilterValues::parse` that can actually reach an empty list:
        // `sutura_domain::question::parse_query` refuses before this point, so this is the wire
        // boundary's own check, reached by a `Filter` deserialized straight from a document.
        assert_eq!(FilterValues::parse(Vec::new()).unwrap_err(), EmptyFilterValues);
        let deserialized: Result<Filter, _> = serde_json::from_str(r#"{"dimension":"region","op":"in","values":[]}"#);
        assert!(deserialized.is_err(), "an empty values list must not deserialize");
    }

    /// `Filter`'s canonical form is `{dimension, op, values}`, a mapping rather than a string, so
    /// `xtask::serde_parse`'s generated corpus cannot reach it the way it reaches every
    /// single-string newtype in `serialized_form_tests.rs`. This is `Term`'s own round-trip test,
    /// asked of `Filter` instead: every value serializes as written, and re-parsing what it wrote
    /// is the same value and the same bytes - `In` and `NotIn`, one value and several.
    #[test]
    fn a_filter_round_trips_through_its_on_disk_shape() {
        let filters = [
            filter("region", FilterOp::In, &["north"]),
            filter("region", FilterOp::In, &["north", "south"]),
            filter("segment", FilterOp::NotIn, &["wholesale"]),
            filter("segment", FilterOp::NotIn, &["wholesale", "consumer"]),
        ];
        for filter in filters {
            let written = serde_json::to_string(&filter).expect("a filter serializes");
            let reread: Filter = serde_json::from_str(&written)
                .unwrap_or_else(|cause| panic!("a filter does not deserialize what it serialized ({written}): {cause}"));
            assert_eq!(reread, filter, "a filter has to survive the shape it writes itself as");
            assert_eq!(
                serde_json::to_string(&reread).expect("a filter serializes"),
                written,
                "the same filter serializes to different bytes on the second pass, which moves the digest"
            );
        }
    }
}
