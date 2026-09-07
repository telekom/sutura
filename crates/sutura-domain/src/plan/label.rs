//! What a result column may be labelled, and the four things a label can be derived from.

use crate::catalog::TIME_BUCKET_LABEL;
use crate::model::{DimensionName, MetricName};
use crate::plan::federated::InternalLabel;

/// The label one column of a result carries, which can only be built out of something already
/// parsed.
///
/// **The half of the labelling scheme that used to be held by review.** A federated leg's result
/// carries two kinds of label in one namespace. The internal kind is
/// [`InternalLabel`](crate::plan::InternalLabel), a type: every rendering starts with a digit, and
/// `crate::model`'s identifier parser refuses a leading digit as a FIRST character, so no
/// [`DimensionName`], [`MetricName`], `ColumnName` or `TableName` can spell one. The public kind was
/// a `String` on [`PlanKey`](crate::plan::PlanKey) and [`LegTerm`](crate::plan::LegTerm), and what
/// kept a public label out of the internal namespace was that `sutura_semantic::plan` happened to
/// derive every one of them from a dimension name, a metric name or
/// [`TIME_BUCKET_LABEL`](crate::catalog::TIME_BUCKET_LABEL) - a derivation held by review, and by no
/// test. `telekom/sutura#337` is the report.
///
/// This type is the other half. There is no constructor taking text, so the four functions below are
/// the whole of what a label can come from, and a computed string is not one of them. That turns
/// *nothing compares the two halves* into *nothing can put a value in both*, which is the stronger
/// version of the same argument and the one the leading digit was chosen to support.
///
/// **Why a newtype over the rendering rather than the four-variant enum the report sketched.** An
/// enum would have to hand out its text, and `Internal(InternalLabel::Leaf(n))` has no `&'static
/// str` rendering to hand out - the position is formatted - so `label()` would return a
/// [`Cow`](std::borrow::Cow) and the five alias call sites in `sutura_sql::generate` would change
/// with it. Measured, not assumed: `aliased(inner: Expr, label: &str)` is called five times there,
/// once per projected column shape. So the rendering is stored and the four constructors are the
/// gate. **The limit, next to the claim:** which of the four a label came from is not recoverable
/// from the value, because nothing reads it back - what the type buys is that the TEXT can only come
/// from one of them.
///
/// A computed string is not a label, and that is a compile error rather than a review finding:
///
/// ```compile_fail
/// use sutura_domain::model::{ColumnName, TableName};
/// use sutura_domain::plan::{PlanColumn, PlanKey};
///
/// // The failure `telekom/sutura#325`'s F2 reproduced: a public key labelled with a computed
/// // string that lands in the internal namespace.
/// fn _computed(table: TableName, column: ColumnName, leaf: usize) -> PlanKey {
///     PlanKey::new(format!("0_leaf_{leaf}"), PlanColumn::new(table, column))
/// }
/// ```
///
/// And the twin, so a rename cannot make that block pass vacuously:
///
/// ```
/// use sutura_domain::model::{ColumnName, DimensionName, TableName};
/// use sutura_domain::plan::{PlanColumn, PlanKey, ResultLabel};
///
/// fn _parsed(table: TableName, column: ColumnName, dimension: &DimensionName) -> PlanKey {
///     PlanKey::new(ResultLabel::dimension(dimension), PlanColumn::new(table, column))
/// }
/// ```
///
/// **It serializes as the bare string it renders**, so the plan goldens are unchanged by this type
/// existing: a plan's serialized form is what a snapshot pins, and a wrapper visible in it would be
/// a diff about a Rust type rather than about what we decided to execute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct ResultLabel(String);

impl ResultLabel {
    /// The label a group-by key carries: the dimension's own certified name.
    #[inline]
    pub fn dimension(name: &DimensionName) -> Self {
        Self(String::from(name.as_str()))
    }

    /// The label the truncated time column carries, which is one constant for every plan.
    ///
    /// No argument, because there is nothing to choose:
    /// [`TIME_BUCKET_LABEL`](crate::catalog::TIME_BUCKET_LABEL) is the one spelling, and
    /// [`Definitions::assemble`](crate::catalog::Definitions) refuses a dimension that shadows it.
    #[inline]
    pub fn bucket() -> Self {
        Self(String::from(TIME_BUCKET_LABEL))
    }

    /// The label the answer's measure carries: the metric's own certified name.
    #[inline]
    pub fn measure(metric: &MetricName) -> Self {
        Self(String::from(metric.as_str()))
    }

    /// A label in the namespace no question can name.
    ///
    /// The one way into that half, and it takes the type rather than its text - so the reserved
    /// spelling still lives in exactly one place.
    #[inline]
    pub fn internal(label: InternalLabel) -> Self {
        Self(label.label())
    }

    /// The text this label carries in a result.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
