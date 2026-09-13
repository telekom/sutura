//! The raw SQL tool's own outcome, refusal vocabulary and statement newtype.
//!
//! # Why this is a separate module, and not a widening of [`crate::query`]
//!
//! `docs/adr/0013` names the mechanism this module exists to be: the certified answer
//! ([`crate::query::ToolOutcome::Answer`]) carries a [`crate::pinned::Provenance`] with no
//! constructor that omits it, so a raw result that could ever be mistaken for one would have to
//! reuse that type. [`RawOutcome`] is a different type instead - **no field of type `Provenance`
//! anywhere in this module**, so labelling a raw answer as certified is unrepresentable rather than
//! merely undone by a rule somebody remembers to apply.
//!
//! [`RawRefusalReason`] is its own vocabulary for the same reason
//! `docs/adr/0013`'s amendment gives: [`crate::query::RefusalReason`] is keyed to a compiled plan -
//! dimensions, grains, federation - and a raw statement has none of those to refuse. What it can be
//! refused for is a bound this deployment applies before or after execution, or the data system's own
//! answer about the statement, and this module's four variants are exactly that list.
//!
//! # What this module does not decide
//!
//! **Neither variant carries the data system's own error text.** `docs/adr/0022` Decision 3 - not yet
//! built, because the raw tool did not exist when that record was written - requires the raw tool's
//! failure text to be treated as untrusted content once it is rendered; until then, the closed enum
//! here is what keeps a driver's `Display` from reaching a caller unquoted. A `String` field would
//! have been the driver's own words; naming the SHAPE of the failure instead is what makes "never
//! echoed" a property of the type rather than a discipline at every call site.

use core::fmt;

/// The largest raw statement this deployment will parse, in bytes.
///
/// Bound at the edge, before anything does work proportional to it - the same argument
/// `crate::knowledge::MAX_NOTE_BODY_BYTES` is bound for, and the reason is identical: an unbounded
/// input is a denial-of-service primitive whatever else it is. 64 KiB is generous for a statement a
/// person or an agent composes by hand and small next to the row and volume bounds that apply to
/// what it returns.
pub const MAX_RAW_STATEMENT_BYTES: usize = 64 * 1024;

/// One statement, as a caller sent it: unparsed text, bounded and non-empty.
///
/// **This is not a SQL type.** Nothing here reads a keyword out of the text - `docs/adr/0013`'s own
/// rule against inspecting a statement to decide read-only applies to every other purpose a parser
/// might be tempted for, and this newtype's whole job is bounding the edge, not understanding the
/// middle. Sutura hands the bytes to the data system's own parser unexamined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawStatement(String);

/// Why text sent as a raw statement was refused before it ever reached a data system.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRawStatement {
    /// Nothing a data system could run: empty, or made entirely of whitespace.
    #[error("a raw statement must not be empty")]
    Empty,
    /// Over [`MAX_RAW_STATEMENT_BYTES`]. Refused rather than truncated: a statement cut at the byte
    /// bound is not the statement the caller sent, and running part of it would answer a different
    /// question under the caller's own name.
    #[error("a raw statement may be at most {limit} bytes, this one has {len}")]
    TooLong { len: usize, limit: usize },
    /// Contains an embedded NUL byte, which no text-protocol statement can carry - `tokio-postgres`
    /// itself refuses one at the wire. Named here, rather than left to surface as a driver error,
    /// because a bound this deployment can decide before opening a connection should not wait for one.
    #[error("a raw statement may not contain a NUL byte")]
    EmbeddedNul,
}

impl RawStatement {
    /// Parses a statement, rejecting anything this deployment will not even attempt to run.
    ///
    /// Trimmed the way `NoteBody::parse` trims prose, so leading and trailing whitespace an editor or
    /// a chat client added is not counted against the byte bound and cannot itself make an otherwise
    /// empty statement look non-empty.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidRawStatement> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidRawStatement::Empty);
        }
        if trimmed.len() > MAX_RAW_STATEMENT_BYTES {
            return Err(InvalidRawStatement::TooLong {
                len: trimmed.len(),
                limit: MAX_RAW_STATEMENT_BYTES,
            });
        }
        if trimmed.contains('\0') {
            return Err(InvalidRawStatement::EmbeddedNul);
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The statement text, for the one adapter that runs it and for an audit record.
    ///
    /// **Never handed to a renderer for a caller-facing message.** This is the field `docs/adr/0013`'s
    /// consequences call "an audit-only field never returned to the caller" - the accessor exists for
    /// `crate::audit::CallRecord::of_raw` and for the execution port, not for a wire type to echo back.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a raw statement was refused, or its execution did not produce rows a caller may see.
///
/// **Closed, and deliberately narrower than [`crate::query::RefusalReason`].** That vocabulary is
/// keyed to a compiled plan - a metric, a grain, a federation shape - and a raw statement has none of
/// those. What can still refuse it is a bound this deployment applies, or the data system's own
/// answer about the statement once it ran.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum RawRefusalReason {
    /// The result had more rows than this deployment's row cap.
    ///
    /// Refused rather than truncated, the same argument `RefusalReason::ResultTooLarge` makes: a
    /// result cut at the cap is a different, smaller number under the caller's own statement, and
    /// nothing downstream could tell that it was cut.
    TooManyRows { limit: u32 },
    /// The data system would not hand this result back in one piece.
    ///
    /// Carries no number, for the reason `crate::query::ResultBound::Volume` carries none: the bound
    /// belongs to the data system, and a figure borrowed from wherever the statement failed would be
    /// a certified-looking number for a bound that is not the one that fired.
    ResultTooLarge,
    /// The statement did not complete: a syntax error the data system's own parser found, a
    /// constraint it enforced, or the connection's own statement timeout firing before it returned.
    ///
    /// **No text from the driver is carried**, and that is the whole point of the variant rather than
    /// a field left unfilled. Whoever controls the statement controls part of the message a database
    /// returns about it - `docs/adr/0013`'s own accounting of what this tool spends - so until
    /// `docs/adr/0022` Decision 3's quoting exists, nothing here forwards the data system's own words.
    /// An operator reads the driver's complaint from the log line this refusal is built from, not
    /// from the field.
    StatementFailed,
    /// The data system refused the statement at the identity or authorization level: a write inside
    /// the read-only transaction sutura wraps every call in, a role lacking a grant the statement
    /// needed, or a row-level policy denying it.
    ///
    /// **Distinguished from [`Self::StatementFailed`] on purpose**, the same split
    /// `RefusalReason::SourceRefused` draws against a plan's own execution failure: this is the data
    /// system saying no about WHO asked and what they may do, not a malformed statement or a timeout.
    /// `docs/adr/0013`'s amendment is explicit that the read-only transaction is a real, server-
    /// enforced boundary for statement-shaped writes and not for a VOLATILE function's own side
    /// effects - see that record for the limit stated with the claim.
    SourceRefused,
}

impl RawRefusalReason {
    /// The machine-readable code a client or an agent branches on.
    ///
    /// The same derivation rule `RefusalReason::code` uses - the `snake_case` spelling of the
    /// variant's own name - kept as a hand-written match rather than shared with that type, because
    /// the two enums do not share a caller: nothing converts one into the other, and a shared
    /// derivation function would be a coupling this module's whole reason for existing argues against.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TooManyRows { .. } => "too_many_rows",
            Self::ResultTooLarge => "result_too_large",
            Self::StatementFailed => "statement_failed",
            Self::SourceRefused => "source_refused",
        }
    }
}

impl fmt::Display for RawRefusalReason {
    /// A sentence with no caller text in it, matching what [`Self::code`] already promises: every
    /// variant is safe to log and safe to show an agent, because none of them carries anything the
    /// statement's author wrote.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyRows { limit } => write!(f, "the result had more than {limit} rows; ask a narrower question"),
            Self::ResultTooLarge => write!(
                f,
                "the data system would not return this result in one piece; ask a narrower question"
            ),
            Self::StatementFailed => write!(f, "the statement did not complete"),
            Self::SourceRefused => write!(f, "the data system refused this statement"),
        }
    }
}

/// What the raw SQL tool produced.
///
/// **The load-bearing type in `docs/adr/0013`.** Compare its shape with
/// [`crate::query::ToolOutcome::Answer`], which carries a `Provenance` with no constructor that omits
/// it: `RawOutcome` has no field of that type anywhere in this module, so a raw result cannot be
/// rendered as certified by filling in a digest - there is nowhere to put one. A `compile_fail`
/// doctest on this type, paired with a compiling twin, is what keeps that a property of the type
/// rather than a claim in this comment: see `crate::raw` module tests.
///
/// The two variants deliberately do not mirror `ToolOutcome`'s field names: `Rows` rather than
/// `Answer`, so a wire type built by matching on both cannot pattern-match its way to identical
/// output.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum RawOutcome {
    /// The statement executed and produced this result.
    ///
    /// Rendered the same way `crate::warehouse::RowSet` renders a certified answer's cells - every
    /// value as text - but there is no `RowSet` here and no shared constructor with one: the raw path
    /// builds this straight from the driver's own columns, so nothing about the certified answer's
    /// row-set type can leak a field this variant does not declare.
    Rows { columns: Vec<String>, rows: Vec<Vec<String>> },
    /// The statement was refused, before or after it reached the data system.
    Refusal { reason: RawRefusalReason },
}

impl RawOutcome {
    /// The refusal reason, if this is one. Convenience for tests and for an audit sink, matching
    /// `ToolOutcome::refusal`'s own shape.
    #[inline]
    #[must_use]
    pub const fn refusal(&self) -> Option<&RawRefusalReason> {
        match self {
            Self::Refusal { reason } => Some(reason),
            Self::Rows { .. } => None,
        }
    }

    /// How many rows this outcome carries, for an audit record. `0` for a refusal - nothing ran, or
    /// nothing came back.
    #[inline]
    #[must_use]
    pub const fn row_count(&self) -> usize {
        match self {
            Self::Rows { rows, .. } => rows.len(),
            Self::Refusal { .. } => 0,
        }
    }
}

/// # `RawOutcome` has no field for a `Provenance`
///
/// ```compile_fail
/// use sutura_domain::raw::RawOutcome;
///
/// let _ = RawOutcome::Rows {
///     provenance: (),
///     columns: Vec::new(),
///     rows: Vec::new(),
/// };
/// ```
///
/// The compiling twin, so the failure above is provably about the extra field and not about a typo
/// in the snippet:
///
/// ```
/// use sutura_domain::raw::RawOutcome;
///
/// let _ = RawOutcome::Rows {
///     columns: Vec::new(),
///     rows: Vec::new(),
/// };
/// ```
mod compile_fail_has_no_provenance {}

#[cfg(test)]
mod tests {
    use super::{InvalidRawStatement, RawOutcome, RawRefusalReason, RawStatement};

    #[test]
    fn a_statement_is_trimmed_and_bounded() {
        assert_eq!(RawStatement::parse("  select 1  ").expect("in bounds").as_str(), "select 1");
        assert_eq!(RawStatement::parse("   \n\t  "), Err(InvalidRawStatement::Empty));
        assert_eq!(RawStatement::parse(""), Err(InvalidRawStatement::Empty));
        let over = "a".repeat(super::MAX_RAW_STATEMENT_BYTES + 1);
        assert_eq!(
            RawStatement::parse(&over),
            Err(InvalidRawStatement::TooLong {
                len: over.len(),
                limit: super::MAX_RAW_STATEMENT_BYTES,
            })
        );
        assert_eq!(RawStatement::parse("select 1\0"), Err(InvalidRawStatement::EmbeddedNul));
    }

    /// THE property this module exists for: serializing a raw answer and a certified one over the
    /// same rows shares no key and no value at the discriminant, and neither the top-level JSON nor
    /// any nested object names a `provenance` or a `definition_digest`.
    ///
    /// Asserted on the SERIALIZED JSON key set directly, per `docs/143-plan.md`'s own mutation
    /// substitute: a typed comparison could not see a mutation that added a `provenance: null` field,
    /// because a `null` still fails "no key present" for an unrelated reason. Reading the parsed
    /// object's own keys is what a `null` cannot hide from.
    #[test]
    fn a_raw_answer_carries_no_definition_digest_and_shares_no_discriminant_with_a_certified_one() {
        let raw = RawOutcome::Rows {
            columns: vec![String::from("n")],
            rows: vec![vec![String::from("1")]],
        };
        let value = serde_json::to_value(&raw).expect("a raw outcome serializes");
        let rendered = serde_json::to_string(&value).expect("a raw outcome round-trips through JSON text");
        assert!(!rendered.contains("provenance"), "{rendered}");
        assert!(!rendered.contains("definition_digest"), "{rendered}");
        assert!(!rendered.contains("definition_version"), "{rendered}");
        // The externally-tagged shape puts the variant name at the object's own single key, so this
        // is the discriminant a certified `ToolOutcome::Answer` and this type would have to share for
        // one to be mistaken for the other - and they do not: that type's key is `Answer`, never `Rows`.
        assert!(rendered.starts_with(r#"{"Rows":"#), "{rendered}");
        assert!(!rendered.contains(r#""Answer""#), "{rendered}");
    }

    #[test]
    fn a_refusal_carries_no_row_and_a_row_count_of_zero() {
        let refusal = RawOutcome::Refusal {
            reason: RawRefusalReason::SourceRefused,
        };
        assert_eq!(refusal.row_count(), 0);
        assert_eq!(refusal.refusal(), Some(&RawRefusalReason::SourceRefused));
        let rows = RawOutcome::Rows {
            columns: Vec::new(),
            rows: vec![vec![]],
        };
        assert_eq!(rows.refusal(), None);
    }

    #[test]
    fn every_refusal_reason_has_a_stable_code_and_a_sentence_with_no_caller_text() {
        for reason in [
            RawRefusalReason::TooManyRows { limit: 10_000 },
            RawRefusalReason::ResultTooLarge,
            RawRefusalReason::StatementFailed,
            RawRefusalReason::SourceRefused,
        ] {
            assert_ne!(reason.code(), "");
            assert_ne!(reason.to_string(), "");
        }
        assert_eq!(RawRefusalReason::TooManyRows { limit: 1 }.code(), "too_many_rows");
        assert_eq!(RawRefusalReason::ResultTooLarge.code(), "result_too_large");
        assert_eq!(RawRefusalReason::StatementFailed.code(), "statement_failed");
        assert_eq!(RawRefusalReason::SourceRefused.code(), "source_refused");
    }
}
