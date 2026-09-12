---
title: How the agent surface learns who is asking
description: The agent surface is served over a pipe, so it carries no per-request principal and answers every question as the deployment. This record takes the decision to serve it over an authenticated HTTP transport inside the server binary instead, and carries the evidence that decides it - what the pinned MCP SDK's HTTP server transport does with an inbound header, measured against the vendored source of the pinned version rather than assumed. It also records that reaching a data source from an enterprise IdP is a CHAIN of exchanges where exactly one hop is built, what a chain changes beyond looping, and a naming collision that makes the tree read as further along than it is.
---

# How the agent surface learns who is asking

Status: **accepted, and nothing is built.** This record takes a transport decision and states the
evidence it rests on; no wiring, no crate change and no gate ships with it. What it decides is
*where* a per-request principal can arrive on the agent surface, and it does that before any code
because the alternative - attaching an exchange to a transport that has nowhere to put a caller -
is the expensive way to find out.

[0014](0014-how-a-caller-proves-who-it-is.md) built leg 1 on the HTTP surface: a deployment
declaring `security.inbound` verifies a caller's own token from a signature.
[0008](0008-a-credential-per-leg-for-the-calling-subject.md) built the port and the broker for leg
2. **Neither reaches the agent surface**, and the reason is not an omission - it is the transport.
[Where each identity claim is proven](../where-identity-is-proven.md) has said so in plain words
since before this record; what follows is the decision that closes it rather than a correction to a
false claim.

## What is true today, measured rather than recalled

Every row below was read off `origin/main` at `5ae3bde` on 2026-09-06, with
`git cat-file blob origin/main:<path>` and `git grep -n <pattern> origin/main -- <path>`. The
command is written here because a bare figure in prose is one nobody can re-check.

| Measured                                                                                                          | Value                                                                                                                                                                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| occurrences of `broker`, case-insensitive, in the agent surface's composition root `crates/sutura-cli/src/mcp.rs` | **0**. `git cat-file blob origin/main:crates/sutura-cli/src/mcp.rs \| grep -c -i broker` over a file `grep -c ''` reports at 471 lines                                                                                                                                              |
| `crates/sutura-mcp/src/lib.rs:162`                                                                                | `pub async fn serve_stdio<S>(service, permitted, prose, admission, reply)` - **no parameter a principal could arrive through**                                                                                                                                                      |
| what that function opens                                                                                          | `rmcp::transport::stdio()` - the process's own pipes                                                                                                                                                                                                                                |
| `crates/sutura-mcp/Cargo.toml` first-party dependencies                                                           | `sutura-app`, `sutura-config`, `sutura-domain`, `sutura-runtime`. **No adapter and no broker**                                                                                                                                                                                      |
| the chain the agent surface establishes                                                                           | `crates/sutura-mcp/src/principal.rs`: `pub(crate) const fn established() -> RequestContext` returning `RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))` - **a `const fn` with no inputs**, which is the honest shape for a transport that authenticates nobody |
| what capabilities that surface grants                                                                             | `crates/sutura-cli/src/mcp.rs:133` and `:233` pass `Permitted::every_capability()`, fixed at construction                                                                                                                                                                           |
| the same question on the HTTP surface                                                                             | `crates/sutura-http/src/capability.rs:138` `pub fn permitted_for(request: &Request) -> Permitted`, **per request**, from the verified caller's scopes                                                                                                                               |

Two things follow, and they are different. The first is that the pipe is the cause: a `Permitted`
fixed at construction and a `const fn` chain are both correct for a transport whose caller is
whoever launched the process. The second is that the command-line tool's
`StaticCredentialBroker` (`crates/sutura-cli/src/sources.rs:147`) is **not** a gap and must not be
reported as one - a single-user tool's operator *is* the subject, and an exchange there would be
impersonation of the person already at the keyboard.

## The two shapes, and the one that was taken

1. **The agent surface over an authenticated HTTP transport, inside the server binary.** Each
   request carries the caller's own bearer; the surface reads it per call and the existing broker
   exchanges it per subject.
2. **Keep the pipe and treat the launching identity as the subject.** Honest for a single-user
   desktop agent, and effectively today's behaviour - but a process serving many people cannot be
   built out of it, at any amount of effort, because there is no per-request principal to start
   from.

**Decision: shape 1**, taken on `telekom/sutura#378` on 2026-09-06 rather than derived in this
record - what this record does is carry the argument and the evidence. Shape 2 is rejected as the
answer for the served deployment. It is *kept* as the answer for the command-line tool, and that
retention is a decision rather than an absence: the two roots are separate binaries, and the tool's
pipe surface stays single-user on purpose.

### The strongest argument against shape 1, recorded where the decision is taken

Every limit below has its own bullet in *What this decision does NOT cover*, and stating them one
at a time understates them, so the composite goes here: **shape 1 produces a surface that reads as
multi-player before it is one.** A deployment on it verifies a caller, names that caller in every
audit record, records which posture each leg ran under - and, because leg 2 has never run against a
real token service, may still read every row as one identity. That is `.agents/skills/sutura/identity/SKILL.md`'s
own framing, and it is a sharper risk on the agent surface than on HTTP, because the reader is a
model summarising an answer rather than an operator reading a status line.

It is not a reason to choose shape 2, which cannot be multi-player at all. It is the reason
`telekom/sutura#376` sits at step two of the ordering below rather than after the wiring, and the
reason the acceptance bar refuses rather than serves. **The mitigation is the ordering and the
refusal; there is no mechanism that stops a deployment being stood up in between**, and that is a
limit of this decision rather than an argument against it.

## The load-bearing question, and its answer

The choice above is only available if the pinned SDK's HTTP server transport can carry an inbound
header to a request handler. This repository's rule is to verify external behaviour rather than
assert it, so what follows was read out of the vendored source of the pinned version on 2026-09-06.

**Pinned:** `Cargo.toml:472` - `rmcp = { version = "3.1", default-features = false, features = ["server", "transport-io"] }`,
resolving to `3.1.4` in `Cargo.lock`. **No HTTP transport feature is enabled today**, which is why
this is a question at all.

### The server transports the pinned version offers

Read off the vendored `src/transport.rs` module declarations of `rmcp` 3.1.4, which is the list that
compiles rather than the list of files: `sink_stream`, `async_rw`, `worker`, `child_process`, `io`
(the pipes, today's choice), `auth` (client-side), `streamable_http_client` and
`streamable_http_server`. **There is exactly one HTTP server transport** - Streamable HTTP, whose
`StreamableHttpService` is behind `#[cfg(all(feature = "transport-streamable-http-server", not(feature = "local")))]`.
There is no server-side SSE transport in this version.

**One file in that directory is not a transport and would be misread as one.** `src/transport/ws.rs`
contains a single line - `// Maybe we don't really need a ws implementation?` - and `transport.rs`
declares it commented out:

```rust
// #[cfg(feature = "transport-ws")]
// pub mod ws;
```

Reading the directory listing rather than the module declarations would have counted a WebSocket
transport this version does not have. That is the general trap: a file is not a module.

### An inbound header does reach a handler, and this is the whole path

The service consumes the body and keeps the rest of the request. In
`src/transport/streamable_http_server/tower.rs`:

```rust
/// The service consumes the request body but injects the remaining
/// [`http::request::Parts`] into [`crate::model::Extensions`], which is
/// accessible through [`crate::service::RequestContext`].
```

That is documentation; the code is beside it, and it is done on every message rather than only on
the handshake. Same file, the session path:

```rust
// inject request part to extensions
match &mut message {
    ClientJsonRpcMessage::Request(req) => {
        req.request.extensions_mut().insert(part);
    }
    ClientJsonRpcMessage::Notification(not) => {
        not.notification.extensions_mut().insert(part);
    }
    _ => {
        // skip
    }
}
```

and the stateless path does the same thing:

```rust
let peer_info = Self::peer_info_for_stateless_request(&request, &parts.headers);
request.request.extensions_mut().insert(parts);
```

Those extensions become the handler's context. In `src/service.rs`:

```rust
pub struct RequestContext<R: ServiceRole> {
    pub ct: CancellationToken,
    pub id: RequestId,
    pub meta: RequestMetaObject,
    pub extensions: Extensions,
    pub peer: Peer<R>,
}
```

filled, in the same file's serve loop, by moving the message's own extensions across:

```rust
let mut extensions = Extensions::new();
let mut meta = RequestMetaObject::new();
std::mem::swap(&mut meta, request.get_meta_mut());
std::mem::swap(&mut extensions, request.extensions_mut());
let context = RequestContext { ct: context_ct, id: id.clone(), peer: peer.clone(), meta, extensions };
```

and read by `Extensions::get`, in `src/model/extension.rs`:

```rust
pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> { /* … */ }
```

`crates/sutura-mcp/src/server.rs:305` already takes that value and discards it:
`async fn call_tool(&self, request: CallToolRequestParams, _context: RequestContext<RoleServer>)`.
So the parameter the principal would arrive through **already exists on the handler**; under the
pipe it is empty, which is why it is bound to `_context` and why that binding is correct today.

**Answer: yes.** Under the pinned version's HTTP server transport, a handler can read
`context.extensions.get::<http::request::Parts>()` and reach `parts.headers`, per request, with no
macro and no SDK feature beyond the transport itself.

### It composes with the router this repository already builds

`crates/sutura-http/src/router.rs:180` returns a `Result<Router, RouterNotBuilt>` over `axum`'s own
`Router`. The SDK's service is a
`tower_service::Service`:

```rust
impl<RequestBody, S, M> tower_service::Service<Request<RequestBody>> for StreamableHttpService<S, M> {
    type Response = BoxResponse;   // = http::Response<BoxBody<Bytes, Infallible>>
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;
```

(`BoxResponse` from `src/transport/common/server_side_http.rs:22`), with a hand-written
unconditional `Clone`. `axum` 0.8.9's `Router::nest_service`
(`axum-0.8.9/src/routing/mod.rs:235-240`) asks for three bounds, and all three are named here
because omitting one is how a composition claim goes wrong:
`T: Service<Request, Error = Infallible> + Clone + Send + Sync + 'static`,
`T::Response: IntoResponse`, and `T::Future: Send + 'static`. The response bound is met by
`axum-core` 0.5.6's
`impl<B> IntoResponse for Response<B> where B: http_body::Body<Data = Bytes> + Send + 'static, B::Error: Into<BoxError>`,
which `BoxBody<Bytes, Infallible>` satisfies; the future is already `BoxFuture<'static, _>`. **`Sync`
is the bound nothing in the SDK states**, and it holds by construction rather than by assertion:
`StreamableHttpService`'s fields (`tower.rs:999-1014`) are the `Clone`-derived
`StreamableHttpServerConfig` plus four `Arc`s, so the type is `Sync` whenever its `S` and `M` are.
`Cargo.lock` holds one `http` (1.5.0), one `http-body` (1.1.0) and one `tower-service` (0.3.3), all
already shared with `axum` - so this is a composition, not a type boundary in the sense
`.agents/skills/sutura/dependencies/SKILL.md` separates from a duplicate.

**The limit on this whole subsection, stated beside the claim rather than left to the pull request
that carried it:** every line above is a **source read of the pinned artefact, not a build.**
Nothing in this tree has been compiled against `transport-streamable-http-server`, so *it composes*
is a type-level argument from the two crates' own signatures and not a green `just validate`. The
first wiring change is where that becomes a measurement, and it may find something these signatures
do not show.

### What turning the feature on costs

`transport-streamable-http-server` pulls **three** features, not two -
`transport-streamable-http-server-session`, `server-side-http` and `transport-worker` - which
between them name `uuid`, `rand`, `tokio-stream`, `http`, `http-body`, `http-body-util`, `bytes`,
`sse-stream`, `base64`, `tower` and `async-trait`. (`transport-worker` names only `tokio-stream`,
already in that list, so the closure is wider than it first reads and the package set is not.
`tower` in that list is the SDK's own FEATURE name, which enables `dep:tower-service` - it is not
the `tower` package.) Counted against `Cargo.lock` at `5ae3bde` with
`grep -c '^name = "<crate>"$'`: **ten of the eleven already resolve; `sse-stream` does not**, and
its own transitive tree is unmeasured here because measuring it needs a resolve. So the honest
statement of the cost is *at least one new package, and an unknown tail below it*.

It pulls **no HTTP client**: `server-side-http` names neither `reqwest` nor `oauth2`, and neither
resolves in `Cargo.lock` today. **No gate holds that**, and saying otherwise would be the defect
`.agents/skills/sutura/invariants/SKILL.md` warns about. `cargo xtask check-shared-client` is not
it: it reads exactly two facts, both about `ureq` - that `Cargo.lock` holds one version of it, and
that `libduckdb-sys` still depends on that version - because it exists to protect
[0018](0018-what-the-bigquery-wire-is-built-from.md)'s *plus zero packages* measurement. **A
`reqwest` arriving here would not trip it.** The no-client property above is a measurement of this
transport's feature closure on 2026-09-06 and is held by review. Whether adding `sse-stream` is
acceptable at all is a dependency decision under `.agents/skills/sutura/dependencies/SKILL.md`,
taken with the resolve in hand, not here.

### What the SDK does not give, and this deployment must

`rmcp` 3.1.4 does know about `WWW-Authenticate` - in **seven** files, `src/transport/auth.rs`
among them - and **every one of them is client-side**: `service/client.rs`,
`transport/auth.rs`, `transport/common/auth/streamable_http_client.rs`,
`transport/common/http_header.rs`, `transport/common/reqwest/streamable_http_client.rs`,
`transport/common/unix_socket.rs`, `transport/streamable_http_client.rs`. Protected-resource
metadata is in `transport/auth.rs` alone, behind an `auth` feature pulling an OAuth client and an
HTTP client.

**The sharper evidence is what the server transport can answer at all.** Its complete status-code
set, read off `src/transport/streamable_http_server/` and `src/transport/common/server_side_http.rs`
with `grep -oh 'StatusCode::[A-Z_]*' | sort -u`, is: `ACCEPTED`, `BAD_REQUEST`, `FORBIDDEN`,
`INTERNAL_SERVER_ERROR`, `METHOD_NOT_ALLOWED`, `NOT_ACCEPTABLE`, `NOT_FOUND`, `OK`,
`PAYLOAD_TOO_LARGE`, `UNPROCESSABLE_ENTITY`, `UNSUPPORTED_MEDIA_TYPE`. **`UNAUTHORIZED` is not in
it.** A transport that cannot spell 401 cannot issue the challenge that goes with it, so the 401,
the challenge and any discovery document are this repository's own work - which is the right side
of the line anyway, because what decides them is
[0014](0014-how-a-caller-proves-who-it-is.md)'s gate and not a transport's.

**A second thing the SDK does not do, and it is the sharper one: it cannot record who opened a
session, structurally.** `SessionManager::create_session(&self)`
(`src/transport/streamable_http_server/session.rs:96-98`) **takes no arguments at all**, so there
is no parameter an identity could arrive through; every later lookup is
`has_session(&self, id: &SessionId)` (`:108`), an existence test on the header and nothing more.
Grepping the whole server transport for `bearer|authorization|principal|auth_` returns **zero**
hits, and for `identity` exactly two, both the type name `TransportAdapterIdentity`. So a later
request on an established `Mcp-Session-Id` may carry a different bearer, or none, and the SDK will
not notice.

**The trap that follows, named because it is the one a later implementer falls into.** In session
mode the handler is built **once per session** - `service_factory` (`tower.rs:999-1014`) called
through `get_service` (`tower.rs:1107-1109`) - so an identity stored as a *field on the handler* is
per-session by construction and will look correct in every single-caller test. The identity must
therefore be read **per request from the parts**, and a request whose parts carry no verified
caller must be refused rather than inheriting whoever opened the session.

**`never cached` above is about the IDENTITY, and it decides nothing about the CREDENTIAL.** The
two are different questions and conflating them would foreclose one of them by accident. Who is
asking is re-established from the request every time, because the transport gives no other honest
answer. What was *minted* for that subject is a separate decision, and it is open:
`telekom/sutura#381` records that `CredentialBroker::mint` runs once per accepted question with no
cache, no pool and no per-subject reuse, which a chain multiplies to N sources by M hops on the
synchronous path of every answer. That issue's own invariant is the one to carry here: **a cache
keyed on anything less than the whole verified input is a cross-subject leak** - subject, audience,
scope and, for a chain, the entire ordered hop sequence, and never the session, the connection or
the process, which is exactly what the SDK's session blindness above makes unsafe. And the line
that does not move: **the source is the backstop for a credential, and there is none for data**, so
caching rows is not on the table at any point, and a cross-user leak never is.

## What this decision does NOT cover

The list is the point of the record, and every entry is a thing a reader could otherwise take
this decision to have settled.

- **It does not deliver leg 2 on any surface.** Leg 1 - knowing who is asking - is built on HTTP.
  Leg 2 - a source executing *as* them - is wired in the server binary and **has never run against
  a real token service**. This decision moves the agent surface onto a transport where leg 1 can
  reach it. It proves nothing about rows.
- **It is not a claim that two callers get two answers.** Nothing in the default suite asserts
  that two subjects read two different row sets: the scaffold for it is
  `two_subjects_with_different_grants_read_two_different_row_sets` at
  `crates/sutura-exec-bigquery/tests/acceptance.rs:768`, `#[ignore]`d and needing a real project.
  This record changes none of that.
- **It does not decide the exchange chain.** See the next section: one hop is built, and the chain
  the requirement asks for is a separate design with its own failure modes.
- **It does not decide whether an exchanged credential may be reused.** Re-establishing the
  identity from each request is settled here; caching what was *minted* for that subject is
  `telekom/sutura#381` and is untouched by this record.
- **It does not decide the dependency question.** Enabling the transport feature adds at least one
  package to the lock. That is taken under the dependencies skill, with a resolve.
- **It does not touch the catalog.** `telekom/sutura#148` - every token holder sees the whole
  bundle - is the metadata half of the same multi-player problem and is independent of every step
  here: no identity provider, no dataset and no round-trip to a cloud. Impersonation at the data
  layer does not narrow a catalog everyone reads whole.
- **It does not retire the pipe.** The command-line tool keeps its single-user surface, with the
  launching identity as the subject, stated as a decision.
- **It does not make anything published able to impersonate.** No published artefact of either
  binary links the dataset adapter (`nix/shipped.nix`, and
  [0017](0017-what-a-bigquery-test-runs-against.md)'s record of the feature being default-off), so
  the whole of this is a build-from-source capability until that trade is taken separately.

## The exchange is a chain, and exactly one hop is built

The requirement this record serves is that the agent surface takes the caller's token from
whatever enterprise identity provider issued it and reaches a credential the data source accepts,
through **however many exchanges that takes**.

Measured at `5ae3bde`: the broker performs one. `crates/sutura-exec-bigquery/src/sts.rs:344`:

```rust
let credential = self
    .exchange
    .exchange(workload.audience(), workload.scope(), assertion)
    .map_err(|cause| ExchangeUnusable::Provider { cause: Box::new(cause) })?;
```

against the port at `crates/sutura-exec-bigquery/src/sts.rs:111`,
`fn exchange(&self, audience: &str, scope: &str, subject_token: &Secret) -> Result<StsCredential, Self::Error>`,
whose one non-test implementor is `StsOverHttp` at `crates/sutura-exec-bigquery/src/wire/sts.rs:95`.
There is no second hop and nowhere to configure one:
`git grep -n -i 'audience\|scope' origin/main -- crates/sutura-config/src/credentials.rs` returns
nothing. Nor is any enterprise provider wired -
`git grep -n -i 'entra\|azure\|keycloak' origin/main -- crates/sutura-config/src crates/sutura-http/src`
returns nothing.

**Four things a chain changes that are not "loop N times", recorded now so they are not discovered
later:**

1. **Per-hop audience and scope, with a bad chain unrepresentable rather than rejected.** One
   `WorkloadIdentity` carries one audience/scope pair. A chain needs an ordered sequence, and the
   configuration must make an out-of-order or incomplete sequence impossible to *write*, in the
   sense the newtype rules in `.agents/skills/sutura/secure-by-design/SKILL.md` mean it - not a
   validation pass over a `Vec` that a later caller can skip.
2. **A hop refuses; it never falls through.** The single hop already answers `Minted::Refused`
   when the workload or the assertion is absent. Every hop must keep that, and a failed hop must
   never leave an earlier hop's credential presented - the shape where a caller is served under an
   intermediate identity is worse than a refusal and reads identically in a log that only records
   success.
3. **The deadline and [0008](0008-a-credential-per-leg-for-the-calling-subject.md) part 6's floor
   fold across all hops.** Today `Expiry::earliest` folds across sources and `with_floor` refuses a
   credential already inside the floor. With N hops the floor applies to the **final** credential,
   and an intermediate that would expire mid-chain has to refuse rather than be presented. That is
   a failure mode with no analogue in the current code.
4. **Which hop failed has to reach the record.** `ExchangeUnusable::Provider` boxes one cause and
   names neither the hop nor the source. An operator cannot tell a provider outage from a
   misconfigured audience from that, and a chain multiplies the ways it is ambiguous.

## A naming collision, stopped here on purpose

`crates/sutura-http/src/identity_e2e.rs:109` and `:127` call `context.chain()`, and
`telekom/sutura#252` landed under the title *the exchange chain, joined through the transport*.
**That `chain` is the identity context** - who asked, and through which transport - and what #252
joined is transport → broker → exchange: the two halves of **one** hop. The same file's own module
documentation says what was missing was that *"nothing joined the two halves of the exchange
chain"*.

It is not a multi-hop token chain, and a reader who takes `chain()` for N hops will be wrong about
how far this tree is. The word is load-bearing in two different senses in two adjacent places, and
this paragraph is the cheapest available fix.

## The acceptance bar, and it does not soften

**A request with no identity, or one whose exchange fails, must be refused - never served under
the process's own identity.** Failing open there does not produce an error a caller can see; it
produces an answer, under the wrong identity, that reads exactly like a correct one. That is the
worst outcome available on this surface, and it is worse than the surface not existing. Fail-open
is not hypothetical in this repository either - the count of gates found failing open in the days
around this record, and its date, are in `telekom/sutura#378`'s decision comment rather than
measured here.

Concretely, three cases and one answer each:

| Case                                                                | Answer                                                                    |
| ------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| no verified caller on the request                                   | refuse; never the deployment's own identity                               |
| a verified caller, exchange refused by the provider                 | refuse; never an earlier hop's credential and never the source's own      |
| a verified caller, no workload declared for an impersonating source | refuse as `credential_unavailable`, which is the port's existing fallback |

## Ordering, fixed

1. **The transport - this record.** Not negotiable as step one: the pipe has no per-request
   principal, so no amount of exchange machinery makes the agent surface multi-user.
2. **Verify the existing single hop against a real token service** (`telekom/sutura#376`). A chain
   built on an unverified hop proves nothing.
3. **An issuer-parameterised harness for an enterprise provider** (`telekom/sutura#105`), which is
   where a chain's first hop gets a real issuer to verify against.
4. **The chain**, with the four changes above designed rather than grown.

**Beside step 4 rather than after it: `telekom/sutura#381`, the credential cache.** A chain turns
one exchange per question into N sources by M hops on the synchronous path of every answer, so the
cache stops being an optimisation and becomes part of the chain's design - which is why it is not a
fifth step. Nothing in this record asserts a per-request cost, so #381 contradicts none of it; what
this record owes it is the boundary drawn above, that re-establishing the *identity* per request
decides nothing about reusing a *credential*.

`telekom/sutura#148` sits outside that order entirely and is the cheapest real multi-player step
available, because it needs none of the four.

## Alternatives considered

**Keep the pipe and treat the launching identity as the subject.** Rejected as the served answer,
kept as the tool's. It is honest and it is free, and it has one property that ends the argument: a
process serving many people cannot be built out of it. The value in having considered it is that
it is *already implemented*, so the cost of choosing it was zero and it still lost - which is the
strongest available statement that the transport is the constraint rather than the effort.

**Carry the caller in a tool argument.** Rejected on the same ground
[0014](0014-how-a-caller-proves-who-it-is.md) rejects it for the HTTP surface, and it is
structural here rather than a rule: `sutura_domain::identity::RequestContext` implements no
`Deserialize`, so a caller cannot state its own chain. A field on the wire type would be a caller
asserting an identity, which is the thing the whole design refuses.

**Put the identity on the handshake and cache it per session.** Rejected on the measurement above:
the SDK binds no session to the identity that opened it, so a cached principal would be presented
for a later request that carried a different bearer or none. Per request, from the parts, or not
at all.

**Wait for the chain, then do the transport.** Rejected on ordering. Without a per-request
principal there is nothing for a chain's first hop to consume, so building the chain first
produces machinery whose input does not exist.
