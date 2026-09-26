//! The deterministic refusals a federated answer can carry, and nothing about how one is computed.
//!
//! **This module used to hold both halves and now holds one.** `FederatedFailure` - thirteen
//! variants describing how `FederatedPlan::combine`'s own row walk could fail - is gone with the
//! function: `docs/adr/0039` step 3 replaced the combine with a `DataFusion` plan in an adapter, so
//! the failure vocabulary belongs to that implementor's own error type and the port's two
//! predicates are what the domain may ask about it
//! ([`FederationCombiner`](super::FederationCombiner)).
//!
//! What stays is the CALLER-FACING classification, which is the half that was never the combine's:
//! [`FederatedAnswerRefusal`] rides to a caller inside
//! [`RefusalReason::FederatedAnswerNotWellFormed`](crate::query::RefusalReason::FederatedAnswerNotWellFormed),
//! and a refusal vocabulary an adapter could mint freely is exactly what
//! [`Warehouse::working_set_exhausted`](crate::warehouse::Warehouse::working_set_exhausted)'s own
//! *a predicate rather than a conversion* argument exists to prevent.
//!
//! Declares no test module of its own: every assertion about this type is where a combiner produces
//! one.

/// A federated answer that could not be computed as asked, told apart from a wiring defect.
///
/// **Every arm here is deterministic.** The same plan against the same legs refuses again, which is
/// the opposite of what a data-system failure means to a transport: a caller told `503` retries,
/// and a retry against any of these returns the same refusal. The implementor's own error type is
/// where the detail lives; this is what a caller is told.
///
/// **Carries no cell.** A join key and a leaf value are exactly the caller data this workspace
/// never puts in a message a caller or an agent reads - `telekom/sutura#929`'s D6 named the case, a
/// join key that could be a customer identifier - so every arm is a bare discriminant. The
/// combiner's own error may carry an Arrow TYPE, which is a driver's metadata rather than anybody's
/// data.
///
/// # Two arms were removed with the hand-written combine, and both for the same kind of reason
///
/// `MixedNumericLeaf` refused a leaf column carrying both integer and real cells, because
/// [`RowSet`](crate::warehouse::RowSet) constrains a row's width and nothing about its cells. **An
/// Arrow column has ONE type**, so the shape is not representable at the port any more and an arm
/// for it would be a refusal nothing can provoke.
///
/// `Overflow` refused a leaf total that crossed an `i64`. That refusal lost the answer, and the
/// combiner does not need it: it sums an exact integral leaf as a 256-bit decimal, and
/// [`ResultBatches::to_rows`](crate::warehouse::ResultBatches::to_rows) widens a zero-scale value
/// that fits an `i64` back to an integer and renders one that does not as its exact text. So a
/// total past `i64::MAX` comes back **exact** where it used to be refused. **The limit that
/// replaces it, stated where the arm was:** `DataFusion`'s own sum accumulator adds with wrapping
/// arithmetic (`add_wrapping`, measured in the pinned 55.1.0 source, and its upstream documentation
/// says an overflow wraps rather than erroring), so the bound is the accumulator's width and not a
/// refusal. A 256-bit accumulator over leaf values a `Decimal128` column can hold needs on the
/// order of `10^38` rows to wrap, which no row ceiling in this workspace permits - but it is a
/// width, not a guard, and a combiner that narrowed the accumulator would silently lose that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum FederatedAnswerRefusal {
    /// The measure is not a finite number: a division met a zero denominator the definition
    /// declared `fails` for, or the arithmetic left the finite range.
    NonFinite,
    /// A link column carries a floating-point type, which `docs/adr/0007`'s float-key rule forbids:
    /// formatting a float into an equality lets distinct values collide.
    FloatLinkKey,
    /// A link value maps to more than one lookup row, or to more than one second-fact row in one
    /// bucket, which would double every measure under it.
    AmbiguousLink,
    /// The two legs' link columns carry types that can never match, so every row on both legs
    /// misses by construction - `telekom/sutura#138`. Silently, before this refusal existed: an
    /// empty inner answer, or a left answer whose every fact row survived with a null remote side.
    LinkTypeMismatch,
    /// A carried leaf's column is not a numeric type, so no total or comparison over it is exact.
    ///
    /// A row-speaking adapter returns an exact `DECIMAL` money column as text to keep it exact, and
    /// a sum over such a column cannot certify a number - so it is refused rather than counted as
    /// zero. A column mixing integers with exact integral text is NOT this case: the interior's own
    /// row builder gives it a zero-scale decimal type, which is numeric and exact.
    NonNumericLeaf,
}
