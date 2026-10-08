//! Running one raw statement over HTTP - `docs/adr/0013`'s tool, off by default.
//!
//! Mirrors `super::query::ask` in every way that matters.
//!
//! The same admission slot is taken before the blocking task. The same span-carrying spawn moves
//! the call onto the blocking pool, because `Surface::run_sql` is synchronous for the reason
//! `Surface::answer` is.
//!
//! The same "a refusal is a result, not an `Err`" shape holds too: a caller reads which happened
//! from `crate::wire::RawOutcomeBody`'s own `outcome` tag, never from being told to retry.
//!
//! What is different is the vocabulary this handler speaks in.
//!
//! No `Query`, no `Outcome`, no `ProvenanceBody` - `sutura_domain::raw::RawStatement` in,
//! `crate::wire::RunSqlOutcome` out. Kept apart deliberately, the same reason `sutura_domain::raw`
//! is a module of its own rather than a widened `sutura_domain::query`.

use axum::Extension;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::deadline::Deadline;

use crate::problem::Failure;
use crate::state::ServiceState;
use crate::wire::{RawMalformedStatement, RawOutcomeBody, RunSqlBody, RunSqlOutcome};

/// The tag this route is grouped under in the generated document.
const TAG: &str = "run_sql";

/// Runs one literal SQL statement against this deployment's configured source, or says why it will
/// not.
#[utoipa::path(
    post,
    path = "/sql/run",
    operation_id = "run_sql",
    tag = TAG,
    request_body = RunSqlBody,
    responses(
        (status = 200, description = "The statement executed.", body = RawOutcomeBody),
        (
            status = 400,
            description = "The body is not a statement, or is empty.",
            body = crate::problem::ProblemBody
        ),
        (
            status = 403,
            description = "FOUR THINGS, and the body shape tells the first from the others - \
                           `outcome: refusal` for the domain refusal, `code` with no `outcome` for \
                           the other three.\n\n\
                           REFUSED (`outcome: refusal`, `code: source_refused`): the data system \
                           refused the statement at the identity or authorization level.\n\n\
                           NOT ENABLED (`code: tool_not_enabled`): this DEPLOYMENT never turned \
                           `run_sql` on. No scope changes this - the switch is `tools.run_sql.\
                           enabled` in this deployment's own settings, not a grant an authorization \
                           server can issue.\n\n\
                           FAILED (`code: insufficient_scope`): your credential IS valid and does \
                           not carry `sutura:sql.run`; the detail names it.\n\n\
                           REFUSED BEFORE ANY CREDENTIAL IS READ (`code: host_not_allowed`, no \
                           `outcome`): this deployment does not answer the `Host` the request names. \
                           Nothing in your credential or your question changes it; the operator \
                           lists the name in `server.allowed_hosts`.",
            body = RawOutcomeBody
        ),
        (
            status = 413,
            description = "REFUSED. `too_many_rows` or `result_too_large` - too much data; ask a \
                           narrower question. `too_many_rows` fires at the same `sutura_domain::\
                           plan::MAX_ROWS` bound the certified path enforces.",
            body = RawOutcomeBody
        ),
        (
            status = 422,
            description = "REFUSED. `code: statement_failed` - the statement did not complete. \
                           `code: deadline_exceeded` - this deployment stopped the statement after \
                           the asker's own per-request budget; narrow the statement rather than \
                           retrying it as is.",
            body = RawOutcomeBody
        ),
        (status = 429, description = "Too many requests from this address.", body = crate::problem::ProblemBody),
        (status = 500, description = "Something on our side went wrong. The body carries no detail.", body = crate::problem::ProblemBody),
        (
            status = 503,
            description = "Worth retrying: `at_capacity` or `identity_unavailable`.",
            body = crate::problem::ProblemBody
        ),
    )
)]
pub(crate) async fn run_sql(
    State(state): State<ServiceState>,
    // See `super::query::ask`'s own parameter for the whole mechanism: a REQUIRED extractor, so an
    // absent `sutura_app::Asked` answers `500` rather than reading as the deployment's own identity.
    axum::Extension(asked): axum::Extension<sutura_app::Asked>,
    // Opened by `middleware::enforce_timeout`, before admission - the same extension mechanism
    // `super::query::ask` reads its own `deadline` through.
    Extension(deadline): Extension<Deadline>,
    body: Result<Json<RunSqlBody>, JsonRejection>,
) -> Result<RunSqlOutcome, Failure> {
    let Json(body) = body.map_err(|rejection| crate::problem::rejected(&rejection))?;
    // `harness/run_sql.rs` pins this path's status and code, not its detail's bytes.
    let statement = RawStatement::try_from(body).map_err(|RawMalformedStatement::Statement { cause }| Failure::NotAQuestion {
        detail: crate::problem::Detail::of(&cause),
    })?;

    let admission_wait = state.metrics().admission_started();
    let slot = state.admission().admit().await.map_err(|shed| {
        state.metrics().shed();
        super::query::refused(&shed)
    })?;
    drop(admission_wait);
    let slot_guard = state.metrics().slot_started();

    let surface = state.surface();
    // What `establish_asked` already derived. Cloned rather than borrowed: the closure below moves
    // onto the blocking pool and outlives this extractor's value.
    let context = asked.context().clone();
    tracing::Span::current().record("asker", context.chain().subject().established());
    let joined = sutura_runtime::spawn_carrying_span(move || {
        let answered = surface.run_sql(&context, &statement, deadline);
        drop(slot);
        drop(slot_guard);
        answered
    })
    .await;
    let outcome = match joined {
        Ok(Ok(answered)) => answered,
        // The same mapping the certified route uses: a raw statement's own failures before
        // execution are exactly `SurfaceFailure`'s shapes - the broker, a wiring defect, the data
        // system - so `super::query::failed` is reused rather than re-decided here.
        Ok(Err(ref failure)) => return Err(super::query::failed(failure)),
        Err(cause) => {
            tracing::error!(error = %cause, "running a raw statement panicked");
            return Err(Failure::Internal);
        }
    };
    Ok(RunSqlOutcome::from(&outcome))
}
