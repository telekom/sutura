//! The label namespace a question cannot reach, and the one function that assigns it.

use crate::federation::Federation;

/// The one definition of what a carried leaf is projected under.
///
/// The splitter and the combiner both call this, so the column the combiner reads a leaf from and
/// the label the splitter projected it under cannot disagree - there is no second copy of the rule.
///
/// **Position, and nothing else.** A ratio of two sums - `sum(a) / sum(b)` - is one aggregating
/// function twice, so naming by aggregate would give both leaves one label and a combine that
/// divides a column by itself. Position cannot collide, and it is all a leg needs: a leg carries one
/// metric, so the metric's name distinguishes nothing inside it. The answer's measure comes back
/// under the metric's own certified name, which `FederatedPlan`'s `measure_label` holds.
pub fn labels(federation: &Federation) -> Vec<InternalLabel> {
    (0..federation.carried().len()).map(InternalLabel::Leaf).collect()
}

/// A column label the splitter and the combiner agree on, in a namespace no question can name.
///
/// **Why a type rather than a convention.** A federated fact leg's result carries public dimension
/// labels beside internal ones - the column the legs are joined on, and one per carried leaf of the
/// measure. The splitter spelled the first with the physical remote join COLUMN's text and the
/// second as `metric__{n}`, and both of those are legal identifiers: a metric with a legal dimension
/// named `customer_key`, backed by a different column, projected two fact columns under one label
/// and the combiner refused the answer it could not disambiguate. A dimension is free to be named
/// anything [`DimensionName`](crate::model::DimensionName) accepts, so the internal labels are what
/// has to move - keeping a legal question legal is the constraint, not a naming rule for authors.
///
/// **What makes the two namespaces disjoint, and it is one character.** Every rendering here starts
/// with a digit, which `crate::model`'s identifier parser refuses as a FIRST character - a leading
/// digit is legal in some dialects and not others, so it was already refused for portability. No
/// `DimensionName`, `MetricName`, `ColumnName` or `TableName` can therefore spell one of these, for
/// any spelling and any length. `federated/tests.rs` asserts that by parsing every constructible
/// label as each of those names and requiring the refusal, rather than leaving it to this paragraph.
///
/// **Every value is valid, so there is nothing to check.** A `usize` position out of a plan's leaf
/// range is a wiring defect the combiner reports as a missing column, not a label this type could
/// have refused - which is why the variants carry their data in the open and no constructor is
/// fallible. What the type buys is that the TEXT can only come from here.
///
/// **The other half of the namespace is held elsewhere, and this is the whole of it in one place.**
/// The labels a result carries besides these are public: the answer's dimension labels, the time
/// bucket's [`TIME_BUCKET_LABEL`](crate::catalog::TIME_BUCKET_LABEL) and the measure's metric name.
/// Those are held against each other at load by
/// [`Definitions::assemble`](crate::catalog::Definitions::assemble) -
/// `DimensionShadowsTimeBucket`, `DimensionShadowsMeasure`, `TwoDimensionsOneLabel` and
/// `LabelShadowsTable`, each case-folded to the coarsest dialect rule. So a public label collides
/// with another public label at load, and cannot collide with an internal one at all. **The limit:**
/// nothing compares the two halves, because a leading digit makes the comparison unnecessary - which
/// is the property the test asserts, and the thing to re-establish if this spelling ever changes.
///
/// **Length is bounded by construction, which the scheme it replaces was not.** The identifier limit
/// is 63 characters because that is the tightest among the data systems targeted, and it is a
/// *silent* limit there: a longer alias is truncated rather than rejected, so two distinct leaf
/// columns become one. `metric__{n}` over a 63-character metric name is 66 characters, so the old
/// scheme could produce exactly that. Nothing here reads a metric's name, and the widest label a
/// `usize` can index is 27 characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InternalLabel {
    /// The column the two legs are joined on, in either leg's result.
    Link,
    /// One carried leaf of the measure, by its position in carried order.
    Leaf(usize),
}

/// The label the link column carries in both legs' results.
const LINK: &str = "0_link";

/// What one carried leaf's label carries before its position.
const LEAF: &str = "0_leaf_";

impl InternalLabel {
    /// The text this label carries in a leg's result.
    ///
    /// The one place the reserved namespace is spelled. A caller that needs it as a column name
    /// takes it from here, so no call site holds a second copy of the spelling.
    #[inline]
    pub fn label(self) -> String {
        match self {
            Self::Link => String::from(LINK),
            Self::Leaf(position) => format!("{LEAF}{position}"),
        }
    }
}
