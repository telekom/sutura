//! Rendering a port's outcome as one [`CallToolResult`] shape - split out of `server.rs` (not a
//! second concern) purely because `cargo xtask max-lines` caps a file at a thousand lines: these
//! six functions turn an outcome (an answer, a refusal, a shed question, a timed-out wait, or a
//! failure) into the one result shape every call site above hands back, and share nothing with the
//! admission or dispatch code around them beyond the types below.

use rmcp::model::{CallToolResult, ContentBlock};
use sutura_app::surface::{SurfaceFailure, cause_chain};
use sutura_config::RequestTimeout;
use sutura_runtime::AtCapacity;

use crate::wire::{OutcomeContent, RawContent};

/// A raw answer or refusal, as one result shape - [`produced`]'s shape, over [`RawContent`].
pub(super) fn raw_produced(outcome: &sutura_domain::raw::RawOutcome) -> CallToolResult {
    let content = RawContent::from(outcome);
    let mut result = CallToolResult::success(vec![ContentBlock::text(content.as_text())]);
    result.structured_content = serde_json::to_value(&content).ok();
    result
}

/// The peer waited out `server.request_timeout_seconds` and the question is still running.
///
/// **A failure and not a refusal**, for `at_capacity`'s reason one bound over: nothing was judged.
/// The HTTP surface answers the same fact `408` with `code: timeout`, and the argument for the
/// channel is the same - a caller must be able to tell *this went wrong* from *you may not ask
/// that*.
///
/// **No digits in the sentence, and that is the same call `at_capacity` makes.** The deadline is the
/// operator's own configuration: the answer to a bound too small for the questions a deployment
/// gets is a settings change or a narrower catalog, and neither is something the asking model can
/// do. So the number goes to the log, where an operator reads it.
///
/// **It does not say *try again*, deliberately.** The question is still executing and still holds
/// its slot, so an immediate repeat costs a second slot for the same answer. What it says instead is
/// the one thing the caller can act on: ask for less.
pub(super) fn outran_its_deadline(reply: RequestTimeout) -> CallToolResult {
    tracing::warn!(
        request_timeout_seconds = reply.seconds(),
        "gave up waiting for a tool call's answer: the question is still running and keeps its slot"
    );
    failed(
        "this deployment gave up waiting for the answer to this question. It may still be running, so asking the same question again will not be faster and will take a second execution slot; ask for a narrower time range or fewer dimensions",
    )
}

/// Every execution slot was taken for the whole admission window, so the question was shed.
///
/// **A failure and not a refusal**, which is the same call `sutura_http::problem` makes for the same
/// fact: a `RefusalReason` says *do not ask this again*, and this question was never judged - it did
/// not run. So it comes back on the third channel, with the one thing an agent can act on. Waiting
/// helps here, which is why the sentence says so and the `SurfaceFailure::Warehouse` one does not.
///
/// **The two numbers go to the log and not into the model's context.** They are an operator's own
/// configuration: the answer to a bound too small for the machine is a settings change, and the
/// answer to a window that expires under normal load is another replica. Neither is something the
/// caller can do.
pub(super) fn at_capacity(shed: &AtCapacity) -> CallToolResult {
    tracing::warn!(
        max_concurrent_queries = shed.bound(),
        admission_timeout_seconds = shed.waited().as_secs(),
        "shed a tool call: every execution slot was taken for the whole admission window"
    );
    failed(
        "this deployment is already answering as many questions at once as it admits, and no slot came free while this call waited; ask again shortly",
    )
}

/// An answer or a refusal, as one result shape.
pub(super) fn produced(outcome: &sutura_domain::query::ToolOutcome) -> CallToolResult {
    let content = OutcomeContent::from(outcome);
    let mut result = CallToolResult::success(vec![ContentBlock::text(content.as_text())]);
    // `ok()` rather than a propagated error: `OutcomeContent` is strings, numbers and vectors, so
    // serializing it cannot fail, and there is no `unwrap` in this workspace to say so. A client
    // that got no structured content still has the text block.
    result.structured_content = serde_json::to_value(&content).ok();
    result
}

/// Something went wrong, said with no detail taken from the cause.
///
/// The text of a `SurfaceFailure`'s chain is a path, a table or a column, and this result reaches a
/// model's context. The cause goes to the log instead, which is the one place text is the point -
/// the same judgement `sutura_http::problem`'s internal failure already makes.
pub(super) fn could_not_answer(failure: &SurfaceFailure) -> CallToolResult {
    tracing::error!(error = %failure, causes = ?cause_chain(failure), "the agent surface could not answer");
    failed(match *failure {
        SurfaceFailure::Compile { .. } => "this deployment could not compile the question",
        SurfaceFailure::Warehouse { .. } => "the data system did not answer",
        // Written for an agent: what it needs is whether waiting helps. It does here, and it does
        // not for the arm below - which is why the two are separate sentences rather than one about
        // credentials.
        SurfaceFailure::Broker { .. } => {
            "the identity provider this deployment depends on did not answer; this may work if you try again shortly"
        }
        SurfaceFailure::Miswired { .. } => {
            "this deployment is misconfigured: what it holds for the data system does not match the question's. Nothing you can change - report it"
        }
    })
}

/// A tool result that says the call failed, with a fixed sentence and nothing derived from a cause.
pub(super) fn failed(detail: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(String::from(detail))])
}
