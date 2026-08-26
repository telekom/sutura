<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-runtime

The public API of `sutura-runtime`, rendered from rustdoc JSON.

Process-lifecycle concerns for a sutura service: the log, the panic hook, the shutdown signal,
the banner, and the bound on how much executes at once.

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
