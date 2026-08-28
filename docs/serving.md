# Serving over HTTP

`sutura-serve` answers certified questions over HTTP. It is a **second binary**, separate from the
`sutura` command-line tool, and the release artifacts do not contain it: the shipped image holds one
executable with no server in it, and putting an async runtime, an I/O driver and a web framework
into all four cross-compiled targets is a decision rather than a side effect.

## Read this part first

**There is no per-caller identity.** No request context reaches the query path, no credential is
minted per request, and the port that would do it is deliberately absent, because in this repository
a port arrives with the adapter that implements it. `AGENTS.md` records "every query runs as the
calling principal" as an aspiration that is *not mechanised*, and the multi-player example explains
why single player makes it trivially true and worth nothing.

So an HTTP endpoint that answers catalog questions is, today, a way to read whatever the process can
read. Where an access token is configured, presenting it proves the caller holds a secret an
operator wrote down - it authenticates the **deployment**, not the caller. It cannot be scoped to a
subset of the catalog, it cannot be revoked for one party without revoking it for all of them, and it
does not reach the data system. Every question is answered with whatever access the service process
already had, whoever asked it.

That sentence is printed at `WARN` on every boot and is in the generated interface description, so an
operator and an integrator both meet it without reading this page.

Rate limiting is not authentication either. It bounds how fast something can be done, not who may do
it, and the bucket it counts against is a network address rather than a principal.

## What it will not start with

Every one of these is a **refusal to start**, not a warning. A warning is read by whoever happens to
be looking at the log in the format the collector was configured for; a process that does not start
is read by everybody. Every refusal is reported at once, so a fix-and-restart loop does not surface
them one at a time.

| Configuration | Why it refuses |
| --- | --- |
| a bind address other hosts can reach, without `security.tls_termination` declared | with no per-caller identity the bind address is the whole perimeter, and the bearer token crosses whatever hop is in front. Saying which thing terminates TLS is how the cleartext segment becomes a stated fact rather than an assumption. Applies in *every* environment, including a laptop. See [TLS](#tls) for the four answers |
| no `security.access_token`, in production or on a non-loopback bind | the alternative is an unauthenticated way to read whatever the process can read |
| an explicit `rate_limit.enabled: false` in production | one question is an aggregate over up to ten years of history, so an unbounded caller is an unbounded load on the data system |
| `server.port: 0` in production | that asks the kernel for an ephemeral port, so nothing can be configured to reach the service |
| an unknown `SUTURA_ENVIRONMENT` | a typo would otherwise select the permissive branch of every decision above |
| any malformed or misspelled configuration key | a key that is silently ignored is a default the operator believes they overrode |

The checks read the **loaded** values, not any one file. The environment-variable layer is applied
last, so a check against a file would be checking something the process is not running on.

## The endpoints

| Method and path | Token | What it is |
| --- | --- | --- |
| `GET /health` | no | Liveness. The body is exactly `{"status":"ok"}` |
| `GET /v1/catalog` | yes, when one is configured | The metrics this catalog defines, with grains, dimensions and the values a filter may use |
| `POST /v1/query` | yes, when one is configured | One certified question. `200` for both outcomes; `503 at_capacity` when no execution slot is free - see [Capacity](#capacity) |
| `GET /openapi.json` | yes, when one is configured | The generated interface description |
| `GET /docs` | yes, when one is configured | A browser interface over that description |

`/health` is outside the version prefix on purpose: a probe must keep working across a version bump
without an orchestrator being reconfigured. It carries no version, no build identifier, no
dependency list, no configuration and no catalog content, because it is the one path an
unauthenticated caller can always reach - so every field it might have is a field handed to anybody
who can route a packet.

The interface description is served everywhere except production, where it is off by default. It
describes the surface, which is business information even with no row of data in it.

### A refusal is a `200`

`POST /v1/query` answers `200` for both outcomes, and the `outcome` field is what a caller branches
on:

```json
{
  "outcome": "answer",
  "provenance": {
    "definition_version": "local-1",
    "definition_digest": "5de2c383b783698082a9e8142a1d032bbc014fe457da9126109df6dd03777e3b"
  },
  "columns": ["period", "recurring_revenue"],
  "rows": [
    ["2026-01-01", "237320"], ["2026-02-01", "232822"], ["2026-03-01", "216700"],
    ["2026-04-01", "206160"], ["2026-05-01", "202994"], ["2026-06-01", "202121"]
  ]
}
```

```json
{
  "outcome": "refusal",
  "reason": { "code": "metric_unknown", "detail": "this catalog defines no metric called `customer_lifetime_value`" }
}
```

Both of those are `examples/single-player` over the wire, each captured as one line of JSON and
reformatted here. `examples/single-player/README.md` has the whole session: the startup output,
the token gate, the liveness probe, the interface description and a refusal to start.

A refusal is a *result*: the caller asked something they may not have, and the answer is no. An error
status would invite a client library to retry, and retrying a governance decision until it succeeds
is precisely the behaviour the refusal exists to prevent. See [Questions and answers](qa.md) for what
is refused and why.

Cells are rendered as text rather than as JSON numbers. A measure over integer minor units does not
survive a round trip through a JSON number in every client, and an anchor is compared as text - one
rendering everywhere means the number in an answer is the number in the anchor that certified it.

A body carrying `sql`, `table` or any other key the question shape does not declare is a `400`
naming the field. Without that, the key would be dropped silently and a caller who believed they sent
SQL would be answered as though they had asked the modelled question instead.

Every other failure is one shape:

```json
{ "code": "unauthorized", "status": 401, "detail": "this service requires a bearer token" }
```

A `500` carries no detail, ever. The text of an internal error is a path, a table name, a column name
or a driver message, and any of those handed to a caller describes the deployment.

Two failures share the `503` status and differ in `code`, which is what a client branches on:
`unavailable` is a data system that did not answer, and `at_capacity` is this service having no
execution slot free - see [Capacity](#capacity). Both are worth retrying, and they are diagnosed in
completely different places.

## Capacity

**Three numbers, and they bound three different things.** The one to read first is that none of them
cancels a question that has started.

| Key | Default | What it bounds |
| --- | --- | --- |
| `runtime.max_concurrent_queries` | `8` | How many questions are **executing** at once. At most 512 |
| `runtime.admission_timeout_seconds` | `5` | How long a question waits for a slot before it is shed. At most 300 |
| `runtime.engine_worker_threads` | as many as the machine reports | How wide the in-process engine's own runtime is. At most 256 |

### Why a bound on execution exists at all

`server.request_timeout_seconds` is a deadline on the **reply**, not on the work. When it expires the
caller gets a `408` and the request handler is dropped - and the question keeps running, because the
`Warehouse` port is synchronous and the task it runs on cannot be aborted. So without a bound on
execution, a caller asking questions that cost more than the timeout gets a fast turnaround while the
deployment keeps the whole cost, and the work accumulates at whatever rate the limiter allows. The
only real limit was memory, and that also defeats the bounded stop below: a process cannot stop while
it is waiting for work nobody can cancel.

`max_concurrent_queries` is that bound. A question holds its slot from the moment it starts until the
data system answers it - **not** until the caller is answered. That is the part that makes the number
mean something: a timed-out request does not hand its slot back early, so the backlog is a number
somebody chose rather than however much memory there is.

A question that cannot get a slot inside `admission_timeout_seconds` is answered `503` with
`code: at_capacity` and a `Retry-After` in seconds, rather than being left in a queue. `503` and not
`429` on purpose: a `429` says "you personally asked too often", which is a claim about the caller
and is the one the rate limiter already makes. This one is about the deployment, and it is true
whoever asked - a caller well inside their own rate limit can meet it.

The admission window is deliberately shorter than the request timeout. A caller who has waited five
seconds for a slot is better served by a `503` they can retry than by a `408` twenty-five seconds
later that says the same thing less clearly. Setting it *above* the request timeout is allowed and
does nothing: the timeout layer answers first.

### What it does not bound

Stated plainly, because each of these has been mistaken for the thing above.

- **It does not cancel anything.** A question that has started runs to completion, holding its slot,
  whatever the caller was told. Cancelling it needs a cancellation token the `Warehouse` port does
  not have, and adding one is a change to every adapter.
- **It does not bound how long one question takes.** One question that runs for an hour holds its
  slot for an hour.
- **It is not a per-caller budget.** One caller can fill every slot and shed everybody else, and
  nothing can tell two callers apart, because there is no per-caller identity - see the first
  section. The limiter bounds an address's *rate*; this bounds the deployment's *concurrency*.
- **It does not reach inside the engine.** The in-process engine has its own blocking thread pool at
  the runtime default, which nothing here sizes.

### The engine's width

The engine drives its own runtime and every request blocks on it from a pool thread, so how wide that
runtime is decides whether concurrent questions actually run concurrently. It used to be one thread,
which was right when the only caller was a command-line tool answering one question and is a ceiling
for a server. Measured - twenty questions per caller over a million rows, sixteen-way host,
throughput normalised to one caller on the old runtime:

| callers | one thread | `engine_worker_threads = callers` |
| --- | --- | --- |
| 1 | 1.00x | 1.07x |
| 2 | 1.02x | 2.08x |
| 4 | 1.03x | 3.94x |
| 8 | 1.01x | 6.06x |

The first column is the point: it is flat. A single-threaded engine runtime does not scale with
callers at all on this workload.

An absent key means "as many threads as this machine reports", resolved to a number at load time so
the startup log prints what is in effect rather than a policy. **A container with a CPU quota should
set it explicitly:** `available_parallelism` reports what the kernel exposes, which on most container
runtimes is the host's core count rather than the cgroup's share - so the default is usually too wide
there, and too wide costs memory as well as scheduling. The number also pins the engine's partition
count, so a narrow runtime does not build wide plans it then executes a few at a time.

The command-line tool is unaffected: it answers one question and exits, and one thread is the right
runtime for that.

## Configuration

Four layers, later beating earlier:

1. the defaults compiled into the binary - complete, so a deployment with no files is a working
   loopback development service rather than a failure;
2. `<dir>/base.yaml`, if `SUTURA_CONFIG_DIR` names a directory holding one;
3. `<dir>/<environment>.yaml`;
4. one environment variable per key: `SUTURA__SERVER__PORT` sets `server.port`.

Every layer is checked with `deny_unknown_fields` at every depth.

The environment is chosen by `SUTURA_ENVIRONMENT` and by nothing else - one of `development`, `test`
or `production`, defaulting to `development`. It is deliberately **not** a configuration key: it
selects which file is layered, so a file that could change it would be self-referential. Both
`environment:` in a file and `SUTURA__ENVIRONMENT` in the shell are unknown-key errors.

| Key | Default | Notes |
| --- | --- | --- |
| `server.host` | `127.0.0.1` | An IP address, never a hostname: a name resolves to whatever the resolver says today. Either family - `::1` and `[::1]` are both read. See [Address families](#address-families) |
| `server.port` | `8080` | |
| `server.request_timeout_seconds` | `30` | At most 300 |
| `server.max_body_bytes` | `65536` | At most one mebibyte. A question is a few hundred bytes |
| `security.access_token` | absent | An RFC 6750 `b64token`, at least 32 characters. Required in production and on a non-loopback bind |
| `security.tls_termination` | `none` | One of `none`, `sidecar`, `ingress`, `in-process`. Must be declared for any bind other hosts can reach |
| `server.tls_certificate` | absent | A PEM chain. Only with `tls_termination: in-process` |
| `server.tls_key` | absent | The matching PEM private key. Both halves or neither |
| `rate_limit.enabled` | follows the environment | Off in development and test, on in production. `false` in production is refused |
| `rate_limit.probe_per_second` | `2` | Liveness and the interface description |
| `rate_limit.probe_burst` | `5` | |
| `rate_limit.api_per_second` | `10` | The versioned API |
| `rate_limit.api_burst` | `20` | |
| `rate_limit.client_address` | `peer` | `peer` or `forwarded`. What a rate-limit bucket is counted against |
| `rate_limit.trusted_proxies` | empty | The hops whose `X-Forwarded-For` is believed. `forwarded` with this empty is refused |
| `telemetry.service_name` | `sutura` | What a collector groups by |
| `telemetry.filter` | `info` | `RUST_LOG` overrides it when set |
| `telemetry.format` | follows the environment | `bunyan` in production, `pretty` elsewhere |
| `api.docs` | follows the environment | Off in production, on elsewhere |
| `catalog.dir` | `catalog` | |
| `catalog.data_dir` | `data` | Where the engine's CSV and Parquet files are |
| `catalog.version` | `unversioned` | A commit id or a build number. What identifies the snapshot |
| `runtime.max_concurrent_queries` | `8` | How many questions execute at once. See [Capacity](#capacity) |
| `runtime.admission_timeout_seconds` | `5` | How long one waits for a slot before it is shed `503` |
| `runtime.engine_worker_threads` | the machine's | How wide the in-process engine runs. Set it under a CPU quota |
| `runtime.shutdown_grace_seconds` | `15` | The budget for the whole of stopping. See [Stopping](#stopping) |

A zero is refused wherever it would read as "no limit", and every bound has a ceiling, because a
value nobody chose is worse than a value somebody has to argue with.

### Address families

`server.host` takes an address of either family. `127.0.0.1` and `::1` are both recognised as
loopback, so neither of them trips the off-host refusal above, and `::1` may be written bracketed or
bare. A rate-limit bucket is keyed on the canonical form of the address, so a client reaching a
dual-stack listener as `::ffff:1.2.3.4` shares the bucket of the same client reaching it as
`1.2.3.4` rather than getting a second one - and a v4 entry in `rate_limit.trusted_proxies` still
matches a v4-mapped peer, while never matching a real v6 address.

**Which families a listener actually accepts is the platform's default, not a decision this service
makes, and there is no key for it.** Nothing sets `IPV6_V6ONLY` either way. On Linux, whose default
is off, binding `::` accepts v4 connections too and reports them as v4-mapped; binding `0.0.0.0`
accepts v4 only. So an operator who wants both families gets them from a platform default rather than
by choosing them, and an operator who wants v6 *only* has no way to say so - they would have to set
the socket option outside this process. That is a posture nobody chose, stated here rather than left
to be discovered; a `server.address_family` key is where it would be fixed.

*Not observed end to end: the v6 serving path is covered by the address-parsing tests and by reading
`tokio::net::TcpListener::bind`, and by nothing that has actually accepted a v6 connection - the
development container has no v6 address at all. A test that skipped itself on a host without v6 would
read as coverage and is deliberately not here.*

## TLS

**Normally something else terminates it, and that is the intended arrangement rather than a
shortcut.** In a cluster an ingress controller or a sidecar ends the connection and the hop from
there to this process is plaintext on the pod network. So a non-loopback bind is *not* refused for
being plaintext; what is refused is a non-loopback bind that has not said where TLS is terminated.
`security.tls_termination` is that statement, and the point of writing it down is that the cleartext
hop it implies becomes a stated fact rather than an assumption - the bearer token crosses that hop.

| Declared | What terminates TLS | What the token crosses in cleartext |
| --- | --- | --- |
| `none` | nothing | the whole path from the caller. Only sane on loopback |
| `sidecar` | a proxy in this pod | a loopback hop inside the pod |
| `ingress` | an ingress controller or gateway | the pod network, from that hop to this process |
| `in-process` | this process | nothing. The connection ends here |

`ingress` is therefore not a weaker `sidecar`: it is the same posture with a longer cleartext
segment, and whether that segment is acceptable is a question about the cluster network. A mesh with
mutual TLS between pods answers it differently from a flat one, and nothing here pretends to know.

### Terminating it in this process

For the deployment where nothing sits in front. It needs **a build that has a TLS listener in it**,
which the default build does not:

```bash
cargo run -p sutura-serve --features tls
```

The feature is default-off because most deployments do not use it, and a TLS stack compiled into an
artifact that will never present a certificate is cost with no return. With the feature off the
dependency is absent from the build rather than merely unused, and asking for `in-process`
termination is a startup refusal that names the feature - so the two cannot disagree.

Then:

```yaml
security:
  tls_termination: "in-process"
server:
  tls_certificate: "/tls/chain.pem"
  tls_key: "/tls/key.pem"
```

Both halves or neither. A certificate with no key is refused rather than half-configured, and a path
set to the empty string is an error naming the key rather than TLS quietly switching itself off -
an empty string is what an unset variable looks like in a shell.

The material is read and validated **before the socket is bound**: the chain must parse, the key must
parse, and the key's public half must match the certificate's. That last check is the one rustls does
not make on your behalf - a mismatched pair builds a server configuration quite happily and then
fails every handshake, at the client, with a signature error that names no file. So a configuration
mistake here is a process that does not start, and there is **no fallback to plaintext**: a port
somebody configured to be encrypted never comes up unencrypted.

rustls, not OpenSSL. No system library and no C toolchain requirement beyond what the build already
has, which is what keeps the statically linked targets buildable - there is no musl OpenSSL in
nixpkgs.

### Certificate renewal without dropping connections

A renewed certificate does not need a restart. The certificate and key paths are re-read on an
interval; when the bytes change, a candidate pair is built and validated in full, and only then does
it become what new handshakes are offered. A handshake already in flight keeps the pair it resolved,
so nothing in progress is disturbed and no connection is dropped.

**A bad new pair does not take the listener down.** If the replacement will not parse, or its key
does not match its certificate, it is logged at `error` naming both paths and discarded - the
listener keeps serving what it was already serving. Reloading into a broken state would be worse than
not reloading at all: every new connection would fail and the working pair would be gone.

Polling rather than a filesystem watch, and deliberately. Kubernetes replaces a projected Secret by
building a new directory and swapping a symlink, so an `inotify` watch on the file path follows the
old inode and never fires; getting that right means watching the directory and interpreting rename
events. Reading the path answers the question with no cases. Comparing the file *contents* rather
than a timestamp is the same choice again: an `mtime` a writer preserved is a rotation that never
happened. The cost is bounded staleness - up to the poll interval between the write and the swap -
which for something an issuer plans days ahead is nothing.

The arrangement this expects is the one most clusters already run: an external certificate manager
owns renewal and writes the files, and this process follows them. There is **no ACME client here**,
and that is a judgement rather than a gap - see below.

### No ACME, and why

`rustls-acme` would do TLS-ALPN-01 with automatic renewal, which sounds like exactly this
requirement. It only makes sense when this process is the edge:

- TLS-ALPN-01 is validated by the certificate authority connecting to **port 443 of the name being
  issued for**. A pod behind an ingress controller is not what answers that connection - the
  ingress is - so the challenge cannot complete. In the deployment this service is normally in, an
  ACME client here would fail every renewal.
- It wants to own the listener, offering its own accept loop. That is the same objection recorded
  against `axum-server` in `crates/sutura-http/src/tls.rs`: this surface has a bounded drain on
  shutdown, and a second serving implementation would mean two drains to keep in agreement.
- Where this process *is* the edge, the ingress that would have terminated TLS is usually also the
  thing that would have obtained the certificate, so the deployments that could use ACME are the
  small ones - which are also the ones where a manually issued pair is least painful.

So the recommendation is file-watch reload plus an external issuer, which is what is built. ACME
belongs behind a *second* feature if it is ever wanted, gated on this process being the edge, and it
should not be the default.

## The log

One decision with two right answers, made from the environment and nothing else. In production a log
line is read by a collector, so it is one JSON object per line in the bunyan schema with the span
context attached; on a laptop the same line is read by a person recompiling every thirty seconds, so
it is indented and coloured. An explicit `telemetry.format` overrides it, and the startup log says
which of the two happened.

On boot the process prints an ASCII banner and the build line to standard output - before any
subscriber exists, so it is readable whatever the log format turns out to be - and then writes the
whole resolved configuration to the log. The configuration line is safe to emit because the only
credential-shaped value in the tree is held in a type whose `Debug` prints a placeholder, and that is
asserted by a test rather than by the log call being careful.

A panic is traced before the process gives up on it. The shipped profiles abort, so there is no
unwinding to catch; what a hook can still do is run first, with the payload and the location in
hand, so the last thing in the log says what happened and where instead of the log simply stopping.

## Stopping

`SIGTERM` or an interrupt - and the platform equivalent elsewhere - drains in-flight work and logs
why it stopped. `runtime.shutdown_grace_seconds` is the budget, and it is the budget for the **whole**
of stopping rather than for the connection drain alone.

Fifteen seconds by default, chosen against the deadline on the other side rather than as a round
number: an orchestrator's usual `SIGTERM`-to-`SIGKILL` window is thirty, and a process still running
when that expires is killed mid-answer.

Stopping is two waits, in this order, sharing one budget:

1. **The connection drain.** Waiting for every open connection is what makes a rolling deployment not
   drop answers, and it is also how one connection nothing is going to close pins the process open.
   So the drain gets the budget, and the deadline arms only *after* shutdown has been asked for -
   before that a long-lived connection is not a deadline.
2. **Questions already executing.** Dropping the serve future ends the drain; it does not end the
   work. A question on the pool cannot be aborted, and the runtime's own shutdown waits for it - with
   no bound at all, which is what this budget's remainder now supplies. Whatever the drain did not
   spend is what the process waits here, and then it stops waiting and exits.

**What the number guarantees, precisely:** how long the process *waits*. Not that work finished, and
not that anything was cancelled - a question still running when the budget is spent is left running,
and the process exits out from under it. That is the honest trade, and it is the right one: a process
that exits on its own terms got to run whatever it does on the way out, and one that is `SIGKILL`ed
did not.

Spending the budget twice - a full grace period for the drain and then a full one again for the pool -
would be twice the number the operator chose, which is the number their kill timer is racing. So the
second wait gets the remainder, and the log line says how much that was.

A grace period shorter than a question is not refused, and it is not a misconfiguration either: it
says "stop on time even if that means abandoning an answer in flight", which is a legitimate posture
for a deployment being replaced. Nothing here can know how long a question takes, so nothing here can
check it - the number that *is* checked is that it is neither zero nor above five minutes.

## Running it

The engine reads files, so there is nothing to provision:

```bash
SUTURA__CATALOG__DIR=examples/single-player/catalog \
SUTURA__CATALOG__DATA_DIR=examples/single-player/data \
SUTURA__CATALOG__VERSION=local-1 \
  cargo run -p sutura-serve
```

It binds `127.0.0.1:8080`, needs no token there, and serves the browser interface at `/docs`. Startup
loads the catalog through its port and re-executes **every declared anchor** against the data system;
a bundle whose anchors do not reproduce the numbers their author certified starts nothing. That is not
a check the startup sequence performs and could forget - the type the service accepts has no other
constructor.

**`examples/single-player/README.md` has the worked session**, and it is the thing to read next
rather than this page: the startup output including the `NO PER-CALLER IDENTITY` line, a question
with its `provenance` block, a refusal over the wire, the `401` a missing token gets, the fifteen
bytes of the liveness body, the interface description and its `404` in production, and a refusal
to start with both of its entries. Every command and every response there was captured from a
running process. This page is the reference for what each knob does; that one is what it looks
like.

## What is not built

Named rather than implied, because an absence that reads as an oversight gets assumed away.

- **No MCP surface.** The seam is there and nothing sits on it: the transport talks to the service
  through one small port, so a second transport consumes the same thing rather than growing its own
  copy of the wiring.
- **No audit sink.** `AGENTS.md` records "every call is attributable, refusals included" as an
  invariant enforced by one. There is none, and there would be no principal to record if there were.
  Every question and every outcome reaches the log, and the log is named for what it is.
- **No request identifier.** It belongs in the failure body and there is nothing to put in it, and a
  field that is always absent is worse than no field.
- **No readiness endpoint.** There is nothing it could report that is not already true of a process
  that is listening: the bundle validated, or the process did not start. One would arrive with the
  first thing that can become unready *after* startup.
- **No CORS.** A browser is not a client of this surface, and an allow-list nobody needs is an
  allow-list somebody widens.
- **No metrics or trace export.** A span per request exists and is rendered into the log, which is
  what makes one request's lines findable. Exporting it is a decision about a backend, a sampling
  rate and an egress path, and none of those has been made.
- **No client TLS, and nothing to attach it to.** Mutual TLS *to* a metadata provider or *to* a data
  source is a real requirement and it has no consumer yet: the only `SemanticCatalog` adapter reads a
  directory, both `Warehouse` adapters read local files, and no crate here holds an HTTP client, a
  socket or a URI type. A configuration group for it would be a port with no adapter, which
  `AGENTS.md` forbids for the reason it forbids a trait with no implementor. It arrives with the
  first networked adapter - a `-postgres` or `-clickhouse` `Warehouse`, or a `-datahub`
  `SemanticCatalog` - and the parsing and validation the inbound listener already does is what it
  will be built out of.
- **No mutual TLS inbound either.** The listener above presents a certificate and verifies no
  client. Client-certificate authentication would be an identity, and this service has none to
  attach one to - see the first section.

## What a caller can still do

Stated plainly, because the posture above is a perimeter and not an authorisation model.

A caller who holds the token can read the whole catalog and ask any question the catalog certifies,
over any period inside the bound, with any permitted filter. There is no way to give one caller less
than that. The controls that exist are the shape of the question - no SQL, no table, no predicate, no
row ids - and the bounds on it, and those apply equally to everybody.

A caller behind a proxy shares a rate-limit bucket with everybody behind the same proxy **unless the
proxy is named**. The default keys on the connection's peer address, which cannot be forged and which
behind an ingress controller is *the ingress* for every request there has ever been - so the whole
internet is one bucket, and the limiter either takes everybody down with one abusive caller or is set
high enough to bound nothing. Setting `rate_limit.client_address: forwarded` and listing the hops in
`rate_limit.trusted_proxies` is what fixes it. Neither half works alone: `forwarded` with an empty
list is refused at startup, because a header nobody vouched for is a bucket the caller picks.

With a proxy named, `X-Forwarded-For` is read only when the *peer* is one of the named hops, and the
entry taken is the rightmost one that is not itself a named hop. A caller who prepends their own
value, or who sends their own header line before the proxy appends one, gets it skipped; a caller who
reaches this service directly and sets the header is ignored entirely. Every helper in the ecosystem
takes the leftmost entry, which hands the key straight to whoever sent the request.

A caller who does not hold the token can still consume rate-limit quota by presenting a wrong one -
that is deliberate and is the point of the limiter sitting *outside* the token gate. It also means an
unauthenticated caller can create a rate-limit bucket on any path that resolves to a handler. Those
buckets are swept on an interval, so the memory is bounded rather than growing for the life of the
process.

An unauthenticated caller can reach `/health` and learn that the process is up, and can learn which
paths exist - a path under the version prefix that matches no route answers `404` without holding a
credential. The paths are in the published interface description in any case. Every path that
resolves to a handler holds a credential.

A caller with the token can occupy every execution slot and shed everybody else, inside their own
rate limit, by asking questions that each cost more than the request timeout. The `503` the others
get is honest and the backlog is bounded, but the *sharing* is not fair and cannot be made fair here:
fairness needs a per-caller identity to be fair between, and there is none. What exists is
`rate_limit.api_per_second`, which bounds how fast one address can start questions.

The same caller can keep a question running after being answered `408`, because nothing cancels one.
So the cost of a question is not bounded by anything the caller experiences - only the *number* of
them running at once is.
