//! The handler: `tools/list`, `tools/call`, and the three channels a caller has to be able to tell
//! apart.
//!
//! # Three outcomes, three channels, and the middle one is the property that matters
//!
//! | What happened | How it comes back | Why |
//! | --- | --- | --- |
//! | The question was answered | a tool result, `isError` absent, `outcome: "answer"` | rows inline, provenance beside them |
//! | The question was **refused** | a tool result, `isError` absent, `outcome: "refusal"` | it is a governance *result*, and nothing about it went wrong |
//! | The arguments were not a question | a JSON-RPC error, `-32602` | a parse failure, named, before the service is reached |
//! | The service could not answer | a tool result with `isError: true`, and no detail | something went wrong, and the detail is a path or a table |
//!
//! **A refusal is not an error and must not look like one.** `sutura_app::surface::Surface::answer`
//! is where a transport inherits that, and its own doc comment says why: a caller must not be able to
//! mistake "you may not ask that" for a hiccup and retry until something works. An `isError: true`
//! refusal would be exactly that mistake, in the one place a model is most likely to act on it - a
//! model told a call errored retries, and a model told the answer is "no, because the catalog defines
//! no such metric" asks something else.
//!
//! # Why a malformed argument is a JSON-RPC error rather than `isError`
//!
//! Because it is not a tool *execution* failure - the tool never ran. `-32602` is the name JSON-RPC
//! already has for it, `MalformedQuestion` names the field, and it is the one channel a caller cannot
//! read as either an answer or a refusal.
//!
//! **The limit, stated with the claim:** rmcp's own documentation notes that clients typically render
//! a protocol error opaquely, so a model may see less than the message carries. `Ok(isError: true)`
//! was the alternative and was rejected: it would put "your arguments were wrong" in the same channel
//! as "the data system is down", and the first is fixable by the caller while the second is not.
//!
//! # Why the port call leaves the async worker
//!
//! `Surface::answer` is synchronous, and the engine behind it drives its own runtime - entering a
//! runtime from within a runtime panics. So the call goes to the blocking pool through
//! `sutura_runtime::spawn_carrying_span`, which is the one call `clippy.toml` permits for this,
//! because a bare `spawn_blocking` loses the request's span on a pool thread.
//!
//! # What this slice does NOT do, on purpose
//!
//! * **No row cap of its own.** The answer cap is `max_rows + 1` and a result that hits it is
//!   refused rather than truncated; whether an *agent* surface wants a lower advisory cap - and what
//!   number - is a real question nobody has measured, so the bound is left exactly where it is.
//! * **No `get_tool`.** Implementing it would have rmcp validate arguments against the advertised
//!   schema before this handler sees them, which is defence in depth and also a second enforcement
//!   point whose message is not ours. One gate, and it is `crate::wire::AskArgs`'s own
//!   `deny_unknown_fields`.
//!
//! # What a scope gates here, and where the control actually is
//!
//! [`AgentSurface::new`] **requires** a [`Permitted`], so a composition root cannot forget to say what
//! the peer may do - the same reason `sutura_app::surface::LocalService::start` requires an audit
//! sink. Given one, this handler does two things with it and only the second is a control:
//!
//! | Where | What it does | What it is |
//! | --- | --- | --- |
//! | `tools/list` | drops a tool the peer may not invoke | **presentation** |
//! | `tools/call` | refuses a capability the peer was not granted, advertised or not | **the control** |
//!
//! Both read the same set, so they cannot disagree - `sutura_app::capability` holds that argument and
//! the test for it. A caller that guessed `ask_metric` without ever being shown it is refused by the
//! second row, which is why the first is described as presentation rather than as security.
//!
//! **And the honest limit, which is not small:** nothing that ships narrows the set here.
//! [`crate::serve_stdio`] passes `Permitted::every_capability`, because this transport speaks over
//! standard input and output and there is no header a token could arrive in - `docs/adr/0014`'s
//! closing section says as much, and says that deciding how this surface is reached at all is an
//! architecture decision rather than a refactor. So the narrowing here is exercised by this module's
//! own tests and by no request path, and the parameter is in place so that the decision arrives as a
//! composition change rather than as a redesign of this handler.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorCode, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler};
use sutura_app::surface::{Surface, SurfaceFailure, cause_chain};
use sutura_app::{Capability, Permitted};
use sutura_domain::query::Query;

use crate::tool;
use crate::wire::{AskArgs, CatalogContent, DescribeCatalogArgs, MalformedQuestion, OutcomeContent};

/// What a cooperative client is told about this server, beyond its tools.
///
/// **Advisory, and it is not where correctness lives.** A gateway of the shape this product runs
/// behind ignores everything but tools, so a client that never reads this must still be answered
/// correctly - and is, because every rule is on the other side of the port.
const INSTRUCTIONS: &str = "Ask this server for numbers rather than for data. \
     List the catalog first to learn what is measured, then ask one governed question about one \
     certified metric; a question outside what the catalog declares comes back as a refusal naming \
     the reason, which is an answer and not a fault. \
     Every answer carries the definition version and digest that produced it - quote them when you \
     report the number. \
     The tools you are shown are the tools you may call: a tool absent from the list is one this \
     deployment will refuse, so do not guess a name.";

/// The agent-facing surface over one [`Surface`].
///
/// Holds the service behind an `Arc` because a tool call is answered on the blocking pool, so the
/// port has to outlive the future that started the call.
pub struct AgentSurface<S> {
    service: Arc<S>,
    /// What the peer on the other end of this transport may do.
    ///
    /// A field and not an `Option`, so "this deployment forgot to say" is not a state that exists -
    /// the same reason `sutura_app::surface::LocalService` takes its audit sink as an argument.
    permitted: Permitted,
}

impl<S> AgentSurface<S> {
    /// Wraps a service, and states what the peer may do.
    ///
    /// Takes the `Arc` rather than making one, so a composition root serving two transports shares
    /// one bundle and one data system rather than opening a second of each.
    ///
    /// **`permitted` is required rather than defaulted, and that is the point of the signature.** A
    /// default here would be a posture chosen by this file for every deployment that ever links it;
    /// `Permitted::every_capability` is the right answer over standard input and output and would be
    /// the wrong answer the moment this surface is reachable over a network, and only a composition
    /// root knows which it is building. See the module documentation for what the value then gates.
    #[must_use]
    pub const fn new(service: Arc<S>, permitted: Permitted) -> Self {
        Self { service, permitted }
    }
}

impl<S> core::fmt::Debug for AgentSurface<S> {
    /// Hand-written because a `Surface` implementation need not be `Debug` - and because printing a
    /// bundle into a log is a page of definitions for no benefit.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AgentSurface").finish_non_exhaustive()
    }
}

impl<S> ServerHandler for AgentSurface<S>
where
    S: Surface,
{
    fn get_info(&self) -> ServerInfo {
        // `Implementation::new` and not `from_build_env`: that helper reads the build environment of
        // the crate it is compiled into, which is the SDK, so a server using it introduces itself as
        // the SDK. `env!` here expands in this crate.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    /// The tools this peer may invoke, and no cursor: the set is bounded by
    /// `sutura_app::Capability` and fits one page by construction. `with_all_items` is what says
    /// that, rather than an empty `next_cursor` a reader has to interpret.
    ///
    /// **Filtered by [`Permitted`], which is presentation** - see the module documentation for which
    /// half of this is the control.
    ///
    /// Not an `async fn`, because there is nothing to await: building the tools is a schema
    /// derivation. The trait declares a future, so this returns a ready one.
    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(tool::every(&self.permitted))))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // Two questions in order, and they are two on purpose: does this surface have such a tool,
        // and may this peer invoke it. Folding them together would make an unpermitted call
        // indistinguishable from a typo.
        let Some(capability) = tool::named(&request.name) else {
            return Err(ErrorData::method_not_found::<CallToolRequestMethod>());
        };
        if !self.permitted.includes(capability) {
            return Err(not_granted(capability));
        }
        // The exhaustive match is what makes a capability added to `sutura_app::Capability` a compile
        // error here rather than a tool that lists and cannot be called.
        let result = match capability {
            Capability::DescribeCatalog => {
                // Destructured rather than discarded, so the parse reads as the check it is: the value
                // has no fields, and what it proves is that the caller sent nothing this tool does not
                // declare.
                let DescribeCatalogArgs {} = catalog(request)?;
                describe(&self.service)
            }
            Capability::AskMetric => {
                let query = question(request)?;
                answer(&self.service, query).await
            }
        };
        Ok(CallToolResponse::Complete(result))
    }
}

/// A capability this peer was not granted, as a JSON-RPC error naming the scope that would grant it.
///
/// **The same code an unknown tool gets**, deliberately: from the caller's side the tool is not on
/// its surface, and inventing a second code would have clients branch on a distinction that stops
/// existing the moment a deployment grants the scope.
///
/// **The message names the scope, and that is a choice worth defending.** It is an enumeration hint -
/// a caller learns the capability exists. What it buys is the fix: the tool set and its scopes are in
/// this repository's published documentation anyway, the caller here is a client an operator
/// configured, and the alternative is an operator debugging an empty tool list against an
/// authorization server with no idea which string is missing. The same trade the HTTP surface makes
/// with RFC 6750's `insufficient_scope`, which carries the scope by design.
fn not_granted(capability: Capability) -> ErrorData {
    tracing::warn!(
        tool = capability.id(),
        scope = capability.scope(),
        "a tool call was refused: this caller was not granted the capability"
    );
    ErrorData::new(
        ErrorCode::METHOD_NOT_FOUND,
        format!(
            "`{}` is not available to this caller. It requires the scope `{}`.",
            capability.id(),
            capability.scope()
        ),
        None,
    )
}

/// The catalog tool's arguments: an object with nothing in it.
///
/// **Parsed rather than ignored**, and the parse IS the check: `DescribeCatalogArgs` carries
/// `deny_unknown_fields`, so an arguments object naming `metric`, `sql` or anything else is a named
/// parse error. A handler that skipped this would accept any object and quietly answer, which is the
/// shape that lets a caller believe it narrowed a listing it did not.
fn catalog(request: CallToolRequestParams) -> Result<DescribeCatalogArgs, ErrorData> {
    let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
    serde_json::from_value(arguments).map_err(|cause| invalid(&MalformedQuestion::NotAnObject { cause }))
}

/// The arguments, parsed into a domain question.
///
/// **This is the whole translation an adapter is allowed to do**: read the wire shape, parse each
/// field into the newtype that establishes its invariant, hand over a `Query`. Nothing here decides
/// what may be asked.
fn question(request: CallToolRequestParams) -> Result<Query, ErrorData> {
    // `Value::Object` rather than the map directly, because `from_value` is what applies
    // `deny_unknown_fields` - and an absent `arguments` is an empty object, so a call with no
    // arguments fails on the missing required fields rather than on a different message.
    let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
    let args: AskArgs = serde_json::from_value(arguments).map_err(|cause| invalid(&MalformedQuestion::NotAnObject { cause }))?;
    Query::try_from(args).map_err(|error| invalid(&error))
}

/// A malformed question, as a JSON-RPC error naming what was wrong.
///
/// The chain is walked into the message because `Display` on a `thiserror` enum prints the outermost
/// sentence only, and here the inner one is the half that names the field or the character set. That
/// is safe for exactly the reason `MalformedQuestion` is careful about: no variant carries the
/// caller's own text except where it has already failed an identifier parse.
fn invalid(error: &MalformedQuestion) -> ErrorData {
    let mut message = error.to_string();
    for cause in cause_chain(error) {
        message.push_str(": ");
        message.push_str(&cause);
    }
    ErrorData::invalid_params(message, None)
}

/// The pinned bundle, as a tool result.
///
/// **No blocking pool and no data system**, which is why this is not `async` and takes no slot: it
/// reads a bundle that was pinned and validated at startup and has not changed since. A catalog edit
/// cannot reach it - that would be a different process. The same judgement
/// `sutura_http::routes::v1::catalog` makes, for the same reason.
///
/// **No audit record either, and that is deliberate rather than an omission.**
/// `sutura_domain::audit::CallRecord` records the outcome of a *question*, and this is not one - there
/// is no `ToolOutcome` to derive a record from and `Surface::definitions` writes none. So the
/// invariant *every outcome is recorded before it is returned* is untouched: this produces no outcome.
/// Whether reading the catalog is itself worth a record is a real question and the answer would be a
/// third `RecordedOutcome` variant, which is a change to what a record means rather than a field added
/// to one.
fn describe<S>(service: &Arc<S>) -> CallToolResult
where
    S: Surface,
{
    let content = CatalogContent::from(service.definitions());
    let mut result = CallToolResult::success(vec![ContentBlock::text(content.as_text())]);
    // `ok()` rather than a propagated error, for the reason `produced` gives: the content is strings,
    // numbers and vectors, so serializing it cannot fail, and there is no `unwrap` in this workspace
    // to say so.
    result.structured_content = serde_json::to_value(&content).ok();
    result
}

/// One question through the port, on the blocking pool, as a tool result.
async fn answer<S>(service: &Arc<S>, query: Query) -> CallToolResult
where
    S: Surface,
{
    let service = Arc::clone(service);
    match sutura_runtime::spawn_carrying_span(move || service.answer(&crate::principal::established(), &query)).await {
        // A refusal and an answer take the same branch, which is the point: both are `Ok`, both are
        // a tool result, and only `outcome` inside the payload tells them apart.
        Ok(Ok(ref outcome)) => produced(outcome),
        Ok(Err(failure)) => could_not_answer(&failure),
        // The blocking task did not finish: it panicked, or the runtime is shutting down. Reported
        // like a failure, because from a caller's side it is the same fact.
        Err(error) => {
            tracing::error!(error = %error, "the blocking task answering a tool call did not finish");
            failed("this deployment could not answer")
        }
    }
}

/// An answer or a refusal, as one result shape.
fn produced(outcome: &sutura_domain::query::ToolOutcome) -> CallToolResult {
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
fn could_not_answer(failure: &SurfaceFailure) -> CallToolResult {
    tracing::error!(error = %failure, causes = ?cause_chain(failure), "the agent surface could not answer");
    failed(match *failure {
        SurfaceFailure::Compile { .. } => "this deployment could not compile the question against its own bundle",
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
fn failed(detail: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(String::from(detail))])
}

#[cfg(test)]
mod tests;
