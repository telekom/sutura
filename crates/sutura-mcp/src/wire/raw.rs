//! The raw SQL tool's wire shape: what `run_sql` takes, and what it answers.
//!
//! Its own module for the reason `wire/catalog.rs` has one - one whole tool, sharing nothing with
//! `ask`'s shapes but `sutura_domain::warehouse::Value::render`.
//!
//! # The discriminant, and why it shares nothing with a certified answer's
//!
//! `docs/adr/0013` requires that a raw result's wire shape share no serialized field name and no
//! discriminant VALUE with [`crate::wire::OutcomeContent::Answer`]'s. That type tags with
//! `outcome: "answer"` / `outcome: "refusal"`; [`RawContent`] tags with `outcome: "raw_rows"` /
//! `outcome: "raw_refusal"` - a different key set at the object's own single discriminant, so a
//! client branching on the string cannot mistake one for the other, and neither serialized object
//! carries a `provenance` or a `definition_digest` key at any depth.

use sutura_domain::raw::{RawOutcome, RawStatement};

/// One raw statement, as a tool call carries it.
///
/// One field, bounded by [`RawStatement::parse`] on the way in - `deny_unknown_fields` is what keeps
/// this tool from ever growing a second field a caller could smuggle a table name or a row-id list
/// through, the same governance boundary [`super::AskArgs`] holds for the certified tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunSqlArgs {
    /// The literal SQL statement to run, as one statement. A string carrying more than one command
    /// is not rejected here - nothing in this tool reads a keyword out of it - but the server refuses
    /// it once it reaches the data system's own parser over the extended query protocol.
    statement: String,
}

/// Why a `run_sql` call's arguments were not a statement.
#[derive(Debug, thiserror::Error)]
pub enum MalformedStatement {
    #[error("the arguments are not a statement")]
    NotAnObject {
        #[source]
        cause: serde_json::Error,
    },
    #[error("`statement` is not a statement this deployment will attempt")]
    Statement {
        #[source]
        cause: sutura_domain::raw::InvalidRawStatement,
    },
}

impl TryFrom<RunSqlArgs> for RawStatement {
    type Error = MalformedStatement;

    fn try_from(args: RunSqlArgs) -> Result<Self, Self::Error> {
        Self::parse(args.statement).map_err(|cause| MalformedStatement::Statement { cause })
    }
}

/// What the raw SQL tool produced, as the tool's structured content.
///
/// **The load-bearing shape.** No field here is named `provenance`, `definition_version` or
/// `definition_digest`, at any depth - there is nowhere on this type to put one, which is what makes
/// a raw answer unable to be rendered as certified rather than merely undecorated as one.
///
/// **The two variant NAMES deliberately do not carry a `Raw` prefix** (`clippy::enum_variant_names`
/// over the type's own already-`Raw`-prefixed name) - only their SERIALIZED tags do, pinned by an
/// explicit `#[serde(rename)]` on each rather than derived from the Rust identifier: `Rows` would
/// otherwise serialize `outcome: "rows"` and `Refusal` would serialize exactly the certified path's
/// own `outcome: "refusal"` - the one collision `docs/adr/0013` forbids.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "outcome")]
pub enum RawContent {
    /// The statement executed.
    #[serde(rename = "raw_rows")]
    Rows {
        columns: Vec<String>,
        /// Every cell as text, through the same renderer the certified path uses - not a shared
        /// wire type, a shared FUNCTION: `sutura_domain::warehouse::Value::render` takes a value and
        /// returns a string, and both tools happen to call it.
        rows: Vec<Vec<String>>,
    },
    /// The statement was refused. Still an `Ok` and still a tool result, for
    /// [`crate::server::AgentSurface::call_tool`]'s reason: a governance outcome is not a fault.
    #[serde(rename = "raw_refusal")]
    Refusal { code: &'static str, detail: String },
}

impl From<&RawOutcome> for RawContent {
    fn from(outcome: &RawOutcome) -> Self {
        match *outcome {
            RawOutcome::Rows { ref columns, ref rows } => Self::Rows {
                columns: columns.clone(),
                rows: rows.clone(),
            },
            RawOutcome::Refusal { ref reason } => Self::Refusal {
                code: reason.code(),
                detail: reason.to_string(),
            },
        }
    }
}

impl RawContent {
    /// The same outcome as text, for the content block beside the structured one -
    /// [`crate::wire::OutcomeContent::as_text`]'s reason, restated for this tool: most clients render
    /// only content blocks.
    ///
    /// Escaped exactly as the certified path's cells are: a raw result's cells come from the same
    /// data system, so the same forgery - a tab opening a column, a newline opening a row - is the
    /// same defect here. `docs/adr/0022` Decision 3's own quoting for this tool's FAILURE text is not
    /// yet built (PR2); what is built is that no variant here carries the driver's own words at all -
    /// see `sutura_domain::raw::RawRefusalReason`'s own documentation for why that is not a gap
    /// being papered over.
    pub(crate) fn as_text(&self) -> String {
        match *self {
            Self::Rows { ref columns, ref rows } => {
                let mut out = String::new();
                out.push_str(
                    &columns
                        .iter()
                        .map(|label| super::escaped(label))
                        .collect::<Vec<_>>()
                        .join("\t"),
                );
                for row in rows {
                    out.push('\n');
                    out.push_str(
                        &row.iter()
                            .map(|cell| super::escaped(cell))
                            .collect::<Vec<String>>()
                            .join("\t"),
                    );
                }
                out
            }
            Self::Refusal { code, ref detail } => format!("refused ({code}): {detail}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::raw::{RawOutcome, RawRefusalReason};

    use super::RawContent;

    #[test]
    fn a_raw_answer_shares_no_discriminant_or_field_with_a_certified_one() {
        let content = RawContent::from(&RawOutcome::Rows {
            columns: vec![String::from("n")],
            rows: vec![vec![String::from("1")]],
        });
        let rendered = serde_json::to_string(&content).expect("a raw content serializes");
        assert!(rendered.contains(r#""outcome":"raw_rows""#), "{rendered}");
        assert!(!rendered.contains(r#""outcome":"answer""#), "{rendered}");
        assert!(!rendered.contains("provenance"), "{rendered}");
        assert!(!rendered.contains("definition_digest"), "{rendered}");
        assert!(!rendered.contains("definition_version"), "{rendered}");
    }

    #[test]
    fn a_refusal_serializes_with_its_code_and_no_caller_text() {
        let content = RawContent::from(&RawOutcome::Refusal {
            reason: RawRefusalReason::TooManyRows { limit: 10_000 },
        });
        let rendered = serde_json::to_string(&content).expect("a raw refusal serializes");
        assert!(rendered.contains(r#""outcome":"raw_refusal""#), "{rendered}");
        assert!(rendered.contains(r#""code":"too_many_rows""#), "{rendered}");
    }
}
