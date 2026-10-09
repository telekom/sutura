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
//! | The peer cancelled a running call | no response | rmcp suppresses it; the handler stops waiting, while the question and its slot continue |
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
//! already has for it, `MalformedQuestion` names the field (except for an arguments object that did
//! not deserialize, whose message never repeats the caller's own keys or values), and it is the one
//! channel a caller cannot read as either an answer or a refusal.
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
//! neither this nor the bound does is cancel a question that is already executing. The `Warehouse`
//! port methods (`dry_run`, `execute`, `execute_raw`) each take a `Deadline` parameter that controls
//! how long the data system works; where a deadline is honored is the adapter's decision. A question
//! keeps its slot until the data system answers it, which is exactly why the backlog is a number
//! somebody chose rather than memory. `Warehouse::dry_run` and `Warehouse::execute` took `Deadline` before `#1144`; `execute_raw` took it in `#1144`.
//!
//! # How long a peer waits for a reply, and what happens when that runs out
//!
//! **`telekom/sutura#339`: the admission window was the only bounded wait on this surface.** A
//! question that could not get a slot came back inside `runtime.admission_timeout_seconds`; a
//! question that GOT one waited as long as the data system took, with nothing in the picture to end
//! it. rmcp applies no per-request deadline of its own, so there was no other bound to
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
//! * The number is already load-bearing on this composition. `sutura`'s `mcp` command opens the
//!   port's own `Deadline` from it (`docs/adr/0029`), and the in-process engine gives up against
//!   that deadline at a cooperative yield - so the engine on this transport already answers to this
//!   key; before this change the *peer* was the only party in that arithmetic with no deadline at
//!   all. This bullet used to say a `bigquery` job derived `timeoutMs`/`jobTimeoutMs` from it
//!   directly; those were `jobs.query` request parameters on a transport that is deleted. A
//!   `BigQuery` job over ADBC is sent what is left of the port deadline as its `jobTimeoutMs`.
//!
//! The key's name says `server` and this transport binds no listener, which is the one argument
//! against reusing it. It is a naming cost rather than a behavioural one, and it is cheaper than
//! two numbers for one wait.
//!
//! **What the key bounds is the WHOLE wait, on both transports, and that took a second pass.** On
//! HTTP it is an outer `tower` layer, so it covers the admission wait as well as the answer. This
//! transport applied it *after* `admit` at first, which made a peer's worst case
//! `admission_timeout_seconds + request_timeout_seconds` - one key with two meanings, which is
//! precisely the divergence reusing the key was chosen to prevent, one level up from the permit
//! set. Found by a review reading both paths. [`answer`] now wraps both waits in the one deadline,
//! so the two surfaces mean the same thing by the same number and the shipped defaults behave
//! exactly as before: a 5-second window inside a 30-second deadline, the window expiring first, a
//! shed question still answered at-capacity.
//!
//! **What the deadline does NOT do, stated with it, because it is the same limit the admission
//! bound has:** it does not stop the question. The permit is owned by the blocking closure, so a
//! question whose reply deadline fired keeps its slot until the data system answers it - the
//! deadline bounds the peer's wait and nothing else. That is deliberate and it is why the sentence
//! the peer gets does not say *try again*: repeating the question would take a second slot while
//! the first is still running. Where a question is stoppable is the data system's decision, not the
//! transport's - each adapter honours the port's `Deadline` parameter differently.
//!
//! **Peer cancellation ends this transport's wait and nothing below it.** rmcp delivers
//! `notifications/cancelled` through `RequestContext::ct`, and [`ServerHandler::call_tool`] selects
//! on that token while a question is pending. The handler returns before its configured deadline;
//! rmcp then suppresses its response on the wire. The blocking `Surface::answer`
//! call cannot be aborted, so it keeps running and owns its execution slot until it returns. This
//! is `telekom/sutura#362`; making the data work itself stoppable remains #160, on the port rather
//! than on this transport.
//!
//! **What this transport still does not bound is the size of what it reads**, which is `#266`'s
//! `H4`: `rmcp`'s stdio transport reads a line off the process's own input with no cap, and this
//! change is about a different thing - how many questions execute at once.
//!
//! **The limit on how far cancellation is exercised, stated with it.** The MCP test sends
//! `notifications/cancelled` over an in-memory protocol connection and observes the pinned SDK's
//! suppression plus this handler's captured diagnostic. It does not claim that closing a transport
//! produces the same notification. The slot-retention assertion reads the held port before release:
//! cancellation drops the future waiting on the blocking task, never the task or the permit it owns.
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
//! [`AgentSurface::new`] **requires** an [`Asking`], so a composition root cannot forget to say who
//! may ask and what they may do - the same reason `sutura_app::surface::LocalService::start` requires
//! an audit sink. Given one, [`AgentSurface::asked`] resolves it to a
//! [`sutura_app::Asked`] for THIS call - once per `TheProcessOwner` construction, fresh per request
//! under `PerRequest` - and this handler does two things with the result, only the second a control:
//!
//! | Where | What it does | What it is |
//! | --- | --- | --- |
//! | `tools/list` | drops a tool the peer may not invoke | **presentation** |
//! | `tools/call` | refuses a capability the peer was not granted, advertised or not | **the control** |
//!
//! Both read the same [`sutura_app::Asked`] for the call, so they cannot disagree -
//! `sutura_app::capability` holds that argument and the test for it. A caller that guessed
//! `ask_metric` without ever being shown it is refused by the second row, which is why the first is
//! described as presentation rather than as security.
//!
//! **A third case exists only under `Asking::PerRequest`, and it is neither row above: no
//! established caller at all.** That is refused before either row is reached -
//! [`AgentSurface::asked`] returns the error and neither `tools/list` nor `tools/call` gets as far as
//! asking `Permitted::includes` anything. See [`crate::Asking`] for why this is an enum with no
//! `Option` anywhere in it: the alternative reading of "nothing established" is "the deployment's own
//! identity," and that reading is exactly what this shape exists to make unrepresentable.
//!
//! **And the honest limit, which is not small:** nothing that ships narrows the set here.
//! [`crate::serve_stdio`] always builds `Asking::TheProcessOwner { permitted: Permitted::every_capability() }`,
//! because this transport speaks over standard input and output and there is no header a token could
//! arrive in - `docs/adr/0014`'s closing section says as much, and says that deciding how this
//! surface is reached at all is an architecture decision rather than a refactor.
//! `Asking::PerRequest` exists on the type and is exercised by this module's own tests, with hand-
//! built `RequestContext` values reusing a `Peer` a real handshake produced - `rmcp::service::Peer::new`
//! is `pub(crate)` in the pinned SDK, so nothing outside `rmcp` can mint one from nothing. Behind
//! this crate's own default-off `http` feature, `crate::http::service` now produces the value over
//! a real request - the pinned streamable-HTTP transport injects the request's own `Parts` into the
//! extensions this arm reads, and `http::tests` drives that over real bytes; PR4 is the composition
//! root that chooses to mount it behind the real `establish_asked` layer.

use std::borrow::Cow;
use std::sync::Arc;

use std::time::Instant;

use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorCode, Implementation,
    InitializeRequestParams, InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler};
use sutura_app::prompt::{PromptInputs, Tool};
use sutura_app::surface::{Surface, cause_chain};
use sutura_app::{Asked, Capability};
use sutura_config::RequestTimeout;
use sutura_domain::query::Query;
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_runtime::Admission;

use crate::Asking;
use crate::tool;
use crate::wire::{AskArgs, CatalogContent, DescribeCatalogArgs, MalformedQuestion, MalformedStatement, RunSqlArgs};

mod bounds;
mod outcome;

/// The agent-facing surface over one [`Surface`].
///
/// Holds the service behind an `Arc` because a tool call is answered on the blocking pool, so the
/// port has to outlive the future that started the call.
pub struct AgentSurface<S> {
    service: Arc<S>,
    /// How this surface learns who is asking and what they may invoke, for each call.
    ///
    /// A field and not an `Option<Permitted>`, so "this deployment forgot to say" is not a state
    /// that exists - the same reason `sutura_app::surface::LocalService` takes its audit sink as an
    /// argument. See [`crate::Asking`] for why it is also not a bare `Permitted`: the value a
    /// `PerRequest` surface acts on is read fresh from each call, never cached on `self`.
    asking: Asking,
    /// How catalog descriptions are treated, so the tool honours `prompt.catalog_prose` the same
    /// way the prompt does - an operator who omits the prose there must not ship it through here.
    prose: sutura_app::prompt::CatalogProse,
    list_physical_schema: bool,
    /// The configured row ceilings the `initialize` prompt states.
    row_ceilings: sutura_domain::plan::RowCeilings,
    /// How many questions may be executing at once, and how long a call waits for a turn.
    ///
    /// Held by value and not behind an `Option`: a deployment that forgot to bound its execution is
    /// not a state that exists here, for the same reason `asking` is not optional. The value is
    /// cheap to hold and every clone shares one permit set - see the module documentation for why
    /// that matters more than where the semaphore was built.
    admission: Admission,
    /// How long a peer waits for a reply before this surface answers instead of waiting.
    ///
    /// `telekom/sutura#339`. Not an `Option` for the third time in this struct: an unbounded wait
    /// was the state that existed, and the fix is that it is no longer representable. It bounds the
    /// WAIT and not the question - see the module documentation.
    reply: RequestTimeout,
    /// The operations the `initialize` prompt describes. The prompt is `sutura prompt`'s own document,
    /// rendered at `initialize` by [`sutura_app::prompt::render`] (`telekom/sutura#776`), and it is
    /// **still advisory**: a gateway that ignores everything but tools is answered correctly, because
    /// every rule is on the other side of the port. An `Arc` because [`crate::http::service`]'s
    /// stateless mode builds a fresh `AgentSurface` per REQUEST.
    tools: Arc<[Tool]>,
    /// The operator's own text, before it is folded into the rendered prompt - the value the
    /// `describe_catalog` tool carries through `tools/call` so a surface that never delivers
    /// `initialize.instructions` still reaches the operator's rules.
    ///
    /// Deliberately the RAW text and not the full rendered prompt: the catalog tool
    /// appends it under its own "operator's instructions" heading, exactly as the prompt does, so
    /// folding the whole prompt here would put a document inside a document.
    ///
    /// `Option<Arc<str>>`: an operator may configure no instructions file, and then there is no
    /// operator text to carry. A `String` would force a bounded empty string into the tuple and the
    /// struct for the common no-file case.
    operator_instructions: Option<Arc<str>>,
}

impl<S> AgentSurface<S> {
    /// Wraps a service, and states what the peer may do and how catalog prose is treated.
    ///
    /// Takes the `Arc` rather than making one, so a composition root serving two transports shares
    /// one bundle and one data system rather than opening a second of each.
    ///
    /// **`asking` is required rather than defaulted, and that is the point of the signature.** A
    /// default here would be a posture chosen by this file for every deployment that ever links it;
    /// `Asking::TheProcessOwner { permitted: Permitted::every_capability() }` is the right answer
    /// over standard input and output and would be the wrong answer the moment this surface is
    /// reachable over a network, and only a composition root knows which it is building. See the
    /// module documentation and [`crate::Asking`] for what the value then gates and why it is a mode
    /// rather than a bare grant.
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
    ///
    /// **`tools` is required and is the fifth: only a composition root has read the settings** that
    /// say which operations this deployment mounts - the same list `sutura prompt` renders.
    #[must_use]
    pub const fn new(
        service: Arc<S>,
        asking: Asking,
        prose: sutura_app::prompt::CatalogProse,
        admission: Admission,
        reply: RequestTimeout,
        tools: Arc<[Tool]>,
        operator_instructions: Option<Arc<str>>,
    ) -> Self {
        Self {
            service,
            asking,
            prose,
            list_physical_schema: false,
            row_ceilings: sutura_domain::plan::RowCeilings::DEFAULT,
            admission,
            reply,
            tools,
            operator_instructions,
        }
    }

    #[must_use]
    pub const fn listing_physical_schema(mut self, enabled: bool) -> Self {
        self.list_physical_schema = enabled;
        self
    }

    /// The row ceilings this deployment configured, which the prompt states.
    /// [`RowCeilings::DEFAULT`](sutura_domain::plan::RowCeilings::DEFAULT) unless set.
    #[must_use]
    pub const fn row_ceilings(mut self, row_ceilings: sutura_domain::plan::RowCeilings) -> Self {
        self.row_ceilings = row_ceilings;
        self
    }

    /// Who this call is attributed to and what it may invoke, resolved from `self.asking` and - under
    /// `Asking::PerRequest` only - this call's own `context`.
    ///
    /// **The one place either arm of [`Asking`] is read**, so `list_tools` and `call_tool` cannot
    /// disagree about which caller a request belongs to - the same reason
    /// `sutura_http::capability::establish_asked` is the one place its own `Asked` is derived on the
    /// other transport.
    ///
    /// `PerRequest` reads an `http::request::Parts`' OWN extensions, not `context.extensions`
    /// directly: that is the shape the pinned `rmcp` HTTP server transport actually produces
    /// (`docs/adr/0023`, quoting `streamable_http_server/tower.rs`) and the shape
    /// `sutura_http::capability::establish_asked` inserts a `sutura_app::Asked` into today - one wire
    /// type, read by both transports, neither depending on the other. Behind this crate's own default-
    /// off `http` feature, `crate::http::service` builds one of those `Parts` over a real request and
    /// `http::tests` exercises this arm through it; the in-process cells here still build one by hand,
    /// the same shape the transport hands over.
    ///
    /// Returns the refusal rather than a fallback when `PerRequest` finds nothing: an absent
    /// `sutura_app::Asked` must never read as `Asking::TheProcessOwner` would, which is why `Asking`
    /// is an enum with no `Option` anywhere in it rather than one with a convenient default.
    fn asked<'a>(&self, context: &'a RequestContext<RoleServer>) -> Result<Cow<'a, Asked>, ErrorData> {
        match &self.asking {
            Asking::TheProcessOwner { permitted } => Ok(Cow::Owned(Asked::established(
                crate::principal::established(),
                permitted.clone(),
            ))),
            Asking::PerRequest => context
                .extensions
                .get::<http::request::Parts>()
                .and_then(|parts| parts.extensions.get::<Asked>())
                .map(Cow::Borrowed)
                .ok_or_else(no_established_caller),
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
    fn get_info(&self) -> ServerConfig {
        // `Implementation::new` and not `from_build_env`: that helper reads the build environment of
        // the crate it is compiled into, which is the SDK, so a server using it introduces itself as
        // the SDK. `env!` here expands in this crate. No `instructions`: see `initialize`.
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")))
    }

    /// The SDK's own negotiation, plus the prompt over THIS caller's view (`docs/adr/0028`). Here and
    /// not in `get_info`, which the SDK also hands out on paths that carry no caller. No established
    /// caller is refused as `tools/list` refuses it; a process owner's view is the whole bundle.
    fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<InitializeResult, ErrorData>> + Send + '_ {
        context.peer.set_peer_info(request.clone());
        let inputs = PromptInputs::new(&self.tools, self.prose, self.operator_instructions.as_deref())
            .listing_physical_schema(self.list_physical_schema)
            .row_ceilings(self.row_ceilings);
        std::future::ready(self.asked(&context).and_then(|asked| {
            let definitions = self.service.definitions();
            let view = sutura_app::scoped_for(&definitions, asked.context());
            self.negotiate_initialize(&request)
                .map(|negotiated| negotiated.with_instructions(sutura_app::prompt::render(&view, &inputs)))
        }))
    }

    /// The tools this peer may invoke, and no cursor: the set is bounded by
    /// `sutura_app::Capability` and fits one page by construction. `with_all_items` is what says
    /// that, rather than an empty `next_cursor` a reader has to interpret.
    ///
    /// **Filtered by `sutura_app::Permitted`, which is presentation** - see the module documentation for which
    /// half of this is the control. Reads the same [`sutura_app::Asked`] `call_tool` reads, through
    /// [`AgentSurface::asked`] - `context` is no longer discarded, because `Asking::PerRequest` reads
    /// it. A caller with no established identity at all is refused here exactly as it is refused a
    /// call: the presentation half must not show a tool set an absent identity was never granted.
    ///
    /// Not an `async fn`, because there is nothing to await: building the tools is a schema
    /// derivation. The trait declares a future, so this returns a ready one.
    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(
            self.asked(&context)
                .map(|asked| ListToolsResult::with_all_items(tool::every(asked.permitted()))),
        )
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // Two questions in order, and they are two on purpose: does this surface have such a tool,
        // and may this peer invoke it. Folding them together would make an unpermitted call
        // indistinguishable from a typo.
        let Some(capability) = tool::named(&request.name) else {
            return Err(ErrorData::method_not_found::<CallToolRequestMethod>());
        };
        // Resolved once, from `self.asking` and (under `PerRequest`) this call's own `context` - see
        // `AgentSurface::asked`. `Asking` has no arm that falls back to the deployment's own identity
        // when none was established, so this is the refusal that makes an absent caller unrepresentable
        // as `Subject::TheDeploymentItself` rather than a courtesy this handler happens to extend.
        let asked = self.asked(&context)?;
        if !asked.permitted().includes(capability) {
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
                // `asked.context()` is the caller this request's own verification established - the
                // same value the port's `answer` and `run_sql` doors carry. Scoping the catalog by it
                // makes THIS door per-caller: one verified caller's view of the bundle is not
                // another's. `scoped_for` maps an absent or process-owner caller to
                // `Subject::TheDeploymentItself`, which reads as the whole bundle, so a stdio operator
                // sees exactly what they saw before.
                //
                // **The operator's own text rides the same door but is NOT audience-scoped.** The
                // catalog tool's knowledge sections, like the `initialize.instructions` prompt, are
                // audience-scoped through this caller's own view, while the operator's instructions
                // are deployment-wide and the same for every caller - in both places - and an
                // operator who names a restricted metric in them discloses it to every caller.
                describe(
                    &self.service,
                    asked.context(),
                    self.prose,
                    self.operator_instructions.as_deref(),
                    self.list_physical_schema,
                )
            }
            Capability::AskMetric => {
                let query = match question(request)? {
                    QuestionAdmission::Query(query) => query,
                    QuestionAdmission::Refused(reason) => {
                        return Ok(CallToolResponse::Complete(outcome::produced(
                            &sutura_domain::query::ToolOutcome::Refusal { reason },
                        )));
                    }
                };
                // Cloned here and not read through `asked` inside the closure below: the port call
                // moves to the blocking pool through `spawn_carrying_span`, whose closure has to be
                // `'static`, while `asked` may borrow this call's own `context` under `PerRequest` -
                // `sutura_app::Asked`'s own module documentation gives the same reason for why it is
                // `Clone` at all.
                let asked_as = asked.context().clone();
                tokio::select! {
                    result = answer(&self.service, &self.admission, self.reply, asked_as, query) => result,
                    () = context.ct.cancelled() => {
                        tracing::warn!("stopped waiting for a tool call because its peer cancelled");
                        return Err(ErrorData::internal_error("the peer cancelled this tool call", None));
                    }
                }
            }
            Capability::RunSql => {
                let statement = run_sql_statement(request)?;
                // The same door `AskMetric` goes through, and deliberately so - `telekom/sutura#703`
                // gates both `Surface::answer` and `Surface::run_sql` as the driving port's two doors,
                // and a raw statement that reached its blocking task under a different identity than
                // an `ask_metric` call on the same connection would be the one place this transport
                // disagreed with itself about who is asking.
                let asked_as = asked.context().clone();
                tokio::select! {
                    result = run_sql(&self.service, &self.admission, self.reply, asked_as, statement) => result,
                    () = context.ct.cancelled() => {
                        tracing::warn!("stopped waiting for a tool call because its peer cancelled");
                        return Err(ErrorData::internal_error("the peer cancelled this tool call", None));
                    }
                }
            }
        };
        Ok(CallToolResponse::Complete(result))
    }
}

/// This transport established no caller for this request, under `Asking::PerRequest`.
///
/// **Refused, never `crate::principal::established()`.** That substitution is the one
/// [`AgentSurface::asked`] exists to make unrepresentable - see the module documentation and
/// [`crate::Asking`]. Behind this crate's own default-off `http` feature, `crate::http::service`
/// produces the value this reads, so the arm answers over that transport; over standard input and
/// output (`serve_stdio`) the arm is never reached - `serve_stdio` builds `Asking::TheProcessOwner`,
/// so this arm exists for the HTTP transport only. A
/// different code from [`not_granted`]'s, and deliberately: this is not a
/// statement about which tool exists or which scope it needs - there is no caller yet to grant or
/// refuse one to.
fn no_established_caller() -> ErrorData {
    tracing::warn!("a tool call was refused: this transport established no caller for it");
    ErrorData::new(
        ErrorCode::INVALID_REQUEST,
        "this deployment could not establish who is asking, so the call is refused rather than \
         answered as the deployment itself",
        None,
    )
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
enum QuestionAdmission {
    Query(Query),
    Refused(sutura_domain::query::RefusalReason),
}

fn question(request: CallToolRequestParams) -> Result<QuestionAdmission, ErrorData> {
    // `Value::Object` rather than the map directly, because `from_value` is what applies
    // `deny_unknown_fields` - and an absent `arguments` is an empty object, so a call with no
    // arguments fails on the missing required fields rather than on a different message.
    let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
    // An earlier check on the same count limits `sutura_domain::question::parse_query` checks
    // downstream, refused here as the same `RefusalReason` so one count has one limit and one
    // channel - see `server::bounds`'s module doc for what firing here actually saves.
    if let Some(reason) = bounds::refused_for_over_cap(&arguments) {
        return Ok(QuestionAdmission::Refused(reason));
    }
    let args: AskArgs = serde_json::from_value(arguments).map_err(|cause| invalid(&MalformedQuestion::NotAnObject { cause }))?;
    match Query::try_from(args) {
        Ok(query) => Ok(QuestionAdmission::Query(query)),
        Err(MalformedQuestion::Refused(reason)) => Ok(QuestionAdmission::Refused(reason)),
        // The caller asked nothing wrong here; this deployment's own clock could not be read.
        // `invalid()` renders every other arm as the caller's mistake, which this is not.
        Err(MalformedQuestion::Range(sutura_runtime::relative_range::RangeResolutionError::Clock(cause))) => {
            tracing::error!(error = %cause, "this deployment's clock could not be read");
            Err(ErrorData::internal_error("this process could not read the time", None))
        }
        Err(error) => Err(invalid(&error)),
    }
}

/// The arguments, parsed into a bounded raw statement.
///
/// The whole translation this tool is allowed to do: read one string, bound it at the edge through
/// [`RawStatement::parse`]. Nothing here reads a keyword out of it - `docs/adr/0013`'s own rule
/// against inspecting a statement to decide read-only applies to every other purpose a parser might
/// be tempted for on this path too.
fn run_sql_statement(request: CallToolRequestParams) -> Result<RawStatement, ErrorData> {
    let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
    // An earlier check than `RawStatement::parse`'s own, same `-32602` it returns for the same reason.
    if let Some(error) = bounds::oversized_statement(&arguments) {
        return Err(invalid_statement(&error));
    }
    let args: RunSqlArgs =
        serde_json::from_value(arguments).map_err(|cause| invalid_statement(&MalformedStatement::NotAnObject { cause }))?;
    RawStatement::try_from(args).map_err(|error| invalid_statement(&error))
}

/// A message built for `ErrorData::invalid_params`, and the only value [`RenderedCause::into_error_data`]
/// may turn into one.
///
/// **The limit, stated beside the claim.** [`rmcp::ErrorData`] is foreign, so this cannot sit in a
/// *field* the way `sutura_http::problem::Detail` sits in `Failure::NotAQuestion` - there is no
/// `ErrorData` field to hold it. What this buys instead is module privacy plus one call site: the
/// tuple field is private and [`Self::of`]/[`Self::outer_only`] are its only constructors, so no code
/// outside this module can mint one from an ad hoc string, and [`Self::into_error_data`] is the only
/// function that calls `ErrorData::invalid_params`, so neither renderer below reaches a peer without
/// going through it. It does **not** stop a *new* call to `ErrorData::invalid_params` written
/// elsewhere in this module with its own string - that is weaker than `Detail`'s field type, which
/// makes exactly that a compile error everywhere in the crate that builds a `Failure`.
pub struct RenderedCause(String);

impl RenderedCause {
    /// Walks `error`'s `#[source]` chain onto one line.
    ///
    /// Safe here because `sutura_domain::question::MalformedQuestion`'s own note guarantees no
    /// variant, and no link of any variant's cause chain, carries the caller's own text - the same
    /// guarantee [`invalid`] relied on before this type existed - for every variant but `NotAnObject`.
    fn of(error: &(dyn core::error::Error + 'static)) -> Self {
        let mut message = error.to_string();
        for cause in cause_chain(error) {
            message.push_str(": ");
            message.push_str(&cause);
        }
        Self(message)
    }

    /// The outer sentence only, with no walk.
    ///
    /// `MalformedStatement::Statement`'s cause, `sutura_domain::raw::InvalidRawStatement`, carries
    /// none of the text it measured - `Empty`, `TooLong { len, limit }`, `EmbeddedNul` name only the
    /// shape of the failure. `NotAnObject`'s cause, on both `MalformedStatement` and
    /// `MalformedQuestion`, is different: its
    /// `#[source]` is a `serde_json::Error` whose own `Display` for an unrecognized field does echo
    /// the caller-chosen key verbatim, bidi controls included (confirmed against a standalone
    /// reproduction pinned to this workspace's `serde_json`/`thiserror` versions, not run in-crate),
    /// so walking it the way [`Self::of`] does would leak that key into a peer's context. `Display`
    /// on this `thiserror` enum prints only the fixed `#[error(...)]` sentence and never reaches
    /// that source - the property this relies on and does not itself enforce.
    fn outer_only(error: &dyn core::error::Error) -> Self {
        Self(error.to_string())
    }

    /// The one call to `ErrorData::invalid_params` a rendered cause may reach.
    ///
    /// `clippy.toml`'s `disallowed-methods` ban on `rmcp::ErrorData::invalid_params` names this as
    /// the one permitted call site: both renderers reach the constructor only through here, so an
    /// exposure added anywhere else in this workspace is a visible diff under `-D warnings`, the
    /// same property `Secret::expose_secret` relies on. That ban is a second, independent
    /// mechanism from this type's own privacy - see the struct's own doc for the limit that makes
    /// it worth keeping rather than redundant: the ban catches a new *direct* call to
    /// `invalid_params` anywhere in the crate, which nothing about this type's privacy does.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one call this crate makes to the banned constructor - both renderers reach it \
                  only through `RenderedCause::of`/`RenderedCause::outer_only` and this method, so a \
                  second direct call written elsewhere is a visible diff under -D warnings"
    )]
    fn into_error_data(self) -> ErrorData {
        ErrorData::invalid_params(self.0, None)
    }
}

/// # `RenderedCause`'s tuple field is private, so nothing outside this module can mint one
///
/// ```compile_fail
/// let _ = sutura_mcp::server::RenderedCause(String::from("leaked"));
/// ```
///
/// The compiling twin, so the failure above is provably about the private field and not about the
/// type being unreachable or a typo in the snippet - the type is genuinely public, only its
/// constructor is not:
///
/// ```
/// use sutura_mcp::server::RenderedCause;
/// fn takes_a_rendered_cause(_: &RenderedCause) {}
/// ```
mod compile_fail_tuple_field_is_private {}

/// A malformed `run_sql` call, as a JSON-RPC error. See [`RenderedCause::outer_only`] for why this
/// must not walk the cause chain.
fn invalid_statement(error: &MalformedStatement) -> ErrorData {
    RenderedCause::outer_only(error).into_error_data()
}

/// A malformed question, as a JSON-RPC error naming what was wrong.
///
/// The chain is walked into the message because `Display` on a `thiserror` enum prints the outermost
/// sentence only, and here the inner one is the half that names the field or the character set. See
/// [`RenderedCause::of`] for why that walk is safe, and [`RenderedCause::outer_only`] for the one
/// variant it is not walked for.
fn invalid(error: &MalformedQuestion) -> ErrorData {
    match error {
        MalformedQuestion::NotAnObject { .. } => RenderedCause::outer_only(error),
        _ => RenderedCause::of(error),
    }
    .into_error_data()
}

/// The served bundle, as THIS caller's view, rendered as a tool result.
///
/// **No blocking pool and no data system**, which is why this is not `async` and takes no slot: it
/// reads the validated bundle being served, taken once for this call, so a refresh that lands while
/// it renders does not change what it renders. The same judgement
/// `sutura_http::routes::v1::catalog` makes, for the same reason.
///
/// **The view is the caller's at this door - `docs/adr/0028`.** A caller sees only the metrics its
/// mapped audiences name, mirrored from `sutura_http::routes::v1::catalog`'s `CatalogBody::of(&view, …)`.
/// `scoped_for` reads an absent or process-owner caller as `Subject::TheDeploymentItself`, which
/// maps to the whole bundle, so an operator in stdio mode still sees exactly what they always saw;
/// a verified caller sees the bundle cut to their grant. That the renderer CANNOT skip the scope is
/// the point of taking `&ScopedView` rather than a bare `&PinnedDefinitions`.
///
/// **The `initialize.instructions` prompt goes through the same view.** `initialize` renders it
/// per caller with `sutura_app::prompt::render`, which takes a `&ScopedView` too, so its
/// metric list and knowledge sections withhold what this listing withholds. What neither scopes is
/// the operator's own text, which is deployment-wide by design.
///
/// **No audit record either, and that is deliberate rather than an omission.**
/// `sutura_domain::audit::CallRecord` records the outcome of a *question*, and this is not one - there
/// is no `ToolOutcome` to derive a record from and `Surface::definitions` writes none. So the
/// invariant *every outcome is recorded before it is returned* is untouched: this produces no outcome.
/// Whether reading the catalog is itself worth a record is a real question and the answer would be a
/// third `RecordedOutcome` variant, which is a change to what a record means rather than a field added
/// to one.
fn describe<S>(
    service: &Arc<S>,
    context: &sutura_domain::identity::RequestContext,
    prose: sutura_app::prompt::CatalogProse,
    operator_instructions: Option<&str>,
    list_physical_schema: bool,
) -> CallToolResult
where
    S: Surface,
{
    // The setting reaches the CONTENT and not only the rendering, which is the whole of `H1` in
    // `#266`: the text block honoured it while `structured_content` beside it carried every
    // description, so a deployment that had withheld its catalog prose shipped it anyway to any
    // client reading the structured half. `CatalogContent::of_with_physical_schema` cannot be
    // called without the answer.
    //
    // And the view is built here, from the caller this request's own verification resolved - see the
    // doc above for why `scoped_for` maps an absent or process-owner caller to the whole bundle.
    let definitions = service.definitions();
    let view = sutura_app::scoped_for(&definitions, context);
    // The operator's own text - the raw value, before it was folded into the rendered prompt - rides
    // the catalog tool so a gateway that surfaces only tools (and never delivers
    // `initialize.instructions`) still reaches the operator's rules.
    let listing = CatalogContent::of_with_physical_schema(&view, prose, operator_instructions, list_physical_schema);
    let mut result = CallToolResult::success(vec![ContentBlock::text(listing.as_text())]);
    // `ok()` rather than a propagated error, for the reason `produced` gives: the content is strings,
    // numbers and vectors, so serializing it cannot fail, and there is no `unwrap` in this workspace
    // to say so.
    result.structured_content = serde_json::to_value(&listing).ok();
    result
}

/// One question, under both bounds, as a tool result.
///
/// **`reply` wraps the WHOLE wait - the admission window included - and that is what makes it the
/// same key it is on HTTP.** There it is an outer `tower` layer, so `server.request_timeout_seconds`
/// bounds a caller's total wait and the admission window is the shorter inner bound. This function
/// applied it after `admit` for one release, which made a peer's worst case here
/// `admission_timeout_seconds + request_timeout_seconds` - one key with two meanings, which is the
/// divergence reusing the key was chosen to prevent, one level up from the permit set. Found by
/// review reading both paths rather than by a test, and closed by moving the timeout out one frame.
///
/// So the shipped behaviour is unchanged and the arithmetic is now stated by the code: the defaults
/// are a 5-second window inside a 30-second deadline, the window expires first, and a shed question
/// still comes back as at-capacity. A deployment whose window is at or above its reply deadline gets
/// the deadline first - exactly what `docs/serving.md` already says of the HTTP layer.
async fn answer<S>(
    service: &Arc<S>,
    admission: &Admission,
    reply: RequestTimeout,
    asked_as: sutura_domain::identity::RequestContext,
    query: Query,
) -> CallToolResult
where
    S: Surface,
{
    // Opened here, before `admitted` - the same instant this function's own reply deadline starts
    // counting from, so the port's budget is inside the caller's whole wait for the same reason
    // `reply` itself wraps the admission window: `docs/adr/0029`.
    let deadline = Deadline::opened_at(Instant::now(), reply.budget());
    match tokio::time::timeout(reply.duration(), admitted(service, admission, asked_as, query, deadline)).await {
        Ok(result) => result,
        Err(_elapsed) => outcome::outran_its_deadline(reply),
    }
}

/// A slot, then the port on the blocking pool, then the outcome.
///
/// **The slot is acquired here and released on the pool thread**, and the two halves are what make
/// the bound a bound on execution - see the module documentation. A question that cannot get one
/// inside the admission window is shed rather than queued.
///
/// Split out of [`answer`] so the reply deadline can wrap both waits rather than only the second.
/// Nothing about the permit changes: this future being dropped at the deadline drops the JOIN HANDLE
/// and nothing else - `tokio` documents that a started blocking task cannot be aborted, and dropping
/// a handle detaches the task rather than ending it - so the question runs on holding the slot the
/// closure owns. Dropped while still WAITING for a slot, it takes none, which is the same cost a
/// shed waiter has: a dropped future rather than a thread.
async fn admitted<S>(
    service: &Arc<S>,
    admission: &Admission,
    asked_as: sutura_domain::identity::RequestContext,
    query: Query,
    deadline: Deadline,
) -> CallToolResult
where
    S: Surface,
{
    // Before the task is spawned, and not inside it: a slot acquired inside the blocking task would
    // be a pool thread already taken while waiting for permission to take one.
    let slot = match admission.admit().await {
        Ok(slot) => slot,
        Err(shed) => return outcome::at_capacity(&shed),
    };
    let service = Arc::clone(service);
    let working = sutura_runtime::spawn_carrying_span(move || {
        // `asked_as` and not `crate::principal::established()`: the caller `AgentSurface::asked`
        // resolved for THIS call, moved into the closure because the closure has to outlive the
        // request's own borrowed extensions - see `sutura_app::Asked`'s own reason for being `Clone`.
        let answered = service.answer(&asked_as, &query, deadline);
        // Explicitly, and here rather than at the top of the closure: the slot is released when the
        // WORK finishes, so it is not handed back by a peer that stopped waiting - and the closure
        // owning it is what makes that structural rather than an ordering somebody maintains.
        //
        // Inside the span as well, because `spawn_carrying_span` scopes the whole closure: a
        // diagnostic emitted while releasing is still attributable to this call.
        drop(slot);
        answered
    });
    match working.await {
        // A refusal and an answer take the same branch, which is the point: both are `Ok`, both are
        // a tool result, and only `outcome` inside the payload tells them apart.
        Ok(Ok(ref outcome)) => outcome::produced(outcome),
        Ok(Err(failure)) => outcome::could_not_answer(&failure),
        // The blocking task did not finish: it panicked, or the runtime is shutting down. Reported
        // like a failure, because from a caller's side it is the same fact.
        Err(error) => {
            tracing::error!(error = %error, "the blocking task answering a tool call did not finish");
            outcome::failed("this deployment could not answer")
        }
    }
}

/// One raw statement, under both bounds, as a tool result - [`answer`]'s shape, over
/// [`Surface::run_sql`] instead of [`Surface::answer`].
///
/// Opens a [`Deadline`] the same instant [`answer`]'s own is.
async fn run_sql<S>(
    service: &Arc<S>,
    admission: &Admission,
    reply: RequestTimeout,
    asked_as: sutura_domain::identity::RequestContext,
    statement: RawStatement,
) -> CallToolResult
where
    S: Surface,
{
    let deadline = Deadline::opened_at(Instant::now(), reply.budget());
    let working = admitted_raw(service, admission, asked_as, statement, deadline);
    match tokio::time::timeout(reply.duration(), working).await {
        Ok(result) => result,
        Err(_elapsed) => outcome::outran_its_deadline(reply),
    }
}

/// A slot, then the port on the blocking pool, then the outcome - [`admitted`]'s shape for the raw
/// path. Reads `asked_as` for the same reason `admitted` does: `telekom/sutura#703` gates this as
/// the driving port's second door, and it must not disagree with `AskMetric` about who is asking on
/// the same connection.
async fn admitted_raw<S>(
    service: &Arc<S>,
    admission: &Admission,
    asked_as: sutura_domain::identity::RequestContext,
    statement: RawStatement,
    deadline: Deadline,
) -> CallToolResult
where
    S: Surface,
{
    let slot = match admission.admit().await {
        Ok(slot) => slot,
        Err(shed) => return outcome::at_capacity(&shed),
    };
    let service = Arc::clone(service);
    let working = sutura_runtime::spawn_carrying_span(move || {
        let answered = service.run_sql(&asked_as, &statement, deadline);
        drop(slot);
        answered
    });
    match working.await {
        Ok(Ok(ref outcome)) => outcome::raw_produced(outcome),
        Ok(Err(failure)) => outcome::could_not_answer(&failure),
        Err(error) => {
            tracing::error!(error = %error, "the blocking task running a raw statement did not finish");
            outcome::failed("this deployment could not run this statement")
        }
    }
}

#[cfg(test)]
mod tests;
