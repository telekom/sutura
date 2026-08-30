//! The WIRE: one [`JobTransport`] that speaks to a `BigQuery` endpoint over HTTP.
//!
//! **This is the seam `docs/adr/0017` left open, filled in by the decision `docs/adr/0018` records.**
//! Behind the crate's default-off `wire` feature, because what arrives with it is an outbound TLS
//! stack and two of the four release triples are musl; that manifest argument is on the `ureq` entry
//! in the workspace root and is not repeated here.
//!
//! # What this module claims, and what it does not
//!
//! **It has never been run against a real project by anybody in this repository, and nothing here
//! pretends otherwise.** There is no `BigQuery` in a container - `docs/adr/0017` refuses an emulator
//! on principle rather than on cost, because it is the option that produces the most
//! confident-looking green - so what the suite beside this module proves is that *this code builds
//! the request it says it builds and reads the answer it says it reads*, over documents that are not
//! the service's. That is a real property and it is smaller than acceptance. The acceptance leg is
//! `crates/sutura-exec-bigquery/tests/acceptance.rs`, it is `#[ignore]`d, it needs a project a
//! developer names in their own environment, and **it is unexecuted.**
//!
//! So: still *Built And Not Wired* in AGENTS.md, one line further along. What moved is that a
//! developer with a project can now try it; what did not move is that `sutura-serve` links no
//! `BigQuery` adapter and refuses `kind: bigquery` by name, and the `data_systems:` axis of the
//! golden matrix still gains no entry - a cell that has never executed reads as coverage.
//!
//! # The four things this module decides
//!
//! - **One page or a refusal.** `jobs.query` answers one page, and completeness is stated as
//!   `totalRows` beside the rows rather than by the rows alone. A `pageToken`, an incomplete job or a
//!   delivered count short of the reported total is refused here - see [`WireError::MoreThanOnePage`]
//!   and [`WireError::NotComplete`] - because to `answer()` a first page would read as *under the
//!   cap, not truncated*, which is the exact row the row-cap invariant exists to hold.
//! - **The service's own result cache is turned OFF.** Not for cost: an anchor that reproduces from a
//!   cache has reproduced the cache, which is `differential.rs`'s own argument. And a cached answer
//!   under a *shared* identity is shared across every asker, so leaving it on would put the
//!   cross-user leak this crate refuses one layer below the code the per-subject step has to change.
//! - **The bearer never travels to a second host.** `max_redirects` is `0`, so there is no redirect
//!   for a credential to follow. That is stronger than the client's own default, which is to strip
//!   the header on redirect: here there is nothing to strip.
//! - **Every foreign string that reaches an error is bounded and filtered.** The endpoint's
//!   `reason` is kept and its free-text `message` is not, because a reason is a fixed vocabulary an
//!   operator can act on and a message is unbounded text from another service heading for a log.
//!
//! # What is deliberately absent
//!
//! - **Paging.** A result bigger than one page is refused rather than assembled. `getQueryResults`
//!   needs the job's `location` for a dataset outside the two multi-regions, and
//!   `SourcePlacement::BigQuery` declares no `location` - `docs/adr/0017` says why that field is not
//!   in this repository yet and that the change adding the wire is the one that decides it. **This
//!   change decides it by not needing it**, and the cost is the refusal above.
//! - **A `location` on the request.** Same reason, one size smaller.
//! - **Retries.** A refused job comes back as [`WireError`] and reaches a caller as
//!   `BigQueryError::Endpoint`, whose transport-facing status is a `503`. Retrying inside an adapter
//!   would spend a caller's request timeout on a decision the caller cannot see.

use core::time::Duration;

use sutura_domain::warehouse::ParamValue;

use crate::transport::{Cell, Field, FieldType, JobRequest, JobRows, JobTransport, ParameterMode};
use crate::wire::credential::AccessTokens;

pub mod credential;

#[cfg(test)]
mod tests;

/// The API this module speaks to. A compile-time constant: there is no configuration key for it, so
/// no deployment can point this at a host of its choosing.
const HOST: &str = "https://bigquery.googleapis.com";

/// How long a job may take at the endpoint before it answers *not complete*.
///
/// **An associated constant of this module rather than a per-request argument, for the reason
/// [`JobRequest::PARAMETER_MODE`] is one:** it is a decision this adapter makes, not one a question
/// carries. The value is chosen against what the refusal costs - an incomplete job is refused rather
/// than polled, so this is effectively the query timeout for this data system, and it wants to be
/// comfortably longer than a question and comfortably shorter than a caller's patience.
const JOB_TIMEOUT_MS: u64 = 55_000;

/// The whole request, including the body. Longer than [`JOB_TIMEOUT_MS`] so the endpoint's own
/// timeout is what expires first and the failure names the job rather than the socket.
const GLOBAL_TIMEOUT: Duration = Duration::from_secs(70);

/// A cap on the answer this module will read into memory.
///
/// One page of `jobs.query` is capped by the endpoint at about ten megabytes, so this is above what
/// it will send and below what would matter; what it actually defends is the case where something
/// that is not the endpoint answers.
const MAX_ANSWER_BYTES: u64 = 32 * 1024 * 1024;

/// A cap on response headers, which are read before any body.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// How much of the endpoint's `reason` is kept. `accessDenied` is twelve characters.
const MAX_REASON_CHARS: usize = 64;

/// The client every request in this crate goes through.
///
/// **Public, and handed to both the transport and the credential source**, so the two share one
/// connection pool and one set of the decisions below - which is what makes *the redirect policy is
/// set once* true rather than true twice.
///
/// Every non-default setting is a decision:
///
/// Plain backticks below rather than intra-doc links, because this function is public and the two
/// items it names are private - `api-docs` reports that pairing as a warning, and the safe form for
/// it is the same one `AGENTS.md` prescribes for a cross-crate reference.
///
/// - `http_status_as_error(false)`, because the client's default turns a `4xx` into an error and
///   discards the body - and the body is where the endpoint says *which* refusal this is. Status is
///   read explicitly instead, in `refusal`.
/// - `https_only(true)`, so a bearer token cannot leave over plaintext even if a URL somewhere loses
///   its scheme. `HOST` is already `https`; this is the second lock.
/// - `max_redirects(0)`, so the credential has no second host to reach. See the module header.
/// - `timeout_global`, so a job cannot hold a pool thread open indefinitely.
/// - `max_response_header_size`, because headers are read before the body's own limit applies.
#[must_use]
pub fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .max_redirects(0)
            .timeout_global(Some(GLOBAL_TIMEOUT))
            .max_response_header_size(MAX_HEADER_BYTES)
            .build(),
    )
}

/// One fallible step of the wire.
///
/// Named for the reason `crate::Mapped` is named: `Result<V, WireError<C::Error>>` is over the
/// `type_complexity` threshold this workspace tightened, and erasing the generic would lose which
/// credential source failed.
type Wired<V, C> = Result<V, WireError<C>>;

/// The cells one page delivered, in the shape [`JobRows::of`] takes them.
///
/// Named for the same reason [`Wired`] is: `Wired<Vec<Vec<Cell>>, C>` is over the threshold, and this
/// is the one place in the crate where the nesting is unavoidable - a page is rows of cells.
type Grid = Vec<Vec<Cell>>;

/// Whether a submission reads data or only validates.
///
/// **An enum rather than a `bool`, because `submit(request, true)` at a call site says nothing.** The
/// two calls have different costs - one is billed and one is documented as using no slots and not
/// being charged - which is exactly the distinction a reader needs at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DryRun {
    /// Validate only. Not billed, no slots, no rows.
    Yes,
    /// Run the job.
    No,
}

impl DryRun {
    /// The flag as the request body writes it.
    const fn asked(self) -> bool {
        matches!(self, Self::Yes)
    }
}

/// Why the endpoint did not answer with rows.
///
/// Generic in the credential source's own error, for the reason [`crate::BigQueryError`] is generic
/// in this one: a caller that knows which credential source is installed can still tell a missing
/// file from a refused refresh, and erasing it here would be the information this whole chain of
/// generics exists to keep.
///
/// **`ureq::Error` appears as a `#[source]` and never as a variant this type re-exports**, which is
/// the shape *Structured Errors* asks for at a boundary: the variant is ours, the chain still walks,
/// and a caller who knows the transport can downcast. It is boxed because it is much larger than
/// every other variant and `clippy::result_large_err` is on.
#[derive(Debug, thiserror::Error)]
pub enum WireError<C>
where
    C: core::error::Error + 'static,
{
    /// No token could be produced, so nothing was sent.
    #[error("no credential was available to submit the job with")]
    Credential {
        #[source]
        cause: C,
    },
    /// A token was produced and its deadline had already passed.
    ///
    /// **Checked here rather than trusted, and it is the one check that would be pointless if
    /// anything were cached.** A source that hands back an expired token is a source with a clock
    /// problem, and presenting it turns that into a `401` from the data system - which reads to an
    /// operator as a permissions fault.
    #[error("the credential expired {at} seconds after the epoch, and it is now {now}")]
    Expired { at: u64, now: u64 },
    /// This process could not read a wall clock.
    ///
    /// Reachable only on a machine whose clock is before the epoch. It is a variant rather than a
    /// fallback because the alternative is presenting a token whose deadline nothing compared.
    #[error("this process could not read the time, so no credential deadline could be compared")]
    NoClock {
        #[source]
        cause: std::time::SystemTimeError,
    },
    /// The request could not be serialized.
    ///
    /// **A defect-only path, and it is named rather than unwrapped.** Everything in the body is a
    /// string, a bool or a number, so nothing here can fail the serializer; the variant exists
    /// because `unwrap` is denied and a silent `unwrap_or_default` would send a different query.
    #[error("the request body could not be serialized, which is a defect in this adapter")]
    RequestNotSerializable {
        #[source]
        cause: serde_json::Error,
    },
    /// The endpoint was not reached.
    #[error("the endpoint was not reached")]
    Unreachable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The endpoint answered and the answer could not be read.
    #[error("the endpoint's answer could not be read")]
    Unreadable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The endpoint refused.
    ///
    /// The status and the endpoint's own `reason`, and deliberately not its message - see the module
    /// header. An absent or unparseable error document leaves `named` empty, which is honest: the
    /// status is what is guaranteed.
    #[error("the endpoint refused the job with {status}: {named}")]
    Refused { status: u16, named: String },
    /// The answer was not the document a query response is.
    #[error("the endpoint's answer was not a query response")]
    NotADocument {
        #[source]
        cause: serde_json::Error,
    },
    /// The endpoint accepted the job and reported errors against it.
    ///
    /// A `200` carrying `errors` is a job that ran and did not work. Distinct from
    /// [`Self::Refused`], because the statement was accepted and the fault is at execution.
    #[error("the endpoint accepted the job and reported an error against it: {named}")]
    Failed { named: String },
    /// The job had not finished when the endpoint answered.
    ///
    /// **Refused rather than polled.** The alternative is `getQueryResults`, which needs a `location`
    /// this deployment does not declare - see the module header - and a partial answer is a wrong
    /// number under a certified name.
    #[error("the job had not finished when the endpoint answered")]
    NotComplete,
    /// The answer is one page of more than one.
    #[error("the endpoint answered with one page of a larger result")]
    MoreThanOnePage,
    /// A complete job that stated no total.
    ///
    /// **Refused rather than read as zero**, because zero is what a complete empty result and a
    /// missing field both look like, and only one of them is an answer this adapter may certify.
    #[error("the endpoint reported the job complete and stated no total row count")]
    NoTotal,
    /// The total was not a number.
    ///
    /// It arrives as text, because the endpoint writes 64-bit integers as JSON strings.
    #[error("the endpoint's total row count was not a number")]
    NotATotal {
        #[source]
        cause: core::num::ParseIntError,
    },
    /// A complete job with rows and no schema to read them against.
    #[error("the endpoint returned {rows} rows and no schema")]
    NoSchema { rows: usize },
    /// A cell that is neither a string nor a null.
    ///
    /// Every scalar the endpoint returns is JSON text whatever its declared type; an array or an
    /// object is a `REPEATED` or `RECORD` column, which is outside [`FieldType`]'s closed
    /// vocabulary. It names the position rather than the value, because the value is a row.
    #[error("the cell at row {row}, column {column} is not a scalar")]
    NotAScalar { row: usize, column: usize },
}

/// One request body, as the endpoint's `QueryRequest` spells it.
///
/// Borrowed throughout: it is built per call, serialized once, and dropped.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryBody<'job> {
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
    /// The endpoint's own timeout for the job.
    timeout_ms: u64,
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
struct QueryAnswer {
    #[serde(default)]
    job_complete: bool,
    #[serde(default)]
    schema: Option<Schema>,
    /// A 64-bit count, written as a JSON string by the service.
    #[serde(default)]
    total_rows: Option<String>,
    #[serde(default)]
    rows: Vec<AnswerRow>,
    #[serde(default)]
    page_token: Option<String>,
    #[serde(default)]
    errors: Vec<ErrorItem>,
}

/// The columns the statement projected.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Schema {
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
struct AnswerRow {
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

/// A refusal's body. Its `message` is deliberately not a field here: what is not read cannot be
/// logged by accident.
#[derive(Debug, serde::Deserialize)]
struct RefusalBody {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    errors: Vec<ErrorItem>,
}

/// One reported error. `reason` is a fixed vocabulary; the message beside it is not read.
#[derive(Debug, serde::Deserialize)]
struct ErrorItem {
    #[serde(default)]
    reason: Option<String>,
}

/// The endpoint's short reason for a refusal, bounded and filtered.
///
/// **Not a slice**, because `clippy::string_slice` is denied and because a byte slice of foreign text
/// can land inside a multi-byte character. Filtering to the character set a reason is spelled in is
/// what keeps a service's answer from writing anything else into a log.
fn reason(named: Option<String>) -> String {
    named
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .take(MAX_REASON_CHARS)
        .collect()
}

/// The refusal a non-success status becomes.
///
/// Best-effort on the document and exact on the status: a body that is not the envelope leaves
/// `named` empty rather than failing, because the status alone is already the answer an operator
/// acts on.
fn refusal<C>(status: u16, body: &str) -> WireError<C>
where
    C: core::error::Error + 'static,
{
    let envelope: RefusalEnvelope = serde_json::from_str(body).unwrap_or_default();
    let named = envelope.error.map_or_else(String::new, |error| {
        // The per-error `reason` first, because it is the specific one; the envelope's `status` is
        // the coarse fallback and is also a fixed vocabulary.
        let first = error.errors.into_iter().find_map(|item| item.reason);
        reason(first.or(error.status))
    });
    WireError::Refused { status, named }
}

/// A `BigQuery` endpoint, reached over HTTP.
///
/// Generic in its credential source rather than holding a boxed one, for the reason
/// [`crate::BigQueryWarehouse`] is generic in its transport: there is one per process, it is chosen
/// at composition, and a generic keeps the source's own error type visible in [`WireError`].
#[derive(Debug)]
pub struct BigQueryWire<C> {
    agent: ureq::Agent,
    credentials: C,
}

/// The request body one job becomes.
///
/// **A free function rather than a method, and that is what makes the request assertable.** Nothing
/// here needs a transport, a credential or a socket, so what this adapter puts on the wire is exactly
/// what a test can read - which is the whole of what the suite beside this module can prove.
fn body<'job>(request: &'job JobRequest<'job>, dry_run: DryRun) -> QueryBody<'job> {
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
        timeout_ms: JOB_TIMEOUT_MS,
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
fn url(request: &JobRequest<'_>) -> String {
    format!("{HOST}/bigquery/v2/projects/{}/queries", request.billing_project().as_str())
}

/// The answer, once it is established that the endpoint reported no error against the job.
///
/// **A `200` carrying `errors` is a job that ran and did not work**, which is not the same fact as a
/// refused statement - so it is checked here, where both the dry run and the real one inherit it.
///
/// Generic in the ERROR type rather than in the credential source, so every check below the socket is
/// reachable from a test that has neither.
fn accepted<C>(parsed: QueryAnswer) -> Wired<QueryAnswer, C>
where
    C: core::error::Error + 'static,
{
    parsed
        .errors
        .iter()
        .find_map(|item| item.reason.clone())
        .map_or(Ok(()), |named| {
            Err(WireError::Failed {
                named: reason(Some(named)),
            })
        })?;
    Ok(parsed)
}

/// The rows one complete job produced, or the reason they are not a complete result.
///
/// The order is the cheapest refusal first, which here is also the clearest: *the job did not finish*
/// before *there is more of it* before *it did not say how much of it there is*.
fn complete<C>(answer: QueryAnswer) -> Wired<JobRows, C>
where
    C: core::error::Error + 'static,
{
    if !answer.job_complete {
        return Err(WireError::NotComplete);
    }
    if answer.page_token.is_some() {
        return Err(WireError::MoreThanOnePage);
    }
    let total: usize = answer
        .total_rows
        .ok_or(WireError::NoTotal)?
        .parse()
        .map_err(|cause| WireError::NotATotal { cause })?;
    let named = columns(answer.schema, answer.rows.len())?;
    let rows = cells(answer.rows)?;
    Ok(JobRows::of(named, rows, total))
}

/// The schema, as the vocabulary this adapter maps.
///
/// A complete job with rows and no schema is refused: there is nothing to read the cells against. A
/// complete job with neither is an empty result, and an empty column list is what that is.
fn columns<C>(schema: Option<Schema>, delivered: usize) -> Wired<Vec<Field>, C>
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
fn cells<C>(rows: Vec<AnswerRow>) -> Wired<Grid, C>
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

impl<C> BigQueryWire<C>
where
    C: AccessTokens,
{
    /// Opens a transport.
    ///
    /// The `agent` is a parameter rather than something built here so it can be the same one the
    /// credential source refreshes through. [`agent`] is the only function that builds one.
    #[must_use]
    pub const fn new(agent: ureq::Agent, credentials: C) -> Self {
        Self { agent, credentials }
    }

    /// Seconds since the epoch, or a named refusal.
    ///
    /// The one clock read in this crate. It is here rather than in the credential source because
    /// [`AccessTokens::bearer`] takes the instant as an argument, which is what makes an expiry
    /// testable without one.
    fn now() -> Wired<u64, C::Error> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .map_err(|cause| WireError::NoClock { cause })
    }

    /// Sends one job and returns the endpoint's answer, checked as far as *the service accepted
    /// this*.
    ///
    /// The completeness and row checks are NOT here, because a dry run has neither rows nor a total
    /// and would fail them - see [`Self::validate_job`] and [`Self::run_job`], which are the two
    /// callers and hold the two different conclusions.
    fn submit(&self, request: &JobRequest<'_>, dry_run: DryRun) -> Wired<QueryAnswer, C::Error> {
        let now = Self::now()?;
        let bearer = self
            .credentials
            .bearer(now)
            .map_err(|cause| WireError::Credential { cause })?;
        // Checked BEFORE anything is built or sent, which is why an expired credential costs no round
        // trip - and why it is the one guard in this function a test can reach with no socket.
        if let Some(at) = bearer.not_after().passed_by(now) {
            return Err(WireError::Expired { at, now });
        }
        let document =
            serde_json::to_vec(&body(request, dry_run)).map_err(|cause| WireError::RequestNotSerializable { cause })?;
        // `Secret::expose` is the one greppable call that lets the token out, and it lets it out into
        // a header value the client parses rather than into a string it concatenates - so a token
        // carrying a newline is a refused request at `send` rather than a second header. That is the
        // library's guarantee and not ours, which is why it is a comment here and not a row in
        // AGENTS.md's table.
        let mut answer = self
            .agent
            .post(url(request))
            .header("authorization", format!("Bearer {}", bearer.token().expose()))
            .header("content-type", "application/json")
            .send(&document)
            .map_err(|cause| WireError::Unreachable { cause: Box::new(cause) })?;
        let status = answer.status();
        let text = answer
            .body_mut()
            .with_config()
            .limit(MAX_ANSWER_BYTES)
            .read_to_string()
            .map_err(|cause| WireError::Unreadable { cause: Box::new(cause) })?;
        if !status.is_success() {
            return Err(refusal(status.as_u16(), &text));
        }
        accepted(serde_json::from_str(&text).map_err(|cause| WireError::NotADocument { cause })?)
    }

    /// What a dry run may conclude, which is *the endpoint accepted this statement* and nothing more.
    ///
    /// **`jobComplete` is deliberately not required here.** A dry run creates no job, so what that
    /// field means for one is not something this repository can verify, and requiring it would risk
    /// refusing a validation that succeeded. What a `2xx` with no reported errors proves is exactly
    /// the claim `PreFlight::Accepted` makes.
    fn validate_job(&self, request: &JobRequest<'_>) -> Wired<(), C::Error> {
        self.submit(request, DryRun::Yes).map(|_| ())
    }

    /// The rows one job produced, or the reason they are not a complete result.
    fn run_job(&self, request: &JobRequest<'_>) -> Wired<JobRows, C::Error> {
        complete(self.submit(request, DryRun::No)?)
    }
}

impl<C> JobTransport for BigQueryWire<C>
where
    C: AccessTokens,
{
    type Error = WireError<C::Error>;

    fn run(&self, request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
        self.run_job(request)
    }

    fn validate(&self, request: &JobRequest<'_>) -> Result<(), Self::Error> {
        self.validate_job(request)
    }
}
