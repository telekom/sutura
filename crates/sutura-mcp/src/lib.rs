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
//! Four properties are load-bearing and each has a test rather than a paragraph:
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
//! nobody's new supply-chain surface. No shipped artifact grows, because **no binary links this
//! crate yet**. `macros` is off, so no attribute of theirs writes code into this crate, and the
//! handler is three methods written by hand.
//!
//! # What is deliberately absent
//!
//! * **A row cap of this transport's own.** The answer cap is `max_rows + 1` and a result that hits
//!   it is refused rather than truncated. Whether an agent surface wants a lower advisory cap - and
//!   what number - is a real question **nobody has measured**, so the bound is left exactly where it
//!   is rather than guessed at here.
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
//! * **A composition root.** Nothing links this crate yet: [`serve_stdio`] is the entry point a
//!   binary would call, and which binary gets it - and how a deployment configures it - is a
//!   composition decision this slice does not take.

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
/// Consumes the service, wraps it in an `Arc`, and returns when the peer closes or is cancelled.
///
/// # Errors
///
/// [`NotServed::Handshake`] if the client never completes `initialize`, and
/// [`NotServed::Interrupted`] if the task driving the session did not finish - a panic, or a runtime
/// shutting down underneath it.
pub async fn serve_stdio<S>(service: S) -> Result<(), NotServed>
where
    S: Surface,
{
    // Every capability, and the reason is the transport rather than a preference: there is no header a
    // token could arrive in on a pipe, so there is no verified claim to narrow by - and a filter over
    // an unverified claim looks like a control and is not one. `AgentSurface::new` requires the value
    // so that this line is where the decision is visible, rather than a default nobody reads.
    let permitted = sutura_app::Permitted::every_capability();
    let running = rmcp::serve_server(
        AgentSurface::new(std::sync::Arc::new(service), permitted),
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
