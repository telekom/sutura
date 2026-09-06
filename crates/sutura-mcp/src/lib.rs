//! The agent-facing surface: one governed tool, over the Model Context Protocol.
//!
//! **Transport only.** It parses the wire shape, translates it into a domain [`Query`], calls
//! `sutura_app::surface::Surface`, and maps the outcome back. Nothing here decides what a question
//! may ask: no predicate is assembled, no dimension is checked, no bound is applied. Every rule is
//! on the other side of that call, which is what makes this crate reviewable by reading its
//! translation rather than its judgement.
//!
//! [`Query`]: sutura_domain::query::Query
//!
//! # What this crate is
//!
//! **One crate, one transport, and the whole tool set** - which is two tools, because
//! `sutura_app::surface::Surface` has two operations: list what this deployment measures, and answer
//! one governed question about one metric. `sutura_app::Capability` is the one source for which those
//! are, so **the two transports cannot disagree about what this deployment offers**; each of them
//! renders that source and each has a `both_transports_describe_the_same_tools` test asserting it did
//! not deviate.
//!
//! Five properties are load-bearing and each has a test rather than a paragraph:
//!
//! * **The schema is generated.** [`tool::input_schema`] is `schemars::schema_for!` over a wire type
//!   in [`wire`] - there is no hand-written JSON object in this crate - and every tool's generated
//!   bytes are committed as a snapshot, so a new or widened tool input lands in a reviewer's diff.
//!   That byte-compare is a mechanism `AGENTS.md` describes as owed rather than standing; this crate
//!   is what owes it.
//! * **A tool a caller may not invoke is neither advertised nor answered.**
//!   [`AgentSurface::new`] requires a `sutura_app::Permitted`, `tools/list` filters on it and
//!   `tools/call` refuses on it. The filtering is presentation and the refusal is the control - see
//!   [`server`], which says so in a table, and says plainly that nothing over standard input and
//!   output narrows the set today.
//! * **A refusal is a RESULT.** It comes back inside a tool result with the error flag unset, not as
//!   a JSON-RPC error and not as `isError`. See [`server`] for the three channels and why they are
//!   three.
//! * **`deny_unknown_fields` survives the transport.** An argument named `sql`, `table` or
//!   `predicate` is a named parse error, asserted through a real client rather than assumed.
//! * **The configured number of questions execute at once, and no more.** The bound is
//!   `sutura_runtime::Admission`, built by the composition root from
//!   `runtime.max_concurrent_queries` rather than by this crate, and the permit belongs to the
//!   blocking work rather than to the future waiting for it. [`server`] carries both halves of that
//!   argument and the limit on how far the second is exercised over the wire.
//! * **A peer's whole wait is bounded too, and it was not** - `telekom/sutura#339`. The admission
//!   window bounded a question that could not START; a question that got a slot waited for as long
//!   as the data system took, because rmcp applies no per-request deadline and this transport
//!   composed no equivalent of the HTTP surface's `tower` layer. It is
//!   `server.request_timeout_seconds` now, wrapping the admission wait as well as the answer - so
//!   the key means on this transport what it means on the other, which took a second pass to get
//!   right. It bounds the WAIT and stops no work; [`server`] says which key, why that one, what
//!   the arithmetic used to be, and what a cancelling peer still does not get.
//!
//! # Why the protocol comes from a dependency
//!
//! MCP is JSON-RPC with a handshake, and framing it by hand is a few hundred lines. It was
//! considered and rejected on evidence rather than on effort: a server we framed ourselves could be
//! tested only against our own parser, and *"a test asserting on source text proves nothing"* is a
//! rule this repository already holds. `rmcp` is the protocol's own Rust SDK, so the tests in
//! [`server`] drive this handler with **its** client over an in-memory pipe - which is the only shape
//! of evidence that says an agent client can actually ask.
//!
//! **What the dependency costs, measured rather than waved at:** `rmcp` with `server` and
//! `transport-io`, plus `schemars`, adds **eight** packages to `Cargo.lock` that were not there -
//! `rmcp`, `pastey`, `schemars`, `schemars_derive`, `dyn-clone`, `ref-cast`, `ref-cast-impl` and
//! `serde_derive_internals` - counted by diffing the package names against the base branch rather
//! than read off a dependency tree. Everything else it wants was already in the lock: `tokio-util`,
//! `uuid`, `chrono` and `futures` arrive here through this crate for the first time, but they are
//! nobody's new supply-chain surface. The only shipped artifact that grows is `sutura` itself,
//! which links this crate behind its `mcp` subcommand - and the dependency is pure Rust, so nothing
//! about the two musl triples changes. `macros` is off, so no attribute of theirs writes code into
//! this crate, and the handler is three methods written by hand.
//!
//! # What is deliberately absent
//!
//! * **A row cap of this transport's own.** The answer cap is `max_rows + 1` and a result that hits
//!   it is refused rather than truncated. Whether an agent surface wants a lower advisory cap - and
//!   what number - is a real question **nobody has measured**, so the bound is left exactly where it
//!   is rather than guessed at here.
//! * **A bound on the SIZE of what arrives**, which is `#266`'s `H4` and is a different thing from
//!   the admission bound this crate now applies. rmcp's stdio transport reads a line off the
//!   process's own standard input with no cap, so one enormous line is read before anything parses
//!   it. Nothing here can bound it: the reader is the SDK's, and the boundary is the process - a
//!   peer that can write to this pipe can already launch the process. It stays named rather than
//!   claimed as covered.
//! * **Any notion of who is asking.** `crate::principal` still answers
//!   `sutura_domain::identity::Subject::TheDeploymentItself`, truthfully: this transport speaks over a
//!   pipe, where there is no header a token could arrive in. `sutura_http::inbound` is where leg 1
//!   lives and it is unreachable from here - an adapter never calls another adapter - so a caller
//!   identity on this surface needs the two decisions `docs/adr/0014`'s closing section names: how it
//!   is reached at all, and which crate the validator moves to.
//!
//!   **The consequence for what a scope gates here is stated rather than left implicit:** the
//!   capability set this surface offers is narrowable, and over standard input and output nothing
//!   narrows it. [`server`] carries that limit beside the mechanism.
//! * **Resources and prompts.** A gateway of the shape this product runs behind surfaces tools and
//!   ignores both, so anything load-bearing has to be a tool. The glossary and the catalog prose stay
//!   where they are - in `sutura_app::prompt`, advisory, for a cooperative client.
//! * **A composition root.** Nothing below `serve_stdio` cares about transport, and the binary that
//!   calls it is `sutura`'s `mcp` subcommand - the single-player answer to *which binary gets it*,
//!   composed in `sutura-cli` the way `query` is. This crate deliberately does not decide that; it
//!   is the transport a composition root calls.

mod principal;
mod refusal;
pub mod server;
#[cfg(test)]
mod testing;
pub mod tool;
pub mod wire;

pub use server::AgentSurface;

use sutura_app::surface::Surface;

/// Serves the agent surface over standard input and output, until the client disconnects.
///
/// The transport an agent client launches a server over: it spawns the process and speaks the
/// protocol on its pipes. There is no socket, no port and no listener, which is also why there is no
/// authentication here - the process boundary is the boundary, and a deployment that needs a
/// network-reachable agent surface needs the identity leg `docs/adr/0014` designs first.
///
/// Takes [`std::sync::Arc<S>`] rather than an owned `S`, for the one edge the engine's own drop
/// cannot cover. The service's engine shuts its nested runtime down through `shutdown_background`, so
/// releasing it is safe on any thread once no question is in flight - and the rmcp task ending is
/// normally that state. What would still abort is releasing the engine in the middle of an answer,
/// while its runtime is inside a `block_on` on a pool thread and this process aborts on a panic. The
/// composition root's outer handle defers that release until its own `shutdown_timeout` has let the
/// in-flight answer finish.
///
/// **`permitted` is required for the same reason it is on [`AgentSurface::new`]: a pipe has no
/// header a token could arrive in, so this transport alone cannot choose who the peer is. The
/// composition root decides** - `sutura`'s `mcp` subcommand passes `Permitted::every_capability`
/// and prints that at startup - so the value lives next to the notice that states it rather than
/// hidden in this function.
///
/// **`admission` is required for the same reason and answers a different question.** rmcp serves
/// requests concurrently - one task per request, and the SDK caps nothing - so without a bound every
/// question a peer sends is executing at once. The surface has no state a question mutates, so the
/// concurrency itself is free; what is not free is the blocking pool thread and the data system each
/// question holds. `sutura_runtime::Admission` is the number of those that may be in flight, the
/// composition root reads it from `runtime.max_concurrent_queries`, and one `Admission` bounds every
/// transport a process serves because its clones share one permit set - held since
/// `telekom/sutura#340` by `cargo xtask check-one-bound`, which counts the construction sites.
///
/// **`reply` is required and bounds the third thing: how long the peer waits.** It is
/// `server.request_timeout_seconds`, the same key the HTTP surface answers `408` from, and it had no
/// counterpart here at all - `telekom/sutura#339`. A question that got a slot waited for as long as
/// the data system took, and a peer that cancelled or disconnected stopped nothing and learnt
/// nothing. The composition root passes the number it read and prints it beside the posture at
/// startup.
///
/// **What that leaves to the engine, stated so the three are not confused:** the working-set ceiling
/// bounds how large one answer may get, the admission bound is how many answers may be being
/// produced, and the reply deadline is how long one peer waits for one of them. None of the three
/// cancels a question already inside the pool - see [`server`] and #160.
///
/// Returns when the peer closes or is cancelled.
///
/// # Errors
///
/// [`NotServed::Handshake`] if the client never completes `initialize`, and
/// [`NotServed::Interrupted`] if the task driving the session did not finish - a panic, or a runtime
/// shutting down underneath it.
pub async fn serve_stdio<S>(
    service: std::sync::Arc<S>,
    permitted: sutura_app::Permitted,
    prose: sutura_app::prompt::CatalogProse,
    admission: sutura_runtime::Admission,
    reply: sutura_config::RequestTimeout,
) -> Result<(), NotServed>
where
    S: Surface,
{
    let running = rmcp::serve_server(
        AgentSurface::new(service, permitted, prose, admission, reply),
        rmcp::transport::stdio(),
    )
    .await
    .map_err(|cause| NotServed::Handshake { cause: Box::new(cause) })?;
    running
        .waiting()
        .await
        .map(|reason| tracing::info!(?reason, "the agent surface's peer went away"))
        .map_err(|cause| NotServed::Interrupted { cause })
}

/// Why the agent surface stopped, when it was not the peer going away.
///
/// **Both variants re-export a third-party error type as a `#[source]`, and that is a deliberate
/// exception worth naming in review.** `AGENTS.md` allows only our own or standard-library errors
/// across a crate boundary and records that a variant carrying somebody else's type is a review
/// question rather than a gate. It is carried here because the alternative is worse: the handshake
/// failure is the SDK's own account of what the peer sent, and flattening it to a sentence would
/// throw away the only description of the fault that exists.
#[derive(Debug, thiserror::Error)]
pub enum NotServed {
    /// The peer never completed the protocol handshake.
    ///
    /// Boxed, and the reason is a lint this workspace keeps on deliberately: the SDK's own
    /// initialize error is nearly five hundred bytes, and `result_large_err` is denied here because
    /// *"a service whose public surface is `ToolOutcome::Refusal` wants to know when the error half
    /// of every `Result` grows"*. The indirection costs an allocation on a path that has already
    /// failed, and `Box<E>` is still an `Error`, so the `#[source]` chain is unchanged.
    #[error("the agent client did not complete the protocol handshake")]
    Handshake {
        #[source]
        cause: Box<rmcp::service::ServerInitializeError>,
    },
    /// The task driving the session did not finish.
    #[error("the task serving the agent surface did not finish")]
    Interrupted {
        #[source]
        cause: tokio::task::JoinError,
    },
}
