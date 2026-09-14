//! What a federated combine can fail with, and which of those a caller should be told to retry.
//!
//! Split out when `federated.rs` reached the unexemptable 1000-line gate, along a concept seam
//! rather than the counter's own fall: the failure vocabulary and its retry classification are one
//! subject, and `combine`'s own machinery is another. Declares no test module of its own - every
//! assertion about these two types is in `federated::tests`, which is what reaches them through
//! `FederatedPlan::combine` and `sutura_app::federated`.

use crate::model::{Aggregate, MetricName};
use crate::warehouse::Value;

/// Why a federated answer could not be assembled.
///
/// The shape failures are defects in this workspace's own wiring - a leg result missing a column
/// [`super::labels`] named, or a row narrower than its result's own columns. The [`NonFinite`](FederatedFailure::NonFinite)
/// variant is a `fails` guard meeting a zero denominator, which no divide-tree node can produce a
/// value for.
///
/// **D6: [`AmbiguousLink`](FederatedFailure::AmbiguousLink)'s `Display` does not interpolate `key`.** A join
/// key is exactly the kind of cell this workspace treats as caller data - the finding named a case
/// where it could be a customer identifier - and `Display` is what every logger and every future
/// refusal surface reads. The field stays for equality in tests; nothing here stops a future arm
/// from interpolating it instead, which is why this is held by review at any new call site rather
/// than by the compiler.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FederatedFailure {
    /// A column `combine` reached for by label was absent from a leg's result.
    ///
    /// The labelling contract is one function - the splitter and the combiner both call [`super::labels`] -
    /// so this is a wiring defect between the two halves rather than a choice either side made.
    #[error("the {side} result has no column `{label}`")]
    MissingColumn { side: &'static str, label: String },
    /// A division happened by a zero denominator while the measure declared `fails`.
    ///
    /// On the mono-source path a non-finite cell is refused at the port; this is this slice's port,
    /// so the guard landing here is an error naming the metric it could not certify.
    #[error("a non-finite value reached the answer for `{metric}`")]
    NonFinite { metric: MetricName },
    /// A leg result had two columns under one label, so the combiner could not tell which of them
    /// a leaf or key names.
    #[error("the {side} result labels two columns `{label}`")]
    DuplicateLabels { side: &'static str, label: String },
    /// A link cell carried a floating-point key, which the ADR's float-key rule forbids.
    #[error("a link column carried a floating-point key ({value})")]
    FloatLinkKey { value: f64 },
    /// A link value had more than one lookup row, which would double every measure.
    ///
    /// D6: see this enum's own header for why `Display` does not interpolate `key`.
    #[error("a link value maps to more than one lookup row")]
    AmbiguousLink { key: String },
    /// A leaf cell that was not a number reached a re-aggregating aggregate.
    ///
    /// The `DuckDB` adapter deliberately returns `DECIMAL` and wide integer columns as
    /// [`Value::Text`] to keep them exact; a sum reaching such a cell cannot certify a number, so
    /// it is refused rather than counted as zero.
    #[error("a `{aggregate:?}` re-aggregation met a non-numeric leaf cell (`{value:?}`)")]
    NonNumericLeaf { aggregate: Aggregate, value: Value },
    /// A leaf column carried two numeric types, so no total or comparison over it is exact.
    ///
    /// A result column in a data system has one logical type. [`crate::warehouse::RowSet`] constrains a row's width and
    /// nothing about its cells, so a column mixing [`Value::Integer`] and [`Value::Real`] cells is
    /// representable here, and the two ways to answer one are both wrong numbers: dropping either
    /// subtotal loses it outright, and folding the integer one into the real one is an `i64 as f64`
    /// widening - the same silent widening `DuckDB`'s own conversion refuses for a 32-bit float and
    /// for a wide integer that does not fit an `i64`. Refused instead, which is also what leaves the
    /// aggregates above comparing and adding one type.
    #[error("a `{aggregate:?}` re-aggregation met a leaf column mixing integer and real cells")]
    MixedNumericLeaf { aggregate: Aggregate },
    /// A leaf total overflowed a 64-bit integer.
    #[error("a `{aggregate:?}` re-aggregation overflowed a 64-bit integer")]
    Overflow { aggregate: Aggregate },
    /// An aggregate the combiner does not know how to re-aggregate with.
    ///
    /// The one path [`super::FederatedPlan::new`] closes is a carried leaf naming an aggregate
    /// `reaggregate::reaggregates` answers `false` for - it refuses such a federation before any
    /// leg runs, so no plan that constructor built carries this value. **The limit: nothing else
    /// closes it, and construction is not restricted to this module.** `FederatedFailure` is `pub`
    /// and re-exported, and the application's federated execution already writes a sibling
    /// variant's literal from outside the crate. So any caller can build this value directly; it
    /// stays a refusal rather than becoming a panic because a value that claims a re-aggregation
    /// which does not exist would answer wrongly, not because the type seals the variant.
    #[error("the combiner does not re-aggregate with `{aggregate:?}`")]
    UnsupportedAggregate { aggregate: Aggregate },
    /// Materialising the answer crossed the byte budget `docs/adr/0009` applies at the conversion
    /// boundary.
    ///
    /// The legs have no row cap - that measured key cardinality rather than bytes, which is exactly
    /// what 0009 retired - so this is the bound on the answer `combine` builds. A refusal is honest
    /// in the way a truncated one is not: the caller sees a `federation_not_executable`-adjacent
    /// refusal rather than a row set that stopped early.
    #[error("the federated answer exceeds the {ceiling_bytes}-byte working-set ceiling")]
    ResourcesExhausted { ceiling_bytes: u64 },
    /// A row whose width contradicts the result's own column count.
    ///
    /// Unreachable by construction on both halves: a leg result is built by [`crate::warehouse::RowSet::new`], which
    /// refuses a ragged row up front, and the answer is projected from a single fixed key list. It is
    /// this slice's defensive arm - the named, reachable-if-the-type-lying shape the old `LegCount`
    /// catch-all used to swallow.
    #[error("a row of the {side} result had the wrong number of cells")]
    MalformedRow { side: &'static str },
    /// The combine tree's cursor read past the leaves this measure's re-aggregation collected.
    ///
    /// Unreachable by construction, for [`MalformedRow`](Self::MalformedRow)'s reason applied one
    /// level up: one value is built per carried leaf and the divide tree walks the identical set of
    /// leaves in the same order, so the two counts cannot diverge.
    #[error("the combine tree for `{metric}` read past the leaves it aggregated")]
    LeafCursorExhausted { metric: MetricName },
}

/// A federated answer that could not be computed as asked, classified apart from a wiring defect.
///
/// **D19 + A4: every arm here is deterministic.** The same plan against the same data refuses
/// again, which is the opposite of what `sutura_app::ServiceError::Federated` used to mean once it
/// reached a transport: an HTTP `503`, the status a data system that might come back produces.
/// [`Self::of`] is the total function that decides which arms count - the wiring defects
/// (`MissingColumn`, `DuplicateLabels`, `UnsupportedAggregate`, `MalformedRow`,
/// `LeafCursorExhausted`) are a defect in this workspace's own splitter, not a caller's, and stay a
/// `ServiceError`; [`FederatedFailure::ResourcesExhausted`] already has its own
/// [`RefusalReason`](crate::query::RefusalReason) variant and is handled before this classification
/// runs.
///
/// **Carries no cell.** `AmbiguousLink`'s join key and `FloatLinkKey`'s value are exactly the
/// caller data this workspace never puts in a message a caller or an agent reads - see
/// [`FederatedFailure`]'s own header for D6, the case that named it. Every arm here is a bare
/// discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum FederatedAnswerRefusal {
    /// A division met a zero denominator the measure declared `fails` for, or its result was not a
    /// finite number.
    NonFinite,
    /// A link column carried a floating-point key, which the ADR's float-key rule forbids.
    FloatLinkKey,
    /// A link value mapped to more than one lookup row.
    AmbiguousLink,
    /// A leaf cell reached a re-aggregating aggregate that is not the numeric type it needs.
    NonNumericLeaf,
    /// A leaf column mixed integer and real cells, so no total or comparison over it is exact.
    MixedNumericLeaf,
    /// A leaf total overflowed a 64-bit integer.
    Overflow,
}

impl FederatedAnswerRefusal {
    /// Classifies `cause` as this deterministic refusal, or `None` for the wiring defects this
    /// workspace still answers for as a `ServiceError`.
    ///
    /// Exhaustive with no wildcard arm, so a ninth [`FederatedFailure`] variant has to say which
    /// side of the split it is on before this compiles.
    #[must_use]
    pub const fn of(cause: &FederatedFailure) -> Option<Self> {
        match *cause {
            FederatedFailure::NonFinite { .. } => Some(Self::NonFinite),
            FederatedFailure::FloatLinkKey { .. } => Some(Self::FloatLinkKey),
            FederatedFailure::AmbiguousLink { .. } => Some(Self::AmbiguousLink),
            FederatedFailure::NonNumericLeaf { .. } => Some(Self::NonNumericLeaf),
            FederatedFailure::MixedNumericLeaf { .. } => Some(Self::MixedNumericLeaf),
            FederatedFailure::Overflow { .. } => Some(Self::Overflow),
            FederatedFailure::MissingColumn { .. }
            | FederatedFailure::DuplicateLabels { .. }
            | FederatedFailure::UnsupportedAggregate { .. }
            | FederatedFailure::ResourcesExhausted { .. }
            | FederatedFailure::MalformedRow { .. }
            | FederatedFailure::LeafCursorExhausted { .. } => None,
        }
    }
}
