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
  `AgentSurface::new` requires a `sutura_app::Permitted`, `tools/list` filters on it and
  `tools/call` refuses on it. The filtering is presentation and the refusal is the control - see
  `server`, which says so in a table, and says plainly that nothing over standard input and
  output narrows the set today.
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
* **Any notion of who is asking.** `crate::principal` still answers
  `sutura_domain::identity::Subject::TheDeploymentItself`, truthfully: this transport speaks over a
  pipe, where there is no header a token could arrive in. `sutura_http::inbound` is where leg 1
  lives and it is unreachable from here - an adapter never calls another adapter - so a caller
  identity on this surface needs the two decisions `docs/adr/0014`'s closing section names: how it
  is reached at all, and which crate the validator moves to.

  **The consequence for what a scope gates here is stated rather than left implicit:** the
  capability set this surface offers is narrowable, and over standard input and output nothing
  narrows it. `server` carries that limit beside the mechanism.
* **Resources and prompts.** A gateway of the shape this product runs behind surfaces tools and
  ignores both, so anything load-bearing has to be a tool. The glossary and the catalog prose stay
  where they are - in `sutura_app::prompt`, advisory, for a cooperative client.
* **A composition root.** Nothing below `serve_stdio` cares about transport, and the binary that
  calls it is `sutura`'s `mcp` subcommand - the single-player answer to *which binary gets it*,
  composed in `sutura-cli` the way `query` is. This crate deliberately does not decide that; it
  is the transport a composition root calls.

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
- `Interrupted` - The task driving the session did not finish.

### Implements

`Debug`, `Display`, `Error`

## `fn serve_stdio`

```rust
pub async fn serve_stdio<S>(service: std::sync::Arc<S>, permitted: sutura_app::Permitted, prose: sutura_app::prompt::CatalogProse, admission: sutura_runtime::Admission, reply: sutura_config::RequestTimeout) -> Result<(), NotServed>
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

**`permitted` is required for the same reason it is on `AgentSurface::new`: a pipe has no
header a token could arrive in, so this transport alone cannot choose who the peer is. The
composition root decides** - `sutura`'s `mcp` subcommand passes `Permitted::every_capability`
and prints that at startup - so the value lives next to the notice that states it rather than
hidden in this function.

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

Returns when the peer closes or is cancelled.

# Errors

`NotServed::Handshake` if the client never completes `initialize`, and
`NotServed::Interrupted` if the task driving the session did not finish - a panic, or a runtime
shutting down underneath it.

## `use AgentSurface`

The agent-facing surface over one `Surface`.

Holds the service behind an `Arc` because a tool call is answered on the blocking pool, so the
port has to outlive the future that started the call.

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
it. rmcp 3.1.4 applies no per-request deadline of its own, so there was no other bound to
inherit - measured on the pinned SDK and not read off its documentation.

So `AgentSurface::new` takes `server.request_timeout_seconds` as well, and the one function
that awaits the port waits under it. When it expires the peer is answered on the fourth
channel above. **The same key the HTTP surface answers `408` from, and reusing it rather than
inventing a key of this transport's own is a decision with two arguments:**

* A second key would be a second number for one fact - *how long a caller waits for a reply* -
  and the two transports would then be able to disagree about it while sharing one execution
  bound.
* The number is already load-bearing on this composition. `sutura`'s `mcp` command passes it to
  `open_engine`, where `QueryDeadline::within_request_timeout` divides it into the deadline a
  `bigquery` job is submitted with. So the engine on this transport already gives up against
  this key; before this change the *peer* was the only party in that arithmetic with no
  deadline at all.

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

**Peer cancellation ends this transport's wait and nothing below it.** rmcp 3.1.4 delivers
`notifications/cancelled` through `RequestContext::ct`, and `ServerHandler::call_tool` selects
on that token while a question is pending. The handler returns before its configured deadline;
the pinned rmcp then suppresses its response on the wire. The blocking `Surface::answer`
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

`AgentSurface::new` **requires** a `Permitted`, so a composition root cannot forget to say what
the peer may do - the same reason `sutura_app::surface::LocalService::start` requires an audit
sink. Given one, this handler does two things with it and only the second is a control:

| Where | What it does | What it is |
| --- | --- | --- |
| `tools/list` | drops a tool the peer may not invoke | **presentation** |
| `tools/call` | refuses a capability the peer was not granted, advertised or not | **the control** |

Both read the same set, so they cannot disagree - `sutura_app::capability` holds that argument and
the test for it. A caller that guessed `ask_metric` without ever being shown it is refused by the
second row, which is why the first is described as presentation rather than as security.

**And the honest limit, which is not small:** nothing that ships narrows the set here.
`crate::serve_stdio` passes `Permitted::every_capability`, because this transport speaks over
standard input and output and there is no header a token could arrive in - `docs/adr/0014`'s
closing section says as much, and says that deciding how this surface is reached at all is an
architecture decision rather than a refactor. So the narrowing here is exercised by this module's
own tests and by no request path, and the parameter is in place so that the decision arrives as a
composition change rather than as a redesign of this handler.

### `struct AgentSurface`

```rust
pub struct AgentSurface<S>
```

The agent-facing surface over one `Surface`.

Holds the service behind an `Arc` because a tool call is answered on the blocking pool, so the
port has to outlive the future that started the call.

#### Methods

```rust
pub const fn new(service: Arc<S>, permitted: Permitted, prose: sutura_app::prompt::CatalogProse, admission: Admission, reply: RequestTimeout) -> Self
```

Wraps a service, and states what the peer may do and how catalog prose is treated.

Takes the `Arc` rather than making one, so a composition root serving two transports shares
one bundle and one data system rather than opening a second of each.

**`permitted` is required rather than defaulted, and that is the point of the signature.** A
default here would be a posture chosen by this file for every deployment that ever links it;
`Permitted::every_capability` is the right answer over standard input and output and would be
the wrong answer the moment this surface is reachable over a network, and only a composition
root knows which it is building. See the module documentation for what the value then gates.

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

#### Implements

`Debug`, `ServerHandler`

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

Beside it, `tests::the_question_tool_takes_exactly_the_five_fields_a_question_has` asserts the
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

`sutura-http` already holds one - `QuestionBody`, with the same five fields and the same
`TryFrom<..> for Query`. Sharing it would mean this adapter depending on that one, and *an
adapter never calls another adapter* is the rule the whole layout rests on: a shape owned by one
transport is a shape every other transport has to reach through it. `CatalogContent` is the
same story against `sutura_http::wire::CatalogBody`.

**So the duplication is deliberate, and it is a cost rather than an oversight.** Nothing in the
compiler makes two wire types stay equal. What guards them is
`AskArgs`'s own `deny_unknown_fields`, asserted through the transport in `crate::server`, plus
the committed schema dump in `crate::tool` - a widened input changes a snapshot and the
byte-compare fails until somebody re-accepts it, which is what puts a new field in a reviewer's
diff.

**What is now mechanical across the two transports is the TOOL SET, and not these shapes.**
`sutura_app::Capability` is the one source both of them render, and
`both_transports_describe_the_same_tools` in `crate::tool` is the assertion. The field lists of
two wire types with the same job are still kept equal by review, and that limit is worth keeping
in front of a reader rather than letting the tool-set test read as covering it.

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

The five fields are the whole input surface of this deployment. There is no field for SQL, a
table, a filter expression or a row-id list, and an argument naming one is a parse error rather
than a key that gets ignored.

#### Implements

`Debug`, `Deserialize<'de>`, `JsonSchema`

### `struct RangeArgs`

```rust
pub struct RangeArgs
```

A half-open period: `start` is included, `end` is not.

Half-open at every grain, which is what makes a month `[2026-06-01, 2026-07-01)` rather than a
last day that differs per month. Both dates are ISO `YYYY-MM-DD`.

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

Every variant names the field, and none of them echoes the caller's value back except where the
value is the thing that failed to parse as an identifier - which is a bounded character set, not
free text.

#### Variants

- `NotAnObject` - The arguments object did not deserialize at all: a missing field, a wrong type, or - the case this crate cares about most - a field the tool surface does not declare.
- `Metric`
- `Grain`
- `Date`
- `Range`
- `Dimension`
- `FilterDimension`
- `FilterValue` - The value is not one a catalog could have declared: nothing, more than one line, a control character, an invisible or direction-changing code point, spacing a reader cannot see, or longer than `sutura_domain::catalog::MAX_DIMENSION_VALUE_CHARS`.

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

Descriptive content only. `sutura_domain::pinned::SemanticCatalog::load` takes no request context
and cannot be given one, so nothing a caller sends selects, widens or parameterizes what this
returns: it is the *pinned* bundle, the same one every answer is computed from.

### `use DescribeCatalogArgs`

This tool takes no arguments. It returns the whole of what this deployment measures, and there is
nothing to filter or select: send an empty object.

### `use DimensionContent`

One dimension of one metric.

### `use MetricContent`

One metric, as much of it as a caller needs to ask a valid question.

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

This tool takes no arguments. It returns the whole of what this deployment measures, and there is
nothing to filter or select: send an empty object.

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

Descriptive content only. `sutura_domain::pinned::SemanticCatalog::load` takes no request context
and cannot be given one, so nothing a caller sends selects, widens or parameterizes what this
returns: it is the *pinned* bundle, the same one every answer is computed from.

##### Methods

```rust
pub fn of(pinned: &PinnedDefinitions, prose: CatalogProse) -> Self
```

The reader's view of a pinned bundle, under the prose setting this deployment was started
with.

**A named constructor rather than a `From`, and the argument is the reason.** A conversion
reachable without the setting fails OPEN - it ships the prose of a deployment that asked for
none, which is the defect this function exists to close, and it is how that defect arrived
here. A second argument cannot be left out.

It also asks nothing of the setting itself: `Carried::under` and `prose::notice` are the
crate's only two readers of it, so this builder cannot fill a `description` or pick a notice
without the operator's decision, and a third `CatalogProse` spelling is a compile error in
both rather than an `else` arm here.

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
