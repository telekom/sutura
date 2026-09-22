//! What the two legs' schemas have to say before a plan is built, and the refusals they carry.
//!
//! **Every check here reads a TYPE and no value, which is the half of the combine that got stronger
//! when `docs/adr/0039` step 3 moved it onto Arrow.** The hand-written combine decided a link
//! column's kind from *the first non-null cell it happened to find*, so two legs whose link columns
//! could never match were refused only when the data proved it - and two EMPTY legs were answered.
//! An Arrow column has one declared type, so the same refusals are decided from the schema, before
//! a batch is read, for every input including an empty one.
//!
//! It is its own module for `crate::translate`'s reason: `combine.rs` builds a plan, this reads a
//! schema, and the two share no state.

use datafusion::arrow::datatypes::{DataType, SchemaRef};

use crate::combine::CombineError;

/// Which side a refusal is about, as the word a message carries.
pub(crate) const FACT: &str = "fact";

/// The lookup leg, as the word a message carries.
pub(crate) const LOOKUP: &str = "lookup";

/// What equality over a link column means, once a type that cannot carry one exactly is refused.
///
/// **Kinds rather than types, and the reason is that the interior's own row builder may give two
/// legs different types for one logical column.** A leg whose link values all fit an `i64` comes
/// back `Int64`; one whose column mixes a fitting value with exact integral text comes back
/// `Decimal128(38, 0)`. Both are exact integers and `DataFusion`'s comparison coercion joins them
/// correctly, so requiring the two schemas to be EQUAL would refuse a legal pair.
///
/// What it must still refuse is a join across kinds - `telekom/sutura#138`: an integer column
/// against a text column misses on every row by construction, and the hand-written combine returned
/// that silently as an empty inner answer or a left answer whose every fact row had a null remote
/// side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkKind {
    /// Any width of integer, or a decimal whose scale is zero. Equality is exact.
    ExactInteger,
    /// A decimal with digits after the point. Equality is exact, and a text rendering of the same
    /// value is NOT this kind - which is the mismatch the pair check refuses.
    ExactDecimal,
    /// Text of any Arrow width.
    Text,
    /// A day number.
    Date,
    /// A boolean.
    Boolean,
}

impl LinkKind {
    /// The word this kind reads as in a refusal a caller sees.
    ///
    /// Prose rather than the Arrow type name, for the reason
    /// `sutura_domain::warehouse::arrow`'s refusals give the opposite way round: an Arrow type is a
    /// driver's metadata and belongs in this crate's own error, while what reaches a caller through
    /// [`FederatedAnswerRefusal`](sutura_domain::plan::FederatedAnswerRefusal) is a bare
    /// discriminant. These words are for the operator reading the typed cause.
    pub(crate) const fn word(self) -> &'static str {
        match self {
            Self::ExactInteger => "an exact integer",
            Self::ExactDecimal => "an exact decimal",
            Self::Text => "text",
            Self::Date => "a date",
            Self::Boolean => "a boolean",
        }
    }
}

/// How a column may be re-aggregated, once a type no total or comparison is exact over is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeafKind {
    /// An integer or a decimal, summed through a 256-bit accumulator at the column's own scale so
    /// no total this workspace can produce wraps it. `scale` is the column's own.
    Exact { scale: i8 },
    /// A 64-bit float, summed as one - the same float addition the mono path's own `SUM` performs.
    Float,
}

/// One leg's schema, with every label the plan names resolved and every type it needs judged.
///
/// **Resolved once, before anything is built**, so a wiring defect between the splitter and this
/// combiner is a named refusal rather than a `DataFusion` plan that would not analyse.
pub(crate) struct LegSchema {
    side: &'static str,
    schema: SchemaRef,
}

impl LegSchema {
    /// One leg's schema, refusing a result that labels two columns the same.
    ///
    /// A duplicate label is the one shape a combine cannot disambiguate - two leaf columns under one
    /// name, or a key colliding with a leaf - so it is caught at the boundary rather than allowed to
    /// answer a wrong number. `RecordBatch` permits it: Arrow validates a batch positionally and
    /// never reads a field name against its siblings.
    pub(crate) fn of(side: &'static str, schema: &SchemaRef) -> Result<Self, CombineError> {
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for field in schema.fields() {
            if !seen.insert(field.name().as_str()) {
                return Err(CombineError::DuplicateLabels {
                    side,
                    label: field.name().clone(),
                });
            }
        }
        Ok(Self {
            side,
            schema: SchemaRef::clone(schema),
        })
    }

    /// The Arrow type of the column this leg carries under `label`.
    pub(crate) fn kind_of(&self, label: &str) -> Result<&DataType, CombineError> {
        self.schema
            .fields()
            .iter()
            .find(|field| field.name() == label)
            .map(|field| field.data_type())
            .ok_or_else(|| CombineError::MissingColumn {
                side: self.side,
                label: String::from(label),
            })
    }

    /// Asserts this leg projects `label` at all, for a column nothing here has to type.
    pub(crate) fn projects(&self, label: &str) -> Result<(), CombineError> {
        self.kind_of(label).map(|_kind| ())
    }

    /// How this leg's link column joins, or the refusal its type carries.
    pub(crate) fn link_kind(&self, label: &str) -> Result<LinkKind, CombineError> {
        let kind = self.kind_of(label)?;
        link_kind(kind).ok_or_else(|| classify_unjoinable(self.side, label, kind))
    }

    /// How one carried leaf re-aggregates, or the refusal its type carries.
    pub(crate) fn leaf_kind(&self, label: &str) -> Result<LeafKind, CombineError> {
        let kind = self.kind_of(label)?;
        leaf_kind(kind).ok_or_else(|| CombineError::NonNumericLeaf {
            label: String::from(label),
            arrow_type: format!("{kind:?}"),
        })
    }
}

/// The two legs' link kinds, refused unless they are the same kind.
///
/// **Checked before either leg's rows are read, which is what makes an empty leg answerable.** The
/// hand-written combine asked the data: with no non-null cell on a side it decided nothing and
/// joined anyway, so a pair that could never match was answered as *no rows* rather than refused.
pub(crate) fn agreeing_link(fact: LinkKind, lookup: LinkKind) -> Result<(), CombineError> {
    if fact == lookup {
        return Ok(());
    }
    Err(CombineError::LinkTypeMismatch {
        fact: fact.word(),
        lookup: lookup.word(),
    })
}

/// How a link column of this Arrow type joins, or `None` for one no exact equality can be taken
/// over.
const fn link_kind(kind: &DataType) -> Option<LinkKind> {
    match *kind {
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Decimal128(_, 0)
        | DataType::Decimal256(_, 0) => Some(LinkKind::ExactInteger),
        DataType::Decimal128(..) | DataType::Decimal256(..) => Some(LinkKind::ExactDecimal),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => Some(LinkKind::Text),
        DataType::Date32 => Some(LinkKind::Date),
        DataType::Boolean => Some(LinkKind::Boolean),
        _ => None,
    }
}

/// Which refusal a link column this combine cannot join carries.
///
/// **A float is the caller-facing one and everything else is ours**, and the split matters because
/// the two reach a caller differently. `docs/adr/0007`'s float-key rule forbids a floating-point
/// link because formatting a float into an equality lets distinct values collide - that is a fact
/// about the DATA and a refusal a caller is told. Any other unjoinable type is one
/// `ResultBatches::to_rows` maps no cell of either, so a question naming it would have failed at
/// the presentation edge whatever this combine did: it is this workspace's own wiring, and it stays
/// a typed failure rather than becoming a refusal about the question.
fn classify_unjoinable(side: &'static str, label: &str, kind: &DataType) -> CombineError {
    match *kind {
        DataType::Float16 | DataType::Float32 | DataType::Float64 => CombineError::FloatLinkKey {
            side,
            arrow_type: format!("{kind:?}"),
        },
        ref other => CombineError::LinkTypeNotMapped {
            side,
            label: String::from(label),
            arrow_type: format!("{other:?}"),
        },
    }
}

/// How a leaf column of this Arrow type re-aggregates, or `None` for one no exact total or
/// comparison can be taken over.
///
/// **Text is the `None` that matters and it is not an oversight.** A row-speaking adapter returns an
/// exact `DECIMAL` money column as text to keep it exact, and the interior's own row builder gives a
/// column of such text `Utf8`. A sum over it cannot certify a number, so it is refused rather than
/// counted as zero. A column mixing integers with exact integral TEXT is not this case: the row
/// builder gives that one `Decimal128(38, 0)`, which lands in [`LeafKind::Exact`].
///
/// A 32-bit float is refused too, for `sutura_domain::warehouse::arrow`'s own reason: widening one
/// to an `f64` makes two adapters disagree about a number neither of them got wrong.
const fn leaf_kind(kind: &DataType) -> Option<LeafKind> {
    match *kind {
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => Some(LeafKind::Exact { scale: 0 }),
        DataType::Decimal128(_, scale) | DataType::Decimal256(_, scale) => Some(LeafKind::Exact { scale }),
        DataType::Float64 => Some(LeafKind::Float),
        _ => None,
    }
}
