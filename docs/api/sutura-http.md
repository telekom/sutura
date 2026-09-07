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

**A refusal says so three ways.** `POST /v1/query` answers a question the caller may not have
with an explicit status - `403`, `404`, `409`, `413`, `422` or `503` depending on why - plus the
stable `code` and the sentence it has always carried in `outcome: refusal`. It was a `200`, on the
argument that an error status invites a client library to retry; the retry premise does not
survive checking, and a `200` made a governance refusal indistinguishable from an answer to
anything reading a status alone. `wire::refusal` holds the mapping, the citations and the reason
for each status. The domain invariant is untouched: `ToolOutcome::Refusal` is still a result and
not an `Err`.

**A caller's identity is established, and it is still not per-caller access.** Where a deployment
declares `security.inbound`, `inbound` verifies the caller's own token - signature against a
pinned asymmetric algorithm, issuer, expiry, and an audience matching this deployment's own
resource identifier - and the request runs under a
`sutura_domain::identity::Subject::Verified`. Where it declares none, the answer is
`sutura_domain::identity::Subject::TheDeploymentItself` and an access token, if one is
configured, proves only that the caller holds a secret an operator distributed.

**What neither shape does is make a data system execute as the asking subject.** That is leg 2, and
the half of it that is built is the credential port: a question cannot execute without a credential
minted for the source it reads, and a subject with no credential there is refused rather than
answered as this process. What no adapter in this build can do is CARRY a per-subject credential, so
every question is still answered with whatever access this process already had. The startup log
prints that limit on every boot, and `inbound` lists the four things `docs/adr/0014` describes
and this does not build.

`crate::principal` is the one place a `sutura_domain::identity::RequestContext` is constructed,
and there are exactly two ways in: one takes no argument, and the other takes a
`VerifiedCaller`, whose only constructor is a signature check and which implements no
`Deserialize`. So no *field* of a request can contribute to a chain either way.

**`/health` carries nothing.** It is the one path an unauthenticated caller can always reach, so
every field it might have is a field handed to anybody who can route a packet. No version, no
build, no configuration, no catalog. A test asserts the body byte for byte.

# What is deliberately absent

* **No CORS layer.** A browser is not a client of this surface. An allow-list nobody needs is an
  allow-list somebody widens.
* **No request identifier on the wire.** One is minted now - `correlation::CorrelationId`, on
  the request span, so every line of one request carries it - and it is deliberately **not** in
  the failure body. Putting it there is a change to the response contract and to the generated
  document, and it buys nothing until somebody is asked to quote it; the honest state is that an
  operator can find a request in the log and a caller cannot yet name one. If a caller ever
  needs to, that is an additive field and this bullet is where it changes.
* **No audit sink *here*, and no store anywhere.** There is a sink now - the port is
  `sutura_domain::audit::AuditSink` and `sutura_app::LocalService` writes one record per outcome
  through it, before the outcome returns - but this crate neither implements it nor chooses it.
  The composition root attaches one; the writer a deployment gets for free is
  `sutura_runtime::TracingAuditSink`, which puts the record on the log this crate already emits
  into. **Nothing retains a record**: sutura writes and keeps nothing, so what happens after the
  write belongs to the deployment's log pipeline, including the case where that is nothing. The
  per-outcome `tracing::info!` this handler used to write was replaced by that record rather than
  joined by it - see `routes::v1::query`.
* **No readiness route.** The module documentation on the liveness route says why: there is
  nothing it could report that is not
  already true of a process that is listening.

# Assembling it

```no_run
use std::sync::Arc;

use sutura_config::{Environment, Settings, Sources};
use sutura_http::{ServiceState, router, serve};
use sutura_runtime::{Admission, Shutdown};

# async fn wire(surface: Arc<dyn sutura_http::Surface>) -> Result<(), Box<dyn core::error::Error>> {
let settings = Settings::load(&Sources::defaults(Environment::Development))?;
let address = settings.server().bind().socket();
// The execution bound, built HERE and handed down - one per process, like the shutdown below.
// This crate cannot build one: see `state` for what `telekom/sutura#340` cost.
let admission = Admission::from_settings(settings.runtime());
let state = ServiceState::new(surface, Arc::new(settings), admission);
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

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## Module `capability`

Which capability each route is, and the layer that refuses one this caller was not granted.

# The mapping is a table, and it is checked at ASSEMBLY rather than per request

`sutura_app::Capability` is the tool set both transports render - see that module for why it is
not owned by either of them. This file is the HTTP side of the rendering: one row per route,
naming the capability it is.

**A route missing from that table is a route this crate refuses to serve.**
`crate::router::RouterNotBuilt::RouteNotGoverned` is returned by `assemble`, which reads the
generated interface description - so the check is over the routes the router actually mounts
rather than over a second list somebody kept in step. That is the same shape as
`InboundIdentityNotAttached`: the failure is a process that does not start, not a request that
slips through.

The alternative was a check inside each handler, and it was rejected for one reason: a handler can
forget. A layer over the whole subtree cannot, and the only place left to forget is a row in
`governed` - which is what the assembly refusal covers.

# What this gates, and what it does not

**It decides which OPERATIONS a caller may invoke. It decides nothing about which rows an answer
contains.** Both routes read the same pinned bundle and every question executes under the same
identity, because no source executes as the asking subject - `docs/adr/0014`'s leg 1 establishes
who is asking and leg 2 does not exist. A caller granted `sutura:metrics.ask` and not
`sutura:catalog.read` cannot list the catalog and gets exactly the same numbers from a question as
anybody else would.

# Where the grant comes from, and the one honest hole in it

`permitted_for` is the whole derivation, and it is two cases:

* A `crate::inbound::VerifiedCaller` in the request extensions - which only
  `crate::inbound::gate::require_verified_caller` inserts, after a signature check - means the
  token's scopes decide, and **only** they do.
* No such extension means no caller identity was established, so there is no verified claim to
  narrow by and every capability is permitted. That is the single-player deployment this service
  ships as, and it is the correct answer rather than a fallback: a filter over an unverified claim
  looks like a control and is not one.

**The hole a reader should check for, and why it is closed:** if the second case could be reached
by a deployment that *meant* to establish an identity, this layer would be a control that silently
turned itself off. It cannot be. `crate::router::assemble` returns
`RouterNotBuilt::InboundIdentityNotAttached` when the settings declare a mode and no gate was
attached, and the gate either refuses the request with a `401` or inserts the extension. So on a
deployment that declares `security.inbound`, a request reaching a handler has been through the
gate. `crate::inbound::tests::router` already asserts the assembly half and the `401`.

# It fails closed, and the refusal is what makes that survivable

A verified caller whose token names no capability scope may do nothing - `sutura_app::Permitted`
carries that decision and its consequence. What keeps a deployment that forgot to author scopes
from being a mystery is the response: `403` with `code: insufficient_scope` and a sentence naming
the exact scope string, which is RFC 6750's own answer to this and is diagnosable without a log.

### `struct GovernedRoute`

```rust
pub struct GovernedRoute
```

One row of the table: a method, the route template as `axum` matched it, and what it is.

**The route template and not the request's path**, which matters: `MatchedPath` is a value from
this process's own routing table, so nothing a caller sends can steer the lookup. The same reason
`crate::router::request_span` reads it rather than `uri().path()`.

A named type rather than a tuple because `crate::router` reads it too, and a three-tuple of
`(Method, String, Capability)` at two call sites is where an argument order gets swapped. Private
fields with accessors, which is the rule for a `pub struct` in a library crate here.

#### Methods

```rust
pub const fn capability(&self) -> Capability
```

What invoking this route is.

```rust
pub fn route(&self) -> &str
```

The route template, as it appears in the generated document and in `MatchedPath`.

#### Implements

`Clone`, `Debug`

### `fn governed`

```rust
pub fn governed() -> [GovernedRoute; 2]
```

Every route this crate governs.

Built as a function rather than a `const` because the paths are composed from `API_V1_PREFIX`
and `base_paths`, and composing them here is what keeps one owner for a path.

`pub` because `crate::router::assemble` reads it to refuse an ungoverned route, and because a test
in `crate::openapi` compares it against the generated document's operation identifiers.

### `fn capability_of`

```rust
pub fn capability_of(method: &axum::http::Method, route: &str) -> Option<sutura_app::Capability>
```

The capability a route is, or `None` if this crate does not govern it.

`None` is what `crate::router::assemble` refuses over. At request time it cannot happen - the
layer is installed on the versioned subtree only, and assembly proved every route in it has a row
- and the layer still refuses rather than passing, because "cannot happen" is not a control.

### `fn permitted_for`

```rust
pub fn permitted_for(request: &axum::extract::Request) -> sutura_app::Permitted
```

What this request's caller may do.

See the module documentation for the two cases and for why the second is not a fallback.

### `fn require_capability`

```rust
pub async fn require_capability(request: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response
```

Refuses a request for a capability this caller was not granted.

A layer over the versioned subtree rather than a check in each handler, so there is nothing for a
handler to forget. Installed INSIDE `crate::inbound::gate::require_verified_caller`, which is what
makes the extension available here - see `crate::router` for the whole order.

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

## Module `correlation`

One identifier per request, so a request's lines can be found together.

# What this is, and the limits of it

A counter seeded at process start, rendered as hex. That is enough for the one job it has:
grouping the lines **one process** wrote for **one request**. It is deliberately not more.

* **It is not globally unique.** Two replicas started at the same nanosecond can mint the same
  value, and nothing here coordinates. A collector query wants this *and* the pod, not this
  alone.
* **It is not a distributed trace id.** There is no propagation to a downstream call, no
  sampling decision, no parent-child relationship and no exporter - `sutura-runtime`'s crate
  documentation records that exporting a trace is a decision about a backend, a sampling rate
  and an egress path, and that none of those has been made.
* **It is not an authentication or ordering signal.** The value is monotonic within a process,
  which makes two lines comparable and makes nothing else true.

No `uuid` dependency, on purpose: a random 128-bit value would buy global uniqueness this
surface has no use for, and it is not worth a dependency in a workspace that has none.

# The inbound header is untrusted text on its way to a log

Reading a caller's identifier is what makes one line in an ingress log and one line here the
same request, so it is worth doing. It is also **log injection** if it is taken as given: a
newline in the value is a log line an attacker writes. `sutura_config::InvalidLogFilter`'s
`ControlCharacter` variant exists for exactly that reason about exactly that class of value, and
this is the same treatment - bound the length, then accept a strict character set and nothing
else.

**A malformed header does not fail the request.** `CorrelationId::from_headers` mints a fresh
value instead, because refusing would let a caller turn a header they control into a `4xx` on a
question that was otherwise fine - and the thing being protected is the *log*, which a fresh id
protects just as well. The refusal is reported at `debug`, by reason and never by value.

### `struct CorrelationId`

```rust
pub struct CorrelationId
```

An identifier for one request, within one process's log.

If a value of this type exists it is at most `MAX_LENGTH` characters of ASCII letters, digits,
`-` and `_`, and is not empty - so it can be written into a log line without escaping and cannot
forge one.
`Ord` is deliberately not derived. Generated values sort by mint order because they are
fixed-width hex; a caller-supplied one does not, so an ordering over the type as a whole would
mean something on half the values and nothing on the other half.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn fresh() -> Self
```

A value nothing else in this process will produce.

Infallible, and it has to be: it is the fallback for every way of getting one, including a
caller's header being refused.

Sixteen hex digits, which is inside what `Self::parse` accepts - asserted by a test rather
than argued, because the two drifting apart would mean this process mints ids it would
itself decline to read back.

```rust
pub fn from_headers(headers: &HeaderMap) -> Self
```

The caller's identifier if it sent a usable one, and a fresh one otherwise.

Never fails. See the module documentation: a header a caller controls must not be able to
turn an otherwise answerable question into a refusal, and a fresh id protects the log
exactly as well as a rejection would.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, NotACorrelationId>
```

Reads a caller-supplied identifier.

Sanitize then validate, in that order and both inside the constructor: the value is trimmed
first so a derived `PartialEq` and `Hash` agree about which id this is without any call site
having to normalise. The length is checked against the *trimmed* value for the same reason.

**Length before character set, deliberately, and this is the ordering the classic advice
asks for**: the expensive check is per-character and the cheap one bounds how many
characters there are.

#### Implements

`AsRef<str>`, `Clone`, `Debug`, `Display`, `Eq`, `Hash`, `PartialEq`

### `enum NotACorrelationId`

```rust
pub enum NotACorrelationId
```

Why a string is not a correlation identifier.

**No variant carries the offending text**, and that is the point of the type rather than an
omission: the value came from a caller, the error's `Display` reaches a log, and echoing it
there is the injection this is guarding against. A position is enough to debug a client with,
and a position cannot forge a line.

#### Variants

- `Empty` - Empty, or only whitespace.
- `TooLong` - Longer than `MAX_LENGTH`.
- `UnacceptableCharacter` - A character outside the accepted set - in practice a newline, a space or a quote.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `constant HEADER`

The header a caller may set to name the request in its own logs too.

### `constant MAX_LENGTH`

The longest value accepted from a caller.

Generous rather than tight, and sized off what a caller plausibly already has: a UUID is 36
characters and a W3C `traceparent` is 55. Anything longer is not an identifier somebody is
correlating with, and an unbounded one is a log line of a size a caller chooses.

## Module `inbound`

Leg 1 of the identity path: how a caller proves who it is, on this transport.

`docs/adr/0014` is the record. It decides two inbound modes with no default, and it puts every
piece of this in the transport on purpose: *"All of it is transport: it parses a wire shape and
produces a domain value, and it decides nothing about what a question may ask."* That is what this
module is - a header becomes a `VerifiedCaller`, which becomes a
`sutura_domain::identity::RequestContext` through `crate::principal::of_verified`, and nothing
here can widen, narrow or parameterize what executes.

| File | What it owns |
| --- | --- |
| `keys` | the key set, its cache, the **rate-limited** refetch on an unknown key id, and the **age bound** that is what makes revocation bounded |
| `token` | algorithm pinning, the audience check, and claims into a principal chain |
| `caller` | `VerifiedCaller` and `Scopes` - the conclusion of a verification, as a type nothing can deserialize |
| `gate` | the layer, and the `401` with its challenge |

`sutura_config::inbound` owns the *declaration* - which mode, which issuer, which audience, which
algorithms - and names no JWT library at all. The two vocabularies meet in exactly one function,
`token::map_algorithm`, which is an exhaustive match.

# What this delivers, and what it must not be read as delivering

**Delivered:** a caller's identity is established from a signed, audience-bound, unexpired token,
and `sutura_domain::identity::Subject::Verified` finally has a constructor with something real
behind it. Every audit record written for such a call names the person rather than the deployment.

**Not delivered, and `docs/adr/0014` says so in the same words:** leg 1 proves who is asking. It
does *not* make a data source execute as that person - that is leg 2, and it needs a credential per
leg plus a source that declares it can impersonate. A deployment with leg 1 and no leg 2 knows who
is asking and still reads every row as one identity. The startup log prints that sentence on every
boot, out of `sutura_config::InboundIdentity::what_it_does_not_do`, rather than leaving a reader to
infer it.

# The five things this does not build, and each is named rather than left to be discovered

An overstated claim is itself the defect, so each of these is written down here rather than found:

1. **A JWKS endpoint.** Keys are read from a file. The cache, the unknown-key refetch and the rate
   limit on it are built and are what a URL source would need anyway - see `keys` for the whole
   argument and for the one property a file cannot have.
2. **The two metadata documents.** A directly validating deployment is supposed to serve
   protected-resource metadata a client can read to learn which authorization server governs it.
   There is no such route. The `401` carries an RFC 6750 challenge naming the realm and no
   `resource_metadata` parameter, so a client is configured with its issuer out of band.
3. **Anything about client registration or client authentication.** Those are decisions for the
   authorization server and for the client; this deployment is a resource server and validates what
   arrives.
4. **A ceiling derived from a scope.** `Scopes` is now read by exactly one thing -
   `crate::capability`, which decides which of this surface's *operations* a caller may invoke and
   decides nothing about which rows an answer contains. A per-caller *budget* still has no port to
   live behind, and `docs/adr/0013`'s raw tool is not built. See `caller` for the limit stated
   beside the claim.
5. **Binding a gateway assertion to a request.** Added by review: in the `behind-gateway` mode the
   replay *window* is bounded - an `iat` is required and `exp - iat` is capped by a value this
   deployment chose - and inside that window an intercepted assertion replays. There is no nonce
   store and nothing hashes a method, a path or a body into the assertion. That is why nothing here
   calls it a proof that *this request* transited anything, and why the hop between the component
   and this process is a trusted transport boundary rather than an incidental one.

# A token cannot become a log field, and this is where the real macro is pinned

This module is the one that holds caller-supplied token material *and* a `tracing` dependency, so
it is where the claim `sutura_domain::identity::Secret` makes can be checked against the macro
rather than against the bound the macro imposes. `docs/adr/0020` decides the type; the domain
carries the `compile_fail` doctest for a `Display` bound, because `sutura-domain` may not acquire
`tracing` - `cargo xtask check-boundaries` walks its whole resolve graph.

`%` records a field through `tracing::field::display`, which is `T: Display`. A `Secret` has none:

```compile_fail
use sutura_domain::identity::Secret;

let token = Secret::new("hunter2");
// `%` wants `Display`. There is not one, so this line does not build.
tracing::info!(token = %token, "a caller presented a token");
```

The compiling twin, differing by the one sigil - `?` is `Debug`, which exists and cannot print the
value:

```
use sutura_domain::identity::Secret;

let token = Secret::new("hunter2");
tracing::info!(token = ?token, "a caller presented a token");
```

**The limit, stated with the claim:** `?token` compiles and is *safe here* because that `Debug` is
`secrecy`'s and cannot render the value. Nothing stops a call site logging
`token.expose_secret()`, which is exactly why that method is named to be conspicuous in a grep
rather than relied on to be absent.

# Why this is not shared with the agent surface

`sutura-mcp` has its own `principal` module and it still answers
`sutura_domain::identity::Subject::TheDeploymentItself`, honestly: it speaks over standard input and
output, where there is no header for a token to arrive in. **Nothing here is reachable from it** -
an adapter never calls another adapter, and that rule is what keeps this module from being the
shape a second transport has to bend around. Which crate this code moves to when that surface
acquires an inbound transport is an architecture decision, not a refactor.

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### Module `caller`

What a verified token establishes: a principal chain, and the scopes it carried.

# The type is the control, not the arity of a function

`AGENTS.md` records an invariant in the form *"`sutura_http::principal::established()` takes no
argument, so the transport has no parameter a request could reach"*. Leg 1 cannot keep that shape
and still work - a verified identity *is* something read out of a request - so what replaces it has
to be at least as strong. It is `VerifiedCaller`:

- **It has one constructor**, `VerifiedCaller::established`, and it is `pub(crate)`. Nothing
  outside this crate can make one at all.
- **Inside this crate, the only caller of that constructor is
  `crate::inbound::token::TokenValidator::verify`** - after a signature check against a pinned
  asymmetric algorithm, an issuer, an expiry and this deployment's own audience.
- **It implements neither `Deserialize` nor `Serialize`**, which is the same mechanism
  `sutura_domain::identity::principal` uses for the chain itself: there is no code that could turn
  caller-supplied bytes into one. A `compile_fail` doctest on `VerifiedCaller` asserts it, with a
  compiling twin so the failure cannot be passing for a typo.

So a handler receiving one has not received a claim; it has received the *conclusion* of a
verification. `crate::principal::of_verified` is the one function that turns it into a request
context, and it is why that module still has no way to build a chain out of a header.

# What the scopes are read for, and what they are still not read for

`docs/adr/0014` Decision 4 says a per-caller ceiling is **derived from the claims** and never read
from anything the caller sends with its question - the same argument that keeps a subject off the
`Query`. `Scopes` is that claim shape, parsed and bounded.

**One consumer exists now**, and it did not when this type landed: `crate::capability` hands
`Scopes::iter` to `sutura_app::Permitted::granted_by`, which decides which of this surface's
capabilities this caller may invoke. A route it may not invoke answers `403` with
`insufficient_scope`.

**Two consumers still do not exist, and the presence of this type must not be read as either.**
`docs/adr/0013`'s raw tool is not built, and a budget keyed on a principal has nowhere to live -
there is no budget port in this workspace.

**And the limit on the one that does exist is the important sentence here:** a scope decides which
*operations* a caller may invoke. It decides nothing about which rows an answer contains. No source
executes as the asking subject - that is leg 2 and it does not exist - so a narrowed caller gets
the same numbers from a question as anybody else would.

#### `enum InvalidScope`

```rust
pub enum InvalidScope
```

Why a scope string is not one.

##### Variants

- `TooLong`
- `TooMany`
- `NotAScopeToken` - A character RFC 6749's `scope-token` production does not allow.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `struct Scopes`

```rust
pub struct Scopes
```

The scopes a verified token carried.

A `BTreeSet` rather than a `Vec`: RFC 6749's scope is a set, an issuer may repeat a value, and the
ordering makes a log line and a test deterministic. Duplicates collapse rather than being refused,
because a repeated scope says the same thing twice and is not a posture.

##### Methods

```rust
pub fn count(&self) -> usize
```

How many distinct scopes were granted.

The one thing that reaches a log line. The scope *names* deliberately do not: they are a
caller's authorization detail, they multiply a log's cardinality, and nothing needs them there
until something filters on them.

```rust
pub fn grants(&self, scope: &str) -> bool
```

Whether one scope was granted.

**On the type rather than left to a caller to write**, so there is one comparison rather than
one per consumer. It is what `docs/adr/0013`'s raw tool would ask, and it is not what the
capability gate asks: that reads `Scopes::iter` and hands the whole set to
`sutura_app::Permitted::granted_by`, so the comparison against a capability's own scope literal
happens once, in the crate that owns the capability, rather than once per transport.

```rust
pub fn iter(&self) -> impl Iterator<Item> + '_
```

Every scope granted, in sorted order.

**The one thing that leaves this type as a set of values**, and it exists for
`sutura_app::Permitted::granted_by`: the surface's capabilities and the scopes that license
them are declared in `sutura_app`, so the derivation from a token's claims to what a caller may
do belongs there and not here. Handing over the values rather than answering
`Scopes::grants` per capability is what keeps this transport from holding a copy of that
comparison - and `sutura_app` needs no parse, because it compares against fixed literals.

**The type does not move, deliberately.** `docs/adr/0014`'s closing section reserves the
decision of which crate a validator lives in for whoever makes the agent surface reachable over
a network, and moving the parse now would be taking it early.

Not `Display` and not a log field: the scope *names* are a caller's authorization detail and
`Scopes::count` is what a line carries.

```rust
pub fn none() -> Self
```

No scopes. What a token with no `scope` claim carried.

**Not a `Default` that could stand in for an unparsed value**: `Default` is derived here because
the empty set is a real answer - an issuer that grants no scopes - rather than because a caller
needs a placeholder. There is nothing this type could be that has not been through a token.

```rust
pub fn parse(written: &str) -> Result<Self, InvalidScope>
```

Parses RFC 6749's space-delimited scope string.

Splits on whitespace rather than on a single space, which is more permissive than the grammar
and is right: an issuer that emitted two spaces has granted the same scopes, and refusing the
token for it would be refusing a caller for their provider's formatting.

##### Implements

`Clone`, `Debug`, `Default`, `Eq`, `PartialEq`

#### `struct VerifiedCaller`

```rust
pub struct VerifiedCaller
```

A caller whose token this deployment verified.

**The only way to one of these is a signature check.** See the module documentation for the three
properties that make that a mechanism rather than a convention.

# A caller cannot state its own identity

There is no `Deserialize`, so caller-supplied bytes cannot become one of these:

```compile_fail
// A transport that tried to read a verified caller off the wire does not compile.
let caller: sutura_http::inbound::VerifiedCaller =
    serde_json::from_str(r#"{"subject":"someone"}"#).expect("no");
drop(caller);
```

The compiling twin, so the failure above cannot be passing for a typo - what a handler can do with
one is read the chain it carries:

```
use sutura_domain::identity::Attribution;

fn read(caller: &sutura_http::inbound::VerifiedCaller) -> bool {
    matches!(caller.chain().attribution(), Attribution::BareSubject { .. })
}
```

##### Methods

```rust
pub const fn assertion(&self) -> &sutura_domain::identity::Secret
```

The caller's own credential this verification accepted.

The token that passed the signature, class, issuer, audience and lifetime checks - the only
way a token enters the request path. A broker that performs an exchange reads it; everything
else ignores it and, on the shared posture, never sees a value that would change a leg.

```rust
pub const fn chain(&self) -> &PrincipalChain
```

Who this call is attributed to.

```rust
pub const fn scopes(&self) -> &Scopes
```

What the token said this caller may do.

Read by nothing on the request path - see the module documentation. A `#[must_use]` on the
accessor is what keeps a call to it from reading as a check.

##### Implements

`Clone`, `Debug`

### Module `gate`

The layer that turns a presented token into a verified caller, or answers `401` with a challenge.

# Where it sits, and why after the deployment token rather than before

`crate::router` installs the layers so a request travels: limiter, then the deployment token gate,
then this. Two reasons, and neither is style:

- **Cost.** The deployment token comparison is two hashes; this is a signature verification. Doing
  the expensive one first would let an unauthenticated caller spend this deployment's CPU.
- **The limiter stays outermost**, which `crate::router` explains at length: a wrong-credential
  attempt has to cost a rate-limit cell or it is an unlimited guessing loop. That argument applies
  to a forged signature exactly as it applies to a wrong shared secret.

In the `direct` mode there is no deployment token to be after -
`sutura_config::NotFitToServe::DeploymentTokenSharesTheHeader` refuses that combination - so the
order matters only in the `behind-gateway` mode, where both are configured and each reads its own
header.

# What a refused request is told, and what it is not

A `401` with an RFC 6750 `WWW-Authenticate` challenge naming the realm, which for a directly
validating deployment is its own resource identifier. `docs/adr/0014` step 1 asks for *"a challenge
naming where to look"*, and this is the half of that which exists: **the two metadata documents the
record describes are not built**, so the challenge carries no `resource_metadata` parameter and a
client learns the authorization server out of band. That is a named gap rather than a silent one -
see `crate::inbound`.

What the response does **not** say is which check failed. The log says - through the `#[source]`
chain on `TokenRejected` - and the caller does not, because "the signature verified and the
audience did not" tells somebody which half of a forgery to fix.

#### `struct InboundGate`

```rust
pub struct InboundGate
```

Everything one deployment needs to establish who a caller is, built once at startup.

**Built by the composition root and not by `crate::ServiceState::new`**, because building it
reads a file: a constructor that could not fail would have to either swallow an unreadable key set
or read it lazily on the first request, and both turn a refusal to start into a deployment that
authenticates nobody. `crate::router` refuses to assemble a router for a deployment whose settings
declare an inbound identity and whose state carries no gate, which is what makes forgetting to
attach one a startup failure rather than an open door.

##### Methods

```rust
pub fn challenge(&self) -> Option<String>
```

The RFC 6750 challenge a refused request carries, where one is meaningful.

**`None` in the `behind-gateway` mode, and that is a fix rather than an omission.** A `Bearer`
challenge tells a client to present a bearer token to *this* resource; behind a component, the
caller holds no token for us and the thing that was missing was a header the component sets.
Sending the challenge anyway would send a well-formed instruction that cannot be followed, and a
client that followed it would start putting credentials in a header this deployment refuses to
read.

**No `error_description`** in the direct case, and that is the same decision the response body
makes: a description would have to say which check failed to be worth anything, and that is the
one thing a caller must not learn.

```rust
pub async fn describe_keys(&self) -> (usize, Vec<String>)
```

How many keys are cached, and their ids. For the startup log.

```rust
pub async fn establish(&self, headers: &HeaderMap, now: Instant) -> Result<VerifiedCaller, TokenRejected>
```

Establishes who is asking, or says why it could not.

The whole request path of leg 1, in one function, so the order of the four steps is readable in
one place: read the header, read the key id it names, find the key, verify.

**Takes the headers rather than the request, and both reasons are worth keeping.** The narrow
one is that it is the whole of what leg 1 may read: a gate that was handed a request could
establish an identity from a path, a query parameter or a body, and the signature is what makes
that unavailable rather than merely unwise. The mechanical one is that `axum::body::Body` is not
`Sync`, so a future holding `&Request` across an await is not `Send` and cannot run as a layer
at all - which is how the narrow reason got discovered.

```rust
pub fn from_declaration(inbound: &InboundIdentity) -> Result<Self, InboundNotUsable>
```

Builds the gate from a declaration `sutura-config` already accepted, reading the key set once.

**Called before the listener opens.** An unreadable or unusable key set is an error here, so it
is a process that does not start rather than one that answers `401` to everybody.

```rust
pub fn header(&self) -> &str
```

The header this gate reads, for a startup log line and for a test.

```rust
pub fn watch_keys_until_shutdown(&self, shutdown: Shutdown)
```

Starts the timer that bounds how long a revoked key keeps verifying.

Called from the composition root, inside the runtime, for the reason
`crate::tls::Renewal::watch_until_shutdown` is: the gate is built before a runtime exists, so it
cannot spawn its own task at construction.

**Forgetting it does not leave revocation unbounded**, and that is deliberate rather than
forgiving: `crate::inbound::keys::KeySetCache::key_for` checks the age itself, so a deployment
serving traffic re-reads within the same horizon. What the timer adds is the bound holding while
nothing is being asked.

##### Implements

`Debug`

#### `struct InboundNotUsable`

```rust
pub struct InboundNotUsable
```

The gate could not be built.

##### Implements

`Debug`, `Display`, `Error`

#### `fn require_verified_caller`

```rust
pub async fn require_verified_caller(__arg0: axum::extract::State<std::sync::Arc<InboundGate>>, request: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response
```

Requires a verified caller, and puts one in the request extensions.

A `from_fn_with_state` middleware over the gate rather than over
`crate::ServiceState`, so the state a handler is given has no way to
reach the validator: the only thing that crosses into the handler is the *result*, as a
`VerifiedCaller` extension that only this function inserts.

**The insertion overwrites**, which matters: `axum` extensions are a map, and a request arriving
with something already under that type - which nothing can construct, but the reasoning should not
rest on that alone - is replaced rather than joined.

### Module `keys`

The signing keys, the cache in front of them, and the two things that make it re-read.

`docs/adr/0014` names key rotation as one of three things a directly validating deployment newly
owns, and it names the standard way to get it wrong: *"Cache the key set, honour its cache
headers, refetch on an unknown key id - and **rate-limit that refetch**. Without the limit, a
forged key id turns every request into an outbound call to the authorization server, which is a
denial-of-service primitive pointed at our own dependency."*

# Two triggers, and the second one is a REVOCATION bound rather than a rotation one

The rate limit was the whole mechanism here once, and review found what that left open: a key
**removed** from the set kept verifying until an unrelated unknown key id happened to arrive. A
caller-driven refetch cannot bound revocation, because the caller presenting a revoked key
presents a `kid` this deployment *has* - so nothing triggers.

So there are two triggers and they answer different questions:

| Trigger | Answers | Bounded by |
| --- | --- | --- |
| an unknown key id | "has a key been ADDED that I have not seen" | `MIN_REFETCH_INTERVAL`, because the trigger is caller-controlled |
| age | "has a key been REMOVED" | `MAX_KEY_SET_AGE`, because the trigger is the clock and a caller cannot make it fire faster |

The age trigger fires from two places, deliberately. `KeySetCache::watch_until_shutdown` is a
timer - the same shape `crate::tls::Renewal::watch_until_shutdown` already uses, spawned from the
composition root inside the runtime - so revocation latency is bounded *whether or not this
deployment is serving traffic*. And `KeySetCache::key_for` checks the age itself, so a
composition root that never armed the timer still cannot serve a stale key set indefinitely: the
first request past the horizon pays one file read. Neither is a second code path - both call
`KeySetCache::poll_once`.

# The bound is one read per window WHATEVER THE CONCURRENCY, and it was not

Review measured three reads where two were required, from two concurrent misses. The cause was
double-checked locking with the second check missing: `KeySetCache::key_for` decided a look was
due from a value read under a *read* lock, and the write lock was taken only to stamp - so two
callers observing the same `last_attempt` both went on to read the source, and an attacker
amplified source I/O by the number of in-flight forged key ids.

`KeySetCache::reserve` is the fix: **the check and the stamp are one lock acquisition**, the
source read stays outside the lock, and it is the only place either window is compared. The two
comparisons in `key_for` remain as a cheap fast path and decide nothing. Because `poll_once` is the
single path, the timer cannot race a caller into two reads either - which is a question worth
asking of a design with two triggers and is answered by there being one gate.

# Why `now` is a parameter everywhere

`KeySetCache::key_for` and `KeySetCache::poll_once` take the current instant rather than
reading the clock. That is what makes the interesting cases - a forged key id arriving inside the
window, a key set going stale, and two callers arriving at the same instant - assertable without a
sleep, which is the same reason `Renewal::poll_once` is public. **A concurrency bound proved by a
sleep being long enough is worse than none**, and this file is the second attempt at this bound.

# What a key set is read from, and the gap that is named rather than hidden

`FileKeySet` is the only source that ships. **There is no HTTPS fetcher**, and that is stated
here rather than left to be discovered: an outbound HTTP client is a supply-chain change with its
own review, and `docs/adr/0014` says plainly that the authorization server then becomes a hard
runtime dependency whose outage must stay *distinguishable from a dead data system*. None of that
is built.

What is built is everything that a URL source would need anyway - the cache, both triggers, and
the limit on the caller-driven one - behind `KeySetSource`, which is one method returning the
document's **bytes**. A JWKS endpoint arrives as a second implementor and changes nothing else in
this file. A file is also a real deployment shape rather than a placeholder: a sidecar that
rewrites a mounted key set is how a process with no egress gets rotation.

**The honest cost of the file source:** it does not honour a cache header, because a file has
none. What bounds staleness is `MAX_KEY_SET_AGE` and nothing the issuer says.

#### `struct KeyId`

```rust
pub struct KeyId
```

A key identifier, out of a token header or out of a key set.

A newtype rather than a `String` because the value arrives from a caller and is then used as a map
key, as the trigger for an outbound fetch, and as a log field. The field is private and
`Self::parse` is the only way in.

##### Methods

```rust
pub fn as_str(&self) -> &str
```

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, NotAKeyId>
```

Parses a key identifier.

**No trim, deliberately**, unlike almost every other parse in this workspace. A `kid` is an
opaque identifier an issuer chose and we compare byte for byte against a key set the same
issuer published; trimming would make a token whose id has a trailing space match a key whose
id does not, and the two are different strings to whoever wrote them.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`

#### `enum NotAKeyId`

```rust
pub enum NotAKeyId
```

Why a string is not a key identifier.

##### Variants

- `Empty`
- `TooLong`
- `NotPrintable` - Anything outside printable ASCII.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `enum InvalidKeySet`

```rust
pub enum InvalidKeySet
```

Why a document is not a usable key set.

##### Variants

- `NotAJwkSet`
- `NoUsableKey`
- `KeyWithoutAnId` - A key with no `kid`.
- `UnusableKeyId`
- `DuplicateKeyId` - Two keys under one id.
- `SymmetricKey` - A symmetric key.
- `UnsupportedKeyFamily`
- `UnusableKey`
- `NoKeyOfThePinnedFamily` - Not one key of the family the pinned algorithms need.

##### Implements

`Debug`, `Display`, `Error`

#### `struct KeySet`

```rust
pub struct KeySet
```

The verifying keys this deployment holds, by id.

A `BTreeMap` rather than a `HashMap`: a key set holds a handful of entries, the ordering makes a
log line and a test deterministic, and there is no hash-collision surface on a caller-supplied
lookup key at all.

##### Methods

```rust
pub fn count(&self) -> usize
```

How many keys are held. For a startup log line, so an operator can see the set was read.

```rust
pub fn get(&self, id: &KeyId) -> Option<DecodingKey>
```

The verifier for one key id, if this set holds it.

Cloned rather than borrowed, because the caller is about to await a lock release and then
verify: a `DecodingKey` is a small owned value - a modulus and an exponent, or a point - and
holding a read lock across the verification would serialise every request behind it.

```rust
pub fn holds(&self, family: KeyFamily) -> bool
```

Does this set hold a key of `family`?

The question `InvalidKeySet::NoKeyOfThePinnedFamily` is asked of. **At least one** rather
than all of them, deliberately: an issuer legitimately publishes RSA and elliptic-curve keys in
one document, and what makes a deployment unable to authenticate anybody is holding *none* of
the kind it pinned.

```rust
pub fn ids(&self) -> Vec<&KeyId>
```

The ids, for a log line and for a test.

```rust
pub fn parse(document: &str) -> Result<Self, InvalidKeySet>
```

Parses a JWK set document.

**Every refusal here is a refusal to start, not a key that gets skipped.** A key set is
operator-supplied configuration and a deployment that silently dropped half of it would
authenticate an arbitrary subset of its callers - which reads exactly like an intermittent
outage. `docs/adr/0014`'s posture is fail-closed on the query path and this is that.

##### Implements

`Clone`, `Debug`

#### `trait KeySetSource`

```rust
pub trait KeySetSource
```

Where a key set is read from.

One method, so a JWKS endpoint is a second implementor and nothing else in this file moves. See
the module documentation for why the only implementor today reads a file.

**It returns the document's BYTES rather than a parsed key set**, and that is what lets
`KeySetCache::poll_once` tell "changed" from "unchanged" the way `crate::tls::Renewal` does. A
comparison of parsed keys could not: the library's key type implements no equality, so the
alternative was comparing key *ids*, which would miss a key whose material rotated under the same
id.

**Synchronous, deliberately.** The one implementor reads a small local file, at most once per
`MAX_KEY_SET_AGE`, and making the trait `async` would either need a boxed future in the
signature or force the file source to pretend. A URL source arrives with a real decision about
where its I/O runs, and that decision belongs in the same change as the client.

#### `enum KeySetUnavailable`

```rust
pub enum KeySetUnavailable
```

The source could not be read, or what it returned is not a key set.

##### Variants

- `Unreadable`
- `Invalid`

##### Implements

`Debug`, `Display`, `Error`

#### `struct FileKeySet`

```rust
pub struct FileKeySet
```

A key set on the local filesystem.

##### Methods

```rust
pub fn at(path: impl Into<PathBuf>) -> Self
```

Names the file. Does not read it: `Self::read` is the read, and the composition root reads
once before the listener opens so an unreadable key set is a refusal to start.

```rust
pub fn path(&self) -> &Path
```

The path, for a startup log line.

##### Implements

`Clone`, `Debug`, `KeySetSource`

#### `enum KeyUnavailable`

```rust
pub enum KeyUnavailable
```

Why a token could not be matched to a verifying key.

Separate from the token's own refusals because the two are different facts about a deployment: a
signature that does not verify is a bad token, and a key id nobody has heard of after a refetch is
either a rotation this deployment has not caught up with or a caller guessing.

##### Variants

- `RefetchRateLimited` - The id is not in the set, and it is too soon to look again.
- `UnknownKeyId` - The id is not in the set after a fresh read.
- `SourceUnavailable`

##### Implements

`Debug`, `Display`, `Error`

#### `enum Refreshed`

```rust
pub enum Refreshed
```

What one look at the source did.

The same three outcomes `crate::tls::Renewed` has, and for the same reasons: an unreadable source
is not a change, and a candidate that was examined and rejected is recorded as examined so
identical bytes on the next tick are silent rather than logging a rejection once per interval
forever.

##### Variants

- `Unchanged` - The document is byte-for-byte what is already in use, or it could not be read.
- `Rotated` - A new document parsed, held a key of the pinned family, and is now in use.
- `Rejected` - A new document was read and is NOT usable. The previous key set keeps verifying.
- `NotDue` - **The source was not looked at**, because the window has not opened or another caller already reserved this look.

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

#### `struct KeySetCache`

```rust
pub struct KeySetCache
```

The key set, cached, with a rate-limited refetch on an unknown key id and an age bound on the
whole set.

`tokio::sync::RwLock` rather than `std::sync::RwLock`, which `clippy.toml` bans: this is held
across an `await` in an async middleware, which is exactly the deadlock that ban is for.

**No read of the source happens while the write lock is held**, which review asked for: the lock
is taken to stamp the attempt, released, the document read, and taken again to swap. The stamp
under the first lock is what keeps two concurrent misses from becoming two reads.

##### Methods

```rust
pub async fn describe(&self) -> (usize, Vec<String>)
```

How many keys are cached, and their ids. For a startup log line and for a test.

```rust
pub async fn key_for(&self, id: &KeyId, now: Instant) -> Result<DecodingKey, KeyUnavailable>
```

The verifier for a key id, re-reading the source when the set is stale or the id is unknown.

The order is: read, then age or window, then `Self::poll_once`. **The two comparisons here are
a cheap FAST PATH and not the decision**, which is the correction review forced: they run under a
read lock, so two callers can both observe the same `last_attempt` and both fall through. Whether
a look at the source actually happens is decided once, atomically, inside `poll_once` - see
`Self::reserve`.

**What a caller that loses the reservation gets, stated because it is a real outcome:** it does
not wait. It answers from whatever is cached, which during a rotation may be an
`KeyUnavailable::UnknownKeyId` for a key the winner is about to install, or - on the age path -
one more use of a key the winner is about to remove. So revocation is bounded by
`MAX_KEY_SET_AGE` plus the duration of one source read, and a rotation can cost a concurrent
caller one `401` it can retry. Making it wait instead would put N request tasks behind one file
read, which is the primitive this whole file is arranged against.

```rust
pub async fn poll_once(&self, now: Instant) -> Refreshed
```

Looks at the source if a look is due, and swaps the key set if what came back is usable and
different.

Public and taking `now`, for the reason `crate::tls::Renewal::poll_once` is public: a test
rotating a key set must not have to wait for a timer, and asserting that a timer fires is a
different assertion from asserting that a rotation works.

**It is DUE-CHECKED, which is a deliberate departure from `Renewal::poll_once`.** That one looks
unconditionally, because a TLS renewal watch is the only thing that calls it. This one is called
by the timer *and* by two paths in `Self::key_for`, one of which a caller triggers - so there
has to be exactly one place the reservation is taken, or the paths are three chances to get the
bound wrong. **That is also the answer to whether the timer can race a caller into two reads: it
cannot, because they are the same path.**

**Loud, and then carry on with what works.** A document that will not parse, or that holds no
key of the pinned family, is `Refreshed::Rejected` and the previous set keeps verifying -
adopting a broken set would turn a rotation mistake into a total outage, which is the trade
`crate::tls` already makes for the same reason.

```rust
pub fn primed(source: Box<dyn KeySetSource>, family: KeyFamily, pinned: String, now: Instant) -> Result<Self, KeySetUnavailable>
```

Reads the source once and caches what it returned.

**Fails rather than starting empty**, which is what makes an unreadable key set a refusal to
start: a cache that began empty would answer every request `401` while looking healthy, and the
rate limit would keep it that way for thirty seconds at a time. It also fails when the document
holds no key of the pinned family - see `InvalidKeySet::NoKeyOfThePinnedFamily`.

```rust
pub fn watch_until_shutdown(cache: &Arc<Self>, interval: Duration, shutdown: Shutdown)
```

Polls on an interval until shutdown is asked for.

A `tokio` task rather than the OS thread `crate::middleware::spawn_reaper` uses, and the
difference is where it is started from - the same difference `crate::tls::Renewal` records: the
reaper is spawned while the router is assembled, before a runtime exists, and this is spawned
from inside the served future.

**Forgetting to arm it does not leave revocation unbounded**, which is why it is not the only
trigger: `Self::key_for` checks the age itself, so a deployment serving traffic re-reads
anyway. What the timer adds is a bound that holds while nothing is being asked.

##### Implements

`Debug`

#### `constant MIN_REFETCH_INTERVAL`

How long after one attempt to reach the source another may be made.

**The rate limit `docs/adr/0014` asks for**, as a constant rather than a configuration key. It is
not a posture decision - nothing a caller can do changes what the right answer is - and a knob
here would only ever be set wrong, in the direction that reopens the denial-of-service primitive.
Thirty seconds is far below any horizon at which a rotation is late and far above the cost of a
forged key id.

#### `constant MAX_KEY_SET_AGE`

How stale a cached key set may be before it is re-read whatever a caller asks for.

**This is the revocation bound**, and it is the number a reviewer should argue with if they argue
with anything here: a key removed from the set keeps verifying for at most this long. A constant
rather than a key for the same reason the limit above is one - and unlike that limit, this one
only ever wants to be *smaller*, so the cost is what sets it. One minute is one small read per
minute per process, which is the same order as
`crate::middleware::REAP_INTERVAL` and is nothing next to a signature verification.

### Module `token`

Verifying one token, and turning its claims into a principal chain.

# The check review found missing, and it is the serious one

**A signature, an issuer and an audience do not identify a token's CLASS.** Without a `typ` check,
any JWT the issuer signed with this audience verifies - and an OIDC ID token has the same issuer
and, whenever the resource identifier equals the client id, the same audience. That is the ordinary
identity-provider arrangement rather than an exotic one, so the substitution is cheap: a document
minted to describe a login establishes a caller for an API call. Review demonstrated it against
this file.

`TokenValidator::verify` now checks `typ` against `sutura_config::RequiredTokenType`, which
defaults to RFC 9068's `at+jwt` in the `direct` mode. **The check happens on `decoded.header`,
after the signature**, and that placement is the point: `TokenValidator::key_id`'s whole doc
comment is that nothing configured applies yet because a JWT header is unauthenticated input, so a
`typ` read there would be a rule applied to a document nobody signed. After `decode` it is a rule
applied to a document the issuer signed - the same reasoning that already puts the actor-nesting
bound there.

# The three things `docs/adr/0014` says a direct deployment owns

Key rotation is `super::keys`. The other two are here:

**Algorithm pinning.** `TokenValidator::new` builds the validation from
`sutura_config::PinnedAlgorithms`, which is a non-empty list of a single key family with no
symmetric variant and no `none` variant *available to construct*. Nothing in this file reads the
`alg` of the token being validated in order to choose how to verify it: the library compares the
header's algorithm against the pinned list and refuses otherwise, and because the list can only
hold asymmetric algorithms, the classic confusion - a token signed `HS256` with the issuer's public
key as the HMAC secret - has no path. `super::keys::InvalidKeySet::SymmetricKey` closes the other
half, which is a key set that published a shared secret.

**The audience.** `Validation::set_audience` is given exactly one value: this deployment's own
resource identifier, out of the configuration, and `aud` is in `required_spec_claims` - so a token
with *no* audience is refused rather than accepted for want of a claim to compare. That is the
security decision in the record: a client's resource indicator is welcome and is an optimisation,
and this check is ours, unconditional, and not skippable when the indicator is absent.

# What is not `deny_unknown_fields`, and why that is right exactly here

`Claims` deliberately accepts unknown fields, which is the opposite of every other wire shape in
this workspace. A token is not a document this deployment defines: an issuer puts `azp`, `jti`,
`email`, `groups` and a dozen vendor claims in one, and refusing a token because its issuer added a
claim would break every deployment on the issuer's next release. What keeps that from being a hole
is that the fields we *do* read are the only ones anything downstream can see - and one of them,
the actor chain, is bounded.

# Where the caller's own text is bounded

Before anything expensive: `MAX_TOKEN_BYTES` caps what is even looked at, and `SubjectId` and
`Actor` are parsed by `sutura_domain::identity`, which bounds their length and refuses a control
character or an invisible one - because those values are written into an audit record that is one
line per call.

#### `enum TokenRejected`

```rust
pub enum TokenRejected
```

Why a presented token did not establish a caller.

**Every variant is "this caller is not authenticated", and none of them is a
`sutura_domain::query::RefusalReason`.** That is the placement `docs/adr/0008` part 6 already gives
an expired assertion: a refusal is a governance answer to a question that was understood, and a
caller who has not proved who they are has not asked a question yet. `crate::problem::Failure` is
where this becomes a status.

**No variant carries the token, a claim value, or a key.** The `#[error]` text names what was
wrong; the value that was wrong stays out of it, because these render into a log an operator reads
and an error is not a place for credential material.

##### Variants

- `Absent` - No token where this deployment reads one.
- `NotABearerToken` - A header with some other authentication scheme.
- `TooLong`
- `UnreadableHeader` - The header is not a JWT header, or names no key.
- `NoKeyId` - No `kid`.
- `UnusableKeyId`
- `NoKey`
- `NotVerified` - The signature, the expiry, the issuer or the audience.
- `UnusableSubject` - A `sub` this workspace will not write into a record.
- `UnusableActor`
- `TooManyActors` - More nesting in `act` than `MAX_ACTORS` allows.
- `UnusableScope`
- `WrongTokenType` - The token is of a class this deployment does not accept.
- `NoIssuedAt` - A transit proof with no `iat`.
- `LifetimeTooLong` - A transit proof declaring a longer life than this deployment will call short-lived.
- `IssuedInTheFuture` - An `iat` in the future by more than the leeway.

##### Implements

`Debug`, `Display`, `Error`

#### `enum PresentedType`

```rust
pub enum PresentedType
```

The `typ` a refused token presented, where it was one at all.

A named type rather than an `Option<TokenType>` in the variant, so the *absent* case renders as a
sentence rather than as `None` - and so the case where a `typ` was present but unusable is
distinguishable from the case where there was none. Both are refusals; they are different
diagnostics.

##### Variants

- `Named` - A `typ` that parsed, and is not the one required.
- `Absent` - No `typ` header at all. Refused by anything but `any` - see `sutura_config::RequiredTokenType::accepts`.
- `Unusable` - A `typ` header holding something no media type could be. Not rendered, for the reason `TokenRejected::WrongTokenType` gives.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

#### `struct TokenValidator`

```rust
pub struct TokenValidator
```

One deployment's whole token check, built once at startup.

Holds the built `Validation` rather than rebuilding it per request, which is not an optimisation:
building it per request would be a per-request opportunity for one of its fields to be set
differently, and every field on it is a control.

##### Methods

```rust
pub const fn audience(&self) -> &ResourceIdentifier
```

The audience this validator requires, for a challenge and for a log line.

```rust
pub fn key_id(token: &str) -> Result<KeyId, TokenRejected>
```

The key id the token names, bounded and parsed, without verifying anything.

**Separate from `Self::verify` because it happens before the signature is checked** and the
caller has to know that: the header of a JWT is unauthenticated input by construction, which is
why the only thing taken out of it is an identifier that gets bounded, parsed and used as a map
key. The algorithm in that header is read by nothing here.

An associated function rather than a method, and that is the same point stated by the signature:
nothing configured applies yet. There is no `&self` because there is nothing on the validator
this step is allowed to consult.

```rust
pub const fn leeway() -> Duration
```

The leeway in effect, so a test asserts the value rather than the constant.

```rust
pub fn new(requirement: &TokenRequirement<'_>) -> Self
```

Builds the validation from a declaration `sutura-config` already accepted or refused.

Every line here is a control, so each one says what it is for. What is **not** here is equally
the point: no `insecure_disable_signature_validation`, no `validate_aud = false`, and no path
that reads the algorithm out of the token in order to pick one.

```rust
pub fn verify(&self, token: &str, key: &DecodingKey) -> Result<VerifiedCaller, TokenRejected>
```

Verifies the token with the key and turns its claims into a caller.

The order is the library's and it is the right one: signature first, then the registered
claims, and only then is the claim payload deserialized into `Claims` - so every check below
is applied to a document an issuer signed rather than to one a caller wrote. That is why the
class check, the actor-nesting bound and the lifetime ceiling all live here and not in
`Self::key_id`.

##### Implements

`Debug`

#### `constant MAX_TOKEN_BYTES`

The largest token this surface will look at.

**Bounded before the signature is checked, because everything before that point is work done on
behalf of an unauthenticated caller.** Eight kibibytes is generous for an access token carrying
groups and an actor chain, and far below the header limit the HTTP implementation would otherwise
be the only bound at. An unbounded input is a denial-of-service primitive whatever else it is.

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

`crate::router` now puts the limiter outside the gate, which is the point
of the reordering: a wrong-token attempt has to cost a cell or it is an unlimited guessing loop.
That makes every reachable path a path an unauthenticated caller can create a bucket on, and
with the header keying above it is one bucket per real client rather than one per ingress. **So
the reordering and `spawn_reaper` are one change and must not be separated:** either alone is
worse than neither.

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
`tower-http` 0.7.0 implements `TimeoutLayer::with_status_code` as
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

# A refusal is not in here, and the reason is not the status any more

Worth stating first, because it is the distinction the whole surface turns on and the obvious
shorthand for it has stopped working. A *refusal* - the caller asked something they may not have -
now carries an error status too, from `crate::wire::refusal`. A *failure* is everything else: a
body that is not a question, a missing credential, a limit reached, a data system that did not
answer. Only failures reach this module.

So the two are no longer told apart by `2xx` against `4xx`. What tells them apart is the **body**,
and that is the deliberate choice rather than a leftover: a refusal keeps
`crate::wire::OutcomeBody::Refusal` with its `outcome` discriminator, and a failure keeps
`ProblemBody`. `outcome` is the one-field test for which arrived, which matters most exactly
where a status is shared - `503` is `unavailable` or `at_capacity` from here, and
`source_unavailable` from there; `413` is a request body over the limit from here, and an answer
over the row cap from there.

**A refusal is not routed through `Failure`, and must not be.** `Failure` is what an `Err`
becomes, and `ToolOutcome::Refusal` is a domain *result*: a `Failure::Refused` variant would put a
governance outcome into the error enum and make the type system agree with the mistake this design
exists to prevent. `docs/adr/0005` is the record.

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
- `InsufficientScope` - The caller is authenticated and was not granted the capability this route needs.
- `NotAQuestion` - The body is not a question. Carries a message naming the field.
- `TooLarge` - The body is larger than the configured bound.
- `RateLimited` - Too many requests from this address, too quickly.
- `Timeout` - The request took longer than the configured bound.
- `Internal` - Something on our side went wrong. Carries nothing.
- `Unavailable` - The data system did not answer. Distinguished from `Self::Internal` because it is the one failure that is worth retrying, and a caller cannot tell from a 500.
- `IdentityUnavailable` - The credential broker did not answer, so nothing could be executed as the asking subject.
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
- `InboundIdentityNotAttached` - The settings declare an inbound identity and the state carries no gate to establish it.
- `RouteNotGoverned` - A route under the version prefix that `crate::capability::governed` names no capability for.

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

# The admission bound is TAKEN, and it used to be built here

**`telekom/sutura#340`, and the sentence this section replaced is the defect.** It read *the
only place that can be true without a second constructor argument is beside the settings it is
derived from*, and it was wrong in the way that matters: `Admission::from_settings` inside
`ServiceState::new` made a second `ServiceState` a second permit set, and a process serving
this transport beside another would have held two semaphores each reporting a limit the other
can exceed. `sutura_runtime::admission`'s own module documentation calls that shape not-a-bound
and says the composition root builds one - and nothing held it.

So the bound arrives as an argument. Three consequences worth naming, because the argument for
deriving it was that a caller can forget a bound:

* **It cannot be forgotten**: the parameter has no default and no `Option`, so a state built
  without one does not compile - the same shape `sutura_app::surface::LocalService::start`
  gives its audit sink.
* **It can be SHARED**: one `Admission` handed to two states is one permit set, which is what
  makes the number a bound on the process rather than on a router.
* **A composition root builds exactly one**, held by `cargo xtask check-one-bound` in
  `just hygiene` rather than by this comment. That gate would fail this crate for building one
  at all.

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
pub const fn inbound_identity(&self) -> Option<&Arc<crate::inbound::InboundGate>>
```

Leg 1, if this deployment has it.

Read by `crate::router` to install the layer, and by nothing else - a handler must not be able
to reach the validator, which is why the middleware takes the gate as its own state rather than
reading it back out of this one.

```rust
pub fn new(surface: Arc<dyn Surface>, settings: Arc<Settings>, admission: Admission) -> Self
```

Builds the state from a started service and the settings it was started under.

Takes the surface already behind an `Arc`, because the composition root owns it: the same
service may be handed to a second transport later, and this crate must not be the one that
decides there is only ever one.

**The admission bound is taken and not derived, which is `telekom/sutura#340`.** It is the
same argument as the surface one line above it, one bound further: the permit set belongs to
the process, so the only component that may decide there is one of it is the composition
root. See the module documentation for what deriving it cost.

```rust
pub fn settings(&self) -> &Settings
```

```rust
pub fn surface(&self) -> Arc<dyn Surface>
```

The service, for a handler that is about to move the call onto the blocking pool.

```rust
pub fn with_inbound_identity(self, gate: Arc<crate::inbound::InboundGate>) -> Self
```

The same state, with leg 1 attached.

Called by the composition root, after it has read the key set the declaration names. A state
whose settings declare an inbound identity and which has not been through this is a state
`crate::router::assemble` refuses.

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
configuration goes to the listener, and the renewal goes to whatever will poll it. `Self::prepare`
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
ones that complete down a bounded channel. `TlsListener`'s `accept` pops from that channel and
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
- `FilterValue` - The value is not one a catalog could have declared: nothing, more than one line, a control character, an invisible or direction-changing code point, spacing a reader cannot see, or longer than `sutura_domain::catalog::MAX_DIMENSION_VALUE_CHARS`.

#### Implements

`Debug`, `Display`, `Error`

### `enum OutcomeBody`

```rust
pub enum OutcomeBody
```

What a question produced.

**The two variants come back with different statuses**, and the `outcome` discriminator is what a
caller branches on within one of them. An answer is a `200`. A refusal is a `403`, `404`, `409`,
`413`, `422` or `503` depending on why - `refusal` holds the mapping and the reasoning, and
`Outcome` is what pairs the two.

It used to be `200` for both, on the grounds that an error status invites a client library to
retry. That was checked and does not hold; more to the point, a `200` made a governance refusal
indistinguishable from an answer to every reader that sees a status and not a body. The body
below is unchanged: same `outcome` tag, same `reason` object, one field added inside it.

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

### `struct LegBody`

```rust
pub struct LegBody
```

One leg of an answer: which source it ran on, and which identity it ran as.

**The posture is a word and the operator's acknowledgement reason is NOT here**, deliberately. The
reason is text an operator wrote for a reviewer, printed by the startup log; putting it on the wire
would send operator prose into an agent's context on every answer, which is a channel nobody asked
for. What a caller needs is which of the two postures produced the rows, and that is the word.

**Reading this is not a control.** It reaches a caller after the rows did, so it cannot prevent a
disclosure. It makes one attributable, and it makes a misconfiguration visible to whoever reads an
answer; what stops a shared source being served unnoticed is a startup refusal.

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`

### `struct RefusalBody`

```rust
pub struct RefusalBody
```

Why a question was refused.

A stable `code` per refusal, the status it came back as, and a sentence. The code is what a
caller branches on; the sentence is for a person; the status is repeated here for the same reason
`crate::problem::ProblemBody` repeats it - a client that logged only the body still has it.
Which status each refusal gets, and why, is in `refusal`.

**Nothing here echoes a value the caller sent.** The domain's refusal variants already stop short
of that - a rejected filter value names the dimension and not the value, on purpose, because
reflecting caller text into a message that reaches a log, a UI and an agent's context is how a
rejected value becomes somebody else's input. The identifiers that *are* echoed are parsed
newtypes over a bounded character set, and the numbers are derived from parsed dates or are this
service's own limits.

#### Methods

```rust
pub const fn code(&self) -> &'static str
```

The code, for a test that asserts on the contract rather than on the prose.

```rust
pub fn detail(&self) -> &str
```

The sentence. A test asserts it is not empty; nothing asserts its wording.

```rust
pub const fn status(&self) -> u16
```

The status, as the body carries it.

#### Implements

`ComposeSchema`, `Debug`, `Serialize`, `ToSchema`

### `struct Outcome`

```rust
pub struct Outcome
```

An outcome, and the status the transport says it with.

**The one conversion from a `ToolOutcome` to a response**, and it is one rather than two
because the status and the body are the same decision. An answer is a `200`; a refusal is the
status `refusal::refused` gives it, which is never a `2xx` - see that module for the whole
argument and for why this file used to claim the opposite.

The *type* invariant is untouched by that. `ToolOutcome::Refusal` is still a domain result and
not an `Err`: it arrives here through `Ok`, this handler cannot get one by mistake, and nothing
on the way turned it into a `crate::problem::Failure`. What changed is only what the transport
says about it.

#### Methods

```rust
pub const fn body(&self) -> &OutcomeBody
```

The body, for a test that asserts on the JSON rather than on the response.

```rust
pub const fn status(&self) -> axum::http::StatusCode
```

The status this outcome comes back as.

#### Implements

`Debug`, `IntoResponse`

### `struct CatalogBody`

```rust
pub struct CatalogBody
```

What this catalog defines.

# Why a structured surface reads the prose setting at all

`prompt.catalog_prose: omitted` is not a mitigation for the forgery `docs/adr/0022` is about -
`serde` owns the field boundary here, so a description cannot cross one whatever it spells, and
this body escapes nothing. It is a decision about **who may put words in front of an agent**: an
operator whose catalog authors are not the people who decide what their agents are told drops the
prose, and a description that still reached an agent through a second transport would make that
setting a statement about one surface rather than about the deployment. So the omission is
honoured wherever the prose is carried, and the escaping stays where the delimiter is.

#### Methods

```rust
pub fn of(pinned: &PinnedDefinitions, prose: sutura_config::CatalogProse) -> Self
```

The reader's view of a pinned bundle, under the prose setting this deployment was started
with.

**A named constructor rather than a `From`, and the argument is the reason.**
`CatalogProse::default()` is `Quoted`, so a conversion reachable without the setting fails
OPEN: it ships the prose of a deployment that asked for none, which is the defect this
function exists to close. A second argument cannot be left out.

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
