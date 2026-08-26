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
| a bind address other hosts can reach, without `security.expose_beyond_loopback: true` | with no per-caller identity, the bind address is the whole perimeter. Applies in *every* environment, including a laptop |
| no `security.access_token`, in production or on a non-loopback bind | the alternative is an unauthenticated way to read whatever the process can read |
| `rate_limit.enabled: false` in production | one question is an aggregate over up to ten years of history, so an unbounded caller is an unbounded load on the data system |
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
| `POST /v1/query` | yes, when one is configured | One certified question |
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
  "provenance": { "definition_version": "2026.06.1", "definition_digest": "befdaa16..." },
  "columns": ["period", "recurring_revenue"],
  "rows": [["2026-01-01", "237320"], ["2026-02-01", "232822"]]
}
```

```json
{
  "outcome": "refusal",
  "reason": { "code": "metric_unknown", "detail": "this catalog defines no metric called `gross_margin`" }
}
```

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
| `server.host` | `127.0.0.1` | An IP address, never a hostname: a name resolves to whatever the resolver says today |
| `server.port` | `8080` | |
| `server.request_timeout_seconds` | `30` | At most 300 |
| `server.max_body_bytes` | `65536` | At most one mebibyte. A question is a few hundred bytes |
| `security.access_token` | absent | At least 32 characters. Required in production and on a non-loopback bind |
| `security.expose_beyond_loopback` | `false` | Must be `true` for any bind other hosts can reach |
| `rate_limit.enabled` | `true` | Required in production |
| `rate_limit.probe_per_second` | `2` | Liveness and the interface description |
| `rate_limit.probe_burst` | `5` | |
| `rate_limit.api_per_second` | `10` | The versioned API |
| `rate_limit.api_burst` | `20` | |
| `telemetry.service_name` | `sutura` | What a collector groups by |
| `telemetry.filter` | `info` | `RUST_LOG` overrides it when set |
| `telemetry.format` | follows the environment | `bunyan` in production, `pretty` elsewhere |
| `api.docs` | follows the environment | Off in production, on elsewhere |
| `catalog.dir` | `catalog` | |
| `catalog.data_dir` | `data` | Where the engine's CSV and Parquet files are |
| `catalog.version` | `unversioned` | A commit id or a build number. What identifies the snapshot |

A zero is refused wherever it would read as "no limit", and every bound has a ceiling, because a
value nobody chose is worse than a value somebody has to argue with.

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
why it stopped. The drain is **bounded**: waiting for every open connection is what makes a rolling
deployment not drop answers, and it is also how one connection nothing is going to close pins the
process open past the deadline an orchestrator is running. The deadline arms only after shutdown has
been asked for, so before that a long-lived connection is not a deadline; when it expires the process
exits and says that it did.

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

## What a caller can still do

Stated plainly, because the posture above is a perimeter and not an authorisation model.

A caller who holds the token can read the whole catalog and ask any question the catalog certifies,
over any period inside the bound, with any permitted filter. There is no way to give one caller less
than that. The controls that exist are the shape of the question - no SQL, no table, no predicate, no
row ids - and the bounds on it, and those apply equally to everybody.

Behind a reverse proxy, every request appears to come from the proxy and shares one rate-limit
bucket. The limiter keys on the connection's peer address rather than on a forwarded header, because
a header is something any caller can set and nothing here can know whether a trusted proxy is in
front.

An unauthenticated caller can reach `/health` and learn that the process is up, and can learn which
paths exist - a path under the version prefix that matches no route answers `404` without holding a
credential. The paths are in the published interface description in any case. Every path that
resolves to a handler holds a credential.
