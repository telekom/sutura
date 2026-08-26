<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-config

The public API of `sutura-config`, rendered from rustdoc JSON.

The service's configuration: layered sources in, one typed tree out, and a refusal instead of
a permissive default.

# What a caller does

Read the environment, then load. Two steps rather than one, because the environment decides
which file is layered and which defaults apply, so it has to be known first.

```
use sutura_config::{Environment, Settings, Sources};

// A test or an example supplies the environment directly; the binary reads it from the
// process with `sutura_config::environment_from_process`.
let settings = Settings::load(&Sources::defaults(Environment::Development))?;
assert!(settings.server().bind().is_loopback());
# Ok::<(), sutura_config::SettingsError>(())
```

# Precedence

Later beats earlier:

1. the defaults embedded in this crate (`Settings` can always be built from them alone);
2. `<dir>/base.yaml`, if a directory was given and the file is there;
3. `<dir>/<environment>.yaml`, likewise;
4. environment variables - `SUTURA__SERVER__PORT` sets `server.port`.

The environment itself is chosen by `SUTURA_ENVIRONMENT` and by nothing else. It is
deliberately *not* a configuration key: it selects which file is layered, so a file that could
change it would be self-referential. Both `environment:` in a file and `SUTURA__ENVIRONMENT` in
the shell are therefore unknown-field errors rather than settings that quietly do nothing.

Every layer is checked with `deny_unknown_fields`, at every depth. A misspelled key is an error
naming the key, not an override that silently did not happen.

# There is no per-caller identity, and this crate says so out loud

sutura has no request context, no credential broker and no way for a caller's identity to reach
the query path. `AGENTS.md` records "every query runs as the calling principal" as an
aspiration that is **not mechanised**, and `examples/multi-player/README.md` explains why
single-player makes it trivially true and worth nothing.

That is a property of the runtime, so it is a property of every deployment this crate
configures. An [`AccessToken`](security::AccessToken) authenticates *the deployment*: a caller
who presents it proves they hold a secret an operator configured, and nothing more. It does not
say which caller, it cannot be scoped to a subset of the catalog, it does not reach the data
system, and every query still runs with whatever access the process already had.
[`SecuritySettings::describes_identity`](security::SecuritySettings::describes_identity) is the
function that answers this, it always answers `false`, and the startup log prints that answer
on every boot so an operator cannot deploy this believing otherwise.

Rate limiting is not authentication either - see `limits` for what it does and does not buy.

# What refuses to start

`NotFitToServe` is the whole list, and each variant is a refusal rather than a warning. A
warning is read by whoever is looking at the log in the format the collector was configured
for; a process that does not start is read by everybody.

- A bind address other hosts can reach with `security.tls_termination: none`. In *every*
  environment, including a laptop. **The bind itself is not refused** - an ingress controller or
  a sidecar terminating TLS in front of a plaintext pod-local listener is the normal
  arrangement - what is refused is not saying which of those it is, because that is what decides
  how far the bearer token travels in cleartext.
- No `security.access_token`, in production or on a non-loopback bind.
- `rate_limit.enabled: false` in production.
- `rate_limit.client_address: forwarded` with an empty `rate_limit.trusted_proxies`, or a
  non-empty list that nothing reads.
- `security.tls_termination: in-process` without a certificate and key, or in a binary built
  without the `tls` feature; or a certificate and key no declaration would ever read.
- `server.port: 0` in production.

The checks read the *loaded* values, not any one file, because the variable layer is applied
last: a check against `production.yaml` would be checking something the process is not running
on.

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## Module `api`

Whether the generated documentation is served, and why the default differs by environment.

The `OpenAPI` document and the browser UI over it are generated from the handlers, so they are
always *correct*; the question is whether an unauthenticated caller should be handed a map of
the surface. On a laptop the answer is obviously yes - it is how the surface is explored at
all. In production it is a decision, and the safer default is the one that has to be turned
on rather than the one that has to be remembered.

It is a default and not a refusal. Serving an interface description is a legitimate choice for
a deployment behind a gateway that already authenticates, and refusing to start over it would
be this crate overruling an operator on something that leaks no data. What it does instead is
log the decision, at a level that shows up.

### `struct ApiSettings`

```rust
pub struct ApiSettings
```

Whether the generated documentation surface is served.

#### Methods

```rust
pub const fn docs_default_for(environment: crate::Environment) -> bool
```

The default for an environment: everywhere but production.

A total match rather than a comparison, so a fourth environment has to state its own
answer instead of inheriting whichever branch it happens to fall into.

```rust
pub const fn docs_enabled(self) -> bool
```

```rust
pub const fn docs_were_explicit(self) -> bool
```

```rust
pub const fn new(docs_enabled: bool, docs_were_explicit: bool) -> Self
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## Module `catalog`

Which catalog the service serves, and where the files it reads live.

Two directories rather than one, because they are owned by different people. The catalog is
authored and reviewed - it is the definitions somebody certified - and the data directory is
wherever the files the engine reads happen to be mounted. Conflating them would make a
deployment that moved its data look like a catalog change, which is the one thing a pinned
bundle exists to make visible.

### `struct CatalogSettings`

```rust
pub struct CatalogSettings
```

Where the definitions and the data are, and what the resulting bundle is called.

#### Methods

```rust
pub fn data_dir(&self) -> &Path
```

```rust
pub fn dir(&self) -> &Path
```

```rust
pub fn parse(dir: PathBuf, data_dir: PathBuf, version: DefinitionVersion) -> Result<Self, InvalidCatalogSettings>
```

Reads the two directories and the version label.

The version arrives already parsed, because what identifies a snapshot of a directory is
a commit id or a build number and only the caller has it. Existence of the directories is
deliberately *not* checked here: this type is the configuration, and a directory that
disappears between reading the configuration and loading the catalog would make an
existence check here a claim that goes stale immediately. The load is what fails.

```rust
pub const fn version(&self) -> &DefinitionVersion
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidCatalogSettings`

```rust
pub enum InvalidCatalogSettings
```

Why a catalog configuration is not usable.

#### Variants

- `EmptyPath` - A path was empty, which resolves to the process working directory - a different directory on every host, and never the one the operator meant.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `environment`

Which deployment this process believes it is.

One value, read before anything else, that decides three things which must not be decided
separately: which configuration file is layered on top of the defaults, whether the log is
machine-readable or human-readable, and how strict the startup refusals are. Separate
switches for those would let a deployment be production for logging and development for
safety, which is exactly the combination nobody would choose on purpose.

Deliberately three values and not a free string. A typo in an environment name is otherwise
the most expensive kind of configuration bug there is: it silently selects the permissive
branch of every decision above, and the log line saying so is in the format nobody is
collecting.

### `enum Environment`

```rust
pub enum Environment
```

A deployment kind.

Ordered from most permissive to least, which is also the order the refusals in
`crate::Settings` tighten in.

#### Variants

- `Development` - A developer's machine. Human-readable logs, loopback only, no token required.
- `Test` - An automated test. Same posture as development, and named separately so a test can assert on it without pretending to be a laptop.
- `Production` - A deployment serving somebody. Machine-readable logs, and every refusal in `crate::Settings::parse` applies.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The canonical spelling, which is also the file stem this environment layers.

```rust
pub const fn is_production(self) -> bool
```

Is this the environment the strict refusals apply to?

A method rather than `== Environment::Production` at each call site: there are five
refusals keyed off it, and a fourth variant added later has to answer this question once.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownEnvironment>
```

Reads an environment name.

Case-insensitive and trimmed, because this arrives from a shell variable and
`SUTURA_ENVIRONMENT=Production ` with a trailing space is not a different deployment.
Nothing else is forgiven: `prod` is not accepted, because an abbreviation somebody has to
guess is a spelling this type exists to remove.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `struct UnknownEnvironment`

```rust
pub struct UnknownEnvironment
```

The string was not one of the three.

Carries what it found, because the whole point of the type is that a typo fails loudly, and
a failure that does not quote the typo makes the operator guess which of the three spellings
they got wrong.

#### Methods

```rust
pub fn found(&self) -> &str
```

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `limits`

How many requests a caller gets, and the two tiers that answer differently.

**Rate limiting is not authentication.** It bounds how fast something can be done, not who
may do it, and on a surface with no per-caller identity - see `crate::security` - the key
it counts against is a network address rather than a principal. A shared egress address is
therefore one bucket for everybody behind it, and a caller with many addresses has many
buckets. Both of those are properties of the mechanism, not bugs in the configuration, and
neither is repaired by tightening the numbers.

What it does buy is real: it turns an unbounded loop against a data system into a bounded
one, and a question here is an aggregate over up to ten years of history, so the cost of one
request is not small.

**The switch follows the environment, and the refusal does not.** `rate_limit.enabled` has no
fixed default: it is off in development and test and on in production, the same shape
`telemetry.format` and `api.docs` already have here. That is a convenience in one direction only.
An explicit `false` in production is still a refusal to start - see
`crate::Settings::refusals` - because a default nobody had to write down and a control an
operator switched off are different facts, and the second one has to be visible.

**Which address the bucket is keyed on is a configuration decision, and it has to be.** The
peer address is unforgeable and is the proxy's for every request behind an ingress controller,
which is one bucket for the whole internet; a forwarded header is per-caller and is a value any
caller can write. `crate::proxy` is where that trade lives, and the refusal that keeps the
header from being believed without a named hop is in `crate::Settings::refusals`.

Two tiers, because the two surfaces have different shapes. The *probe* tier covers what a
caller may poll - liveness, and the generated `OpenAPI` document - and is tight, because nothing
there changes between two requests. The *api* tier covers the versioned API, where a legitimate
caller asks several questions in a row.

The names avoid the word that would be natural for the first tier, and not for a style reason:
`cargo xtask check-boundaries` flags a field whose name *begins* with `pub` as a public field,
because its scan is line-oriented and tests `starts_with("pub")`. A field called `public` is
therefore a gate failure on correct code. Renaming here is the cheap side of that trade, and the
names are more precise anyway - the first tier is not the unauthenticated one, since the
interface description sits behind the access token when one is configured.

### `struct Quota`

```rust
pub struct Quota
```

A sustained rate and the burst allowed above it.

Both non-zero: a quota of zero requests per second is a closed door, which is what
`enabled: false` says properly, and a zero burst is a limiter that rejects the first request
of every idle period.

#### Methods

```rust
pub const fn burst(self) -> NonZeroU32
```

```rust
pub const fn parse(name: &'static str, per_second: u32, burst: u32) -> Result<Self, InvalidQuota>
```

Reads a tier.

`name` is the configuration key this tier came from, so the error says which of the two
tiers is wrong rather than that one of them is.

```rust
pub const fn per_second(self) -> NonZeroU32
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidQuota`

```rust
pub enum InvalidQuota
```

Why a pair of numbers is not a quota.

#### Variants

- `Zero` - One of the two was zero.
- `BurstBelowRate` - The burst is below the sustained rate, which is a limiter that cannot sustain its own rate: the bucket refills faster than it can hold.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct RateLimitSettings`

```rust
pub struct RateLimitSettings
```

Both tiers, the switch, and what a bucket is keyed on.

The switch is separate from the numbers on purpose. A deployment that turns limiting off is
making a decision, and it should be one word in a file rather than a quota set so high it
never fires - which reads as a configured limit and is not one.

**The switch itself has no fixed default; it follows `crate::Environment`.** Off on a laptop,
because a limiter that fires while somebody is iterating is a bug report about sutura that is
really a bug report about the tier; on in production, because that is the deployment an
unbounded caller costs something. The same shape as `telemetry.format` and `api.docs`, including
the recorded flag - see `Self::enabled_default_for`.

**Not `Copy`, and that is the trusted-proxy list.** It is a `Vec`, so this group is cloned
rather than copied and the accessors borrow. Every call site is inside the assembled router,
once, at startup.

#### Methods

```rust
pub const fn api(&self) -> Quota
```

The tier for the versioned API.

```rust
pub const fn client_address(&self) -> ClientAddressSource
```

Where the address a bucket is keyed on comes from.

```rust
pub const fn enabled(&self) -> bool
```

```rust
pub const fn enabled_default_for(environment: crate::Environment) -> bool
```

The default for an environment: on in production, off everywhere else.

**A default and not a refusal in one direction, and a refusal in the other.** Nothing here
stops a developer switching the limiter on, and nothing here decides production: an explicit
`enabled: false` in production is refused by
`crate::Settings::refusals` regardless of what this function
would have returned. The two must not be conflated - default-off in development is a
convenience, and silently-off in production is how a deployment loses a control nobody
noticed it had.

A total match rather than a comparison, so a fourth environment has to state its own answer
instead of inheriting whichever branch it happens to fall into.

`crate::Environment::Test` is grouped with development, and that is an argument rather than
a convenience. A test that exercises the limiter cannot rely on this value anyway: asserting
a refusal needs a quota small enough to exhaust in two requests, so such a test writes
`rate_limit.enabled` and a tier down together - which is what
`sutura-http`'s harness already does. So defaulting on here would buy no coverage, and would
charge every unrelated test in the suite a limiter it never asked for, at a tier
(`probe_burst: 5`) that a loop over fixtures can exhaust. A limiter nothing exercises is
untested code; the answer to that is a test that names the switch, not a default that fires
during somebody else's assertion.

```rust
pub const fn enabled_was_explicit(&self) -> bool
```

Did an operator write the switch down, or did the environment decide it?

For the startup log, which says which arm was taken and whether anybody chose it.

```rust
pub const fn new(enabled: bool, enabled_was_explicit: bool, probe: Quota, api: Quota, client_address: ClientAddressSource, trusted_proxies: TrustedProxies) -> Self
```

```rust
pub const fn probe(&self) -> Quota
```

The tier for what an unauthenticated caller can reach.

```rust
pub const fn trusted_proxies(&self) -> &TrustedProxies
```

The hops whose forwarded header is believed.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## Module `proxy`

Which address a request is counted against, and who is allowed to say what it is.

# The problem this module exists for

A rate limiter needs a key, and on a surface with no per-caller identity - see
`crate::security` - the only key available is a network address. There are two ways to learn
one and they fail in opposite directions.

The **peer address** is the far end of the TCP connection. It cannot be forged by a caller,
and behind a reverse proxy or an ingress controller it is the *proxy's* address for every
request that has ever arrived - so every caller on the internet shares one bucket. Either one
abusive caller limits everybody, or the quota is set high enough to be no limit at all.

A **forwarded header** carries the address the proxy saw. It is the right answer behind a
proxy and it is a value any caller can write, so on a service that is *not* behind one it
makes every bucket the caller's to choose - which is worse than one shared bucket, because it
is a limiter that reports a configured limit and bounds nothing.

Neither is correct on its own. What is correct is a header read **only from a peer the
operator named**, which is what `TrustedProxies` is: an explicit list, empty by default, and
`ClientAddressSource::Forwarded` refuses to start without one rather than trusting a
spoofable header because a key was left at its default.

# Which entry of the header is the caller

`X-Forwarded-For` is appended to, hop by hop, so it reads left to right as oldest to newest.
Everything to the left of what our own trusted proxy wrote is a value the caller supplied, and
a caller who writes `X-Forwarded-For: 10.0.0.1` gets that value read as their address by
anything that takes the leftmost entry.

So the walk is from the **right**: skip the entries that are addresses of trusted proxies, and
the first entry that is not one is the client. If every entry is a trusted proxy, or the list
runs out, or an entry is not an address at all, the peer address is used - it is the one value
in the request nobody but the network can choose.

### `struct Cidr`

```rust
pub struct Cidr
```

An address or a block of them, as an operator writes it.

`10.0.0.0/8` or a bare `10.0.0.7`, in either address family. A bare address is a block whose
prefix covers every bit, so there is one shape to match against rather than two.

#### Methods

```rust
pub fn contains(&self, address: IpAddr) -> bool
```

Is `address` inside this block?

Mixed families never match: a v4 block does not contain a v6 address, and the canonical
form applied on both sides is what keeps a v4 peer arriving over a dual-stack socket from
being one.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidTrustedProxy>
```

Reads one entry.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidTrustedProxy`

```rust
pub enum InvalidTrustedProxy
```

Why a string is not an address block.

#### Variants

- `NotAnAddress` - The part before the slash was not an `IPv4` or `IPv6` literal.
- `PrefixNotANumber` - The part after the slash was not a number.
- `PrefixTooLong` - The prefix is longer than the address family has bits.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct TrustedProxies`

```rust
pub struct TrustedProxies
```

The peers whose forwarded header this service will believe.

**Empty by default, and the emptiness is the safe posture rather than an unset value.** With
nothing in the list the peer address is the key, which is correct for a service with no proxy
in front. It becomes non-empty only when an operator names the hop.

#### Methods

```rust
pub const fn is_empty(&self) -> bool
```

Did the operator name any hop at all?

```rust
pub const fn len(&self) -> usize
```

How many blocks are trusted, for the startup log.

```rust
pub fn parse<S>(entries: &[S]) -> Result<Self, InvalidTrustedProxy>
```

Reads the configured list.

```rust
pub fn trusts(&self, address: IpAddr) -> bool
```

Is `address` one of the hops the operator named?

#### Implements

`Clone`, `Debug`, `Default`, `Eq`, `PartialEq`

### `enum ClientAddressSource`

```rust
pub enum ClientAddressSource
```

Where the address a request is counted against comes from.

#### Variants

- `Peer` - The far end of the connection. Unforgeable, and one bucket for everything behind a proxy.
- `Forwarded` - `X-Forwarded-For`, read only from a peer in `TrustedProxies`, falling back to the peer address for anything else.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The spelling, for the startup log.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownClientAddressSource>
```

Reads the configured value.

```rust
pub const fn reads_a_header(self) -> bool
```

Does this source read a caller-supplied header?

#### Implements

`Clone`, `Copy`, `Debug`, `Default`, `Display`, `Eq`, `PartialEq`

### `struct UnknownClientAddressSource`

```rust
pub struct UnknownClientAddressSource
```

The configured value did not name a source.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `runtime`

How much work may be in flight at once, how wide the engine runs, and how long stopping may
take.

# Why this is its own group and not four more `server` keys

Because it bounds a different thing from anything in `crate::server`. Those keys are about one
request: where it arrives, how big it may be, how long the caller waits for a reply. These are
about the *process*: how many questions execute at once, how wide the engine that executes them
is, and what the budget for winding down is.

The distinction matters most for the timeout. `server.request_timeout_seconds` is a deadline on
the **reply**, not a bound on the **work**. When it expires the caller is answered `408` and the
handler future is dropped - and a started `tokio::task::spawn_blocking` task cannot be aborted,
so the question keeps running. Without something else in the picture, a caller asking questions
that cost more than the timeout gets a fast turnaround while the deployment keeps the whole
cost, and in-flight work accumulates at the rate limit with nothing shedding it. The blocking
pool defaults to 512 threads with an unbounded queue, so that backlog is bounded by memory.

`QueryConcurrency` is the bound on the work. It is the number of questions that may be
*executing*, and the permit is held by the blocking task rather than by the handler future - so
a timed-out request does not hand its slot back until the work it started actually finishes.
That is what makes the backlog a number somebody chose.

`AdmissionTimeout` stops the queue in front of that bound from being a second unbounded thing.
A question that cannot get a slot inside it is refused rather than left waiting, and a refused
waiter costs a dropped future rather than a thread.

`EngineWorkers` is not a bound at all - it is a width. The in-process engine drives its own
runtime and blocks on it, so a single-threaded one is a contention point every concurrent
question shares. See `sutura_exec_datafusion::DataFusionWarehouse`.

`ShutdownGrace` is the budget for stopping, and it covers the whole of stopping rather than
the connection drain alone.

Every type here has the shape of `crate::server::RequestTimeout`, which is the pattern this
crate already had: a private field, one `parse` that is the only constructor, a ceiling the type
declares, and a zero refused rather than read as `no limit`. They share
`InvalidBound` rather than growing a second error, so a bad value here lands in the same
`SettingsError` variant a bad server bound does.

### `struct QueryConcurrency`

```rust
pub struct QueryConcurrency
```

How many questions may be executing at once.

Not how many may be *in flight*: a request waiting for a slot, parsing a body or writing a
response is not counted. This is the number of questions holding a blocking-pool thread and a
data system, which is the resource that runs out.

#### Methods

```rust
pub const fn count(self) -> usize
```

```rust
pub const fn parse(count: usize) -> Result<Self, InvalidBound>
```

Reads a concurrency bound.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct AdmissionTimeout`

```rust
pub struct AdmissionTimeout
```

How long a question may wait for one of those slots.

Bounded because the alternative is a queue nothing empties. Shorter than the request timeout by
default, on purpose: a caller who has waited five seconds for a slot is better served by a
`503` they can retry than by a `408` twenty-five seconds later that says the same thing less
clearly.

#### Methods

```rust
pub const fn duration(self) -> Duration
```

```rust
pub const fn parse(seconds: u64) -> Result<Self, InvalidBound>
```

Reads a wait in whole seconds.

```rust
pub const fn seconds(self) -> u64
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct EngineWorkers`

```rust
pub struct EngineWorkers
```

How many threads the in-process engine's own runtime gets.

Resolved to a number at load time rather than kept as "whatever the machine has", so the value
in the startup log is the value in effect. An absent key follows
`std::thread::available_parallelism`; a present one wins, which is what a container with a CPU
quota needs - `available_parallelism` reports what the kernel exposes, and on most container
runtimes that is the host's core count rather than the cgroup's share.

#### Methods

```rust
pub const fn count(self) -> usize
```

```rust
pub fn parse(configured: Option<usize>) -> Result<Self, InvalidBound>
```

Reads a worker count, or resolves the absent one.

Not a `const fn`, unlike its neighbours, because the absent case asks the operating system
how many threads this machine can run at once. A machine that will not answer is treated as
one, which is the behaviour this adapter had before the key existed rather than a failure to
start.

```rust
pub const fn was_chosen(self) -> bool
```

Did an operator write this number, or did the machine?

For the startup log, and for the same reason `api.docs` records it: an operator reading
`engine_worker_threads = 8` needs to know whether that was their decision.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct ShutdownGrace`

```rust
pub struct ShutdownGrace
```

How long stopping may take, once stopping has been asked for.

Chosen against the deadline on the other side rather than as a round number. An orchestrator
sends a termination signal and starts a kill timer - the usual window is thirty seconds - and a
process still running when that expires is killed mid-answer, so whatever it would have done on
the way out does not happen. Fifteen seconds leaves room for the exit itself.

It bounds the *whole* of stopping and not the connection drain alone. The drain gets the budget
first; what is left of it is what the runtime will wait for a blocking task it cannot cancel.
See `sutura_runtime::Shutdown::remaining_grace`.

#### Methods

```rust
pub const fn duration(self) -> Duration
```

```rust
pub const fn parse(seconds: u64) -> Result<Self, InvalidBound>
```

Reads a grace period in whole seconds.

```rust
pub const fn seconds(self) -> u64
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct RuntimeSettings`

```rust
pub struct RuntimeSettings
```

Everything about how much runs at once and how the process stops.

#### Methods

```rust
pub const fn admission_timeout(self) -> AdmissionTimeout
```

```rust
pub const fn engine_workers(self) -> EngineWorkers
```

```rust
pub const fn max_concurrent_queries(self) -> QueryConcurrency
```

```rust
pub const fn new(max_concurrent_queries: QueryConcurrency, admission_timeout: AdmissionTimeout, engine_workers: EngineWorkers, shutdown_grace: ShutdownGrace) -> Self
```

Assembles the group from parts that have each already been parsed.

Infallible, like `crate::server::ServerSettings::new` and for the same reason: there is no
cross-field rule inside this group. The one relationship worth knowing - that an admission
timeout above the request timeout is never reached - spans two groups and is documented on
`AdmissionTimeout::MAX_SECONDS` rather than refused.

```rust
pub const fn shutdown_grace(self) -> ShutdownGrace
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## Module `security`

The access control this service has, and the honest name for what it is not.

**There is no per-caller identity in sutura today, and nothing here invents one.** No request
context reaches the query path, no credential is minted per request, and the `CredentialBroker`
port that would do it is deliberately absent because a port arrives with its adapter.
`examples/multi-player/README.md` is where that gap is written down for a reader.

So what an `AccessToken` does is narrower than authentication, and the narrowness is the
point of this module documentation: a presented token proves the caller holds a secret the
deployment was configured with. It proves nothing about *which* caller, it cannot be scoped,
it cannot be revoked for one party without revoking it for all of them, and it does not reach
the data system - every query still runs with whatever access the process already had.

It is worth having anyway, because the alternative on a non-loopback interface is an
unauthenticated way to read whatever the process can read. It is not worth mistaking for
identity, which is why `SecuritySettings::describes_identity` exists as an associated function
that always answers the same thing: the startup log prints it, so an operator cannot deploy this
believing otherwise.

### `struct AccessToken`

```rust
pub struct AccessToken
```

A pre-shared secret a caller presents to reach the service.

Held as a `Secret`, so the whole settings tree can be written to the startup log with
`Debug` and the token cannot come out with it.

**Not comparable with `==`, and that is inherited rather than reimplemented.** `Secret`
implements no `PartialEq` on purpose: a derived comparison on credential material returns on
the first differing byte, which is a timing oracle at whatever call site adds it. The
comparison lives here instead, once, as `AccessToken::matches_in_constant_time`.

#### Methods

```rust
pub fn matches_in_constant_time(&self, presented: &str) -> bool
```

Does `presented` equal the configured token?

Named for the property rather than for the operation, because the property is the only
reason this function exists rather than a `==`.

**Both sides are hashed first, and that is not ceremony.** `subtle` compares equal-length
byte slices without branching, which removes the early-return oracle - but comparing the
raw strings would still have to decide what to do about differing lengths, and every
answer to that leaks the length before it leaks anything else. Reducing both sides to a
fixed 32 bytes removes the question: every comparison is over the same number of bytes
whatever arrived.

What this still does not do: it is not a password hash. There is no salt and no work
factor, because the input is a high-entropy secret an operator generated rather than
something a person chose, and nothing here is stored for an attacker to find offline.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidAccessToken>
```

Reads a configured token.

**Parses the wire grammar, not merely a length.** The type is named for a value that
arrives in an `Authorization` header, so what it accepts is what such a header can carry:
see `Self::wire_grammar` and `InvalidAccessToken::NotRepresentableOnTheWire`.

#### Implements

`Clone`, `Debug`

### `enum InvalidAccessToken`

```rust
pub enum InvalidAccessToken
```

Why a string is not usable as an access token.

#### Variants

- `TooShort` - Shorter than `AccessToken::MIN_LENGTH`.
- `Untrimmed` - Whitespace at either end, which is almost always a copy-paste artefact and would otherwise make every request fail for a reason nobody can see in a log.
- `NotRepresentableOnTheWire` - A character no `Authorization` header could carry to us.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum TlsTermination`

```rust
pub enum TlsTermination
```

Where TLS is terminated for this deployment.

**A declaration, not a control.** Nothing here encrypts anything except
`Self::InProcess`; the other three name a terminator that lives somewhere else, and the point
of writing it down is that the *cleartext hop* it implies is then a stated fact rather than an
assumption. The bearer token crosses that hop in the clear, and how far the hop reaches is the
whole difference between the three:

| Declared | What terminates TLS | What the token crosses in cleartext |
| --- | --- | --- |
| `none` | nothing | the whole path from the caller. Only sane on loopback |
| `sidecar` | a proxy in this pod | a loopback hop inside the pod |
| `ingress` | an ingress controller or gateway | the pod network, from that hop to this process |
| `in-process` | this process | nothing - the connection ends here |

So `ingress` is not a weaker `sidecar`: it is the same posture with a longer cleartext segment,
and whether that segment is acceptable is a question about the cluster network - a mesh with
mutual TLS between pods answers it differently from a flat one. This type does not pretend to
know, and a startup log that said "TLS enabled" would be pretending.

#### Variants

- `None` - Nothing terminates TLS. Plaintext from the caller to here.
- `Sidecar` - A terminator inside this pod or on this host, reached over loopback.
- `Ingress` - An ingress controller or gateway. The hop from it to this process crosses the pod network.
- `InProcess` - This process. Requires the `tls` feature and a certificate and key.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The spelling, for the startup log.

```rust
pub const fn cleartext_hop(self) -> &'static str
```

The cleartext hop this declaration implies, as a sentence for the startup log.

A function rather than a comment for the same reason
`SecuritySettings::describes_identity` is one: the log, the documentation and this type
read the same value, so none of them can drift into claiming end-to-end encryption.

```rust
pub const fn is_declared(self) -> bool
```

Was anything said at all?

`Self::None` is the default, so "not declared" and "declared as nothing" are the same
value - which is why the refusal for a non-loopback bind is keyed on this rather than on an
`Option`. An operator who means plaintext on loopback writes nothing and gets it.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownTlsTermination>
```

Reads the configured value.

```rust
pub const fn terminates_here(self) -> bool
```

Does this process hold the TLS connection itself?

#### Implements

`Clone`, `Copy`, `Debug`, `Default`, `Display`, `Eq`, `PartialEq`

### `struct UnknownTlsTermination`

```rust
pub struct UnknownTlsTermination
```

The configured value did not name a place TLS is terminated.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct SecuritySettings`

```rust
pub struct SecuritySettings
```

The access posture, and the declaration that goes with a non-loopback bind.

Two fields rather than one, because they answer different questions and collapsing them was
the tempting mistake: a token says *who may reach this*, and the declaration says *what, if
anything, encrypts the path it travels*. A deployment that sets a token but binds the wildcard
with nothing in front has answered only the first.

**The declaration replaced a boolean, and that is the point of it.** The boolean it replaced -
`expose_beyond_loopback` - recorded that somebody meant to publish the service and said nothing
about what protects the token in flight, so a wildcard bind with no terminator anywhere read
exactly like one behind a gateway. A value naming the terminator cannot be satisfied by
agreeing that off-host is intended.

#### Methods

```rust
pub const fn access_token(&self) -> Option<&AccessToken>
```

The configured token, if there is one.

```rust
pub const fn describes_identity() -> bool
```

Does anything here establish who the caller is?

Always `false`, and it is a function rather than a comment so the startup log and the
documentation read the same value. When a `CredentialBroker` and a request context exist,
this stops being a constant and the log line changes with it; until then a deployment is
told, on every boot, that the token authenticates the deployment and not the caller.

```rust
pub const fn new(access_token: Option<AccessToken>, tls_termination: TlsTermination) -> Self
```

```rust
pub const fn tls_termination(&self) -> TlsTermination
```

Where the operator said TLS is terminated.

```rust
pub const fn token_state(&self) -> &'static str
```

Whether a token is configured, as a word for the startup log.

A method and not a `Debug` of the option, so the log line cannot become the token by
somebody changing the field type later.

#### Implements

`Clone`, `Debug`, `Default`

## Module `server`

Where the service listens, and the two bounds every request is held to.

The interesting type here is `BindAddress`, and it is interesting for a security reason
rather than an ergonomic one. This service has no per-caller identity - see the module
documentation on `crate` - so the interface it listens on is the whole of its perimeter.
Making the bind an IP address rather than a string is what lets
`BindAddress::is_loopback` be a fact the startup refusals can read, instead of a substring
test somebody has to remember to write.

### `struct BindAddress`

```rust
pub struct BindAddress
```

The socket the service listens on.

An `IpAddr` and a port, never a hostname. A hostname is refused rather than resolved: a name
resolves to whatever the resolver says today, which may be a public interface tomorrow, and
a perimeter that moves when DNS moves is not a perimeter. The operator writes the interface
they mean.

#### Methods

```rust
pub const fn is_loopback(self) -> bool
```

Is this address reachable only from this host?

The question the production refusals are keyed on. The two wildcard addresses answer
`false`, which is the answer that matters: the wildcard is the bind that reaches every
interface, and it is the one an operator reaches for without meaning to publish anything.

```rust
pub fn parse(host: impl AsRef<str>, port: u16) -> Result<Self, InvalidBindAddress>
```

Reads a host and port.

```rust
pub const fn port(self) -> u16
```

The port, so a caller can report the one it actually got.

```rust
pub const fn socket(self) -> SocketAddr
```

The address, for whatever binds the socket.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidBindAddress`

```rust
pub enum InvalidBindAddress
```

Why a host and port are not an address to listen on.

#### Variants

- `NotAnIpAddress` - The host was not an `IPv4` or `IPv6` literal.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct RequestTimeout`

```rust
pub struct RequestTimeout
```

How long one request may take before the service gives up on it.

Bounded at both ends. Zero is a service that answers nothing, and an hour is a connection
held open long enough that a handful of them are the outage: a question here is one
aggregate over a bounded range, so a minute is already generous and five is the ceiling.

#### Methods

```rust
pub const fn duration(self) -> Duration
```

```rust
pub const fn parse(seconds: u64) -> Result<Self, InvalidBound>
```

Reads a timeout in whole seconds.

Seconds and not a duration string: sub-second precision is meaningless for a bound this
coarse, and a parser for a suffixed number is a second grammar for a single value.

```rust
pub const fn seconds(self) -> u64
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct BodyLimit`

```rust
pub struct BodyLimit
```

The largest request body the service will read.

A modelled question is a metric name, a grain, two dates and at most four dimensions, which
is a few hundred bytes. The bound exists because a body limit is the cheapest availability
control there is, and because the default in most stacks is whatever arrives.

#### Methods

```rust
pub const fn bytes(self) -> usize
```

```rust
pub const fn parse(bytes: usize) -> Result<Self, InvalidBound>
```

Reads a body limit in bytes.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct ShutdownGrace`

```rust
pub struct ShutdownGrace
```

How long stopping may take, once stopping has been asked for.

Chosen against the deadline on the other side rather than as a round number. An orchestrator
sends a termination signal and starts a kill timer - the usual window is thirty seconds - and a
process still running when that expires is killed mid-answer, so whatever it would have done on
the way out does not happen. Fifteen seconds leaves room for the exit itself.

It bounds the *whole* of stopping and not the connection drain alone: the drain gets the budget
first, and what is left of it is what the runtime will wait for a blocking task it cannot
cancel. See `sutura_runtime::Shutdown::remaining_grace`.

Zero is refused for the reason every bound in this module is. A zero grace period means "drop
every in-flight answer immediately", which is a decision somebody would write differently.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidBound`

```rust
pub enum InvalidBound
```

Why a bound is not a bound.

#### Variants

- `Zero` - Nothing here may be zero: a zero timeout answers nothing and a zero body limit accepts nothing, and both read as no limit at all to somebody writing the file.
- `TooLarge` - Above the ceiling this type declares.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct TlsMaterial`

```rust
pub struct TlsMaterial
```

A certificate chain and the private key that goes with it, as paths.

**Paths and nothing more, and the split is deliberate.** This crate holds no framework and
reads no files: it parses the *pair* - both halves or neither - and stops there. Whether the
files are readable, whether they are PEM at all, and whether the key matches the certificate
are questions only the TLS implementation can answer, so they are answered once, in
`sutura_http::tls`, before the socket is bound. Two checks in two crates would be two messages
for one mistake, and the weaker one would be the reassuring one.

#### Methods

```rust
pub fn certificate(&self) -> &Path
```

The certificate chain, in PEM.

```rust
pub fn key(&self) -> &Path
```

The private key, in PEM.

```rust
pub fn parse(certificate: Option<&str>, key: Option<&str>) -> Result<Option<Self>, InvalidTlsMaterial>
```

Reads the pair, or nothing.

`None` for both is "no in-process TLS", which is the default and is not an error.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum InvalidTlsMaterial`

```rust
pub enum InvalidTlsMaterial
```

Why a pair of paths is not usable TLS material.

#### Variants

- `OnlyOneHalf` - One half was given and the other was not.
- `EmptyPath` - A path was given as an empty string, which is not a path.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct ServerSettings`

```rust
pub struct ServerSettings
```

Everything about the socket, the two per-request bounds, and the TLS material if there is any.

**Not `Copy`, and that is the TLS paths.** Every accessor borrows or returns a `Copy` value, and
the group itself is read once, at assembly time.

#### Methods

```rust
pub const fn bind(&self) -> BindAddress
```

```rust
pub const fn max_body(&self) -> BodyLimit
```

```rust
pub const fn new(bind: BindAddress, request_timeout: RequestTimeout, max_body: BodyLimit, tls: Option<TlsMaterial>) -> Self
```

Assembles the group from parts that have each already been parsed.

Infallible, and that is the shape the newtypes buy: there is no cross-field rule inside
this group, so once every part exists the group exists. The cross-field rules - the ones
that pair a bind address with an environment, a token and a TLS declaration - live in
`crate::Settings::parse`, because they need the other groups to decide.

```rust
pub const fn request_timeout(&self) -> RequestTimeout
```

```rust
pub const fn tls(&self) -> Option<&TlsMaterial>
```

The certificate and key this process would terminate TLS with, if any were configured.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## Module `telemetry`

What the log looks like, and who is meant to read it.

One decision with two right answers, which is why it is a type rather than a flag. In
production a log line is read by a collector: it has to be one JSON object per line, with the
span context attached, so a query can find every line belonging to one request. On a laptop
the same line is read by a person compiling every thirty seconds, and a JSON object per line
is unreadable there.

The default therefore follows `crate::Environment` and nothing else, so the two cannot be
set inconsistently by omission. An explicit value overrides it - a developer debugging what
the collector will actually receive needs that - and the startup log says which of the two
happened.

### `enum LogFormat`

```rust
pub enum LogFormat
```

How a log line is rendered.

#### Variants

- `Bunyan` - One JSON object per line, in the bunyan schema. What a collector ingests.
- `Pretty` - Indented, coloured, and meant for a terminal.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

```rust
pub const fn default_for(environment: Environment) -> Self
```

The format an environment gets when the configuration does not say.

**This is the split, in one function.** Production is machine-readable and everything else
is human-readable, and it is expressed as a total match rather than an `if` so a fourth
environment cannot inherit an answer nobody chose for it.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownLogFormat>
```

Reads a format name.

#### Implements

`Clone`, `Copy`, `Debug`, `Deserialize<'de>`, `Display`, `Eq`, `PartialEq`, `Serialize`

### `struct UnknownLogFormat`

```rust
pub struct UnknownLogFormat
```

The string was neither format.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct LogFilter`

```rust
pub struct LogFilter
```

A tracing filter directive, as written for `RUST_LOG`.

Kept as text here and turned into a real filter by whatever installs the subscriber, because
the type that parses one lives in `tracing-subscriber` and this crate holds no framework. What
*is* checked here is that it is not empty and not a smuggled second line: an empty filter
silently means "no directives", which is a service that logs at the default level while its
configuration says otherwise.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidLogFilter>
```

Reads a filter directive.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidLogFilter`

```rust
pub enum InvalidLogFilter
```

Why a string is not a filter directive.

#### Variants

- `Empty` - Empty, or only whitespace.
- `ControlCharacter` - A control character, which in practice is a newline pasted in with the value. A newline in a filter is a log line an attacker can forge if the value ever reaches the log itself.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct ServiceName`

```rust
pub struct ServiceName
```

The name every log line is attributed to.

A separate type because it is the field a collector groups by, so an empty or whitespace one
makes every line from this deployment unattributable.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidServiceName>
```

Reads a service name.

Narrow on purpose: a name with a space or a quote in it has to be escaped by every
consumer, and the one that forgets produces a log line that does not parse.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum InvalidServiceName`

```rust
pub enum InvalidServiceName
```

Why a string is not a service name.

#### Variants

- `Empty`
- `NotAnIdentifier`

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `struct TelemetrySettings`

```rust
pub struct TelemetrySettings
```

Everything about the log.

#### Methods

```rust
pub const fn filter(&self) -> &LogFilter
```

```rust
pub const fn format(&self) -> LogFormat
```

```rust
pub const fn format_was_explicit(&self) -> bool
```

```rust
pub const fn new(service_name: ServiceName, filter: LogFilter, format: LogFormat, format_was_explicit: bool) -> Self
```

```rust
pub const fn service_name(&self) -> &ServiceName
```

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`
