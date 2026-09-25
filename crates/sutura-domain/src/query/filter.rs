//! [`Filter`]: one dimension's worth of what a question may filter by.
//!
//! Split out of `query.rs` because the parent module is near its line limit, and because the three
//! variants below - equality, and a set the caller allows or excludes - are one cohesive idea with
//! its own doc comment, not three loose fields. See `query.rs`'s own header for why the value is a
//! [`DimensionValue`] and not a `String`; that argument is unchanged by there now being a set of
//! them.

use crate::catalog::DimensionValue;
use crate::model::DimensionName;
use crate::nonempty::NonEmpty;

/// One clause of a question's filter: a dimension, and what it must - or must not - equal.
///
/// **Every value is still a parsed [`DimensionValue`] from the same allowlist an equality filter is
/// checked against.** `In`/`NotIn` do not open a second, laxer path for a caller's text: each value
/// in the set is checked against the metric's declared allowlist exactly as [`Self::Eq`]'s single
/// value is, and an unlisted member of the set is
/// [`RefusalReason::DimensionValueNotAllowed`](super::RefusalReason::DimensionValueNotAllowed) - the
/// same refusal, reused, because a set's member and a single equality's value are the same kind of
/// thing to the allowlist that checks them.
///
/// **No comparison operator reaches this type**, deliberately: there is no `Gt`, `Lt` or `Like`
/// variant, and none is planned. A comparison against free text is exactly the shape a certified
/// number cannot come from - the allowlist is what makes a filter value bounded and enumerable, and
/// a range or a pattern match has no allowlist to check against.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, tag = "op", rename_all = "snake_case")]
pub enum Filter {
    /// The dimension equals this one value.
    Eq {
        dimension: DimensionName,
        value: DimensionValue,
    },
    /// The dimension equals one of these values - "segment A or segment B".
    In {
        dimension: DimensionName,
        values: NonEmpty<DimensionValue>,
    },
    /// The dimension equals none of these values.
    NotIn {
        dimension: DimensionName,
        values: NonEmpty<DimensionValue>,
    },
}

impl Filter {
    /// An equality filter - the constructor every existing caller of the old two-field struct
    /// already spells, unchanged, because the type it built kept its name and its meaning.
    #[inline]
    pub const fn new(dimension: DimensionName, value: DimensionValue) -> Self {
        Self::Eq { dimension, value }
    }

    /// A membership filter: the dimension must equal one of `values`.
    #[inline]
    pub const fn in_set(dimension: DimensionName, values: NonEmpty<DimensionValue>) -> Self {
        Self::In { dimension, values }
    }

    /// An exclusion filter: the dimension must equal none of `values`.
    #[inline]
    pub const fn not_in_set(dimension: DimensionName, values: NonEmpty<DimensionValue>) -> Self {
        Self::NotIn { dimension, values }
    }

    /// The dimension every variant filters on.
    #[inline]
    pub const fn dimension(&self) -> &DimensionName {
        match self {
            Self::Eq { dimension, .. } | Self::In { dimension, .. } | Self::NotIn { dimension, .. } => dimension,
        }
    }

    /// Every value this filter carries, in declared order. One for [`Self::Eq`], the whole set for
    /// [`Self::In`]/[`Self::NotIn`].
    ///
    /// The one place a caller who does not care which variant this is reads every value it holds -
    /// [`super::Query::literals`] uses it so a field added here is a field the no-injection golden
    /// sees without a second edit.
    pub fn values(&self) -> Vec<&DimensionValue> {
        match self {
            Self::Eq { value, .. } => vec![value],
            Self::In { values, .. } | Self::NotIn { values, .. } => values.iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Filter;
    use crate::catalog::DimensionValue;
    use crate::model::DimensionName;
    use crate::nonempty::NonEmpty;

    fn region() -> DimensionName {
        DimensionName::parse("region").expect("a test dimension is a dimension")
    }

    fn value(text: &str) -> DimensionValue {
        DimensionValue::parse(text).expect("a test value is a value")
    }

    #[test]
    fn eq_carries_one_value() {
        let filter = Filter::new(region(), value("north"));
        assert_eq!(filter.dimension(), &region());
        assert_eq!(filter.values(), vec![&value("north")]);
    }

    #[test]
    fn in_carries_every_value_in_order() {
        let values = NonEmpty::parse(vec![value("north"), value("south")]).expect("two values is not empty");
        let filter = Filter::in_set(region(), values);
        assert_eq!(filter.values(), vec![&value("north"), &value("south")]);
        assert!(matches!(filter, Filter::In { .. }));
    }

    #[test]
    fn not_in_is_a_distinct_variant_from_in() {
        let values = NonEmpty::one(value("north"));
        let filter = Filter::not_in_set(region(), values);
        assert!(matches!(filter, Filter::NotIn { .. }));
    }

    #[test]
    fn the_wire_shape_is_tagged_by_op() {
        let json = serde_json::to_value(Filter::new(region(), value("north"))).expect("a filter serializes");
        assert_eq!(json["op"], "eq");
        assert_eq!(json["dimension"], "region");
        assert_eq!(json["value"], "north");
    }
}
