<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-runtime

The public API of `sutura-runtime`, rendered from rustdoc JSON.

Process-lifecycle concerns for a sutura service: the log, the panic hook, the shutdown signal,
the banner, the bound on how much executes at once, and the audit sink a deployment gets for
free.

The sink is here for the same reason everything else is: it writes onto the process subscriber
this crate installs, so it is a *use* of a process-global rather than a second installation of
one. It is the first implementor of `sutura_domain::audit::AuditSink`, which is what keeps that
port from being a guess at a signature - see `audit`.

# Why this is its own crate

Everything here is process-*global*. Installing a subscriber, replacing the panic hook and
registering signal handlers are one-per-process operations that a library must not perform as a
side effect of being used - so they belong somewhere a composition root calls deliberately,
rather than inside a transport crate that a test also links.

It also means the transport crates do not have to agree on any of it. `sutura-http` takes a
shutdown as an argument and emits `tracing` events like any other library; nothing in it
installs anything. A second surface - an MCP transport, say - gets the same treatment for free.

# The order the composition root uses it in

```no_run
use sutura_config::{Settings, Sources, environment_from_process};
use sutura_runtime::{Admission, banner, shutdown::Shutdown, telemetry};

# fn main() -> Result<(), Box<dyn core::error::Error>> {
// 1. The environment, first: it decides the log format and which file is layered.
let environment = environment_from_process()?;
// 2. The banner, to standard output, before any subscriber exists.
banner::print(env!("CARGO_PKG_VERSION"), environment);
// 3. The configuration. A refusal here is a process that does not start.
let settings = Settings::load(&Sources::from_process_environment(environment, None))?;
// 4. The log, then the panic hook - in that order, so a panic during setup is not the one
//    that gets lost.
telemetry::install(settings.telemetry())?;
sutura_runtime::install_panic_hook();
// 5. What was resolved, including the line about what this service does not do.
banner::announce(&settings);
// 6. The shutdown, shared with the server and with the signal listener. Built with the
//    configured grace period, so the number an operator wrote is the number that bounds
//    stopping.
let shutdown = Shutdown::with_grace(settings.runtime().shutdown_grace().duration());
tokio::spawn(sutura_runtime::shutdown::listen(shutdown.clone()));
// 7. The bound on how many questions execute at once, shared with every transport. One per
//    process: two independently sized ones would each report a limit the other can exceed.
let admission = Admission::from_settings(settings.runtime());
# Ok(())
# }
```

# What is deliberately not here

No metrics and no traces-to-a-collector. A span per request exists and is rendered into the log,
which is what makes one request's lines findable; exporting it anywhere is a decision about a
backend, a sampling rate and an egress path, and none of those has been made. Adding a
dependency now to satisfy the word "observability" would be the shape of the thing without the
thing.

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## Module `admission`

How many questions may be executing at once, and how long a caller waits for a turn.

# Why this is a bound on the work and not a bound on the reply

`server.request_timeout_seconds` is a deadline on the **reply**. When it expires the caller is
answered `408` and the handler future is dropped - and a started `tokio::task::spawn_blocking`
task cannot be aborted, so the question keeps running. Without something else in the picture a
caller asking questions that cost more than the timeout gets a fast turnaround while the
deployment keeps the whole cost, and in-flight work accumulates at the rate limit with nothing
shedding it. The blocking pool defaults to 512 threads with an unbounded queue, so that backlog
is bounded by memory.

`Admission` is the bound. It is the number of questions that may be *executing*, and the slot
is meant to be held by the blocking task rather than by the handler future - so a timed-out
request does not hand its slot back until the work it started actually finishes.

# What it does not do, stated first because it is the part that gets assumed

**It does not cancel anything.** The `Warehouse` port is synchronous and has no cancellation
token, so a question already handed to the blocking pool runs to completion whatever the caller
is told. A timed-out request therefore still holds its slot until the data system answers it -
which is exactly why the slot matters: the backlog becomes a number somebody chose instead of
memory.

**It is not a per-caller budget.** It bounds the deployment, not a principal. One caller filling
every slot sheds every other caller, and nothing here can tell the two apart - that needs an
identity, which this service does not have. `sutura_config::RateLimitSettings` is what bounds a
single address's rate, and it counts requests rather than execution.

**It is not a queue.** Waiting is bounded by the admission timeout and a waiter that runs out of
it is shed. A refused waiter costs a dropped future rather than a thread, which is what stops
the queue in front of the bound from being a second unbounded thing.

# Why it lives here rather than in the transport

The resource it bounds is the *process*: a blocking-pool thread holding a data system. A second
transport - an MCP surface, say - would need the same bound over the same pool, and two
independently sized semaphores would be two controls each reporting a limit that the other can
exceed. So the composition root builds one, exactly as it builds one `crate::Shutdown`.

### `struct AtCapacity`

```rust
pub struct AtCapacity
```

Nothing was free inside the admission timeout.

Carries both numbers because the answer to it differs by which one is wrong: a bound that is too
small for the machine is a configuration change, and a wait that expires under normal load is a
deployment that needs another replica.

#### Methods

```rust
pub const fn bound(self) -> usize
```

How many slots there are in total.

```rust
pub const fn waited(self) -> Duration
```

How long the caller waited before being shed.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct Admission`

```rust
pub struct Admission
```

The bound on how many questions execute at once, and the bound on waiting for a turn.

Cloneable and cheap: every clone shares one permit set, which is the property that makes this a
bound at all. A per-request or per-connection copy would be a number that reads like a limit and
bounds nothing.

#### Methods

```rust
pub async fn admit(&self) -> Result<Slot, AtCapacity>
```

Waits for a slot, for at most the admission timeout.

The returned `Slot` must be moved into whatever does the work rather than held by the
awaiting future, or the bound becomes a bound on *starting* work instead of on running it -
see the module documentation.

```rust
pub const fn bound(&self) -> usize
```

How many questions may execute at once.

```rust
pub fn free(&self) -> usize
```

How many slots are free right now.

For a log line and for a test. Not for a decision: it is true when it is read and can be
false by the time it is acted on, which is what `Self::admit` exists to avoid.

```rust
pub fn from_settings(runtime: RuntimeSettings) -> Self
```

The same, from the group the two keys live in.

What a composition root calls, so the two values cannot be taken from different places.

```rust
pub fn new(concurrency: QueryConcurrency, timeout: AdmissionTimeout) -> Self
```

Builds the bound from two values that have each already been parsed.

```rust
pub const fn wait(&self) -> Duration
```

How long a question may wait for a slot.

#### Implements

`Clone`, `Debug`

### `type_alias Slot`

The right to execute one question, held for as long as the question runs.

An alias for `tokio`'s owned permit rather than a newtype around it, because the type's whole
contract is its `Drop`: the slot comes back when the value is dropped, and a wrapper would only
add a way to get that wrong. Owned rather than borrowed so it can be moved into the blocking
task, which is the whole point - see the module documentation.

## Module `audit`

The one implementor of `AuditSink` that needs nothing from anybody.

A structured writer over the tracing subscriber this crate already composes. It is here rather
than in a transport for the reason everything in this crate is here: the subscriber is
process-global, and the sink is a use of it rather than a second installation.

# Why this is the first implementor rather than a fake

`AGENTS.md` says a port trait arrives with its first implementor, because a trait with no
implementor is a guess at a signature. This is the sink a deployment that attaches nothing else
gets: it writes into the log pipeline the deployment already runs, so there is no configuration,
no credential and no second egress path to decide about. **Whether that pipeline keeps anything
is the deployment's answer, not ours** - which is the limit `sutura_domain::audit` states, and
this type is where it is most visible.

# It replaces a log line rather than adding a channel beside one

Before this existed, `sutura_http`'s query handler wrote one `tracing::info!` per outcome, and
its own doc comment said there was no audit sink and nothing recorded a principal chain. The
fields that line carried - the row count, the definition version, the refusal variant - are
carried here, so an operator's existing filters keep working while the line gains the thing it
was missing. What that line also carried, and this does not, is the HTTP status: the status is
the transport's and the application cannot see it. Nothing is lost, because `tower_http`'s
response line already carries it inside the same request span, configured at `info` in
`sutura_http::router` - which is where a status belongs.

### `struct TracingAuditSink`

```rust
pub struct TracingAuditSink
```

Writes each record as one structured event on the process subscriber.

There is nothing to configure, and a `new` that took a level or a target would be two ways to
write the same record. The subscriber decides where the event goes, which is the one decision a
deployment already makes.

A private marker field rather than a unit struct, for two reasons and the first is the honest
one: `cargo xtask check-boundaries` reads a file line by line, and a unit `pub struct` leaves its
scan waiting for a body - so the next braced block in the file is read as this struct's field
list and a `pub fn` in it is reported as a `pub` field. That is a defect in the gate rather than
in this type, and it is written down here rather than worked around silently. The second reason
stands on its own: a field, even an empty one, means `Self::new` is the only way in from
outside this crate, where a unit struct is its own literal.

#### Methods

```rust
pub const fn new() -> Self
```

#### Implements

`AuditSink`, `Clone`, `Copy`, `Debug`, `Default`

## Module `banner`

What the process says about itself on the way up.

Two things, and they go to different places on purpose.

The banner is `println!`, before any subscriber exists. It is for a person watching a terminal
or scrolling to the top of a container log, and it answers "what is this and which build is it"
without needing the log format to be readable yet.

The configuration report is `tracing`, after the subscriber is installed, so it lands in
whatever a collector is ingesting. It is for an operator, and the lines that matter most are
the ones about what this service does *not* do.

### `fn print`

```rust
pub fn print(version: &str, environment: sutura_config::Environment)
```

Prints the banner and the build line to standard output.

Before the subscriber, deliberately. `version` is the caller's `CARGO_PKG_VERSION`: taking it
as an argument rather than reading this crate's own means the number printed is the binary's,
which is the one somebody is trying to identify.

### `fn announce`

```rust
pub fn announce(settings: &sutura_config::Settings)
```

Writes the resolved configuration to the log.

**Every value comes from the loaded `Settings`, not from a file.** The environment layer is
applied last, so a report built from a file could describe a deployment the process is not
running as - which is the same reason the refusals in `sutura-config` read the loaded value.

The whole tree goes out as one `Debug` field. That is safe because the only credential-shaped
value in it is held in a type whose `Debug` redacts, and `sutura-config` has a test asserting
that at the outermost struct - not because this function was careful.

## Module `blocking`

Handing synchronous work to the blocking pool without losing the request it belongs to.

# Why a helper rather than a convention

`tokio::task::spawn_blocking` runs its closure on a thread that has no idea which request it is
serving. `tracing`'s current span is a thread-local, so every line the closure emits lands
outside the span the transport opened - which is exactly the context an operator filters on. The
fix is three lines and it was already written correctly at the one call site that existed.
Nothing made the *next* one write it, and a missing span is invisible: the code compiles, the
work runs, the answer is right, and one request's lines are simply not findable together.

So the three lines live here and `tokio::task::spawn_blocking` is on the `disallowed-methods`
list in `clippy.toml`, with this module's own call carrying the one `#[expect]` for it. That
turns "remember to carry the span" into a lint, which is what the *Agent Operating Contract*
asks for: a rule with no mechanism is a wish.

# Why it lives in this crate

The same reason `crate::admission` does: **the resource is the process.** The blocking pool is
one pool per runtime, shared by every transport, and a second transport would need the same
span-carrying spawn over the same pool. A copy per transport is two conventions that can differ.

# What it does not do, stated because the name invites the assumption

It does not bound anything and it does not cancel anything. `tokio` documents that a started
blocking task cannot be aborted and that runtime shutdown waits for one, so a caller who has
given up does not stop the work. `crate::Admission` is what bounds how many of these run at
once; this only decides which span they are attributed to.

# The one thing that has to be true of the deployment

Carrying the span works because the span's own `Dispatch` and the pool thread's default
dispatcher are the *same* subscriber - the process-global one that
`crate::telemetry::install` sets. Entering the span registers it on the pool thread inside
that subscriber; the event then finds it there. Under a subscriber scoped to one thread with
`tracing::subscriber::with_default` the two are different, and the pool thread's line goes to
whatever global default exists instead. That is why the test for this is an integration test
that installs a global subscriber, and not a unit test in this file.

### `fn spawn_carrying_span`

```rust
pub fn spawn_carrying_span<Work, Answer>(work: Work) -> tokio::task::JoinHandle<Answer>
```

Spawns `work` on the blocking pool, entered in the caller's current span.

The span is captured *here*, on the caller's thread, and entered inside the closure - which is
the only order that works: reading the current span from the blocking thread would read that
thread's span, which is none.

Everything the closure does is inside the span, its own `Drop`s included. That matters where a
permit or a guard is released at the end of the closure: the release happens inside the span
too, so a diagnostic emitted while dropping is still attributable to the request.

# Example

```
# async fn call() -> Result<u8, tokio::task::JoinError> {
// A synchronous port call. Anything traced inside belongs to the caller's request.
sutura_runtime::spawn_carrying_span(|| 7_u8).await
# }
```

## Module `panics`

A panic that reaches the log rather than only the terminal.

**A hook, not a `catch_unwind`, and the shipped profiles are why.** `panic = "abort"` is set on
every profile this repository ships, so there is no unwinding to catch: the process is going to
die. What a hook can still do is run *first*, on the panicking thread, while the payload and the
location are in hand - so the last thing in the log says what happened and where, instead of
the log simply stopping.

That distinction is the whole value. A container that exits with a message on standard error and
nothing in the collected log looks, from the outside, exactly like one that was evicted.

The previous hook is kept and called afterwards, so the default rendering and any backtrace
still appear. Replacing it outright would trade one loss for another.

### `fn install_panic_hook`

```rust
pub fn install_panic_hook()
```

Makes every subsequent panic emit a `tracing` error before the default hook runs.

Call it before the first thread is spawned and after the subscriber is installed. Before the
subscriber it would still work - `tracing` drops events with no subscriber rather than failing -
but the panic it was installed for would be the one that is lost.

Idempotent: the second and later calls do nothing.

## Module `shutdown`

Stopping on purpose, and saying why.

Three things have to be true of a shutdown, and each of them is a separate part of this module:

* **A signal reaches the server.** An orchestrator sends `SIGTERM` and then, some seconds
  later, `SIGKILL`. A process that ignores the first is killed mid-request.
* **In-flight work drains.** `axum` waits for every open connection, which is what makes a
  rolling deployment not drop answers - and which is also how one wedged connection pins the
  process open past the kill deadline. So the drain is *bounded*: see
  `Shutdown::grace_period`, and `Shutdown::remaining_grace` for what is left of that budget
  once the drain has had its turn. The two together are what make the number a bound on
  *stopping* rather than on the connection drain alone: dropping an `axum` serve future ends the
  drain, and the runtime then still waits for every blocking task it cannot cancel.
* **The reason is recorded.** A process that vanished and a process that was asked to stop look
  identical in a log that says nothing, and only one of them is a bug.

The signal is translated into a `Shutdown` rather than being awaited directly by the server,
and that is what makes this testable: a test triggers the same value a signal would, with no
process-wide side effect and nothing to install.

### `enum ShutdownReason`

```rust
pub enum ShutdownReason
```

Why the process is stopping.

#### Variants

- `Interrupt` - An interactive interrupt. A person pressed a key.
- `Terminate` - A termination request. An orchestrator is replacing this process, and a deadline is running.
- `Requested` - Something in this process asked for it - a fatal error on a path that is not the server, or a test.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

### `struct Shutdown`

```rust
pub struct Shutdown
```

A shared "stop now" flag that remembers why.

Cloneable and cheap, so the server, the signal listener and anything else that has to wind down
all hold the same one. Built on a `watch` channel rather than a cancellation token from a
utility crate: the channel is in `tokio` already, and carrying the *reason* in the value is
what makes the log line at the end say something.

#### Methods

```rust
pub const fn grace_period(&self) -> Duration
```

The whole budget for stopping, from the moment stopping is asked for.

```rust
pub fn new() -> Self
```

A fresh, untriggered shutdown with the default grace period.

```rust
pub fn reason(&self) -> Option<ShutdownReason>
```

The reason, if shutdown has been asked for.

```rust
pub fn remaining_grace(&self) -> Duration
```

What is left of that budget.

**The connection drain is not the whole of stopping, and this is the difference.** Dropping
an `axum` serve future ends the drain and returns; the runtime then still waits for every
blocking task, because `tokio` documents that a started `spawn_blocking` task cannot be
aborted and that runtime shutdown waits for one. A process that spent its whole grace period
draining connections and then waited a full grace period again for the blocking pool would
take twice the number an operator configured - and that number was chosen against their
orchestrator's kill timer, so twice it is being killed mid-answer.

The full grace period before stopping has been asked for, because there is nothing to count
from yet: this is a bound on the *rest* of stopping, and stopping has not started.

Saturating, so an overrun is zero rather than a wrapped duration. Zero is a legitimate
answer and means the budget is spent: whatever waits on it should not wait at all.

Measured on `std::time::Instant` and not on `tokio`'s clock, deliberately. What this number
is racing is an orchestrator's kill timer, which is wall time, and what consumes it is
`tokio::runtime::Runtime::shutdown_timeout`, which is also wall time. A test-controllable
clock here would make the two disagree in exactly the deployment where it matters.

```rust
pub async fn requested(&self) -> ShutdownReason
```

Resolves when shutdown has been asked for.

Checks the current value before waiting, which is the bug this shape exists to avoid: a
`watch` receiver created after the send marks that value as already seen, so a naive
`changed().await` on a shutdown that has *already* happened waits for a second one that
never comes.

```rust
pub fn trigger(&self, reason: ShutdownReason)
```

Asks for shutdown, and logs why.

**The first reason wins, and a second call is a no-op.** A `SIGINT` arriving while a
`SIGTERM` drain is already running should not restart the clock or rewrite the record of
what started it.

```rust
pub fn with_grace(grace: Duration) -> Self
```

A fresh shutdown with an explicit grace period. What a test uses.

#### Implements

`Clone`, `Debug`, `Default`

### `fn listen`

```rust
pub async fn listen(shutdown: Shutdown)
```

Waits for the first operating-system shutdown signal and triggers `shutdown`.

Spawned as a task by the composition root. It returns once it has triggered, so a caller can
join it; it does not loop, because the first reason wins.

## Module `telemetry`

The log: one subscriber, two renderings, and the environment decides which.

In production a log line is read by a collector. It has to be one JSON object per line with the
span context attached, or a query cannot find every line belonging to one request. On a laptop
the same line is read by a person who is recompiling every thirty seconds, and a JSON object
per line is unreadable there.

Both are correct for their reader, so the choice is made once, from
`sutura_config::LogFormat`, whose default follows the environment. That is the whole of the
split - there is no second switch that could disagree with it.

# What overrides what

`RUST_LOG`, if it is set, beats `telemetry.filter`. That is the convention every Rust operator
already has, and the configured value is the fallback rather than the ceiling. Unlike the usual
spelling of this, an *invalid* `RUST_LOG` is an error rather than something quietly discarded:
a discarded filter means the process logs at some other level and nothing says so, which is the
failure mode a filter exists to prevent.

### `enum TelemetryNotInstalled`

```rust
pub enum TelemetryNotInstalled
```

Why the log could not be set up.

#### Variants

- `Filter` - A filter directive would not parse. Either the configured one or `RUST_LOG`; `source` says which, because they fail for different reasons and only one of them is in a file.
- `FilterNotUnicode` - `RUST_LOG` held bytes that are not text.

#### Implements

`Debug`, `Display`, `Error`

### `fn subscriber`

```rust
pub fn subscriber<Sink>(telemetry: &sutura_config::TelemetrySettings, sink: Sink) -> Result<BuiltSubscriber, TelemetryNotInstalled>
```

Builds the subscriber for these settings, writing to `sink`.

Boxed, and that is what lets the two arms be one type: a pretty formatter and a bunyan
formatter compose different layer stacks, so there is no `impl Subscriber` they share. The box
is paid once, at startup.

Generic in the sink so a test can build both arms over a buffer and assert on the bytes. That
is the only way to test this: installing a subscriber is process-global and happens once.

### `fn install`

```rust
pub fn install(telemetry: &sutura_config::TelemetrySettings) -> Result<(), TelemetryNotInstalled>
```

Installs the subscriber for these settings as the process-wide one, writing to standard output.

Standard output rather than standard error, for both formats: a collector reads one stream, and
splitting the log across two means half of it is interleaved somewhere else. Diagnostics that
have to be visible *before* this is installed - the banner, and a configuration that refused to
load - go to their own stream and say so.

Idempotent in the sense that matters: a second call is ignored rather than failing, because the
only thing that could call twice is a test harness and a panic there would be about the harness
rather than about the service.

### `constant FILTER_VARIABLE`

The variable that overrides the configured filter.

### `type_alias BuiltSubscriber`

A subscriber built for one set of settings.

A named alias because the inline form is over the complexity threshold in `clippy.toml`, and
naming it is the better half of that trade: what matters about the type is that it is *one*
type, which is the whole reason the box is there.

## Module `testing`

The log-capture writer.

`cfg(test)` for this crate's own suite, and behind `test-capture` for another crate's. The
feature's comment in `Cargo.toml` says why only the writer is exposed and not the helpers
around it.
Reading the log back, for a test that has to assert on what was logged.

It exists because everything this crate does is observable only as log output: a subscriber that
renders the wrong format, a panic hook that emits nothing, a shutdown that does not say why -
none of those has a return value to assert on. So the tests install a subscriber over a buffer
and assert on the bytes.

Scoped and not global for this crate's own unit tests, on purpose:
`tracing::subscriber::set_global_default` succeeds once per process, and this crate's tests need
several different subscribers. `tests/blocking_span.rs` is the deliberate exception, and its
header says why a global one is the only thing that can prove what it proves.

# Two visibilities, and the reason for the split

`Capture` is `pub` under `cfg(test)` **or** the `test-capture` feature, because `sutura-http`
has the same problem and a second copy of a writer is a second thing to keep in step. The
helpers that build a subscriber around it stay `cfg(test)`: they use `expect`, which is denied
outside test code, so a feature that exposed them would make `--all-features` fail to lint.

### `struct Capture`

```rust
pub struct Capture
```

A writer that keeps what was written.

`std::sync::Mutex` is on the workspace's disallowed list, and the reason recorded there is that
an async task holding one across an `await` can deadlock the executor. Neither half applies
here: `MakeWriter` and `io::Write` are synchronous traits that cannot await, the guard is taken
and dropped inside one statement, and no runtime is involved in the tests that use this.

#### Methods

```rust
pub fn contents(&self) -> String
```

Everything written so far, as text.

```rust
pub fn new() -> Self
```

An empty buffer, teeing to standard output if `TEE_VARIABLE` is set.

#### Implements

`Clone`, `Default`, `MakeWriter<'a>`, `Write`

### `constant TEE_VARIABLE`

Set this to anything non-empty to have a captured log also reach standard output.

`cargo nextest` shows a failing test's output, so this is what makes the log of the request
under test readable without making every other run louder.
