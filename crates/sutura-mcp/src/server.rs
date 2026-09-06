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
//! | Every execution slot was taken for the whole admission window | a tool result with `isError: true`, and a sentence saying to ask again | the question was never judged, so it is not a refusal - and unlike the row above, waiting is the fix |
//! | The reply outran `server.request_timeout_seconds` | a tool result with `isError: true`, and a sentence saying the question may still be running | the peer's WAIT is bounded and the question is not: see the section on the reply deadline |
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
//! # How many questions may be executing, and where the permit lives
//!
//! **[`sutura_runtime::Admission`] and not a bound of this transport's own**, because the resource
//! is the *process*: one blocking pool, one set of data systems, and two independently sized
//! semaphores would be two controls each reporting a limit the other can exceed. So the value
//! arrives at [`AgentSurface::new`] from a composition root that read
//! `runtime.max_concurrent_queries`, and every clone of an `Admission` shares one permit set -
//! which is what lets one process serve two transports under one number.
//!
//! **The slot is taken before the blocking task is spawned and released INSIDE it.** Taken inside
//! would be a pool thread already occupied while waiting for permission to occupy one; released by
//! the async worker would make it a bound on *starting* work rather than on running it, and
//! `tokio` documents that a started blocking task cannot be aborted - so a caller that has gone
//! away does not stop the question it asked. A permit handed back early is worse than no bound at
//! all, because it reads as a control.
//!
//! **This defines the response to running out of admission and nothing about stopping work.** A
//! shed call is answered on the third channel above, inside the bounded admission window. What
//! neither this nor the bound does is cancel a question that is already executing: the `Warehouse`
//! port is synchronous and carries no deadline, so a question inside the pool runs to completion
//! whatever the peer is told - and it keeps its slot until it does, which is exactly why the
//! backlog is a number somebody chose rather than memory. Making running work stoppable is #160's
//! subject, on the port rather than on either transport.
//!
//! # How long a peer waits for a reply, and what happens when that runs out
//!
//! **`telekom/sutura#339`: the admission window was the only bounded wait on this surface.** A
//! question that could not get a slot came back inside `runtime.admission_timeout_seconds`; a
//! question that GOT one waited as long as the data system took, with nothing in the picture to end
//! it. rmcp 3.1.4 applies no per-request deadline of its own, so there was no other bound to
//! inherit - measured on the pinned SDK and not read off its documentation.
//!
//! So [`AgentSurface::new`] takes `server.request_timeout_seconds` as well, and the one function
//! that awaits the port waits under it. When it expires the peer is answered on the fourth
//! channel above. **The same key the HTTP surface answers `408` from, and reusing it rather than
//! inventing a key of this transport's own is a decision with two arguments:**
//!
//! * A second key would be a second number for one fact - *how long a caller waits for a reply* -
//!   and the two transports would then be able to disagree about it while sharing one execution
//!   bound.
//! * The number is already load-bearing on this composition. `sutura`'s `mcp` command passes it to
//!   `open_engine`, where `QueryDeadline::within_request_timeout` divides it into the deadline a
//!   `bigquery` job is submitted with. So the engine on this transport already gives up against
//!   this key; before this change the *peer* was the only party in that arithmetic with no
//!   deadline at all.
//!
//! The key's name says `server` and this transport binds no listener, which is the one argument
//! against reusing it. It is a naming cost rather than a behavioural one, and it is cheaper than
//! two numbers for one wait.
//!
//! **What the deadline does NOT do, stated with it, because it is the same limit the admission
//! bound has:** it does not stop the question. The permit is owned by the blocking closure, so a
//! question whose reply deadline fired keeps its slot until the data system answers it - the
//! deadline bounds the peer's wait and nothing else. That is deliberate and it is why the sentence
//! the peer gets does not say *try again*: repeating the question would take a second slot while
//! the first is still running. Making running work stoppable is #160's subject, on the port.
//!
//! **And what it still leaves unbounded, on this transport only:** a peer that sends
//! `notifications/cancelled` stops nothing and observes nothing until the deadline fires. rmcp
//! delivers that cancellation as `RequestContext::ct` and `call_tool` here does not read it, so a
//! cancelled call goes on waiting out its deadline. Reading the token would make cancellation
//! observable to the peer and would be the first place this crate depends on an rmcp behaviour its
//! own documentation describes loosely - `telekom/sutura#362`, filed rather than folded in. It
//! would free an async worker and never a slot, which is why it is a separate decision.
//!
//! **What this transport still does not bound is the size of what it reads**, which is `#266`'s
//! `H4`: `rmcp`'s stdio transport reads a line off the process's own input with no cap, and this
//! change is about a different thing - how many questions execute at once.
//!
//! **The limit on how far the shedding is exercised, stated with it.** `rmcp` 3.1.4 answers a
//! `notifications/cancelled` by cancelling a token this handler does not read, and it spawns each
//! request as a detached task - so on that SDK a peer that cancels or disconnects does not drop the
//! future that is waiting for the answer. The property that the permit belongs to the work rather
//! than to that future is therefore asserted by dropping the future in a test, not by cancelling a
//! call over the wire.
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
use sutura_config::RequestTimeout;
use sutura_domain::query::Query;
use sutura_runtime::{Admission, AtCapacity};

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
    /// How catalog descriptions are treated, so the tool honours `prompt.catalog_prose` the same
    /// way the prompt does - an operator who omits the prose there must not ship it through here.
    prose: sutura_app::prompt::CatalogProse,
    /// How many questions may be executing at once, and how long a call waits for a turn.
    ///
    /// Held by value and not behind an `Option`: a deployment that forgot to bound its execution is
    /// not a state that exists here, for the same reason `permitted` is not optional. The value is
    /// cheap to hold and every clone shares one permit set - see the module documentation for why
    /// that matters more than where the semaphore was built.
    admission: Admission,
    /// How long a peer waits for a reply before this surface answers instead of waiting.
    ///
    /// `telekom/sutura#339`. Not an `Option` for the third time in this struct: an unbounded wait
    /// was the state that existed, and the fix is that it is no longer representable. It bounds the
    /// WAIT and not the question - see the module documentation.
    reply: RequestTimeout,
}

impl<S> AgentSurface<S> {
    /// Wraps a service, and states what the peer may do and how catalog prose is treated.
    ///
    /// Takes the `Arc` rather than making one, so a composition root serving two transports shares
    /// one bundle and one data system rather than opening a second of each.
    ///
    /// **`permitted` is required rather than defaulted, and that is the point of the signature.** A
    /// default here would be a posture chosen by this file for every deployment that ever links it;
    /// `Permitted::every_capability` is the right answer over standard input and output and would be
    /// the wrong answer the moment this surface is reachable over a network, and only a composition
    /// root knows which it is building. See the module documentation for what the value then gates.
    ///
    /// **`prose` is required for the same reason, and it is a composition-root value.** `sutura`'s
    /// `mcp` subcommand passes what this deployment renders; a `CatalogProse` with no default keeps
    /// `quoted` from being a posture chosen here for a deployment that meant something else.
    ///
    /// **`admission` is required and is not built here, and that is the third instance of the same
    /// rule.** A bound this file constructed would be a number chosen for every deployment that
    /// links it, and - worse - a *second* permit set in any process that also serves HTTP, where
    /// two limits each reporting a bound the other can exceed is not a bound. So the composition
    /// root reads `runtime.max_concurrent_queries` and `runtime.admission_timeout_seconds` and
    /// hands one [`Admission`] to whatever serves. Since `telekom/sutura#340` that is held by
    /// `cargo xtask check-one-bound` rather than by this paragraph.
    ///
    /// **`reply` is required and is the fourth, and it is the one whose absence was a missing bound
    /// rather than a misplaced one** - `telekom/sutura#339`. It is
    /// `server.request_timeout_seconds`, the same key the HTTP surface answers `408` from, and with
    /// no counterpart here a peer that got an execution slot waited for as long as the data system
    /// took. The module documentation carries why this key rather than one of this transport's own,
    /// and what the deadline does not stop.
    #[must_use]
    pub const fn new(
        service: Arc<S>,
        permitted: Permitted,
        prose: sutura_app::prompt::CatalogProse,
        admission: Admission,
        reply: RequestTimeout,
    ) -> Self {
        Self {
            service,
            permitted,
            prose,
            admission,
            reply,
        }
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
                describe(&self.service, self.prose)
            }
            Capability::AskMetric => {
                let query = question(request)?;
                answer(&self.service, &self.admission, self.reply, query).await
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
fn describe<S>(service: &Arc<S>, prose: sutura_app::prompt::CatalogProse) -> CallToolResult
where
    S: Surface,
{
    // The setting reaches the CONTENT and not only the rendering, which is the whole of `H1` in
    // `#266`: the text block honoured it while `structured_content` beside it carried every
    // description, so a deployment that had withheld its catalog prose shipped it anyway to any
    // client reading the structured half. `CatalogContent::of` cannot be called without the answer.
    let content = CatalogContent::of(service.definitions(), prose);
    let mut result = CallToolResult::success(vec![ContentBlock::text(content.as_text())]);
    // `ok()` rather than a propagated error, for the reason `produced` gives: the content is strings,
    // numbers and vectors, so serializing it cannot fail, and there is no `unwrap` in this workspace
    // to say so.
    result.structured_content = serde_json::to_value(&content).ok();
    result
}

/// One question through the port, on the blocking pool, under both bounds, as a tool result.
///
/// **The slot is acquired on this line and released on the pool thread**, and the two halves are
/// what make the bound a bound on execution - see the module documentation. A question that cannot
/// get one inside the admission window is shed rather than queued.
///
/// **And the wait for the answer is bounded by `reply`** - `telekom/sutura#339`. Both bounds are
/// here rather than one being a layer, because this transport has no layers: there is no listener,
/// no router and no `tower` stack, so the deadline a `tower` layer applies on the HTTP surface has
/// to be applied by the one function that awaits the port.
async fn answer<S>(service: &Arc<S>, admission: &Admission, reply: RequestTimeout, query: Query) -> CallToolResult
where
    S: Surface,
{
    // Before the task is spawned, and not inside it: a slot acquired inside the blocking task would
    // be a pool thread already taken while waiting for permission to take one.
    let slot = match admission.admit().await {
        Ok(slot) => slot,
        Err(shed) => return at_capacity(&shed),
    };
    let service = Arc::clone(service);
    let working = sutura_runtime::spawn_carrying_span(move || {
        let answered = service.answer(&crate::principal::established(), &query);
        // Explicitly, and here rather than at the top of the closure: the slot is released when the
        // WORK finishes, so it is not handed back by a peer that stopped waiting - and the closure
        // owning it is what makes that structural rather than an ordering somebody maintains.
        //
        // Inside the span as well, because `spawn_carrying_span` scopes the whole closure: a
        // diagnostic emitted while releasing is still attributable to this call.
        drop(slot);
        answered
    });
    // **The peer's wait, bounded.** The timeout drops the JOIN HANDLE and nothing else: `tokio`
    // documents that a started blocking task cannot be aborted, and dropping a handle detaches the
    // task rather than ending it. So the question runs on holding the slot the closure owns, which
    // is the same posture the HTTP surface's `408` has and is why nothing here releases a permit.
    match tokio::time::timeout(reply.duration(), working).await {
        // A refusal and an answer take the same branch, which is the point: both are `Ok`, both are
        // a tool result, and only `outcome` inside the payload tells them apart.
        Ok(Ok(Ok(ref outcome))) => produced(outcome),
        Ok(Ok(Err(failure))) => could_not_answer(&failure),
        // The blocking task did not finish: it panicked, or the runtime is shutting down. Reported
        // like a failure, because from a caller's side it is the same fact.
        Ok(Err(error)) => {
            tracing::error!(error = %error, "the blocking task answering a tool call did not finish");
            failed("this deployment could not answer")
        }
        Err(_elapsed) => outran_its_deadline(reply),
    }
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
fn outran_its_deadline(reply: RequestTimeout) -> CallToolResult {
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
fn at_capacity(shed: &AtCapacity) -> CallToolResult {
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
