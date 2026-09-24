<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-mcp

The public API of `sutura-mcp`, rendered from rustdoc JSON.

The agent-facing surface: one governed tool, over the Model Context Protocol.

**Transport only.** It parses the wire shape, translates it into a domain `Query`, calls
`sutura_app::surface::Surface`, and maps the outcome back. Nothing here decides what a question
may ask: no predicate is assembled, no dimension is checked, no bound is applied. Every rule is
on the other side of that call, which is what makes this crate reviewable by reading its
translation rather than its judgement.

`Query`: sutura_domain::query::Query

# What this crate is

**One crate, one transport, and the whole tool set** - which is two tools, because
`sutura_app::surface::Surface` has two operations: list what this deployment measures, and answer
one governed question about one metric. `sutura_app::Capability` is the one source for which those
are, so **the two transports cannot disagree about what this deployment offers**; each of them
renders that source and each has a `both_transports_describe_the_same_tools` test asserting it did
not deviate.

Five properties are load-bearing and each has a test rather than a paragraph:

* **The schema is generated.** `tool::input_schema` is `schemars::schema_for!` over a wire type
  in `wire` - there is no hand-written JSON object in this crate - and every tool's generated
  bytes are committed as a snapshot, so a new or widened tool input lands in a reviewer's diff.
  That byte-compare is a mechanism `AGENTS.md` describes as owed rather than standing; this crate
  is what owes it.
* **A tool a caller may not invoke is neither advertised nor answered.**
  `AgentSurface::new` requires an `Asking`, which resolves to a `sutura_app::Permitted` either
  once at construction (`TheProcessOwner`) or fresh per call (`PerRequest`); `tools/list` filters
  on it and `tools/call` refuses on it. The filtering is presentation and the refusal is the
  control - see `server`, which says so in a table, and says plainly that nothing over standard
  input and output narrows the set today.
* **A refusal is a RESULT.** It comes back inside a tool result with the error flag unset, not as
  a JSON-RPC error and not as `isError`. See `server` for the three channels and why they are
  three.
* **`deny_unknown_fields` survives the transport.** An argument named `sql`, `table` or
  `predicate` is a named parse error, asserted through a real client rather than assumed.
* **The configured number of questions execute at once, and no more.** The bound is
  `sutura_runtime::Admission`, built by the composition root from
  `runtime.max_concurrent_queries` rather than by this crate, and the permit belongs to the
  blocking work rather than to the future waiting for it. `server` carries both halves of that
  argument and the limit on how far the second is exercised over the wire.
* **A peer's whole wait is bounded too, and it was not** - `telekom/sutura#339`. The admission
  window bounded a question that could not START; a question that got a slot waited for as long
  as the data system took, because rmcp applies no per-request deadline and this transport
  composed no equivalent of the HTTP surface's `tower` layer. It is
  `server.request_timeout_seconds` now, wrapping the admission wait as well as the answer - so
  the key means on this transport what it means on the other, which took a second pass to get
  right. It bounds the WAIT and stops no work; `server` says which key, why that one, what
  the arithmetic used to be, and what a cancelling peer still does not get.

# Why the protocol comes from a dependency

MCP is JSON-RPC with a handshake, and framing it by hand is a few hundred lines. It was
considered and rejected on evidence rather than on effort: a server we framed ourselves could be
tested only against our own parser, and *"a test asserting on source text proves nothing"* is a
rule this repository already holds. `rmcp` is the protocol's own Rust SDK, so the tests in
`server` drive this handler with **its** client over an in-memory pipe - which is the only shape
of evidence that says an agent client can actually ask.

**What the dependency costs, measured rather than waved at:** `rmcp` with `server` and
`transport-io`, plus `schemars`, adds **eight** packages to `Cargo.lock` that were not there -
`rmcp`, `pastey`, `schemars`, `schemars_derive`, `dyn-clone`, `ref-cast`, `ref-cast-impl` and
`serde_derive_internals` - counted by diffing the package names against the base branch rather
than read off a dependency tree. Everything else it wants was already in the lock: `tokio-util`,
`uuid`, `chrono` and `futures` arrive here through this crate for the first time, but they are
nobody's new supply-chain surface. The only shipped artifact that grows is `sutura` itself,
which links this crate behind its `mcp` subcommand - and the dependency is pure Rust, so nothing
about the two musl triples changes. `macros` is off, so no attribute of theirs writes code into
this crate, and the handler is three methods written by hand.

# What is deliberately absent

* **A row cap of this transport's own.** The answer cap is `max_rows + 1` and a result that hits
  it is refused rather than truncated. Whether an agent surface wants a lower advisory cap - and
  what number - is a real question **nobody has measured**, so the bound is left exactly where it
  is rather than guessed at here.
* **A bound on the SIZE of what arrives**, which is `#266`'s `H4` and is a different thing from
  the admission bound this crate now applies. rmcp's stdio transport reads a line off the
  process's own standard input with no cap, so one enormous line is read before anything parses
  it. Nothing here can bound it: the reader is the SDK's, and the boundary is the process - a
  peer that can write to this pipe can already launch the process. It stays named rather than
  claimed as covered.
* **A caller identity reaching this surface over the pipe.** `Asking` has a `PerRequest` arm -
  `telekom/sutura#378` PR2 - but `serve_stdio` still only ever builds `Asking::TheProcessOwner`,
  and that is not a placeholder: over standard input and output there is no header a token could
  arrive in, so `crate::principal::established()` (still
  `sutura_domain::identity::Subject::TheDeploymentItself`) is the honest chain for this transport
  regardless of what the type can now express. `sutura_http::inbound` is where leg 1 lives and it
  is unreachable from here - an adapter never calls another adapter - so, over the pipe,
  `Asking::PerRequest` is read but never produced. Behind this crate's own default-off `http`
  feature, `http::service` is what produces one over a real request - the streamable-HTTP
  transport - and PR4 (`#758`) is the composition root that already mounts it, behind its own
  optional `agent` feature and a settings switch; nothing this repository publishes turns that
  feature on.

  **The consequence for what a scope gates here is stated rather than left implicit:** the
  capability set this surface offers is narrowable, and over standard input and output nothing
  narrows it - `serve_stdio` always builds `Asking::TheProcessOwner` from one `Permitted` fixed at
  startup. `server` carries that limit beside the mechanism.
* **Resources and prompts.** A gateway of the shape this product runs behind surfaces tools and
  ignores both, so anything load-bearing has to be a tool. The glossary and the catalog prose stay
  where they are - in `sutura_app::prompt`, advisory, for a cooperative client.
* **A composition root.** Nothing below `serve_stdio` cares about transport, and the binary that
  calls it is `sutura`'s `mcp` subcommand - the single-player answer to *which binary gets it*,
  composed in `sutura-cli` the way `query` is. This crate deliberately does not decide that; it
  is the transport a composition root calls.

## `enum Asking`

```rust
pub enum Asking
```

How `AgentSurface` learns who is asking, for one call.

**Not an `Option<Permitted>`, and that is the whole point of the type.** An absent value inside
`PerRequest` is a REFUSAL - see `server` - and folding "no identity" and "the deployment's own
identity" into the two sides of one `Option` would make the compiler unable to tell them apart at
the one call site that matters: an exhaustive match over this enum with no wildcard arm is what
stops a later edit from quietly substituting one for the other, the way `serve_stdio`'s own
unconditional `principal::established()` call used to before this type existed.

# Why the identity is not a field on `AgentSurface` itself

A field would be set once, when the surface is constructed, and read on every call after - which
is exactly right for `TheProcessOwner` and exactly wrong the moment a caller identity can vary
per request. `docs/adr/0023` names this trap by its mechanism: the pinned MCP SDK builds a
session's handler ONCE (`service_factory`), so an identity cached anywhere on `self` is
per-session by construction and looks correct in every single-caller test. `PerRequest` instead
names a MODE, and the value itself is read fresh out of the request's own
`rmcp::service::RequestContext::extensions` on every call - see `server` for where.

### Variants

- `TheProcessOwner` - The launching identity is the subject, for the whole life of this surface. The pipe's own shape and a decision rather than a gap - see `serve_stdio` and the module documentation's *what is deliberately absent* section.
- `PerRequest` - Established fresh from each request's own carried `sutura_app::Asked`. An absent value is a refusal, never `TheProcessOwner`'s fallback - see `rmcp::ServerHandler::call_tool`.

  Nothing in this crate produces an `Asked` today: over standard input and output there is no
  request to read one from. The arm exists so the exhaustive match in `server` is already
  total the day an HTTP transport starts producing one, rather than growing a second match
  somebody has to remember to make exhaustive under `-D warnings` later.

### Implements

`Clone`, `Debug`

## `enum NotServed`

```rust
pub enum NotServed
```

Why the agent surface stopped, when it was not the peer going away.

**Both variants re-export a third-party error type as a `#[source]`, and that is a deliberate
exception worth naming in review.** `AGENTS.md` allows only our own or standard-library errors
across a crate boundary and records that a variant carrying somebody else's type is a review
question rather than a gate. It is carried here because the alternative is worse: the handshake
failure is the SDK's own account of what the peer sent, and flattening it to a sentence would
throw away the only description of the fault that exists.

### Variants

- `Handshake` - The peer never completed the protocol handshake.

  Boxed, and the reason is a lint this workspace keeps on deliberately: the SDK's own
  initialize error is nearly five hundred bytes, and `result_large_err` is denied here because
  *"a service whose public surface is `ToolOutcome::Refusal` wants to know when the error half
  of every `Result` grows"*. The indirection costs an allocation on a path that has already
  failed, and `Box<E>` is still an `Error`, so the `#[source]` chain is unchanged.
- `Interrupted` - The task driving the session did not finish.

### Implements

`Debug`, `Display`, `Error`

## `fn serve_stdio`

```rust
pub async fn serve_stdio<S>(service: std::sync::Arc<S>, permitted: sutura_app::Permitted, prose: sutura_app::prompt::CatalogProse, admission: sutura_runtime::Admission, reply: sutura_config::RequestTimeout, instructions: std::sync::Arc<str>) -> Result<(), NotServed>
```

Serves the agent surface over standard input and output, until the client disconnects.

The transport an agent client launches a server over: it spawns the process and speaks the
protocol on its pipes. There is no socket, no port and no listener, which is also why there is no
authentication here - the process boundary is the boundary, and a deployment that needs a
network-reachable agent surface needs the identity leg `docs/adr/0014` designs first.

Takes `std::sync::Arc<S>` rather than an owned `S`, for the one edge the engine's own drop
cannot cover. The service's engine shuts its nested runtime down through `shutdown_background`, so
releasing it is safe on any thread once no question is in flight - and the rmcp task ending is
normally that state. What would still abort is releasing the engine in the middle of an answer,
while its runtime is inside a `block_on` on a pool thread and this process aborts on a panic. The
composition root's outer handle defers that release until its own `shutdown_timeout` has let the
in-flight answer finish.

**`permitted` is required for the same reason `AgentSurface::new` requires an `Asking`: a pipe
has no header a token could arrive in, so this transport alone cannot choose who the peer is. The
composition root decides** - `sutura`'s `mcp` subcommand passes `Permitted::every_capability` and
prints that at startup - so the value lives next to the notice that states it rather than hidden
in this function. This function wraps it as `Asking::TheProcessOwner` before handing it to the
surface; there is no path through `serve_stdio` to `Asking::PerRequest` at all.

**`admission` is required for the same reason and answers a different question.** rmcp serves
requests concurrently - one task per request, and the SDK caps nothing - so without a bound every
question a peer sends is executing at once. The surface has no state a question mutates, so the
concurrency itself is free; what is not free is the blocking pool thread and the data system each
question holds. `sutura_runtime::Admission` is the number of those that may be in flight, the
composition root reads it from `runtime.max_concurrent_queries`, and one `Admission` bounds every
transport a process serves because its clones share one permit set - held since
`telekom/sutura#340` by `cargo xtask check-one-bound`, which counts the construction sites.

**`reply` is required and bounds the third thing: how long the peer waits.** It is
`server.request_timeout_seconds`, the same key the HTTP surface answers `408` from, and it had no
counterpart here at all - `telekom/sutura#339`. A question that got a slot waited for as long as
the data system took, and a peer that cancelled or disconnected stopped nothing and learnt
nothing. The composition root passes the number it read and prints it beside the posture at
startup.

**What that leaves to the engine, stated so the three are not confused:** the working-set ceiling
bounds how large one answer may get, the admission bound is how many answers may be being
produced, and the reply deadline is how long one peer waits for one of them. None of the three
cancels a question already inside the pool - see `server` and #160.

**`instructions` is the fourth required value, and it is what a peer's `initialize` result
carries as `instructions` - `telekom/sutura#776`.** It has to be rendered before this call, by
`sutura_app::prompt::render` over the settings and the pinned bundle this `service` answers
from, because this crate performs no catalog I/O of its own; the composition root that already
read both is `sutura`'s `mcp` subcommand.

Returns when the peer closes or is cancelled.

# Errors

`NotServed::Handshake` if the client never completes `initialize`, and
`NotServed::Interrupted` if the task driving the session did not finish - a panic, or a runtime
shutting down underneath it.

## `use AgentSurface`

The agent-facing surface over one `Surface`.

Holds the service behind an `Arc` because a tool call is answered on the blocking pool, so the
port has to outlive the future that started the call.

## Module `http`

The streamable-HTTP transport, default-off behind this crate's own `http` feature.

`telekom/sutura#378` PR3, `docs/adr/0023`. See its own module documentation for what it does
and, as importantly, what it does not: nothing served mounts it yet.
The streamable-HTTP transport, as a plain `tower_service::Service` a composition root mounts.

**`#[cfg(feature = "http")]` only** - `telekom/sutura#378` PR3, `docs/adr/0023`. Nothing served
links this today: `crate::http::service` builds a value a composition root's own router can
`axum::Router::nest_service` behind its existing layers, so `sutura-cli`'s leg 1
(`sutura_http::inbound`) and `sutura_http::capability::establish_asked` run in front of it
exactly as they run in front of every other route on that surface - PR4's job. This crate carries
no dependency on `axum` outside its own `#[cfg(test)]` tests: `StreamableHttpService` is a bare
`tower_service::Service`, which is what a transport crate composes against rather than an
adapter's own router type - see `.agents/skills/sutura/crate-map/SKILL.md`'s "a transport is
transport-only."

# `Asking::PerRequest`, and why this exists at all

`crate::Asking::PerRequest` reads a `sutura_app::Asked` out of
`context.extensions.get::<http::request::Parts>().and_then(|parts| parts.extensions.get::<Asked>())`
(`server.rs`). Traced against the pinned SDK's own streamable-HTTP server, that `Parts` value is
exactly the request's own `http::request::Parts` - the SAME value
`sutura_http::capability::establish_asked` inserts an `Asked` into on the HTTP surface's other
routes, before that request ever reaches the nested tower service (`docs/adr/0023`, quoting
`streamable_http_server/tower.rs`). `crate::http::service` is what makes a REAL one of those reachable at
all; until PR4 mounts it behind the real `establish_asked` layer, this module's own tests stand
in for that layer with a fake one, inserting a chosen `Asked` the same way.

# Session semantics: `legacy_session_mode: false`, and what that decision rests on

`crate::http::config` pins `StreamableHttpServerConfig::legacy_session_mode` to `false` rather than the
SDK's own default (`true`). Read against the pinned transport's own `handle_post`
(`transport/streamable_http_server/tower.rs`): with `legacy_session_mode: false`, EVERY request -
`initialize` included - is served through its "Stateless mode" branch, which calls this crate's
own `service_factory` closure FRESH per request (one new `crate::AgentSurface` per call, never
reused across requests) and drives exactly one message through
`rmcp::transport::OneshotTransport` - never `SessionManager::create_session`,
`has_session` or `create_stream`. A `Mcp-Session-Id` header is never looked up under this
config, by ANY message type - confirmed by reading the branch rather than assumed from its name.

**This is the property the invariant asks for, held by absence rather than by a check: a
streamable-HTTP session cannot outlive or cross the verified caller, because under this config
there is no session for one request's identity to leak into another's.** Nothing here refuses a
caller that PRESENTS a foreign or fabricated `Mcp-Session-Id` - that header is simply inert - and
this module's own `tests::a_reused_session_id_carries_no_weight_across_two_different_callers` is
the cell that proves it: two requests carrying the SAME fabricated header, two different fake
`Asked` values, and each response reflects only its OWN request's identity.

**`json_response: true` is pinned alongside it, and it is a second, narrower decision.** Neither
`AgentSurface::list_tools` nor `AgentSurface::call_tool` ever emits an intermediate
notification or a server-initiated request ahead of its final reply - `sutura_mcp` has never had
a use for either - so the SDK's own documented fallback ("if the handler emits a notification or
request before the final response, the server falls back to `text/event-stream`") never fires
for this deployment's own tool set, and every ordinary call gets back a plain JSON body instead
of an SSE stream to parse. A future tool that DID need to stream would need this reconsidered.

**What this module does NOT settle, named rather than left implicit:**
- **A client negotiating the SDK's OWN advertised `LATEST` protocol version (`2025-11-25`) is
  still served correctly under this config, including its very first `tools/list` call with no
  prior handshake required.** Read carefully because `handle_post` has TWO `else` arms that are
  easy to conflate at a skim: the one reached when `legacy_session_mode: true` and a message
  arrives with no `Mcp-Session-Id` header (which DOES require the message to be an
  `InitializeRequest` or `DiscoverRequest`, refusing anything else) and the one reached when
  `legacy_session_mode: false` (the "Stateless mode" arm, a sibling of the whole
  `if use_session {}` statement, not nested inside it) - which serves EVERY message type
  one-shot, `initialize` included. `crate::http::config`'s pin selects the second; the cells in this
  module exercise it directly rather than trust this paragraph.
- **`allowed_hosts`/`allowed_origins` are left at the SDK's own defaults**
  (`["localhost", "127.0.0.1", "::1"]`, no origin check) - a composition root serving this
  outside loopback must override them, or the transport refuses every request with a `Host`
  header it does not recognise. PR4's job to state, not this module's.
- **The exact SEP-2243 header-validation helpers this module's tests exercise
  (`validate_standard_headers`, `validate_request_protocol_version_meta`) were read for their
  no-op conditions on a plain, non-`stateless_protocol_metadata_required` request and not
  exhaustively traced line by line.** Flagged as the narrowest residual risk in this file; it
  is settled by the four cells below compiling and passing against the pinned SDK, not by an
  exhaustive manual trace.

### `fn config`

```rust
pub fn config() -> rmcp::transport::StreamableHttpServerConfig
```

This deployment's fixed transport configuration - see the module documentation for
`legacy_session_mode` and `json_response`.

A function rather than a `const`: `StreamableHttpServerConfig` is `#[non_exhaustive]` - a
struct-expression literal cannot name its fields at all - and its `Default` builds a fresh
`CancellationToken`, so the two pins below can only be applied through the SDK's own builder.

**The limit the two pins carry, stated rather than assumed contractually:** only
`legacy_session_mode` and `json_response` are set here; the other eight fields are inherited
from the SDK's `Default` through the builder and are not pinned - a future field with an unsafe
default would arrive silently, and `allowed_hosts` stays loopback-only, so a composition root
serving outside loopback must override it (the transport refuses every unrecognised `Host`, see
the module documentation). And `service` still constructs a `LocalSessionManager`; that
manager is kept idle by `legacy_session_mode: false` alone. Nothing here binds a session to a
caller, and the SDK's own `create_session` takes no identity argument regardless - the caller is
re-resolved per request out of each request's `Asked`, never out of a session.

### `fn service`

```rust
pub fn service<S>(surface: std::sync::Arc<S>, prose: sutura_app::prompt::CatalogProse, admission: sutura_runtime::Admission, reply: sutura_config::RequestTimeout, instructions: std::sync::Arc<str>) -> rmcp::transport::StreamableHttpService<crate::AgentSurface<S>, rmcp::transport::streamable_http_server::session::local::LocalSessionManager>
```

Builds the streamable-HTTP transport over one `Surface`, as a plain `tower_service::Service`
for a composition root to `nest_service` behind its own layers.

**Always `Asking::PerRequest`, and that is not a parameter.** This constructor exists
specifically for a transport reachable over a network; `Asking::TheProcessOwner` is
`crate::serve_stdio`'s answer for a pipe, and giving this function a choice would make "which
one" a call-site decision a composition root could get backwards. See `crate::Asking` for why
the two are not interchangeable.

`service_factory` is called by the SDK ONCE PER REQUEST under `config`'s stateless mode (see
the module documentation) - never once per process and never once per session - so each call
clones the shared `service`/`admission` handles rather than allocating a second data-system
connection or a second permit set. `instructions` is cloned the same way, and for the same
reason it is an `Arc<str>` rather than a `String`: this factory runs on every request, not only
on the `initialize` that reads it back.

## Module `server`

The handler: `tools/list`, `tools/call`, and the three channels a caller has to be able to tell
apart.

# Three outcomes, three channels, and the middle one is the property that matters

| What happened | How it comes back | Why |
| --- | --- | --- |
| The question was answered | a tool result, `isError` absent, `outcome: "answer"` | rows inline, provenance beside them |
| The question was **refused** | a tool result, `isError` absent, `outcome: "refusal"` | it is a governance *result*, and nothing about it went wrong |
| The arguments were not a question | a JSON-RPC error, `-32602` | a parse failure, named, before the service is reached |
| The service could not answer | a tool result with `isError: true`, and no detail | something went wrong, and the detail is a path or a table |
| Every execution slot was taken for the whole admission window | a tool result with `isError: true`, and a sentence saying to ask again | the question was never judged, so it is not a refusal - and unlike the row above, waiting is the fix |
| The reply outran `server.request_timeout_seconds` | a tool result with `isError: true`, and a sentence saying the question may still be running | the peer's WAIT is bounded and the question is not: see the section on the reply deadline |
| The peer cancelled a running call | no response | rmcp suppresses it; the handler stops waiting, while the question and its slot continue |

**A refusal is not an error and must not look like one.** `sutura_app::surface::Surface::answer`
is where a transport inherits that, and its own doc comment says why: a caller must not be able to
mistake "you may not ask that" for a hiccup and retry until something works. An `isError: true`
refusal would be exactly that mistake, in the one place a model is most likely to act on it - a
model told a call errored retries, and a model told the answer is "no, because the catalog defines
no such metric" asks something else.

# Why a malformed argument is a JSON-RPC error rather than `isError`

Because it is not a tool *execution* failure - the tool never ran. `-32602` is the name JSON-RPC
already has for it, `MalformedQuestion` names the field, and it is the one channel a caller cannot
read as either an answer or a refusal.

**The limit, stated with the claim:** rmcp's own documentation notes that clients typically render
a protocol error opaquely, so a model may see less than the message carries. `Ok(isError: true)`
was the alternative and was rejected: it would put "your arguments were wrong" in the same channel
as "the data system is down", and the first is fixable by the caller while the second is not.

# Why the port call leaves the async worker

`Surface::answer` is synchronous, and the engine behind it drives its own runtime - entering a
runtime from within a runtime panics. So the call goes to the blocking pool through
`sutura_runtime::spawn_carrying_span`, which is the one call `clippy.toml` permits for this,
because a bare `spawn_blocking` loses the request's span on a pool thread.

# How many questions may be executing, and where the permit lives

**`sutura_runtime::Admission` and not a bound of this transport's own**, because the resource
is the *process*: one blocking pool, one set of data systems, and two independently sized
semaphores would be two controls each reporting a limit the other can exceed. So the value
arrives at `AgentSurface::new` from a composition root that read
`runtime.max_concurrent_queries`, and every clone of an `Admission` shares one permit set -
which is what lets one process serve two transports under one number.

**The slot is taken before the blocking task is spawned and released INSIDE it.** Taken inside
would be a pool thread already occupied while waiting for permission to occupy one; released by
the async worker would make it a bound on *starting* work rather than on running it, and
`tokio` documents that a started blocking task cannot be aborted - so a caller that has gone
away does not stop the question it asked. A permit handed back early is worse than no bound at
all, because it reads as a control.

**This defines the response to running out of admission and nothing about stopping work.** A
shed call is answered on the third channel above, inside the bounded admission window. What
neither this nor the bound does is cancel a question that is already executing: the `Warehouse`
port is synchronous and carries no deadline, so a question inside the pool runs to completion
whatever the peer is told - and it keeps its slot until it does, which is exactly why the
backlog is a number somebody chose rather than memory. Making running work stoppable is #160's
subject, on the port rather than on either transport.

# How long a peer waits for a reply, and what happens when that runs out

**`telekom/sutura#339`: the admission window was the only bounded wait on this surface.** A
question that could not get a slot came back inside `runtime.admission_timeout_seconds`; a
question that GOT one waited as long as the data system took, with nothing in the picture to end
it. rmcp applies no per-request deadline of its own, so there was no other bound to
inherit - measured on the pinned SDK and not read off its documentation.

So `AgentSurface::new` takes `server.request_timeout_seconds` as well, and the one function
that awaits the port waits under it. When it expires the peer is answered on the fourth
channel above. **The same key the HTTP surface answers `408` from, and reusing it rather than
inventing a key of this transport's own is a decision with two arguments:**

* A second key would be a second number for one fact - *how long a caller waits for a reply* -
  and the two transports would then be able to disagree about it while sharing one execution
  bound.
* The number is already load-bearing on this composition. `sutura`'s `mcp` command opens the
  port's own `Deadline` from it (`docs/adr/0029`), and the in-process engine gives up against
  that deadline at a cooperative yield - so the engine on this transport already answers to this
  key; before this change the *peer* was the only party in that arithmetic with no deadline at
  all. This bullet used to say a `bigquery` job derived `timeoutMs`/`jobTimeoutMs` from it
  directly; those were `jobs.query` request parameters on a transport that is deleted. A
  `BigQuery` job over ADBC is sent what is left of the port deadline as its `jobTimeoutMs`.

The key's name says `server` and this transport binds no listener, which is the one argument
against reusing it. It is a naming cost rather than a behavioural one, and it is cheaper than
two numbers for one wait.

**What the key bounds is the WHOLE wait, on both transports, and that took a second pass.** On
HTTP it is an outer `tower` layer, so it covers the admission wait as well as the answer. This
transport applied it *after* `admit` at first, which made a peer's worst case
`admission_timeout_seconds + request_timeout_seconds` - one key with two meanings, which is
precisely the divergence reusing the key was chosen to prevent, one level up from the permit
set. Found by a review reading both paths. `answer` now wraps both waits in the one deadline,
so the two surfaces mean the same thing by the same number and the shipped defaults behave
exactly as before: a 5-second window inside a 30-second deadline, the window expiring first, a
shed question still answered at-capacity.

**What the deadline does NOT do, stated with it, because it is the same limit the admission
bound has:** it does not stop the question. The permit is owned by the blocking closure, so a
question whose reply deadline fired keeps its slot until the data system answers it - the
deadline bounds the peer's wait and nothing else. That is deliberate and it is why the sentence
the peer gets does not say *try again*: repeating the question would take a second slot while
the first is still running. Making running work stoppable is #160's subject, on the port.

**Peer cancellation ends this transport's wait and nothing below it.** rmcp delivers
`notifications/cancelled` through `RequestContext::ct`, and `ServerHandler::call_tool` selects
on that token while a question is pending. The handler returns before its configured deadline;
rmcp then suppresses its response on the wire. The blocking `Surface::answer`
call cannot be aborted, so it keeps running and owns its execution slot until it returns. This
is `telekom/sutura#362`; making the data work itself stoppable remains #160, on the port rather
than on this transport.

**What this transport still does not bound is the size of what it reads**, which is `#266`'s
`H4`: `rmcp`'s stdio transport reads a line off the process's own input with no cap, and this
change is about a different thing - how many questions execute at once.

**The limit on how far cancellation is exercised, stated with it.** The MCP test sends
`notifications/cancelled` over an in-memory protocol connection and observes the pinned SDK's
suppression plus this handler's captured diagnostic. It does not claim that closing a transport
produces the same notification. The slot-retention assertion reads the held port before release:
cancellation drops the future waiting on the blocking task, never the task or the permit it owns.

# What this slice does NOT do, on purpose

* **No row cap of its own.** The answer cap is `max_rows + 1` and a result that hits it is
  refused rather than truncated; whether an *agent* surface wants a lower advisory cap - and what
  number - is a real question nobody has measured, so the bound is left exactly where it is.
* **No `get_tool`.** Implementing it would have rmcp validate arguments against the advertised
  schema before this handler sees them, which is defence in depth and also a second enforcement
  point whose message is not ours. One gate, and it is `crate::wire::AskArgs`'s own
  `deny_unknown_fields`.

# What a scope gates here, and where the control actually is

`AgentSurface::new` **requires** an `Asking`, so a composition root cannot forget to say who
may ask and what they may do - the same reason `sutura_app::surface::LocalService::start` requires
an audit sink. Given one, `AgentSurface::asked` resolves it to a
`sutura_app::Asked` for THIS call - once per `TheProcessOwner` construction, fresh per request
under `PerRequest` - and this handler does two things with the result, only the second a control:

| Where | What it does | What it is |
| --- | --- | --- |
| `tools/list` | drops a tool the peer may not invoke | **presentation** |
| `tools/call` | refuses a capability the peer was not granted, advertised or not | **the control** |

Both read the same `sutura_app::Asked` for the call, so they cannot disagree -
`sutura_app::capability` holds that argument and the test for it. A caller that guessed
`ask_metric` without ever being shown it is refused by the second row, which is why the first is
described as presentation rather than as security.

**A third case exists only under `Asking::PerRequest`, and it is neither row above: no
established caller at all.** That is refused before either row is reached -
`AgentSurface::asked` returns the error and neither `tools/list` nor `tools/call` gets as far as
asking `Permitted::includes` anything. See `crate::Asking` for why this is an enum with no
`Option` anywhere in it: the alternative reading of "nothing established" is "the deployment's own
identity," and that reading is exactly what this shape exists to make unrepresentable.

**And the honest limit, which is not small:** nothing that ships narrows the set here.
`crate::serve_stdio` always builds `Asking::TheProcessOwner { permitted: Permitted::every_capability() }`,
because this transport speaks over standard input and output and there is no header a token could
arrive in - `docs/adr/0014`'s closing section says as much, and says that deciding how this
surface is reached at all is an architecture decision rather than a refactor.
`Asking::PerRequest` exists on the type and is exercised by this module's own tests, with hand-
built `RequestContext` values reusing a `Peer` a real handshake produced - `rmcp::service::Peer::new`
is `pub(crate)` in the pinned SDK, so nothing outside `rmcp` can mint one from nothing. Behind
this crate's own default-off `http` feature, `crate::http::service` now produces the value over
a real request - the pinned streamable-HTTP transport injects the request's own `Parts` into the
extensions this arm reads, and `http::tests` drives that over real bytes; PR4 is the composition
root that chooses to mount it behind the real `establish_asked` layer.

### `struct AgentSurface`

```rust
pub struct AgentSurface<S>
```

The agent-facing surface over one `Surface`.

Holds the service behind an `Arc` because a tool call is answered on the blocking pool, so the
port has to outlive the future that started the call.

#### Methods

```rust
pub const fn new(service: Arc<S>, asking: Asking, prose: sutura_app::prompt::CatalogProse, admission: Admission, reply: RequestTimeout, instructions: Arc<str>) -> Self
```

Wraps a service, and states what the peer may do and how catalog prose is treated.

Takes the `Arc` rather than making one, so a composition root serving two transports shares
one bundle and one data system rather than opening a second of each.

**`asking` is required rather than defaulted, and that is the point of the signature.** A
default here would be a posture chosen by this file for every deployment that ever links it;
`Asking::TheProcessOwner { permitted: Permitted::every_capability() }` is the right answer
over standard input and output and would be the wrong answer the moment this surface is
reachable over a network, and only a composition root knows which it is building. See the
module documentation and `crate::Asking` for what the value then gates and why it is a mode
rather than a bare grant.

**`prose` is required for the same reason, and it is a composition-root value.** `sutura`'s
`mcp` subcommand passes what this deployment renders; a `CatalogProse` with no default keeps
`quoted` from being a posture chosen here for a deployment that meant something else.

**`admission` is required and is not built here, and that is the third instance of the same
rule.** A bound this file constructed would be a number chosen for every deployment that
links it, and - worse - a *second* permit set in any process that also serves HTTP, where
two limits each reporting a bound the other can exceed is not a bound. So the composition
root reads `runtime.max_concurrent_queries` and `runtime.admission_timeout_seconds` and
hands one `Admission` to whatever serves. Since `telekom/sutura#340` that is held by
`cargo xtask check-one-bound` rather than by this paragraph.

**`reply` is required and is the fourth, and it is the one whose absence was a missing bound
rather than a misplaced one** - `telekom/sutura#339`. It is
`server.request_timeout_seconds`, the same key the HTTP surface answers `408` from, and with
no counterpart here a peer that got an execution slot waited for as long as the data system
took. The module documentation carries why this key rather than one of this transport's own,
and what the deadline does not stop.

**`instructions` is required and is the fifth, for the same reason as the rest: only a
composition root has read the settings and the pinned bundle both** - `sutura`'s `mcp`
subcommand and `sutura-cli`'s `serve::agent::mount` each render it with
`sutura_app::prompt::render`, the same call `sutura prompt` makes, over the same bundle
this `service` answers from. No default here, and deliberately: a sentence this crate hard-
coded could never have named a tool, a metric or a refusal that this deployment actually has.

#### Implements

`Debug`, `ServerHandler`

### `struct RenderedCause`

```rust
pub struct RenderedCause
```

A message built for `ErrorData::invalid_params`, and the only value `RenderedCause::into_error_data`
may turn into one.

**The limit, stated beside the claim.** `rmcp::ErrorData` is foreign, so this cannot sit in a
*field* the way `sutura_http::problem::Detail` sits in `Failure::NotAQuestion` - there is no
`ErrorData` field to hold it. What this buys instead is module privacy plus one call site: the
tuple field is private and `Self::of`/`Self::outer_only` are its only constructors, so no code
outside this module can mint one from an ad hoc string, and `Self::into_error_data` is the only
function that calls `ErrorData::invalid_params`, so neither renderer below reaches a peer without
going through it. It does **not** stop a *new* call to `ErrorData::invalid_params` written
elsewhere in this module with its own string - that is weaker than `Detail`'s field type, which
makes exactly that a compile error everywhere in the crate that builds a `Failure`.

## Module `tool`

The tools this surface advertises, and the schemas they are described by.

# There is one source for WHICH tools exist, and it is not this file

`sutura_app::Capability` is the tool set. This module decides how each capability is *described*
to a model and nothing else: the name comes from `Capability::id`, the gate comes from
`Capability::scope`, and `every` is a walk over `Capability::every()` with no second list to
keep in step with it.

That is the shape `docs/implementation-plan.md`'s `both_transports_describe_the_same_tools` asks
for. It could not be a comparison between the two transports - `sutura-mcp` and `sutura-http`
cannot see each other - so it is one source in the crate that owns the driving port, plus a test
in each transport asserting it did not deviate from that source. Two tests, one source, and
`tests::both_transports_describe_the_same_tools` is this crate's half.

**The prose is deliberately NOT shared.** A tool description is written for a model choosing
whether to call it; an `OpenAPI` summary is written for a person reading an interface description.
`AGENTS.md` already draws that line for the two refusal vocabularies - *"Nothing compares the two
sentences, and nothing should"* - and the same reasoning applies here.

# The schema is GENERATED

`input_schema` is `schemars::schema_for!` over a wire type and nothing else. There is no
hand-written JSON object anywhere in this crate, which is the property that had to be true from
the first line: a hand-written schema and a hand-written parser drift, and the drift is invisible
until a caller trusts the schema. Here the description a client reads and the deserializer that
enforces it come from one type - `additionalProperties: false` included, because `schemars` reads
the same `deny_unknown_fields` serde reads.

# The dump, and why it is a gate rather than a convenience

`.agents/skills/sutura/query-surface`'s governance-boundary table wants a widened tool input to be
caught by something a reviewer cannot miss. `tests::the_advertised_tool_schemas_are_the_committed_ones`
is that byte-compare: **every** capability's generated schema is snapshotted, so a new or widened
field changes a snapshot and the test fails until somebody re-accepts it. That puts the new surface
in the diff, which is the whole mechanism - `deny_unknown_fields` stops an *undeclared* field from
being answered, and this stops a *declared* one from arriving unreviewed.

Beside it, `tests::the_question_tool_takes_exactly_the_six_fields_a_question_has` asserts the
property rather than the bytes: a field named `sql`, `table`, `where`, `predicate` or `rows` is not
merely a snapshot change but a named failure. A snapshot can be re-accepted without thought; that
one cannot.

### `fn input_schema`

```rust
pub fn input_schema(capability: sutura_app::Capability) -> rmcp::model::JsonObject
```

One capability's input schema, generated from its wire type.

Built on demand rather than cached: it is computed once per `tools/list`, which is once per session
for every client that exists, and a `LazyLock` would buy nothing measurable while adding a static
nobody can see the value of in a test.

**The match is what ties a capability to a wire type**, and it is exhaustive: a capability with no
arguments type does not compile.

### `fn tool`

```rust
pub fn tool(capability: sutura_app::Capability) -> rmcp::model::Tool
```

One capability, as `tools/list` returns it.

### `fn every`

```rust
pub fn every(permitted: &sutura_app::Permitted) -> Vec<rmcp::model::Tool>
```

The tools this caller is advertised, in capability order.

**Presentation, and the module documentation says so plainly.** A caller that names a tool absent
from this list is refused by `crate::server::AgentSurface::call_tool` asking
`Permitted::includes` again, so filtering here is what a cooperative client is *shown* and not
what stops an uncooperative one.

### `fn named`

```rust
pub fn named(name: &str) -> Option<sutura_app::Capability>
```

The capability a tool name refers to, if this surface has one under that name.

**Independent of what was advertised**, which is the point: this answers *does this deployment
have such a tool* and the caller's permission is a separate question the server then asks. Folding
the two together is how a caller ends up told "no such tool" for one it holds the scope for.

## Module `wire`

The tool's arguments and its result, and the conversions to and from the domain.

# Why this crate has its own wire type

`sutura-http` already holds one - `QuestionBody`, with the same six fields. Sharing the STRUCT
would mean this adapter depending on that one, and *an adapter never calls another adapter* is
the rule the whole layout rests on: a shape owned by one transport is a shape every other
transport has to reach through it. `CatalogContent` is the same story against
`sutura_http::wire::CatalogBody`.

**So the wire STRUCT is deliberately duplicated, and it is a cost rather than an oversight.**
Nothing in the compiler makes two wire types stay equal. What guards them is `AskArgs`'s own
`deny_unknown_fields`, asserted through the transport in `crate::server`, plus the committed
schema dump in `crate::tool` - a widened input changes a snapshot and the byte-compare fails
until somebody re-accepts it, which is what puts a new field in a reviewer's diff.

**What moved inward is the PARSE, and it is not a cost any more.** `TryFrom<AskArgs> for Query`
used to re-derive the same seven failure modes `sutura-http`'s own `TryFrom` did, out of its own
copy of `MalformedQuestion`, its own `grain_of`, its own `range_of` - identical logic, kept equal
only by review. That translation lives in `sutura_domain::question` now, and what this crate
keeps of its own is the one failure mode a transport's own deserialization step can produce
before that function is ever reached: `MalformedQuestion::NotAnObject`, for an arguments object
that fails to deserialize into `AskArgs` at all - which HTTP's Axum extractor rejects earlier
in its own stack, so `sutura-http` has no arm for it and needs none.

**What is now mechanical across the two transports is the TOOL SET and the QUESTION PARSE, and
not these wire shapes.** `sutura_app::Capability` is the one source both of them render for the
first, and `both_transports_describe_the_same_tools` in `crate::tool` is the assertion;
`sutura_domain::question::parse_query` is the one function both `TryFrom` impls call for the
second. The field lists of two wire types with the same job are still kept equal by review, and
that limit is worth keeping in front of a reader rather than letting either test read as
covering it.

# Why the derive is here and not on `Query`

The schema has to be generated - a hand-written tool schema and a hand-written parser drift, and
the drift is invisible until a caller trusts the schema. The derive that generates it is
`schemars`, and `sutura-domain`'s whole transitive tree is walked by `cargo xtask
check-boundaries` against an allowlist, so putting a macro crate on a domain type is an
architecture decision rather than a convenience. The derive goes on the wire type, which is
where a wire format belongs anyway.

A wire type is also allowed to be *worse* than a domain type, and should be. Every field of
`AskArgs` is a plain string, because that is what arrives; each one is then parsed into the
newtype that establishes its invariant, and a failure names the field.

# `deny_unknown_fields`, and what it is for here

The governance boundary, across a JSON parser. Without it an arguments object carrying `sql:` or
`table:` deserializes cleanly with the extra key dropped on the floor, and a model that believes
it sent SQL is answered as though it had asked the modelled question instead. With it, the
attempt is a named parse error. `sutura_domain::query::Query` has no field for any of that; this
is what keeps that true on the way in.

It is also what `schemars` reads to emit `additionalProperties: false`, so the advertised schema
and the enforcement come from the same attribute rather than from two decisions that could
disagree.

### `struct AskArgs`

```rust
pub struct AskArgs
```

One governed question, as a tool call carries it.

The six fields are the whole input surface of this deployment. There is no field for SQL, a
table, a filter expression or a row-id list, and an argument naming one is a parse error rather
than a key that gets ignored.

#### Implements

`Debug`, `Deserialize<'de>`, `JsonSchema`

### `struct TopArgs`

```rust
pub struct TopArgs
```

A `top` clause: rank by `by`, in `direction`, keep the first `n`.

#### Implements

`Debug`, `Deserialize<'de>`, `JsonSchema`

### `struct RangeArgs`

```rust
pub struct RangeArgs
```

A half-open period: `start` is included, `end` is not. Either an absolute period
(`start`/`end`) or a period relative to today (`last`) - never both, never neither.

Half-open at every grain, which is what makes a month `[2026-06-01, 2026-07-01)` rather than a
last day that differs per month. Both dates are ISO `YYYY-MM-DD`.

#### Implements

`Debug`, `Deserialize<'de>`, `JsonSchema`

### `struct LastArgs`

```rust
pub struct LastArgs
```

A count of calendar periods before today, resolved at request time rather than authored as
dates - `telekom/sutura#778`.

#### Implements

`Debug`, `Deserialize<'de>`, `JsonSchema`

### `struct FilterArgs`

```rust
pub struct FilterArgs
```

One equality filter.

#### Implements

`Debug`, `Deserialize<'de>`, `JsonSchema`

### `enum MalformedQuestion`

```rust
pub enum MalformedQuestion
```

Why an arguments object is not a question.

#### Variants

- `NotAnObject` - The arguments object did not deserialize at all: a missing field, a wrong type, or - the case this crate cares about most - a field the tool surface does not declare.

  **The one variant with no HTTP analogue**, which is why it lives here rather than in
  `sutura_domain::question`: Axum's JSON extractor rejects a body that fails to deserialize
  before `TryFrom<QuestionBody> for Query` is ever reached, so `sutura-http`'s own
  `MalformedQuestion` has no arm for this case and does not need one. MCP's own
  `serde_json::from_value` step, in `crate::server`, is what can still fail this way here.
- `Question` - Every other way a question can be malformed: which field, and none of the caller's own value at any link of the chain `crate::server`'s `invalid()` walks - see that type's own note. Shared with `sutura-http`, which parses the same six fields into the same domain types and would otherwise carry its own copy of this whole vocabulary.
- `Range` - A relative `range` needs a failure mode the domain does not have and must not gain - see `sutura_runtime::relative_range`, the resolver `sutura-http` shares this variant's whole purpose with.

#### Implements

`Debug`, `Display`, `Error`

### `enum OutcomeContent`

```rust
pub enum OutcomeContent
```

What a question produced, as the tool's structured content.

**One shape, and the rows are IN it.** A handle-plus-fetch result was considered and withdrawn:
the rule it came from is that a federated join is done by the engine rather than by the model,
which `docs/adr/0007` already decides above this port, and read as a context-window rule it would
have cost an agent the ability to answer a question about a number without a second call.

The `outcome` discriminator is what a client branches on, and it is the same tag and the same
`reason` object the HTTP surface serializes - deliberately, so the two transports describe one
answer even though the types are separate.

#### Variants

- `Answer` - The question was answered.
- `Refusal` - The question was refused. **Still an `Ok`, and still a tool RESULT** - see `crate::server::AgentSurface::call_tool`.

#### Implements

`Debug`, `Serialize`

### `struct ProvenanceContent`

```rust
pub struct ProvenanceContent
```

Which definitions produced this answer.

Always present on an answer, and it is the reason an answer can be trusted at all: the version
names the snapshot and the digest is over its canonical form, so the same question against a
different bundle is visibly a different answer.

#### Implements

`Debug`, `Serialize`

### `struct LegContent`

```rust
pub struct LegContent
```

One leg of an answer: which source it ran on, and which identity it ran as.

**The posture is a word, and the operator's acknowledgement reason is not here.** The reason is
prose an operator wrote for a reviewer and the startup log prints it; sending it to an agent on
every answer would be a channel from a configuration file into a model's context that nobody asked
for. What an agent needs is which of the two postures produced the rows.

Reading it is not a control: it reaches the agent after the rows did.

#### Implements

`Debug`, `Serialize`

### `struct RefusalContent`

```rust
pub struct RefusalContent
```

Why a question was refused: a stable code a caller branches on, and a sentence a person reads.

**Nothing here echoes a value the caller sent.** The domain's refusal variants already stop short
of that - a rejected filter value names the dimension and not the value, because reflecting
caller text into a message that reaches a log and a model's context is how a rejected value
becomes somebody else's input.

#### Methods

```rust
pub const fn code(&self) -> &'static str
```

The code, for a test that asserts on the contract rather than on the prose.

```rust
pub fn detail(&self) -> &str
```

The sentence. A test asserts it is not empty; nothing asserts its wording.

#### Implements

`Debug`, `Serialize`

### `use CatalogContent`

What this deployment measures, as the catalog tool's structured content.

**A second wire type beside `sutura_http::wire::CatalogBody`, with the same fields, and that is
the same deliberate cost `super::AskArgs` already pays.** An adapter never calls another adapter, so
this crate cannot import that shape; what keeps the two equal is review plus the fact that both
are built from the one `sutura_domain::pinned::PinnedDefinitions` accessor set, which is where a
missing field would show up as a missing call rather than as a silent divergence.

**What narrows this listing is the CALLER's identity - `docs/adr/0028` - and nothing the caller
SENDS.** `sutura_domain::pinned::SemanticCatalog::load` takes no request context and cannot be
given one, so no argument selects, widens or parameterizes what this returns: the caller's mapped
audiences (which `describe` reads and this constructor takes as a `ScopedView`) decide which
metrics the listing holds, while the bundle underneath is the same one every answer is computed
from. Invisible means absent, and it is the transport's job to build the view, never this type's.

### `use DescribeCatalogArgs`

This tool takes no arguments. It returns the catalog of what this deployment measures, narrowed
to what the calling principal may see: there is nothing to filter or select, so send an empty
object.

### `use DimensionContent`

One dimension of one metric.

### `use MetricContent`

One metric, as much of it as a caller needs to ask a valid question.

### `use MalformedStatement`

Why a `run_sql` call's arguments were not a statement.

### `use RawContent`

What the raw SQL tool produced, as the tool's structured content.

**The load-bearing shape.** No field here is named `provenance`, `definition_version` or
`definition_digest`, at any depth - there is nowhere on this type to put one, which is what makes
a raw answer unable to be rendered as certified rather than merely undecorated as one.

**The two variant NAMES deliberately do not carry a `Raw` prefix** (`clippy::enum_variant_names`
over the type's own already-`Raw`-prefixed name) - only their SERIALIZED tags do, pinned by an
explicit `#[serde(rename)]` on each rather than derived from the Rust identifier: `Rows` would
otherwise serialize `outcome: "rows"` and `Refusal` would serialize exactly the certified path's
own `outcome: "refusal"` - the one collision `docs/adr/0013` forbids.

### `use RunSqlArgs`

One raw statement, as a tool call carries it.

One field, bounded by `RawStatement::parse` on the way in - `deny_unknown_fields` is what keeps
this tool from ever growing a second field a caller could smuggle a table name or a row-id list
through, the same governance boundary `super::AskArgs` holds for the certified tool.

### Module `catalog`

The catalog tool's wire shape: what `describe_catalog` takes, what it answers, and the text half
of that answer.

**Its own module because `wire.rs` was twenty-one lines under the thousand-line limit
`cargo xtask max-lines` enforces and cannot exempt**, at the banner that file already carried -
and the seam is one whole TOOL rather than a share of lines. `wire.rs` keeps the question and the
answer of `ask`; everything here belongs to the other tool a caller can reach, and the two share
nothing but `prose`, which is where the `Option` behind a description stays private.

Why the arguments type has braces and no fields, and why the reasoning about a wire type is a
plain comment rather than a doc comment, are both stated at `DescribeCatalogArgs` - `schemars`
puts a root doc comment into the schema's `description`, which is text a MODEL reads.

#### `struct DescribeCatalogArgs`

```rust
pub struct DescribeCatalogArgs
```

This tool takes no arguments. It returns the catalog of what this deployment measures, narrowed
to what the calling principal may see: there is nothing to filter or select, so send an empty
object.

##### Implements

`Debug`, `Deserialize<'de>`, `JsonSchema`

#### `struct CatalogContent`

```rust
pub struct CatalogContent
```

What this deployment measures, as the catalog tool's structured content.

**A second wire type beside `sutura_http::wire::CatalogBody`, with the same fields, and that is
the same deliberate cost `super::AskArgs` already pays.** An adapter never calls another adapter, so
this crate cannot import that shape; what keeps the two equal is review plus the fact that both
are built from the one `sutura_domain::pinned::PinnedDefinitions` accessor set, which is where a
missing field would show up as a missing call rather than as a silent divergence.

**What narrows this listing is the CALLER's identity - `docs/adr/0028` - and nothing the caller
SENDS.** `sutura_domain::pinned::SemanticCatalog::load` takes no request context and cannot be
given one, so no argument selects, widens or parameterizes what this returns: the caller's mapped
audiences (which `describe` reads and this constructor takes as a `ScopedView`) decide which
metrics the listing holds, while the bundle underneath is the same one every answer is computed
from. Invisible means absent, and it is the transport's job to build the view, never this type's.

##### Methods

```rust
pub fn of(view: &ScopedView<'_>, prose: CatalogProse, agent_instructions: &str) -> Self
```

The reader's view of one pinned bundle, under the prose setting this deployment was started
with and under the caller this read is for.

**A named constructor rather than a `From`, and the argument is the reason.** A conversion
reachable without the setting fails OPEN - it ships the prose of a deployment that asked for
none, which is the defect this function exists to close, and it is how that defect arrived
here. A second argument cannot be left out.

**The view is the second reason a `From` would be wrong, and it is the one `docs/adr/0028`
exists to close.** A caller may see only the metrics its granted audiences name; rendering
from a bare `&PinnedDefinitions` would hand every caller the whole bundle again, which is
the defect this surface shipped until it took the view. `ScopedView` borrows the bundle, so
this builder cannot reach `SemanticCatalog::load` - a per-caller filter stays off the
request path as a property of the type, never a call the renderer happens to omit.

It also asks nothing of the setting itself: `Carried::under` and `prose::notice` are the
crate's only two readers of it, so this builder cannot fill a `description` or pick a notice
without the operator's decision, and a third `CatalogProse` spelling is a compile error in
both rather than an `else` arm here.

`agent_instructions` is already-rendered text - the same document `initialize.instructions`
carries - rather than something this builder derives: `sutura-mcp` never reads settings, so
the operator's own section could not be assembled here even given the bundle, and the
composition root is where the one rendering happens for both surfaces.

##### Implements

`Debug`, `Serialize`

#### `struct MetricContent`

```rust
pub struct MetricContent
```

One metric, as much of it as a caller needs to ask a valid question.

##### Implements

`Debug`, `Serialize`

#### `struct DimensionContent`

```rust
pub struct DimensionContent
```

One dimension of one metric.

##### Implements

`Debug`, `Serialize`

### Module `raw`

The raw SQL tool's wire shape: what `run_sql` takes, and what it answers.

Its own module for the reason `wire/catalog.rs` has one - one whole tool, sharing nothing with
`ask`'s shapes but `sutura_domain::warehouse::Value::render`.

# The discriminant, and why it cannot be mistaken for a certified answer's

`docs/adr/0013` requires that a raw result's wire shape share no discriminant VALUE and no
provenance-shaped key with `crate::wire::OutcomeContent::Answer`'s - a WEAKER claim than "no
field name in common", and the one this module's own test asserts. `columns` and `rows` ARE
shared field names (both walk the same rows, so both need the same two labels for them); what
neither shares is the VALUE at `outcome` - that type tags with `outcome: "answer"` /
`outcome: "refusal"`, `RawContent` with `outcome: "raw_rows"` / `outcome: "raw_refusal"` - and
neither raw variant carries a `provenance` or a `definition_digest` key at any depth, which is
the property that actually keeps a raw result from being rendered as certified.

#### `struct RunSqlArgs`

```rust
pub struct RunSqlArgs
```

One raw statement, as a tool call carries it.

One field, bounded by `RawStatement::parse` on the way in - `deny_unknown_fields` is what keeps
this tool from ever growing a second field a caller could smuggle a table name or a row-id list
through, the same governance boundary `super::AskArgs` holds for the certified tool.

##### Implements

`Debug`, `Deserialize<'de>`, `JsonSchema`

#### `enum MalformedStatement`

```rust
pub enum MalformedStatement
```

Why a `run_sql` call's arguments were not a statement.

##### Variants

- `NotAnObject`
- `Statement`

##### Implements

`Debug`, `Display`, `Error`

#### `enum RawContent`

```rust
pub enum RawContent
```

What the raw SQL tool produced, as the tool's structured content.

**The load-bearing shape.** No field here is named `provenance`, `definition_version` or
`definition_digest`, at any depth - there is nowhere on this type to put one, which is what makes
a raw answer unable to be rendered as certified rather than merely undecorated as one.

**The two variant NAMES deliberately do not carry a `Raw` prefix** (`clippy::enum_variant_names`
over the type's own already-`Raw`-prefixed name) - only their SERIALIZED tags do, pinned by an
explicit `#[serde(rename)]` on each rather than derived from the Rust identifier: `Rows` would
otherwise serialize `outcome: "rows"` and `Refusal` would serialize exactly the certified path's
own `outcome: "refusal"` - the one collision `docs/adr/0013` forbids.

##### Variants

- `Rows` - The statement executed.
- `Refusal` - The statement was refused. Still an `Ok` and still a tool result, for `ServerHandler::call_tool`'s reason: a governance outcome is not a fault.

##### Implements

`Debug`, `Serialize`
