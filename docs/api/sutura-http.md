<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-http

The public API of `sutura-http`, rendered from rustdoc JSON.

The HTTP surface. Transport only.

# What this crate is allowed to do

Turn bytes into a `sutura_domain::query::Query`, hand it to a port, and turn what comes back
into bytes. Nothing here decides whether a question may be answered - that is
`sutura-semantic` and `sutura-app`, behind the `Surface` port - and nothing here reads a
catalog directory or opens a data system. The one place either port is touched is
`surface::LocalService::start`, which is called by the composition root.

# The three properties a reader should check first

**A refusal is a `200`.** `POST /v1/query` answers `200` with `outcome: refusal` when a question
is one the caller may not have. An error status would invite a client library to retry, and
retrying a governance decision until it succeeds is the behaviour the refusal exists to prevent.

**There is no per-caller identity.** No request context reaches the query path, no credential is
minted per request, and the `CredentialBroker` port that would do it is deliberately absent
because a port arrives with its adapter. Where an access token is configured, presenting it
proves the caller holds a secret an operator configured - it authenticates the *deployment*, not
the caller, and every question is still answered with whatever access the process already had.
That sentence is in the generated document, in the startup log and in
`sutura_config::security`, because those are three different readers.

**`/health` carries nothing.** It is the one path an unauthenticated caller can always reach, so
every field it might have is a field handed to anybody who can route a packet. No version, no
build, no configuration, no catalog. A test asserts the body byte for byte.

# What is deliberately absent

* **No CORS layer.** A browser is not a client of this surface. An allow-list nobody needs is an
  allow-list somebody widens.
* **No request identifier.** It belongs in the failure body and there is nothing to put in it:
  nothing in this service mints one yet, and a field that is always absent is worse than no
  field.
* **No audit sink.** `AGENTS.md` records "every call is attributable, refusals included" as an
  invariant enforced by one. There is none, and there is no principal to record if there were.
  Every question and every outcome reaches the log, and the log is named for what it is.
* **No readiness route.** The module documentation on the liveness route says why: there is
  nothing it could report that is not
  already true of a process that is listening.

# Assembling it

```no_run
use std::sync::Arc;

use sutura_config::{Environment, Settings, Sources};
use sutura_http::{ServiceState, router, serve};
use sutura_runtime::Shutdown;

# async fn wire(surface: Arc<dyn sutura_http::Surface>) -> Result<(), Box<dyn core::error::Error>> {
let settings = Settings::load(&Sources::defaults(Environment::Development))?;
let address = settings.server().bind().socket();
let state = ServiceState::new(surface, Arc::new(settings));
serve(router(&state)?, address, Shutdown::new()).await?;
# Ok(())
# }
```

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

## Module `client_address`

The address a rate-limit bucket is counted against, and why it is not simply the peer.

# The bug this module exists to fix

`tower_governor` ships two extractors and both are wrong for one of the two deployments this
service has. `PeerIpKeyExtractor` keys on the connection's far end, which cannot be forged and
which behind an ingress controller is *the ingress* for every request that has ever arrived - so
every caller on the internet shares one bucket, and the limiter either takes everybody down with
one abusive caller or is set high enough to bound nothing. `SmartIpKeyExtractor` reads
`X-Forwarded-For` unconditionally, which is correct behind a proxy and is a value any caller can
write, so with no proxy in front every caller picks their own bucket.

`ClientAddress` is the third option: read the header **only from a peer the operator named**,
and use the peer address for everything else. Whether it reads a header at all is
`sutura_config::ClientAddressSource`, the named hops are
`sutura_config::TrustedProxies`, and the combination that would trust a header with nobody
named is a startup refusal rather than a default - see `sutura_config::NotFitToServe`.

# Reading the header from the right

`X-Forwarded-For` is appended to hop by hop, so it reads oldest to newest left to right. The
*rightmost* entry is the one our own trusted proxy wrote; everything left of it arrived from the
caller. Taking the leftmost entry - which is what every `maybe_x_forwarded_for` helper does,
upstream's included - hands the key straight to whoever sent the request.

So the walk is from the right, skipping entries that are themselves trusted hops, and the first
entry that is not one is the client. Four things fall back to the peer address, which is the one
value in a request that nobody but the network chooses:

* the peer is not a trusted proxy - the header is then a caller's own text;
* there is no header;
* an entry is not an address, so nothing further left can be trusted either;
* every entry is a trusted hop, so the header names no client.

Multiple `X-Forwarded-For` header *lines* are walked from the last line backwards as well.
`HeaderMap::get` returns the first, and a caller who sends their own line before the proxy
appends a second one would otherwise have theirs read.

### `struct ClientAddress`

```rust
pub struct ClientAddress
```

Where a bucket's key comes from, as a value the limiter layer is built with.

`Clone` because `KeyExtractor` requires it and the layer clones it per connection; the trusted
list is behind an `Arc` so that clone is a pointer bump rather than a copy of every block.

#### Methods

```rust
pub fn attribute(&self, headers: &HeaderMap, peer: IpAddr) -> IpAddr
```

The address this request is attributed to.

Public and taking the parts rather than a request, so the whole decision is testable without
building one - the interesting cases are all about which entry of a header is believed.

```rust
pub fn from_settings(limits: &sutura_config::RateLimitSettings) -> Self
```

Reads the configuration's own two values, so a caller cannot pair a source with the wrong
list.

```rust
pub const fn new(source: ClientAddressSource, trusted: Arc<TrustedProxies>) -> Self
```

Builds the extractor the configuration describes.

#### Implements

`Clone`, `Debug`, `KeyExtractor`

## Module `constants`

Every path this service answers on, in one place.

The version prefix is a constant rather than a literal at each mount point, and that is the
whole of the versioning story: a `v2` is a second module that mounts under a second prefix
beside this one, sharing the state, the layers and the generated document. Nothing existing has
to move, which is what "additive" means.

The generated `OpenAPI` document is built from the same constants, so a path in the document and
a path in the router cannot disagree about where a handler lives.

### `constant API_V1_PREFIX`

The versioned API prefix.

### `constant HEALTH_PATH`

Liveness. Deliberately *outside* the version prefix.

A liveness probe is not part of the API contract - it is a property of the process, and it must
keep working across a version bump without an orchestrator being reconfigured. It is also the
one path that is not behind the access token, because a probe has no credential to present.

### `constant OPENAPI_JSON_PATH`

Where the generated interface description is served, when it is served at all.

### `constant SWAGGER_UI_PATH`

Where the browser interface over that description is served.

### Module `base_paths`

The mount point of each group inside a version.

#### `constant CATALOG`

What this catalog defines.

#### `constant QUERY`

Asking one certified question.

## Module `middleware`

The two layers that stand in front of a handler, and the reason there are exactly two.

# The token gate

`require_token` compares a bearer token against the configured one in constant time and
answers `401` otherwise. **It is not authentication of a caller.** It proves the caller holds a
secret an operator configured, which is a different and much smaller claim - see
`sutura_config::security` for what that does and does not buy, and the startup log for the line
an operator reads about it.

When no token is configured the gate passes everything through. That is only reachable on a
loopback bind outside production, because `sutura-config` refuses to start in any other
combination - so the permissive branch here is guarded by a startup refusal rather than by this
function being careful.

# The limiter

Two tiers, and a third function that is a no-op with the same shape.

**What a bucket is keyed on is configured, not decided here, and the reason is in
`crate::client_address`.** Neither of `tower_governor`'s own extractors is right for both of
this service's deployments: the peer address is unforgeable and is the ingress controller's for
every request behind one, and a forwarded header is per-caller and is a value any caller can
write. So the key comes from `ClientAddress`, which reads the header only from a hop the
operator named.

Rate limiting is not authentication. It bounds how fast something can be done, not who may do
it.

# The limiter has to be reaped, and that is not tuning

`governor`'s keyed store grows one entry per distinct key and sheds nothing until it is asked
to. Nothing asked. That was survivable while the token gate sat *outside* the limiter, because
an unauthenticated request was refused before it could create a bucket - so the only
unauthenticated path into a limiter was liveness.

`crate::router` now puts the limiter outside the gate, which is the point of the reordering:
a wrong-token attempt has to cost a cell or it is an unlimited guessing loop. That makes every
reachable path a path an unauthenticated caller can create a bucket on, and with the header
keying above it is one bucket per real client rather than one per ingress. **So the reordering
and `spawn_reaper` are one change and must not be separated:** either alone is worse than
neither.

### `struct LimiterHandle`

```rust
pub struct LimiterHandle
```

One built tier, and the handle on its keyed state.

**This exists because `GovernorLayer::new(Arc::new(config))` used to be the last anyone saw of
the configuration.** The layer keeps its own `Arc` and exposes nothing, so with no handle kept
there was no way to call `retain_recent` and nothing anywhere did - the store grew one entry per
distinct key for the life of the process.

#### Methods

```rust
pub fn reap(&self)
```

Drops every key whose state is indistinguishable from a fresh one, and gives the memory
back.

Dropping such a key changes no decision: a caller whose bucket was reaped gets a fresh
bucket, and a fresh bucket is exactly what the reaped state said they had.

```rust
pub const fn tier(&self) -> &'static str
```

Which tier this is, for a log line.

```rust
pub fn tracked(&self) -> usize
```

How many keys the store is holding.

An estimate by the store's own documentation, which is what makes it a size to watch rather
than a number to assert an exact bound on. A test asserts that it goes up with distinct
callers and back down after a sweep, which is the property that matters.

#### Implements

`Clone`, `Debug`

### `struct LimiterNotBuilt`

```rust
pub struct LimiterNotBuilt
```

A configured quota did not produce a limiter.

**Unreachable, and an error rather than a panic or a fallback anyway.** `finish` returns `None`
only for a zero replenishment period or a zero burst, and `Quota::parse` refuses both - so this
cannot happen from a loaded configuration. It is an error because the two alternatives are both
wrong: a panic takes the process down for something the type system already ruled out, and a
silent fallback to an unlimited layer is a service that reports a configured limiter and has
none. This way an impossible state is a refusal to start.

#### Implements

`Debug`, `Display`, `Error`

### `fn spawn_reaper`

```rust
pub fn spawn_reaper(handles: &[LimiterHandle], interval: std::time::Duration) -> Result<(), std::io::Error>
```

Sweeps every tier's keyed state on a fixed interval, until nothing is left to sweep.

**An OS thread and not a `tokio` task, and that is forced rather than preferred.** The router is
assembled before the runtime exists - the composition root builds it, then builds the runtime,
because the engine behind the `Warehouse` port drives its own and `Runtime::block_on` panics
inside one. A `tokio::spawn` here would therefore panic at startup, and a spawn guarded by
`Handle::try_current` would silently do nothing, which is the failure mode this whole function
exists to remove. The work is a `retain` over a concurrent map and does not need an executor.

Returns an error rather than carrying on without a sweeper: a process that cannot start a
housekeeping thread is a process whose keyed store grows without bound, and that should be a
refusal to start rather than a line in a log.

### `fn require_token`

```rust
pub async fn require_token(__arg0: axum::extract::State<crate::state::ServiceState>, request: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response
```

Requires a bearer token, when one is configured.

A `from_fn_with_state` middleware rather than an extractor, because an extractor runs per
handler and this has to run for a whole subtree. It runs for every path in that subtree that
RESOLVES to a handler; a path under the prefix matching no route skips it and falls through to
the top-level `404`, which `crate::router` states along with why that is acceptable.

### `fn probe_rate_limit_layer`

```rust
pub fn probe_rate_limit_layer(quota: sutura_config::Quota, key: crate::client_address::ClientAddress) -> Result<(RateLimit, LimiterHandle), LimiterNotBuilt>
```

The tier for what an unauthenticated caller can reach.

### `fn api_rate_limit_layer`

```rust
pub fn api_rate_limit_layer(quota: sutura_config::Quota, key: crate::client_address::ClientAddress) -> Result<(RateLimit, LimiterHandle), LimiterNotBuilt>
```

The tier for the versioned API.

### `fn disabled_rate_limit_layer`

```rust
pub const fn disabled_rate_limit_layer() -> tower::layer::util::Identity
```

A limiter that limits nothing.

**The type is deliberately not `RateLimit`, and it still composes.** `axum::Router::layer` is
generic over the layer and returns a plain `Router` - the service type is erased - so the two
arms of "limiting on" and "limiting off" have the same type at the call site without either of
them pretending to be the other. `Identity` is a zero-sized type and this is a `const fn`, so
the disabled path costs nothing at runtime and nothing at all in the response path.

A quota set so high it never fires would have been the alternative, and it is worse: it reads as
a configured limit in a log and in a review, and it is not one.

### `fn enforce_timeout`

```rust
pub async fn enforce_timeout(__arg0: axum::extract::State<std::time::Duration>, request: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response
```

Gives up on a request that outran the configured bound, with the documented body.

**Written here rather than taken from `tower_http`, and the reason is a body.** Pinned
`tower-http` 0.6.11 implements `TimeoutLayer::with_status_code` as
`Response::new(B::default())` - the status and an *empty* body - so the `408` this surface
documents, and which `problem.rs` promises carries a `crate::problem::ProblemBody` like every
other failure, was a status nothing put a body behind. `Failure::Timeout` existed and was never
constructed. Ten lines here is the whole cost of the response shape being one shape.

It bounds *the response*, which is what a caller experiences, and not the work: a question
already handed to the blocking pool keeps running until the data system answers it. Cancelling
that needs a cancellation token the `Warehouse` port does not have.

### `type_alias RateLimit`

A limiter layer, keyed by whatever `ClientAddress` attributes a request to, reporting its
state in response headers.

A named alias because the inline form is over the complexity threshold in `clippy.toml`. The
`StateInformationMiddleware` in it is not incidental: it is what makes the layer emit the
remaining-quota headers, and it is part of the type because that choice is made at construction.

### `constant REAP_INTERVAL`

How often the keyed state is swept.

A constant rather than a configuration key. It is not a posture decision - nothing a caller can
do changes what the right answer is - and a sweep is `O(keys)` over a concurrent map, so a knob
here would only ever be set wrong. One minute is far below any horizon at which the map's size
matters and far above the cost of the sweep.

## Module `openapi`

The generated interface description.

**Generated from the handlers, never written by hand.** `utoipa_axum::routes!` registers a
handler in the router and in the document from the same `#[utoipa::path]` attribute, so a
handler with no attribute is a compile error and a handler mounted somewhere else moves in both
places at once. A hand-written document is a second source of truth that goes stale on the first
change nobody remembered to mirror.

The derive below carries only the components and the tags. Every *path* comes from the router
fragments, merged here.

# Determinism, and the hazard to know about

An interface description is something a client generator runs over and a reviewer diffs, so
identical inputs have to produce identical bytes. Two things about `utoipa` bear on that:

* The `preserve_order` feature is on. Without it the component and path maps are unordered, and
  the document's key order changes between builds.
* `utoipa::openapi::extensions::Extensions` - the `x-*` keys - is backed by a `HashMap`, whose
  iteration order is randomised **per map instance**, not just per process. So a document
  carrying any extension serializes differently on every call to `to_json`, and no feature flag
  fixes it. The repair is to reparse the output into an order-preserving tree, sort only the
  extension keys and re-emit.

**This service sets no extension anywhere, so the repair is not built.** That is a statement
about today rather than a claim about the format, and it is enforced rather than asserted:
`tests::the_document_is_byte_stable_across_independent_builds` builds the document twice, from
scratch, and compares the bytes. Because the randomisation is per instance, that test fails the
moment an `x-*` extension is added - and the failure is the signal to build the ordered-tree
repair, which needs one more dependency and belongs in this file.

# No security scheme is declared, and that is honest

`utoipa` can describe a bearer scheme, and describing one here would put an `Authorize` button
in the browser UI. It is deliberately absent: a scheme in the document reads as an
authentication model, and this service has none - the token authenticates the *deployment*, not
the caller. The `401` on each operation says what actually happens, and
`DESCRIPTION` says what it means.

### `struct ApiDoc`

```rust
pub struct ApiDoc
```

The document, before the route fragments are merged into it.

Components and tags only. The schemas listed here are the ones referenced from a response body
or a request body; anything reachable from one of them is pulled in by the derive.

#### Implements

`OpenApi`

### `fn document`

```rust
pub fn document() -> utoipa::openapi::OpenApi
```

The whole document: the derived shell plus one fragment per version.

A `v2` adds one line here and nothing else, which is what makes versioning additive: the
fragment carries its own absolute paths because the prefix is applied by the `nest` below.

### `fn document_json`

```rust
pub fn document_json() -> Result<String, serde_json::Error>
```

The document as JSON.

One function, used by the route that serves it and by any tooling that dumps it, so the two
cannot emit different bytes. See the determinism note in this module's documentation for what
that currently rests on.

## Module `problem`

The one body every failure comes back as, and the one thing that turns a failure into a
response.

# A refusal is not in here

Worth stating first, because it is the distinction the whole surface turns on. A *refusal* - the
caller asked something they may not have - is a `200` carrying
`crate::wire::OutcomeBody::Refusal`. A *failure* is everything else: a body that is not a
question, a missing credential, a limit reached, a data system that did not answer. Only
failures reach this module.

# What a failure body may say

`Failure::Internal` carries no detail, ever. The text of an internal error is a path, a table
name, a column name or a driver message, and handing any of those to an unauthenticated caller
is a description of the deployment. The detail is logged and the response says the status and
nothing else. Every other variant's detail is either fixed text or a message about the caller's
own request.

### `enum Failure`

```rust
pub enum Failure
```

Why a request could not be answered.

One enum rather than a status code chosen per call site: the status, the code and the sentence
come from the variant, so two handlers cannot answer the same situation with different numbers.

#### Variants

- `Unauthorized` - A credential is required and was absent, malformed or wrong.
- `NotAQuestion` - The body is not a question. Carries a message naming the field.
- `TooLarge` - The body is larger than the configured bound.
- `RateLimited` - Too many requests from this address, too quickly.
- `Timeout` - The request took longer than the configured bound.
- `Internal` - Something on our side went wrong. Carries nothing.
- `Unavailable` - The data system did not answer. Distinguished from `Self::Internal` because it is the one failure that is worth retrying, and a caller cannot tell from a 500.
- `AtCapacity` - Every execution slot was taken for the whole admission window, so the question was shed.

#### Implements

`Debug`, `IntoResponse`

### `struct ProblemBody`

```rust
pub struct ProblemBody
```

The failure body.

Three fields and no more. A request identifier would belong here and there is none: nothing in
this service mints one yet, and a field that is always absent is worse than no field.

#### Methods

```rust
pub const fn code(&self) -> &'static str
```

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`

## Module `router`

Assembling the router: three tiers, and what guards each.

# The tiers

| Tier | Reachable by | Rate limit | Token |
| --- | --- | --- | --- |
| liveness | anybody who can route a packet | public | no |
| documentation | anybody, when it is served at all | public | yes, when one is configured |
| `v1` | a caller with the token, when one is configured | general | yes, when one is configured |

Liveness has no token because a probe has no credential to present, which is exactly why its
body carries nothing.

# Layer order, and why it reads backwards

`Router::layer` wraps what is already there, so **the last layer added is the outermost and runs
first**. Reading the assembly below from the bottom up gives the order a request travels in.
Getting this wrong is not a style problem: a token gate applied outside the tracing layer
produces a `401` with no span, and a body limit applied inside the JSON extractor is a limit that
never fires.

# The limiter is OUTSIDE the token gate, and that was a bug fix

It used to be inside it. `route_layer` for the gate was added last, so the gate was the
outermost layer of the versioned subtree and answered `401` **without calling `next.run`** - so a
wrong-token attempt never reached the limiter and never cost a cell. An unlimited burst of
authentication attempts against a 32-character shared secret is the one thing a rate limiter in
front of a bearer token is for.

The order below is therefore: limiter, then gate, then the handler. Both subtrees that have a
gate - the versioned API and the documentation - are assembled the same way, because the
documentation router had the same inversion.

**This is why the sweeper is started here and in the same change.** With the gate outermost, an
unauthenticated request was refused before it could create a bucket, so the only unauthenticated
path into a limiter was liveness. With the limiter outermost, every path an unauthenticated
caller can reach creates one - so fixing the order alone converts a narrow leak in
`governor`'s never-reaped keyed store into a surface-wide one. See
`middleware::spawn_reaper`.

`route_layer` rather than `layer` for the token gate, and that is load-bearing: `route_layer`
runs only for a request that matched a route in that subtree, so it applies to every real path
under the version prefix without also applying to the liveness probe merged in beside it. A
`layer` there would gate the probe as well, and a probe has no credential to present.

**The consequence, stated rather than discovered later:** a path under the version prefix that
matches no route skips the gate and falls through to the top-level `404`. So an unauthenticated
caller can learn which paths exist, though not what is behind them - and the paths are in the
published interface description anyway. Every path that resolves to a handler does hold a
credential. There is a test on each half of that.

# Why this returns a `Result`

Because a limiter that will not build must stop the process rather than quietly become no
limiter, and so must a sweeper that will not start: a keyed store nothing sweeps grows for the
life of the process. Neither can happen from a loaded configuration - `Quota` refuses the values
that would cause the first - and the alternative to an error is either a panic for something the
types already ruled out or a fail-open fallback. See `middleware::LimiterNotBuilt`.

### `enum RouterNotBuilt`

```rust
pub enum RouterNotBuilt
```

Why the router could not be assembled.

#### Variants

- `Limiter`
- `Reaper` - The housekeeping thread for the limiter's keyed state would not start.

#### Implements

`Debug`, `Display`, `Error`

### `struct Assembled`

```rust
pub struct Assembled
```

The router, and the limiter state something has to keep sweeping.

**Two values because they have two owners.** The router goes to whatever serves it; the handles
go to the sweeper. `router` wires the second half up itself, which is what makes the
production path correct by default; `assemble` hands both back for a test that wants to
observe the keyed store directly.

#### Methods

```rust
pub fn into_router(self) -> Router
```

The router, for whatever will serve it.

```rust
pub fn limiters(&self) -> &[LimiterHandle]
```

The tiers that were built, for a sweeper or for an assertion.

#### Implements

`Debug`

### `fn router`

```rust
pub fn router(state: &crate::state::ServiceState) -> Result<axum::Router, RouterNotBuilt>
```

Builds the whole router for this state, and starts the sweeper for its keyed state.

Everything the posture decides is decided here, once, from settings that were already
refused-or-accepted at startup. A handler cannot re-decide any of it, which is the point: a
request never arrives at a branch that could turn a control off.

### `fn assemble`

```rust
pub fn assemble(state: &crate::state::ServiceState) -> Result<Assembled, RouterNotBuilt>
```

The same assembly, with the limiter handles handed back instead of swept.

For a caller that wants to sweep on its own schedule, and for a test that wants to assert on the
keyed store. It starts no thread, so a test suite that assembles one router per test does not
accumulate one sweeper per test.

## Module `server`

Binding a socket, serving, and stopping without dropping an answer.

# The bounded drain

`axum` waits for every in-flight connection when it is asked to stop, which is what makes a
rolling deployment not drop answers - and which is also how one connection nothing is going to
close pins the process open past the deadline an orchestrator is running. So the drain is capped:
the deadline arms only *after* shutdown has been asked for, and when it expires the serve future
is dropped and the process goes on to exit.

Two properties fall out of that shape and both are deliberate. Before shutdown the server runs
unbounded, so a long-lived connection is not a deadline. And after it, the worst case is the
grace period rather than forever - see `Shutdown::grace_period`.

# Why the peer address is threaded through

`into_make_service_with_connect_info` is not optional: the rate limiter keys on the connection's
peer address, and without the connect info there is no address to key on - the limiter would
answer every request with "cannot extract key" and limit nothing. That is the failure mode where
a limiter appears configured and is not.

# Plaintext and TLS are one serve path

`serve` and `serve_tls` differ in the listener they build and in nothing else. Both hand it
to `run`, which is where `axum::serve`, the graceful-shutdown future and the bounded drain above
live - so the drain semantics are defined once and cannot drift between the two. That was the
deciding argument for wrapping the listener rather than taking `axum-server`, which brings its
own separately-implemented shutdown; `crate::tls` records the comparison in full.

`serve_tls` exists only under the `tls` feature. With the feature off there is no TLS listener in
the build **and** `sutura_config::Settings::refusals` will not let a process start that was asked
to terminate TLS in-process - the two are wired to the same feature name so they cannot disagree.
Nothing here falls back to plaintext: material that will not load is an error returned before a
socket is bound.

### `enum ServeFailed`

```rust
pub enum ServeFailed
```

Why the server stopped, other than being asked to.

#### Variants

- `Bind`
- `Serve`
- `Tls` - The configured certificate and key are not usable.

#### Implements

`Debug`, `Display`, `Error`

### `fn serve`

```rust
pub async fn serve(router: axum::Router, address: std::net::SocketAddr, shutdown: sutura_runtime::Shutdown) -> Result<(), ServeFailed>
```

Binds `address`, serves `router`, and returns when the shutdown has drained or the deadline
expired.

The bound address is read back from the socket rather than assumed, so a port of zero - a test
asking the kernel to choose - is reported as the port it actually got.

### `fn serve_tls`

```rust
pub async fn serve_tls(router: axum::Router, address: std::net::SocketAddr, shutdown: sutura_runtime::Shutdown, material: &sutura_config::TlsMaterial) -> Result<(), ServeFailed>
```

The same, with the connection terminated here.

The material is loaded and validated FIRST, before anything is bound. So a certificate that will
not parse, or a key that does not belong to it, is a process that does not start - not a port
that accepts connections and fails every handshake, and not a plaintext port. See `crate::tls`
for the rotation this also starts, which is what keeps a renewed certificate from needing a
restart.

## Module `state`

What every handler is handed.

Three things, each cheap to clone, so cloning the state per connection is a few pointer bumps:
the `Surface` the question goes to, the `Settings` the token gate and the assembled router
were built from, and the `Admission` bound the query handler takes a slot from.

The settings are kept rather than read once at assembly time because the token gate needs them
per request. Nothing else does - the layers were all decided at startup - and that is
deliberate: a value a handler can read is a value a handler can branch on, and the posture
decisions in this service are supposed to be settled before the first request arrives.

# The admission bound is built here, from the settings this state was given

And that is the whole of why nothing else had to change to install it. `Admission` is a bound,
so it has to be *one* value shared by every request - a per-request copy would read like a limit
and bound nothing - and the only place that can be true without a second constructor argument is
beside the settings it is derived from. `ServiceState::new` therefore takes exactly what it
took before, and every caller of it, production and test alike, gets the configured bound rather
than having to remember to pass one.

`Clone` on this type shares that bound rather than duplicating it, because the field is an
`Admission` whose own `Clone` shares one permit set. That is the property the whole control
rests on, and it is the one a future field here must not break.

### `struct ServiceState`

```rust
pub struct ServiceState
```

The request state.

#### Methods

```rust
pub const fn admission(&self) -> &Admission
```

The bound on how many questions execute at once.

Borrowed rather than cloned, so a handler takes a slot from *this* bound. A clone would be
correct too - `Admission` shares its permit set - and a borrow says so at the call site.

```rust
pub fn definitions(&self) -> &sutura_domain::pinned::PinnedDefinitions
```

The service, borrowed, for a handler that only reads the pinned bundle.

```rust
pub fn new(surface: Arc<dyn Surface>, settings: Arc<Settings>) -> Self
```

Builds the state from a started service and the settings it was started under.

Takes the surface already behind an `Arc`, because the composition root owns it: the same
service may be handed to a second transport later, and this crate must not be the one that
decides there is only ever one.

The admission bound is derived from the settings rather than passed in beside them. See the
module documentation: a bound that a caller supplies is a bound a caller can forget.

```rust
pub fn settings(&self) -> &Settings
```

```rust
pub fn surface(&self) -> Arc<dyn Surface>
```

The service, for a handler that is about to move the call onto the blocking pool.

#### Implements

`Clone`, `Debug`

## Module `surface`

The application-facing surface, re-exported. **The port itself is not declared here any more.**

`Surface`, its failure types and its one implementor `LocalService` live in
`sutura_app::surface`. They used to live in this file, and a review found the problem: this is a
transport adapter, the module comment said a future MCP transport would consume the same trait,
and a trait declared here is a trait that other transport would have to reach through an HTTP
crate. Nothing in this repository may depend on an adapter. The argument for where it went, and
for why deleting the one-implementation trait was the weaker of the two options, is in
`sutura_app::surface`'s own module documentation.

What is left is a re-export, so the paths this crate already uses - `crate::surface::Surface` in
the request state, `crate::surface::SurfaceFailure` in the query route - keep resolving. **The
canonical path is `sutura_app::surface`**, and a second transport imports it from there without
naming this crate at all.

The tests below stayed with the fakes rather than with the code: `crate::testing` holds the two
port doubles they need, and they exercise the re-exported types, so what they assert about the
erasure is unchanged.

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

## Module `tls`

In-process TLS termination: the listener, and the certificate rotation that keeps it up.

Compiled only under the `tls` feature. See the manifest for why that feature is default-off -
briefly, because TLS is normally terminated by an ingress controller or a sidecar, so plaintext
on the pod network is the intended arrangement and the shipped artifacts should not carry a TLS
stack they do not use. This module is for the deployment where nothing terminates in front.

# rustls, and `tokio-rustls` rather than `axum-server`

rustls and not OpenSSL is decided by the shipped artifacts rather than by preference: two of the
four are musl, nixpkgs has no musl OpenSSL, and an `openssl-sys` edge would either fail the cross
build or need a vendored C OpenSSL per target. rustls needs no system library.

Which rustls wrapper was a real choice, and it went to `tokio-rustls` on two counts.

**Dependency count.** `tokio-rustls` is the *only* new crate in the graph: `rustls`,
`rustls-pki-types`, `rustls-webpki`, `ring` and `untrusted` are already resolved, because
`libduckdb-sys` carries `ureq` and `ureq` carries a TLS stack. `axum-server` would have added
itself, `hyper-util`, `rustls-pemfile` and `arc-swap` on top of the same rustls.

**Graceful shutdown.** This is the heavier reason. `crate::server` has a *bounded* drain - the
deadline arms only after shutdown is asked for, and the serve future is dropped when it expires -
and it is built on `axum::serve(..).with_graceful_shutdown(..)`. `axum-server` does not compose
with that; it replaces it, with its own `Handle::graceful_shutdown(Some(duration))`. Taking it
would have given the TLS path a second, separately-implemented drain, so a change to the shutdown
semantics would have to be made twice and could be made once. Wrapping the listener instead
leaves `axum::serve`, `drain` and `report` exactly as they are: **one serve path and one drain,
for plaintext and for TLS.**

What `axum-server` would have brought for free is its `RustlsConfig::reload_from_pem_file`. That
is replaced here by something narrower - see the rotation section - and the narrower version
refuses a pair that does not match, which upstream's does not.

Two more decisions live in `listener` rather than here, because they are about accepting a
connection rather than about what certificate is presented on it: the handshake is done off the
accept path, and the peer address is carried through with `tap_io` so the rate limiter keys the
same way over TLS as it does over plaintext.

# Rotation without dropping a connection

A certificate expires and something replaces the files - `cert-manager` writing a Secret, or an
operator with a new pair. A restart is not zero downtime, so the listener has to pick the new
pair up in place.

The mechanism is a `rustls::server::ResolvesServerCert` over a `tokio::sync::watch` channel.
`ServerConfig` is built **once** and never rebuilt; what changes is the `Arc<CertifiedKey>` the
resolver hands back, and it is read once per handshake. So a handshake in flight keeps the pair it
resolved and the next one gets the new pair - there is no window in which a connection is
serving half of each, and no connection is dropped.

`tokio::sync::watch` and not `arc-swap`, which is the crate usually reached for here: `borrow()`
is synchronous, which is what the resolver needs, and `tokio` is already a dependency. A crate
for one atomic pointer swap is a crate. `std::sync::RwLock` is banned in `clippy.toml` and would
have been the wrong reach anyway.

**A bad new pair does not take the listener down.** This is the half that matters more than the
rotation itself, because reloading into a broken state is worse than not reloading: every new
connection would fail and the old, working pair would be gone. So a candidate is fully built
before anything is swapped - parsed, the key loaded by the provider, and the key checked against
the certificate's `SubjectPublicKeyInfo` - and a candidate that fails any of that is logged at
`error` and discarded. The listener keeps serving what it was serving.

# Polling, not `inotify`

`Renewal::poll_once` compares the file *bytes* to the ones in use. That is deliberately not a
filesystem watch, and not for want of a crate:

* Kubernetes replaces a projected Secret by building a new timestamped directory and swapping a
  symlink. An `inotify` watch registered on the *file* path follows the old inode and never fires;
  getting it right means watching the directory and interpreting rename events. Polling the path
  sees the new content on the next tick, with no cases.
* Comparing content rather than `mtime` costs a read of two small files on a slow interval and
  answers the question directly. An `mtime` that a writer preserved is a rotation that never
  happened.
* It adds no dependency.

The cost is bounded staleness: up to `RENEWAL_INTERVAL` between the write and the swap. For a
certificate rotation, which is planned days ahead by whatever issues them, that is nothing.

### `enum TlsNotUsable`

```rust
pub enum TlsNotUsable
```

Why the configured certificate and key are not usable TLS material.

Every variant names the path. `sutura_config::TlsMaterial` parses the *pair* - both halves or
neither - and stops there, because that crate reads no files; everything below is a question only
a TLS implementation can answer, and this is where they are all answered. Once, before the socket
is bound.

#### Variants

- `Unreadable` - The file could not be read at all: absent, or not readable by this process.
- `NoCertificate` - The file was read and held no PEM certificate.
- `NoKey` - The file was read and held no PEM private key.
- `Malformed` - A PEM block was found and did not parse.
- `KeyDoesNotMatch` - The key does not belong to the certificate.
- `NotConfigurable` - The server configuration itself would not build.

#### Implements

`Debug`, `Display`, `Error`

### `struct Termination`

```rust
pub struct Termination
```

A validated TLS configuration, and the means to keep it current.

**Two values because they have two owners**, the way `crate::router::Assembled` is two: the
configuration goes to the listener, and the renewal goes to whatever will poll it. `prepare`
hands both back rather than starting the poll itself, so a test can drive a rotation a step at a
time instead of waiting on a wall clock.

#### Methods

```rust
pub fn config(&self) -> Arc<ServerConfig>
```

The configuration a listener is built from.

```rust
pub fn into_parts(self) -> (Arc<ServerConfig>, Renewal)
```

The two halves, for a caller that owns them separately.

```rust
pub fn prepare(material: &TlsMaterial) -> Result<Self, TlsNotUsable>
```

Reads the pair, validates it, and builds the configuration around it.

Called before the socket is bound, so a configuration mistake is a process that does not
start rather than a port that accepts connections and fails every handshake.

#### Implements

`Debug`

### `enum Renewed`

```rust
pub enum Renewed
```

What one look at the files decided.

#### Variants

- `Unchanged` - The files are byte for byte what is already being served.
- `Rotated` - The files changed, the new pair validated, and it is what new handshakes will use.
- `Rejected` - The files changed and the new pair is not usable. What was being served still is.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct Renewal`

```rust
pub struct Renewal
```

Keeps the presented certificate current with what is on disk.

Owns the sending half of the channel the resolver reads, so this is the only thing that can
change what a handshake is offered.

#### Methods

```rust
pub fn poll_once(&mut self) -> Renewed
```

Looks at the files once.

Public so a test can rotate a certificate without waiting for an interval to elapse, which
is the difference between asserting that rotation works and asserting that a timer fires.

```rust
pub fn watch_until_shutdown(self, interval: Duration, shutdown: Shutdown)
```

Polls on an interval until shutdown is asked for.

A `tokio` task rather than the OS thread `crate::middleware::spawn_reaper` uses, and the
difference is where it is started from: the reaper is spawned while the router is assembled,
before a runtime exists, and this is spawned from inside `serve`.

#### Implements

`Debug`

### `use None`

### `constant RENEWAL_INTERVAL`

How often the certificate and key files are re-read.

A constant rather than a configuration key, for the reason `crate::middleware::REAP_INTERVAL` is
one: nothing a caller can do changes what the right answer is. Thirty seconds is far below any
horizon at which a certificate rotation is urgent - an issuer plans one days ahead - and far
above the cost of reading two small files.

### Module `listener`

Accepting a TLS connection, and why the handshake is not on the accept path.

# The handshake is not on the accept path

`axum::serve::Listener::accept` is the one place a TLS wrapper obviously goes, and putting the
handshake there is a denial of service: `accept` is called in a loop by one task, so a client
that opens a connection and never sends a `ClientHello` stalls **every** subsequent accept for as
long as it likes. One socket would be the outage.

So the shape here is a task that owns the `TcpListener`, spawns each handshake, and sends the
ones that complete down a bounded channel. `TlsListener::accept` pops from that channel and
does no work. Handshakes are therefore concurrent, capped by a semaphore so a flood cannot spawn
without bound, and each one has its own deadline.

A connection still mid-handshake when shutdown arrives is dropped rather than drained. That is
correct rather than a compromise: it carries no request yet, so there is no answer to lose.

# `ConnectInfo`, and the reason for `tap_io`

The limiter keys on the peer address, which reaches a handler as `ConnectInfo<SocketAddr>` - and
`crate::client_address` is where the consequence of losing it is written down: the limiter reports
that it cannot extract a key and bounds nothing.

`axum` gives `SocketAddr` a `Connected` implementation for `TcpListener` specifically, and a
blanket one for `TapIo<L, F>` where `L::Addr` is the address type. It does *not* have one for an
arbitrary listener, and this crate cannot add it - `Connected` and `SocketAddr` are both foreign,
so the orphan rule forbids it. Wrapping in `tap_io` is therefore not decoration: it is what makes
the TLS path produce the same `ConnectInfo<SocketAddr>` the plaintext path produces, so the
limiter keys identically on both. The alternative was a local `Peer(SocketAddr)` newtype, which
would have compiled and left the limiter keying on nothing over TLS.

#### `struct TlsListener`

```rust
pub struct TlsListener
```

A TLS listener `axum::serve` can drive.

Holds no socket. The socket is owned by the task `TlsListener::wrap` spawns, and this is the
receiving end of the connections that task has finished handshaking - see the module
documentation for why the handshake is not done here.

##### Methods

```rust
pub fn wrap(tcp: TcpListener, config: Arc<ServerConfig>) -> std::io::Result<Self>
```

Takes over a bound socket and starts handshaking on it.

The `TcpListener` is moved into the accept task, which is what makes it impossible to accept
a plaintext connection on this socket by accident: after this call there is no other handle
to it.

##### Implements

`Debug`, `Listener`

## Module `wire`

The shapes on the wire, and the conversions to and from the domain.

# Why these exist rather than serializing the domain types

Two reasons, and the first is structural. `sutura-domain` may depend on `serde` and `thiserror`
and nothing else - a machine-checked rule - so it cannot carry a `utoipa::ToSchema` derive, and
a document generated from the handlers needs one for every body. The wire shapes live here, in
the adapter, which is where a wire format belongs anyway.

The second is that a wire type is allowed to be *worse* than a domain type, and should be. Every
field of `QuestionBody` is a plain string, because that is what arrives; each one is then
parsed into the newtype that establishes its invariant, and a failure names the field. A body
deserialized straight into `Query` would give a caller `invalid type: string` and no field.

# `deny_unknown_fields`, and what it is for here

The same thing it is for on a catalog document, arrived at from the other side. Without it a
body carrying `sql:` or `table:` deserializes cleanly with the extra key dropped on the floor,
and a caller who believes they sent SQL gets an answer to a different question. With it, the
attempt is a 400 naming the field. The tool surface has no field for any of that - see
`sutura_domain::query` - and this is what keeps that true across a JSON parser.

### `struct QuestionBody`

```rust
pub struct QuestionBody
```

A question, as it arrives.

#### Implements

`ComposeSchema`, `Debug`, `Deserialize<'de>`, `ToSchema`

### `struct RangeBody`

```rust
pub struct RangeBody
```

A half-open period: `start` is included, `end` is not.

Half-open at every grain and in every dialect, which is what makes a month
`[2026-06-01, 2026-07-01)` rather than a last day that differs per month.

#### Implements

`ComposeSchema`, `Debug`, `Deserialize<'de>`, `ToSchema`

### `struct FilterBody`

```rust
pub struct FilterBody
```

One equality filter.

#### Implements

`ComposeSchema`, `Debug`, `Deserialize<'de>`, `ToSchema`

### `enum MalformedQuestion`

```rust
pub enum MalformedQuestion
```

Why a body is not a question.

Every variant names the field, and none of them echoes the caller's value back except where the
value is the thing that failed to parse as an identifier - which is a bounded character set, not
free text.

#### Variants

- `Metric`
- `Grain`
- `Date`
- `Range`
- `Dimension`
- `FilterDimension`

#### Implements

`Debug`, `Display`, `Error`

### `enum OutcomeBody`

```rust
pub enum OutcomeBody
```

What a question produced.

**Both variants come back with `200`, and that is the contract rather than an oversight.** A
refusal is a *result*: the caller asked something they may not have, and the answer is no. An
error status would invite a client library to retry it, and retrying a governance decision until
it succeeds is precisely the behaviour the refusal exists to prevent. The `outcome` field is
what a caller branches on.

#### Variants

- `Answer` - The question was answered.
- `Refusal` - The question was refused.

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`

### `struct ProvenanceBody`

```rust
pub struct ProvenanceBody
```

Which definitions produced this answer.

Always present on an answer, and it is the reason an answer can be trusted at all: the version
names the snapshot and the digest is over its canonical form, so the same question against a
different bundle is visibly a different answer.

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`

### `struct RefusalBody`

```rust
pub struct RefusalBody
```

Why a question was refused.

A stable `code` per refusal, plus a sentence. The code is what a caller branches on; the
sentence is for a person.

**Nothing here echoes a value the caller sent.** The domain's refusal variants already stop
short of that - a rejected filter value names the dimension and not the value, on purpose,
because reflecting caller text into a message that reaches a log, a UI and an agent's context is
how a rejected value becomes somebody else's input. The identifiers that *are* echoed are parsed
newtypes over a bounded character set, and the numbers are derived from parsed dates.

#### Methods

```rust
pub const fn code(&self) -> &'static str
```

The code, for a test that asserts on the contract rather than on the prose.

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`

### `struct CatalogBody`

```rust
pub struct CatalogBody
```

What this catalog defines.

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`

### `struct MetricBody`

```rust
pub struct MetricBody
```

One metric, as much of it as a caller needs to ask a valid question.

Descriptive content only. Nothing here selects, widens or parameterizes what executes - the
catalog port takes no request context and cannot - so this is a reader's view of a pinned
bundle rather than an input to one.

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`

### `struct DimensionBody`

```rust
pub struct DimensionBody
```

One dimension of one metric.

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`
