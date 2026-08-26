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

**The key is the peer address, not a forwarded header, and that is a security decision.**
`tower_governor` also offers an extractor that reads `X-Forwarded-For` and `X-Real-IP`, which is
what a service behind a trusted reverse proxy wants - and which any caller can set on a service
that is *not* behind one, making every bucket a caller's to choose. Since nothing here can know
whether a trusted proxy is in front, the unspoofable key is the correct default. The consequence
is stated rather than hidden: behind a proxy, every request appears to come from the proxy and
shares one bucket.

Rate limiting is not authentication. It bounds how fast something can be done, not who may do
it.

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
pub fn probe_rate_limit_layer(quota: sutura_config::Quota) -> Result<RateLimit, LimiterNotBuilt>
```

The tier for what an unauthenticated caller can reach.

### `fn api_rate_limit_layer`

```rust
pub fn api_rate_limit_layer(quota: sutura_config::Quota) -> Result<RateLimit, LimiterNotBuilt>
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

### `type_alias RateLimit`

A limiter layer, keyed by peer address, reporting its state in response headers.

A named alias because the inline form is over the complexity threshold in `clippy.toml`. The
`StateInformationMiddleware` in it is not incidental: it is what makes the layer emit the
remaining-quota headers, and it is part of the type because that choice is made at construction.

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
limiter. It cannot happen from a loaded configuration - `Quota` refuses the values that would
cause it - and the alternative to an error is either a panic for something the types already
ruled out or a fail-open fallback. See `middleware::LimiterNotBuilt`.

### `enum RouterNotBuilt`

```rust
pub enum RouterNotBuilt
```

Why the router could not be assembled.

#### Variants

- `Limiter`

#### Implements

`Debug`, `Display`, `Error`

### `fn router`

```rust
pub fn router(state: &crate::state::ServiceState) -> Result<axum::Router, RouterNotBuilt>
```

Builds the whole router for this state.

Everything the posture decides is decided here, once, from settings that were already
refused-or-accepted at startup. A handler cannot re-decide any of it, which is the point: a
request never arrives at a branch that could turn a control off.

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

### `enum ServeFailed`

```rust
pub enum ServeFailed
```

Why the server stopped, other than being asked to.

#### Variants

- `Bind`
- `Serve`

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

## Module `state`

What every handler is handed.

Two things behind an `Arc` each, so cloning the state per connection is two pointer bumps: the
`Surface` the question goes to, and the `Settings` the token gate and the assembled router
were built from.

The settings are kept rather than read once at assembly time because the token gate needs them
per request. Nothing else does - the layers were all decided at startup - and that is
deliberate: a value a handler can read is a value a handler can branch on, and the posture
decisions in this service are supposed to be settled before the first request arrives.

### `struct ServiceState`

```rust
pub struct ServiceState
```

The request state.

#### Methods

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

What a transport needs from the service, with the ports' generic parameters erased.

# Why this trait exists at all

`sutura_app::answer` is generic over a `Warehouse`, and `Warehouse` has an associated error
type. An `axum` handler is a concrete function registered in a route table, and
`utoipa_axum::routes!` names it by path - so a handler cannot be generic over the warehouse
without the whole router becoming generic in it, and the generated document becoming generic in
it too.

`Surface` is the seam. It is a *driving* port - the direction a request arrives from, rather
than a dependency the domain inverts - and, like every port in this repository, it arrives with
its implementor: `LocalService` is in this file, is the only one, and does nothing but call
through to `sutura_app`. Nothing here re-implements a rule that lives inside the hexagon, and a
second transport - an MCP one, say - consumes this same trait rather than growing its own copy
of the wiring.

# Why the methods are synchronous

Because `Warehouse` is. The port takes `&self` and returns a `Result`, and the engine behind
it drives its own single-threaded runtime and blocks on it. Calling that from inside an `async`
handler on a worker thread would panic - a runtime cannot be entered from within a runtime - so
the handler moves the call onto the blocking pool. Making this trait `async` would hide that
requirement behind a signature that looks like it had been dealt with.

# Where the typed error goes

Erasing the generic means the adapter's own error type cannot survive as a type. It is walked to
text here - message plus the whole `#[source]` chain - which is the same trade `sutura-app`
makes at the same kind of boundary and for the same reason: the alternative is `Display` on the
outermost error, which prints one sentence and discards the part naming the table, the column or
the file.

### `trait Surface`

```rust
pub trait Surface
```

The service, as a transport sees it.

`Send + Sync + 'static` because it is shared between connections and moved onto the blocking
pool. Held behind an `Arc` in the request state.

### `enum SurfaceFailure`

```rust
pub enum SurfaceFailure
```

Something went wrong that is not a refusal.

Neither variant is something a caller can fix by asking differently, which is why neither is a
refusal: one is our own bundle or generator being wrong, and the other is the data system not
answering.

#### Variants

- `Compile`
- `Warehouse`

#### Methods

```rust
pub fn chain(&self) -> &[String]
```

The `#[source]` chain, outermost first, for the log.

#### Implements

`Debug`, `Display`, `Error`

### `enum ServiceNotStarted`

```rust
pub enum ServiceNotStarted
```

Why a service could not be started.

#### Variants

- `Catalog` - The catalog adapter could not produce a bundle.
- `NotValidated` - The bundle loaded and an anchor did not reproduce the number its author certified, or could not be run at all.

#### Implements

`Debug`, `Display`, `Error`

### `struct LocalService`

```rust
pub struct LocalService<W>
```

The one implementation: a validated bundle and one data system, behind the ports.

Holds the bundle as `Validated`, which has no constructor other than one that executes every
anchor against a warehouse - so a `LocalService` that exists is one whose anchors held. That
is not a check this type performs; it is a type it could not otherwise have been built from.

#### Methods

```rust
pub fn start<C>(catalog: &C, warehouse: W) -> Result<Self, ServiceNotStarted>
```

Loads a catalog through its port, re-runs every anchor against `warehouse`, and returns a
service only if all of them held.

Both ports are consumed here and nowhere else in this crate: the transport never reads a
catalog directory and never opens a data system, which is what keeps it transport-only.

#### Implements

`Debug`, `Surface`

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
