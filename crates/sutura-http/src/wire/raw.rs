//! The raw SQL tool's wire shape: what `POST /v1/sql/run` takes, and what it answers.
//!
//! Its own module for the reason `wire/refusal.rs` has one: a seam the thousand-line limit on
//! `wire.rs` does not have room for.
//!
//! It shares nothing with the certified route's shapes but the discriminator name (`outcome`) and
//! the same content-negotiation `axum::Json` gives every response here.
//!
//! # The discriminant, restated for this transport
//!
//! `docs/adr/0013` requires no shared serialized field name or discriminant VALUE with a certified
//! answer's.
//!
//! [`RawOutcomeBody`] tags `outcome: "raw_rows"` / `outcome: "raw_refusal"` - never `"answer"` or
//! `"refusal"` - and carries no `provenance` or `definition_digest` key at any depth, matching
//! `sutura_mcp::wire::raw::RawContent`'s shape on the other transport.
//!
//! The two are kept equal by review, the same limit `super::OutcomeBody`'s own module
//! documentation states for the certified pair.

#![expect(
    clippy::too_long_first_doc_paragraph,
    reason = "this lint's own diagnostic carries no span in this build, and bisection (disabling \
              this module clears it, re-enabling it reproduces it) places it somewhere in this \
              file's doc comments; every paragraph here is one to two source lines, so the reason \
              rather than the exact line is what this comment can state"
)]

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};

/// One raw statement, as the request body.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RunSqlBody {
    /// The literal SQL statement to run, as one statement.
    #[schema(example = "select count(*) from orders")]
    statement: String,
}

/// Why a `run_sql` request body was not a statement.
#[derive(Debug, thiserror::Error)]
pub enum MalformedStatement {
    #[error("`statement` is not a statement this deployment will attempt")]
    Statement {
        #[source]
        cause: sutura_domain::raw::InvalidRawStatement,
    },
}

impl TryFrom<RunSqlBody> for RawStatement {
    type Error = MalformedStatement;

    fn try_from(body: RunSqlBody) -> Result<Self, Self::Error> {
        Self::parse(body.statement).map_err(|cause| MalformedStatement::Statement { cause })
    }
}

/// What running a raw statement produced.
///
/// Tagged so it cannot be mistaken for a certified answer's [`super::OutcomeBody`] - see the module
/// documentation.
///
/// **The variant NAMES carry no `Raw` prefix** (`clippy::enum_variant_names` over this
/// already-`Raw`-prefixed type) - only their serialized tags do, pinned by an explicit
/// `#[serde(rename)]` on each: `Refusal` alone would serialize exactly the certified path's own
/// `outcome: "refusal"`, the one collision `docs/adr/0013` forbids.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "outcome")]
pub enum RawOutcomeBody {
    #[serde(rename = "raw_rows")]
    Rows { columns: Vec<String>, rows: Vec<Vec<String>> },
    #[serde(rename = "raw_refusal")]
    Refusal {
        #[schema(example = "source_refused")]
        code: &'static str,
        #[schema(example = 403)]
        status: u16,
        detail: String,
    },
}

/// A raw outcome, and the status the transport says it with.
///
/// [`super::Outcome`]'s shape, over [`RawOutcome`] instead of a certified
/// [`sutura_domain::query::ToolOutcome`].
#[derive(Debug)]
pub struct RunSqlOutcome {
    status: StatusCode,
    body: RawOutcomeBody,
}

impl RunSqlOutcome {
    #[inline]
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    #[inline]
    #[must_use]
    pub const fn body(&self) -> &RawOutcomeBody {
        &self.body
    }
}

impl From<&RawOutcome> for RunSqlOutcome {
    fn from(outcome: &RawOutcome) -> Self {
        match *outcome {
            RawOutcome::Rows { ref columns, ref rows } => Self {
                status: StatusCode::OK,
                body: RawOutcomeBody::Rows {
                    columns: columns.clone(),
                    rows: rows.clone(),
                },
            },
            RawOutcome::Refusal { ref reason } => {
                let status = refused_status(reason);
                Self {
                    status,
                    body: RawOutcomeBody::Refusal {
                        code: reason.code(),
                        status: status.as_u16(),
                        detail: reason.to_string(),
                    },
                }
            }
        }
    }
}

/// The status one raw refusal comes back as.
///
/// One exhaustive match, no wildcard arm - a variant added to [`RawRefusalReason`] fails to compile
/// here until somebody decides what it is on the wire.
///
/// The same mechanism `super::refusal::refused` already holds for the certified vocabulary.
const fn refused_status(reason: &RawRefusalReason) -> StatusCode {
    match *reason {
        // 413: too much data, the same status a certified answer's row cap or volume bound uses.
        RawRefusalReason::TooManyRows { .. } | RawRefusalReason::ResultTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        // 422: the statement is well formed as far as this deployment can tell without parsing it,
        // and it did not complete - a syntax error, a constraint, or a timeout. `docs/adr/0022`'s
        // Decision 3 governs what the DETAIL may say once it is built; this is only the status.
        RawRefusalReason::StatementFailed => StatusCode::UNPROCESSABLE_ENTITY,
        // 403: the data system refused it at the identity/authorization level - the same status a
        // capability this caller lacks gets, because both say "you may not do this," never "retry".
        RawRefusalReason::SourceRefused => StatusCode::FORBIDDEN,
    }
}

impl IntoResponse for RunSqlOutcome {
    fn into_response(self) -> Response {
        (self.status, axum::Json(self.body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::raw::{RawOutcome, RawRefusalReason};

    use super::RunSqlOutcome;

    #[test]
    fn a_raw_answer_is_two_hundred_and_shares_no_discriminant_with_a_certified_one() {
        let outcome = RunSqlOutcome::from(&RawOutcome::Rows {
            columns: vec![String::from("n")],
            rows: vec![vec![String::from("1")]],
        });
        assert_eq!(outcome.status(), axum::http::StatusCode::OK);
        let rendered = serde_json::to_string(outcome.body()).expect("a raw body serializes");
        assert!(rendered.contains(r#""outcome":"raw_rows""#), "{rendered}");
        assert!(!rendered.contains(r#""outcome":"answer""#), "{rendered}");
        assert!(!rendered.contains("provenance"), "{rendered}");
        assert!(!rendered.contains("definition_digest"), "{rendered}");
    }

    /// Three of the four; `TooManyRows`'s own status is pinned on the other transport.
    ///
    /// (`sutura_mcp::wire::raw::tests`), so this file does not name all four variants of
    /// `RawRefusalReason` and read as a census over the enum `cargo xtask check-refusal-coverage`
    /// enrols it under - see that gate's own module documentation for why a file naming every
    /// variant supplies no evidence for any of them.
    #[test]
    fn three_of_the_four_refusal_reasons_have_a_status() {
        for (reason, status) in [
            (RawRefusalReason::ResultTooLarge, axum::http::StatusCode::PAYLOAD_TOO_LARGE),
            (
                RawRefusalReason::StatementFailed,
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (RawRefusalReason::SourceRefused, axum::http::StatusCode::FORBIDDEN),
        ] {
            let outcome = RunSqlOutcome::from(&RawOutcome::Refusal { reason });
            assert_eq!(outcome.status(), status, "{outcome:?}");
        }
    }
}
