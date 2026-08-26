//! Asking one certified question.
//!
//! # A refusal is a `200`
//!
//! The single most important thing about this handler. `ToolOutcome::Refusal` is a *result*: the
//! caller asked something they may not have, and the answer is no. It comes back with `200` and an
//! `outcome` of `refusal`, because a `4xx` invites a client library to retry - and retrying a
//! governance decision until it succeeds is exactly the behaviour the refusal exists to prevent.
//!
//! The statuses in this handler's documented responses are therefore all about things that are *not*
//! refusals: a body that is not a question, a missing credential, a limit, a data system that did
//! not answer.
//!
//! # Why the call goes onto the blocking pool
//!
//! `Warehouse` is a synchronous port, and the in-process engine behind it drives its own
//! single-threaded runtime and blocks on it. Calling that from an `async` handler on a worker
//! thread panics - a runtime cannot be entered from within a runtime - and the panic happens at
//! collect time, deep inside the engine, which is a long way from the line that caused it.
//! `spawn_blocking` is not an optimisation here; it is the only correct way to call this port.
//!
//! The current span is carried across, so the lines the engine emits belong to the same request as
//! the lines this handler emits.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use sutura_domain::query::{Query, ToolOutcome};

use crate::problem::Failure;
use crate::state::ServiceState;
use crate::surface::SurfaceFailure;
use crate::wire::{OutcomeBody, QuestionBody};

/// The tag this route is grouped under in the generated document.
const TAG: &str = "query";

/// Answers one modelled question, or says why it will not.
#[utoipa::path(
    post,
    path = "/query",
    tag = TAG,
    request_body = QuestionBody,
    responses(
        (
            status = 200,
            description = "The outcome. `outcome: answer` carries rows and the provenance of the \
                           definitions that produced them; `outcome: refusal` carries a code and a \
                           sentence. A refusal is a RESULT and not an error - retrying it will not \
                           change the answer.",
            body = OutcomeBody
        ),
        (status = 400, description = "The body is not a modelled question. The detail names the field.", body = crate::problem::ProblemBody),
        (status = 401, description = "No valid bearer token was presented.", body = crate::problem::ProblemBody),
        (status = 408, description = "The request exceeded this service's time bound.", body = crate::problem::ProblemBody),
        (status = 413, description = "The body is larger than this service will read.", body = crate::problem::ProblemBody),
        (status = 429, description = "Too many requests from this address.", body = crate::problem::ProblemBody),
        (status = 500, description = "Something on our side went wrong. The body carries no detail.", body = crate::problem::ProblemBody),
        (status = 503, description = "The data system did not answer. Worth retrying.", body = crate::problem::ProblemBody),
    )
)]
#[expect(
    clippy::cognitive_complexity,
    reason = "the tracing macros expand into branches; the control flow is parse, ask, report"
)]
pub(crate) async fn ask(
    State(state): State<ServiceState>,
    body: Result<Json<QuestionBody>, JsonRejection>,
) -> Result<Json<OutcomeBody>, Failure> {
    let Json(body) = body.map_err(|rejection| rejected(&rejection))?;
    let query = Query::try_from(body).map_err(|cause| Failure::NotAQuestion {
        detail: describe(&cause),
    })?;

    // What was asked, before it is answered. Deliberately not the filter values: a rejected or
    // accepted value reflected into a log is a value that outlives the request, and the surface
    // already refuses to echo one back to the caller.
    tracing::info!(
        metric = %query.metric(),
        grain = %query.grain(),
        dimensions = query.dimensions().len(),
        filters = query.filters().len(),
        "question received"
    );

    let surface = state.surface();
    let span = tracing::Span::current();
    let joined = tokio::task::spawn_blocking(move || span.in_scope(|| surface.answer(&query))).await;

    let outcome = match joined {
        Ok(answered) => answered.map_err(|failure| failed(&failure))?,
        Err(cause) => {
            // The blocking task panicked. The panic hook has already traced the payload and the
            // location; this says which request it took down with it.
            tracing::error!(error = %cause, "answering panicked");
            return Err(Failure::Internal);
        }
    };
    report(&outcome);
    Ok(Json(OutcomeBody::from(&outcome)))
}

/// Why the body did not become a `QuestionBody`.
///
/// **Two outcomes from one rejection type, and the split was found by a test rather than by
/// reading.** The body-limit layer causes the JSON extractor to reject with a length-limit error,
/// which is a `JsonRejection` like a malformed body is - so mapping every rejection to `400` made a
/// body over the bound indistinguishable from a body with a typo in it, and the documented `413`
/// was a status nothing produced. Branching on the rejection's own status rather than on its
/// variant keeps that true across an `axum` release that adds a variant.
fn rejected(rejection: &JsonRejection) -> Failure {
    if rejection.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
        return Failure::TooLarge;
    }
    Failure::NotAQuestion {
        // `body_text` and NOT the `#[source]` walk `describe` does. Each level of an `axum`
        // rejection's chain restates the whole message, so walking it produced the same sentence
        // three times in a row - observed in a response, not deduced. `body_text` is the one
        // sentence the rejection is designed to hand a caller, and it names the offending key for
        // an unknown field and the position for malformed JSON.
        detail: rejection.body_text(),
    }
}

/// Maps a failure to a status, and logs the part the caller must not be told.
///
/// The split is the point. A data system that did not answer is a `503` and worth retrying; our own
/// bundle or generator being wrong is a `500` and is not. Neither response carries the message,
/// because a driver's complaint names a table, a column or a file.
#[expect(
    clippy::cognitive_complexity,
    reason = "both arms are a tracing macro expanding into branches; the control flow is one match"
)]
fn failed(failure: &SurfaceFailure) -> Failure {
    match *failure {
        SurfaceFailure::Warehouse { ref message, .. } => {
            tracing::error!(error = %message, chain = ?failure.chain(), "the data system did not answer");
            Failure::Unavailable
        }
        SurfaceFailure::Compile { ref message, .. } => {
            tracing::error!(error = %message, chain = ?failure.chain(), "the pinned bundle did not compile this question");
            Failure::Internal
        }
    }
}

/// One line per outcome, so a refusal is as visible in the log as an answer.
///
/// `AGENTS.md` records "every call is attributable, refusals included" as an invariant enforced by
/// an audit sink, and there is no audit sink: nothing here records a principal chain, because there
/// is no principal to record. This is a log line, and it is named for what it is.
#[expect(
    clippy::cognitive_complexity,
    reason = "both arms are a tracing macro expanding into branches; the control flow is one match"
)]
fn report(outcome: &ToolOutcome) {
    match *outcome {
        ToolOutcome::Answer {
            ref provenance,
            ref rows,
        } => tracing::info!(
            rows = rows.rows().len(),
            definition_version = %provenance.version(),
            "answered"
        ),
        ToolOutcome::Refusal { ref reason } => tracing::info!(
            // `Debug` of a refusal reason is safe to log: the domain has a test asserting that a
            // rejected filter value is not in it.
            reason = ?reason,
            "refused"
        ),
    }
}

/// An error and its causes, on one line, for a caller.
///
/// `Display` on a `thiserror` enum prints the outermost message only, and for a malformed question
/// the outer message is the field and the cause is what was wrong with it - so both halves are
/// needed for the message to be actionable.
fn describe(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}
