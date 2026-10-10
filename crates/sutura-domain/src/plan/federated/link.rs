//! What equality over a federated link column means, and how a catalog's declared type says it.
//!
//! One vocabulary for both halves of the check: the combiner reads a [`LinkKind`] off each leg's
//! Arrow schema once the legs have run, and the splitter reads one off the catalog's declared
//! [`ColumnType`](crate::catalog::ColumnType) before either leg does. The two agree on what a kind
//! is because they share this type, so there is no second list of kinds to keep in step.

use crate::catalog::ColumnType;

/// What equality over a link column means, once a type that cannot carry one exactly is refused.
///
/// **Kinds rather than types, and the reason is that the interior's own row builder may give two
/// legs different types for one logical column.** A leg whose link values all fit an `i64` comes
/// back `Int64`; one whose column mixes a fitting value with exact integral text comes back
/// `Decimal128(38, 0)`. Both are exact integers and `DataFusion`'s comparison coercion joins them
/// correctly, so requiring the two types to be EQUAL would refuse a legal pair.
///
/// What it must still refuse is a join across kinds - `telekom/sutura#138`: an integer column
/// against a text column misses on every row by construction, and the hand-written combine returned
/// that silently as an empty inner answer or a left answer whose every fact row had a null remote
/// side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// Any width of integer, or a decimal whose scale is zero. Equality is exact.
    ExactInteger,
    /// A decimal with digits after the point. Equality is exact, and a text rendering of the same
    /// value is NOT this kind - which is the mismatch the pair check refuses.
    ExactDecimal,
    /// Text of any width.
    Text,
    /// A day number.
    Date,
    /// A boolean.
    Boolean,
}

impl LinkKind {
    /// The word this kind reads as in a message an operator reads.
    ///
    /// Prose rather than a type name, for the reason `crate::warehouse::arrow`'s refusals give the
    /// opposite way round: an Arrow type is a driver's metadata and belongs in the combiner's own
    /// error, while what reaches a caller through
    /// [`FederatedAnswerRefusal`](super::FederatedAnswerRefusal) is a bare discriminant.
    pub const fn word(self) -> &'static str {
        match self {
            Self::ExactInteger => "an exact integer",
            Self::ExactDecimal => "an exact decimal",
            Self::Text => "text",
            Self::Date => "a date",
            Self::Boolean => "a boolean",
        }
    }

    /// The kind a source's own spelling of a column type declares, or `None` for one this does not
    /// know - which DEFERS to the combiner's check over the legs' real schemas, never refuses.
    ///
    /// **Decided from the name before any parameter list, case-folded**, so `bigint`, `INT64` and
    /// `Int64` agree and `VARCHAR(255)`, `varchar(max)` and `VARCHAR2(10 BYTE)` are all text. A
    /// decimal is told apart by its scale and only when the scale is written: `NUMERIC(38, 0)` is
    /// an integer exactly as a zero-scale Arrow decimal is, `NUMERIC(38, 9)` is a decimal, and a
    /// bare `NUMERIC` or `NUMERIC(38)` is `None` because the spelling alone does not say which.
    /// Anything else - a float, a timestamp, a nested or wrapped type - is `None`.
    ///
    /// **A parameter list is not read, only closed.** `varchar(...)` is text whatever sits inside
    /// the parentheses, so the text inside is exactly as hostile as the catalog that supplied it;
    /// nothing here returns it, and [`QuotedColumnType`] is what lets a refusal carry the type.
    ///
    /// This is the one place the catalog's declared type is BRANCHED on. It reads the type to
    /// refuse a join, and never to cast, render or execute anything.
    pub fn declared(declared: &ColumnType) -> Option<Self> {
        let lowered = declared.as_str().to_ascii_lowercase();
        let (name, parameters) = match lowered.split_once('(') {
            Some((name, rest)) => (name.trim_end(), Some(rest.strip_suffix(')')?)),
            None => (lowered.as_str(), None),
        };
        match name {
            "smallint" | "int" | "integer" | "bigint" | "int2" | "int4" | "int8" | "int16" | "int32" | "int64" | "uint8"
            | "uint16" | "uint32" | "uint64" => Some(Self::ExactInteger),
            "numeric" | "decimal" => parameters.and_then(Self::of_scale),
            "string" | "text" | "varchar" | "varchar2" | "nvarchar" | "nvarchar2" | "char" | "nchar" | "character"
            | "character varying" => Some(Self::Text),
            "date" => Some(Self::Date),
            "boolean" | "bool" => Some(Self::Boolean),
            _ => None,
        }
    }

    /// A decimal's kind from the `precision, scale` list it was written with.
    fn of_scale(parameters: &str) -> Option<Self> {
        let (_, scale) = parameters.split_once(',')?;
        match scale.trim().parse::<u32>().ok()? {
            0 => Some(Self::ExactInteger),
            _ => Some(Self::ExactDecimal),
        }
    }
}

/// A declared column type, quoted and escaped so a refusal may carry it.
///
/// **A source's own dictionary supplied the text**, a catalog adapter read it off a platform nobody
/// here controls, and a refusal reaches a log, a person and an agent's context. So it is never
/// carried raw: the only way to build one is [`From<&ColumnType>`], which writes it as a quoted
/// literal with every quote, backslash and non-printing character escaped, and the field cannot be
/// reached any other way. The quoting is the standard library's `{:?}` over a string, the one
/// [`InvalidColumnType`](crate::catalog::InvalidColumnType) already gives a value in its own
/// messages.
///
/// **What this does and does not bound.** A quoted type cannot close its own quotes or break a line,
/// so it stays one delimited span inside a sentence. It does not make the words inside it benign: a
/// type may still read like an instruction to a model, and is at most
/// [`MAX_COLUMN_TYPE_CHARS`](crate::catalog::MAX_COLUMN_TYPE_CHARS) characters before escaping.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QuotedColumnType(String);

impl From<&ColumnType> for QuotedColumnType {
    fn from(declared: &ColumnType) -> Self {
        Self(format!("{:?}", declared.as_str()))
    }
}

impl core::fmt::Display for QuotedColumnType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}
