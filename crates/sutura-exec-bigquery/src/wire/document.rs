//! The two documents this adapter exchanges with the endpoint, and the code that builds and reads
//! them.
//!
//! **Split out of `wire.rs` when that file crossed 1000 lines**, and the cut is at a real seam rather
//! than at a line count: everything here is about the SHAPE of a request and an answer, and nothing
//! here opens a socket, holds a credential or reads a clock. That is also what makes it the half a
//! test can exercise - `super::tests` asserts on the SERIALIZED body and over answer documents, which
//! is the whole of what this repository can prove without a project.
//!
//! Every decision these types carry is argued in `wire.rs`'s own header, which is the one place to
//! read them together; what is written here is why each field is the way it is.

use sutura_domain::warehouse::ParamValue;

use crate::transport::{Cell, Field, FieldType, JobRequest, JobRows, ParameterMode};
use crate::wire::{Grid, HOST, JobBounds, ReasonCode, WireError, Wired};

/// One request body, as the endpoint's `QueryRequest` spells it.
///
/// Borrowed where it can be: it is built per call, serialized once, and dropped.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QueryBody<'job> {
    /// The statement, with its values still absent from it.
    query: &'job str,
    /// `false`, always, and written rather than defaulted - the endpoint's own default is `true`, and
    /// everything this crate renders is `GoogleSQL`. [`JobRequest::USE_LEGACY_SQL`] is where the
    /// decision lives.
    use_legacy_sql: bool,
    /// `POSITIONAL`, from [`JobRequest::PARAMETER_MODE`]. A query uses one parameter form or the
    /// other and not both.
    parameter_mode: &'static str,
    /// The values, in the order the statement's placeholders take them. Entries carry no `name`,
    /// which is what makes them positional.
    query_parameters: Vec<Parameter>,
    /// Where an unqualified table name resolves, which is why nothing rendered needs qualifying.
    default_dataset: Dataset<'job>,
    /// Whether this validates or runs.
    dry_run: bool,
    /// How long the CLIENT waits for the answer.
    ///
    /// **On its own this bounds nothing at the service**, which is the correction worth carrying: the
    /// endpoint documents it as how long to wait, and an expired one returns `jobComplete: false`
    /// with a `jobReference` while **the job keeps running and keeps billing**. Set equal to
    /// [`Self::job_timeout_ms`] so the client stops waiting at the same instant the job is cancelled,
    /// rather than abandoning a live job the way an earlier version of this code did.
    timeout_ms: u64,
    /// How long the JOB may run before the service cancels it.
    ///
    /// This is the field that is actually a deadline. It is the same number as
    /// [`Self::timeout_ms`] and they are two fields because they bound two different things.
    job_timeout_ms: u64,
    /// The most this job may be billed for scanning, as text.
    ///
    /// A job that would exceed it fails and is not charged - which is why this is the bound rather
    /// than a client-side comparison against a dry run's estimate. See
    /// [`BytesBilledCeiling`](crate::wire::BytesBilledCeiling).
    maximum_bytes_billed: String,
    /// `false`. See the module header: an anchor that reproduces from a cache has reproduced the
    /// cache.
    use_query_cache: bool,
}

/// One bind parameter, with no name.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Parameter {
    parameter_type: ParameterType,
    parameter_value: ParameterValue,
}

/// A parameter's declared type.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterType {
    #[serde(rename = "type")]
    kind: &'static str,
}

/// A parameter's value, as text.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterValue {
    value: String,
}

/// The dataset an unqualified table name resolves in.
///
/// **No `projectId`, and its absence is load-bearing rather than an omission.** An omitted project
/// resolves in the request's own project, which is the billing one, so this can only describe a
/// dataset inside the project paying for the job - the limit `SourcePlacement::BigQuery` already
/// states and `docs/adr/0017` records.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Dataset<'job> {
    dataset_id: &'job str,
}

/// The endpoint's answer.
///
/// **No `deny_unknown_fields`, and that is the deliberate exception to a rule this repository applies
/// to every shape it owns.** The rule is about documents *this repository defines*, where an
/// undeclared field is an author's mistake worth naming. This document is defined by the service:
/// it already carries `kind`, `jobReference`, `cacheHit`, `totalBytesProcessed` and more that
/// nothing here reads, and it will carry more later. Refusing an answer the day it grows a field
/// would be a gate that fails on a correct input.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QueryAnswer {
    #[serde(default)]
    job_complete: bool,
    #[serde(default)]
    schema: Option<Schema>,
    /// A 64-bit count, written as a JSON string by the service.
    #[serde(default)]
    total_rows: Option<String>,
    #[serde(default)]
    pub(super) rows: Vec<AnswerRow>,
    #[serde(default)]
    page_token: Option<String>,
    #[serde(default)]
    errors: Vec<ErrorItem>,
}

/// The columns the statement projected.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Schema {
    #[serde(default)]
    fields: Vec<SchemaField>,
}

/// One column, as the endpoint described it.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchemaField {
    #[serde(default)]
    name: String,
    /// The endpoint's own spelling, decoded by [`FieldType::parse`] - which is where the legacy
    /// names a query response uses become the modern vocabulary this adapter maps.
    #[serde(rename = "type", default)]
    kind: String,
}

/// One row.
#[derive(Debug, serde::Deserialize)]
pub(super) struct AnswerRow {
    #[serde(default)]
    f: Vec<AnswerCell>,
}

/// One cell. `v` is a JSON string, a null, or - for a column outside this adapter's vocabulary -
/// something else, which is refused as [`WireError::NotAScalar`].
#[derive(Debug, serde::Deserialize)]
struct AnswerCell {
    #[serde(default)]
    v: serde_json::Value,
}

/// The envelope a refusal arrives in.
#[derive(Debug, Default, serde::Deserialize)]
struct RefusalEnvelope {
    #[serde(default)]
    error: Option<RefusalBody>,
}

/// A refusal's body.
///
/// **`message` IS read, bounded, and that is a reversal a live run forced.** It was deliberately
/// absent - the argument being that what is not read cannot be logged by accident - and then the first
/// real submission came back `400 invalidQuery` with no way to tell WHICH construct the service
/// disliked. A status and a reason code that together say *your SQL is wrong* are not actionable, and
/// `docs/adr/0017` had already measured that this dialect's risk is exactly the construct a parse
/// check cannot see.
///
/// So the message is kept through [`detail`], which caps it and strips anything that could forge a log
/// line. The original concern was unbounded foreign text reaching a log, and a bound answers it -
/// dropping the field answered it by removing the diagnostic too.
#[derive(Debug, serde::Deserialize)]
struct RefusalBody {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    errors: Vec<ErrorItem>,
}

/// One reported error. The provider's text is mapped to [`ReasonCode`] at the error boundary; the
/// message beside it is not read.
#[derive(Debug, serde::Deserialize)]
struct ErrorItem {
    #[serde(default)]
    reason: Option<String>,
}

/// The endpoint's free-text message, bounded and stripped of anything that could forge a log line.
///
/// **A bound rather than an absence**, for the reason [`RefusalBody`] gives: the first live submission
/// returned `400 invalidQuery`, and the reason code alone did not say which construct was wrong.
///
/// What is removed is every control character and everything outside printable ASCII, so a service's
/// answer cannot write a newline, an escape sequence or a right-to-left override into a log; what
/// survives is the sentence. Bounded by CHARACTERS rather than bytes, because `clippy::string_slice` is
/// denied and a byte slice of foreign text can land inside a character.
fn detail(message: Option<String>) -> super::EndpointMessage {
    // The cap and the strip moved onto `EndpointMessage::bounded` when the message became a type:
    // what may be RENDERED and what may be STORED are one rule, and it belongs with the value.
    super::EndpointMessage::bounded(message)
}

/// The refusal a non-success status becomes.
///
/// Best-effort on the document and exact on the status: a body that is not the envelope leaves
/// `named` empty rather than failing, because the status alone is already the answer an operator
/// acts on.
pub(super) fn refusal<C>(status: u16, body: &str) -> WireError<C>
where
    C: core::error::Error + 'static,
{
    let envelope: RefusalEnvelope = serde_json::from_str(body).unwrap_or_default();
    let (named, detail) = envelope.error.map_or_else(
        || (ReasonCode::Absent, super::EndpointMessage::bounded(None)),
        |error| {
            // The per-error `reason` first, because it is the specific one; the envelope's `status` is
            // the coarse fallback. Both are mapped to the closed local vocabulary before rendering.
            let first = error.errors.iter().find_map(|item| item.reason.as_deref());
            (
                ReasonCode::from_provider(first.or(error.status.as_deref())),
                detail(error.message),
            )
        },
    );
    WireError::Refused { status, named, detail }
}

/// The request body one job becomes.
///
/// **A free function rather than a method, and that is what makes the request assertable.** Nothing
/// here needs a transport, a credential or a socket, so what this adapter puts on the wire is exactly
/// what a test can read - which is the whole of what the suite beside this module can prove.
pub(super) fn body<'job>(
    request: &'job JobRequest<'job>,
    dry_run: super::DryRun,
    bounds: JobBounds,
    left: core::time::Duration,
) -> QueryBody<'job> {
    // **What is LEFT of this call's budget, not the whole of it**, which is the correction
    // `super::CallDeadline` carries: the token exchange happens first and spends part of it, and a job
    // asked to run for the full budget after that would outlive the caller by whatever the exchange
    // cost. Saturating into `u64` because `Duration::as_millis` is a `u128`; the value is bounded by
    // `QueryDeadline::MAX_SECONDS` long before it could get near either limit.
    let deadline = u64::try_from(left.as_millis()).unwrap_or(u64::MAX);
    QueryBody {
        query: request.statement(),
        use_legacy_sql: JobRequest::USE_LEGACY_SQL,
        parameter_mode: match JobRequest::PARAMETER_MODE {
            ParameterMode::Positional => "POSITIONAL",
        },
        query_parameters: request.params().iter().map(parameter).collect(),
        default_dataset: Dataset {
            dataset_id: request.default_dataset().as_str(),
        },
        dry_run: dry_run.asked(),
        // The two are one number on purpose - see the field documentation. The client stops waiting
        // exactly when the service cancels, so there is no window in which a job outlives the request
        // that asked for it.
        timeout_ms: deadline,
        job_timeout_ms: deadline,
        maximum_bytes_billed: bounds.max_bytes_billed().as_text(),
        use_query_cache: false,
    }
}

/// One bind parameter, typed as the endpoint needs it.
///
/// An exhaustive match over [`ParamValue`], so a third domain value cannot be sent untyped: the
/// compiler asks here. A date travels as its ISO text, which is what `DATE` takes and what
/// `sutura-exec-duckdb` binds too.
fn parameter(value: &ParamValue) -> Parameter {
    let (kind, text) = match *value {
        ParamValue::Text(ref text) => ("STRING", text.clone()),
        ParamValue::Date(date) => ("DATE", date.to_iso()),
    };
    Parameter {
        parameter_type: ParameterType { kind },
        parameter_value: ParameterValue { value: text },
    }
}

/// The URL a job is submitted to.
///
/// **Interpolated with no escaping, and the reason it is not an injection is that the value cannot be
/// anything else:** `ProjectId::parse` accepts `[a-z0-9-]` and nothing more, so no `/`, `?`, `#`, `%`,
/// whitespace or non-ASCII character can be in it. That check lives in `crate::transport`
/// deliberately - *a check belongs where the risk is*, and this line is the risk.
pub(super) fn url(request: &JobRequest<'_>) -> String {
    format!("{HOST}/bigquery/v2/projects/{}/queries", request.billing_project().as_str())
}

/// The reason the endpoint reported against a job, mapped to the closed vocabulary or absent.
///
/// **Read as a DIAGNOSTIC and never as a verdict.** `QueryResponse.errors` is documented as *"the
/// first errors or warnings encountered"*, with the explicit note that entries *"do not necessarily
/// mean that the job has completed or was unsuccessful"* - so a non-empty array is not a failure and
/// an earlier version of this module was wrong to refuse on it. What it is good for is saying WHY,
/// once one of the shape checks in `complete` has already decided that something is wrong.
pub(super) fn reported(answer: &QueryAnswer) -> ReasonCode {
    answer
        .errors
        .iter()
        .find_map(|item| item.reason.as_deref())
        .map_or(ReasonCode::Absent, |named| ReasonCode::from_provider(Some(named)))
}

/// The rows one complete job produced, or the reason they are not a complete result.
///
/// **Failure is derived from the shape and the reported reason is folded in**, which is the reversal
/// this function exists to carry. The order is the cheapest refusal first, which here is also the
/// clearest: *the job did not finish* before *there is more of it* before *it did not say how much of
/// it there is*.
///
/// **The limit, stated because it is an absence and absences read as "there were none":** a job that
/// IS complete and carries a warning is answered, and the warning goes nowhere. `RowSet` has no field
/// for it and this crate has no logging dependency; adding one for a line nobody has ever seen is a
/// dependency decision this change does not take.
pub(super) fn complete<C>(answer: QueryAnswer) -> Wired<JobRows, C>
where
    C: core::error::Error + 'static,
{
    let named = reported(&answer);
    if !answer.job_complete {
        return Err(WireError::NotComplete { named });
    }
    if answer.page_token.is_some() {
        return Err(WireError::MoreThanOnePage);
    }
    let Some(total) = answer.total_rows else {
        return Err(WireError::NoTotal { named });
    };
    let total: usize = total.parse().map_err(|cause| WireError::NotATotal { cause })?;
    let columns = columns(answer.schema, answer.rows.len())?;
    let rows = cells(answer.rows)?;
    Ok(JobRows::of(columns, rows, total))
}

/// That one job which produces no result set finished, or the reason it did not.
///
/// **One check, and the two it deliberately omits are the point.** `totalRows` and the schema are
/// what [`complete`] reads, and a `CREATE OR REPLACE TABLE` job has no result set for either to be
/// about - requiring them here would refuse a load that worked, on a response shape this repository
/// has not measured. `pageToken` is omitted for the same reason: there are no rows to page.
///
/// What is NOT omitted is `jobComplete`. A statement whose job has not finished has not necessarily
/// created anything, and answering `Ok` to that is the failure mode this function exists to prevent:
/// a half-loaded fixture whose corpus run then disagrees with the engine for a reason that looks like
/// a dialect bug.
#[cfg(feature = "fixtures")]
pub(super) fn applied<C>(answer: &QueryAnswer) -> Wired<(), C>
where
    C: core::error::Error + 'static,
{
    if answer.job_complete {
        Ok(())
    } else {
        Err(WireError::NotComplete { named: reported(answer) })
    }
}

/// The schema, as the vocabulary this adapter maps.
///
/// A complete job with rows and no schema is refused: there is nothing to read the cells against. A
/// complete job with neither is an empty result, and an empty column list is what that is.
pub(super) fn columns<C>(schema: Option<Schema>, delivered: usize) -> Wired<Vec<Field>, C>
where
    C: core::error::Error + 'static,
{
    match schema {
        Some(schema) => Ok(schema
            .fields
            .into_iter()
            .map(|field| Field::of(field.name, FieldType::parse(&field.kind)))
            .collect()),
        None if delivered == 0 => Ok(Vec::new()),
        None => Err(WireError::NoSchema { rows: delivered }),
    }
}

/// The cells, as the seam's own text-or-nothing shape.
///
/// **Nothing is typed here, and that is the seam's decision rather than laziness.** Every scalar
/// arrives as JSON text whatever its declared type, and `BigQueryWarehouse`'s own `cell` is where text
/// becomes a domain value - so the fake transport and this one cannot disagree about what a `NUMERIC`
/// is.
pub(super) fn cells<C>(rows: Vec<AnswerRow>) -> Wired<Grid, C>
where
    C: core::error::Error + 'static,
{
    let mut out: Grid = Vec::with_capacity(rows.len());
    for (row, delivered) in rows.into_iter().enumerate() {
        let mut built: Vec<Cell> = Vec::with_capacity(delivered.f.len());
        for (column, cell) in delivered.f.into_iter().enumerate() {
            built.push(match cell.v {
                serde_json::Value::Null => Cell::Null,
                serde_json::Value::String(text) => Cell::Text(text),
                // An array or an object is a `REPEATED` or a `RECORD`, which `FieldType` has no
                // variant for; a bare number or a bare bool is a shape the endpoint does not document
                // for a cell. Refused NAMING the position, rather than flattened into text that would
                // go on to parse as something.
                serde_json::Value::Bool(_)
                | serde_json::Value::Number(_)
                | serde_json::Value::Array(_)
                | serde_json::Value::Object(_) => {
                    return Err(WireError::NotAScalar { row, column });
                }
            });
        }
        out.push(built);
    }
    Ok(out)
}
