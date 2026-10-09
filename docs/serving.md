# Serving over HTTP

`sutura serve` answers certified questions over HTTP. It is a subcommand of `sutura`, and each release publishes it as a
tarball and as a container image. It loads the catalog, runs every declared anchor against the data system, and then
opens the listener. A catalog whose anchors do not reproduce the certified numbers does not start.

## Run it

The default bind is `127.0.0.1:8080`. This command serves the [example data](getting-started.md#get-the-example-data)
as one operator who reads their own files:

```bash
cd examples/single-player
SUTURA__SECURITY__IDENTITY=single-user \
SUTURA__SECURITY__SINGLE_USER_BECAUSE="one operator reading their own files" \
SUTURA__SOURCES__LOCAL__KIND=files \
SUTURA__SOURCES__LOCAL__DATA_DIR="$PWD/data" \
SUTURA__SOURCES__LOCAL__POSTURE=shared-service-user \
  sutura serve
```

The image starts the server when its argument is `serve`: `docker run ghcr.io/telekom/sutura:latest serve`. Inside a
container, loopback is the container, so use `--network host`, or bind `0.0.0.0` with the settings that
[the start-up refusals](#start-up-refusals) require. [Verify a release](verifying-a-release.md) before you run it.

sutura reads its settings from the built-in defaults, `<dir>/base.yaml`, `<dir>/<environment>.yaml` and environment
variables, in that order. [Configuration](configuration.md) lists the layers and the groups.

| Setting                          | Default     | Meaning                                                                                   |
| -------------------------------- | ----------- | ----------------------------------------------------------------------------------------- |
| `server.host`                    | `127.0.0.1` | An IP address of either family, never a hostname                                          |
| `server.port`                    | `8080`      | The listening port                                                                        |
| `server.request_timeout_seconds` | `30`        | The longest wait of a caller. At most 300                                                 |
| `server.max_body_bytes`          | `65536`     | The largest request body. At most 1 MiB                                                   |
| `server.allowed_hosts`           | none        | Host names, with no port, that `/v1/*`, `/docs`, `/openapi.json` and `/mcp` answer        |
| `security.access_token`          | none        | The deployment token: an RFC 6750 `b64token` of at least 32 characters                    |
| `security.metrics_token`         | none        | The token of `GET /metrics`, with the same form. It must differ from the deployment token |

Each data system and each catalog has its own settings. See the [integrations](integrations/index.md).

## Start-up refusals

Every row below is a refusal to start, not a warning. sutura reports all refusals at once and names the setting.

| Configuration                                                                                                   | What to do                                                                                  |
| --------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| A bind address that other hosts can reach, and no `security.tls_termination`                                    | Declare `sidecar`, `ingress` or `in-process` ([TLS](#tls))                                  |
| No `security.access_token` and no `security.inbound`, in production or on a non-loopback bind                   | Set one of them                                                                             |
| `security.access_token` together with `security.inbound.mode: direct`                                           | Remove one: both use `Authorization: Bearer`                                                |
| `GET /metrics` reachable with no `security.metrics_token`, in production or on a non-loopback bind              | Set `security.metrics_token`                                                                |
| `security.metrics_token` equal to `security.access_token`                                                       | Use two values                                                                              |
| `server.agent_surface.enabled` with no `security.inbound`, unless the deployment is a guarded `single-user` one | Declare `security.inbound` (see [the agent surface](#the-agent-surface-over-http))          |
| `server.agent_surface.enabled` with no `security.inbound` and a source with `impersonation-at-source`           | Declare `security.inbound`                                                                  |
| A `security.inbound` block with no `mode`                                                                       | Write `direct` or `behind-gateway`                                                          |
| `security.inbound.algorithms` that names `none`, an `HS*` algorithm, nothing, or keys of two families           | Name algorithms of one asymmetric key family                                                |
| A `security.inbound.key_set_file` that is unreadable or not a JWK set                                           | Fix the file. A key with no `kid`, a symmetric key and two keys under one `kid` are refused |
| A key set with no key of the kind that `security.inbound.algorithms` needs                                      | Add a key of that kind, or change the algorithms                                            |
| `behind-gateway` with no `security.inbound.transit_token_type`                                                  | Write the `typ` of the gateway, or `any`                                                    |
| `transit_max_lifetime_seconds` outside 1 to 3600, or above 300 without `accept_long_transit_lifetime: true`     | Use a value in range, or accept the long lifetime by name                                   |
| `security.inbound.token_type: any` in `direct` without `accept_any_token_type: true`                            | Keep the default `at+jwt`, or accept `any` by name                                          |
| A `rate_limit.trusted_proxies` block with a `/0` prefix, without `accept_every_address_as_proxy: true`          | Name the real proxy hops                                                                    |
| `rate_limit.enabled: false` in production                                                                       | Remove it                                                                                   |
| `server.port: 0` in production                                                                                  | Set a port                                                                                  |
| An unknown `SUTURA_ENVIRONMENT`, or an unknown or misspelled key                                                | Fix the name                                                                                |
| A configured source and no `security.identity`                                                                  | Write `single-user` or `multi-user`                                                         |
| A `shared-service-user` source in `multi-user` mode with no `acknowledged_because`                              | Write the reason on the source                                                              |
| A source that the catalog reads and no `sources.<alias>` entry declares                                         | Declare the source                                                                          |
| `posture: impersonation-at-source` on a source whose adapter cannot impersonate                                 | Use a posture the adapter supports                                                          |
| An anchor on a metric over an `impersonation-at-source` source with no `verification_identity`                  | Declare `verification_identity`, or remove the anchor                                       |
| `tools.run_sql.enabled: true` with `security.identity: multi-user`                                              | Use `single-user`, or leave the tool off                                                    |
| A feature that the build lacks (`tls`, `agent`, or a data system or catalog adapter)                            | The message names the feature. Use a build that has it                                      |

Before it opens the listener, sutura asks each data system whether it holds the tables of the catalog. A missing
table, a refused listing and an inconsistent listing refuse to start. A data system that cannot be asked gives a
`WARN` in the log, and the server starts.

## Who is asking

Without `security.inbound`, the deployment token authenticates the deployment and not the caller. Every question runs
with the access of the deployment.

With `security.inbound`, every request carries a token that sutura verifies: signature, issuer, expiry and an audience
that equals the resource identifier of the deployment. The verified subject goes into every audit record. There is
no default mode:

| Mode             | Use it when                                                                       |
| ---------------- | --------------------------------------------------------------------------------- |
| `direct`         | Callers send their own access token in `Authorization: Bearer`                    |
| `behind-gateway` | A gateway authenticates the caller and sends a signed assertion in its own header |

[Inbound identity](integrations/identity.md) has the settings, the key set and its rotation. In `direct` mode, a
refused request gets `401` with a `WWW-Authenticate: Bearer` challenge. sutura does not say which check failed.

The `scope` claim of the token decides which operations the caller can use:

| Scope                 | Operation                            |
| --------------------- | ------------------------------------ |
| `sutura:catalog.read` | `GET /v1/catalog`                    |
| `sutura:metrics.ask`  | `POST /v1/query`                     |
| `sutura:sql.run`      | `POST /v1/sql/run`, the raw SQL tool |

A verified token with none of these scopes gets `403` with `code: insufficient_scope`, and the detail names the
scope to grant. A scope decides which operations the caller can use, not which rows an answer holds. The posture of
the source decides whose access governs the rows.

`security.identity` is `single-user` or `multi-user`. `single-user` needs `security.single_user_because`.
`multi-user` needs `acknowledged_because` on each `shared-service-user` source.

### The agent surface over HTTP

`sutura serve` mounts the agent surface at `/mcp` (MCP over streamable HTTP) when `server.agent_surface.enabled` is
`true`. The default is `false`. A build without the `agent` feature refuses the setting and names the feature.

- With `security.inbound`, `tools/list` shows only the tools that the scope of the caller grants. The tools are
  `describe_catalog` and `ask_metric`, and `run_sql` when it is on.
- Without `security.inbound`, `/mcp` serves only a `single-user` deployment that is reachable from this host only, or
  that is behind both `security.access_token` and `rate_limit.enabled`. Then it answers every caller as the
  deployment, with every tool. A declared `rate_limit.trusted_proxies` hop, a `security.tls_termination` of `sidecar`
  or `ingress`, or a non-loopback entry in `server.allowed_hosts` counts as off-host.
- `/mcp` has the same rate limit, body cap, deployment token and `Host` list as `/v1`.

`sutura mcp` serves the same two tools over standard input and output, for a locally launched agent. It has no
caller identity and grants every capability. `runtime.max_concurrent_queries` and
`server.request_timeout_seconds` bound it.

### The raw SQL tool

`tools.run_sql.enabled` is `false` by default. When it is off, `run_sql` is not in `tools/list` or in `/openapi.json`,
and a call by name is refused as `tool_not_enabled`. When it is on, the caller needs `sutura:sql.run` as well. The
tool answers only where the deployment has one source. The row cap of a certified answer applies, and every call
writes an audit record. The page of the data system says what bounds a statement:
[Postgres](integrations/data-systems/postgres.md), [DuckDB](integrations/data-systems/duckdb.md).

## Endpoints

| Method and path                                               | Token                                                     | Purpose                                                                                      |
| ------------------------------------------------------------- | --------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `GET /health`                                                 | none                                                      | Liveness. The body is `{"status":"ok"}`                                                      |
| `GET /.well-known/oauth-protected-resource[/<resource path>]` | none, `direct` mode only                                  | RFC 9728 metadata of the resource                                                            |
| `GET /v1/catalog`                                             | `security.access_token` if set, and `sutura:catalog.read` | The metrics, grains, dimensions, allowed filter values and knowledge                         |
| `POST /v1/query`                                              | `security.access_token` if set, and `sutura:metrics.ask`  | One certified question                                                                       |
| `POST /v1/sql/run`                                            | `security.access_token` if set, and `sutura:sql.run`      | The raw SQL tool, when it is on                                                              |
| `POST /mcp`                                                   | as `/v1`, when `server.agent_surface.enabled`             | The agent surface                                                                            |
| `GET /metrics`                                                | `security.metrics_token` only                             | Counters in the Prometheus text format                                                       |
| `GET /openapi.json`, `GET /docs`                              | `security.access_token` if set                            | The interface description and a browser interface. Off in production by default (`api.docs`) |

`/health` and `/metrics` sit outside the `/v1` prefix, and `/metrics` is outside the concurrency bound. `/metrics` shares
the listener with the API, so only a network control can keep it internal. Its labels are fixed, and no label holds a question, a
caller or a source.

### A refusal carries a status

`POST /v1/query` answers `200` only when the question was answered. A refusal has `"outcome": "refusal"` and a
status, a stable `code` and a sentence:

```json
{
  "outcome": "refusal",
  "reason": {
    "code": "metric_unknown",
    "status": 404,
    "detail": "this catalog defines no metric called `customer_lifetime_value`"
  }
}
```

| Status | `code`                                                                                                                                                                                                                                                                                                                                                  |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `404`  | `metric_unknown`                                                                                                                                                                                                                                                                                                                                        |
| `403`  | `dimension_not_permitted`, `dimension_not_filterable`, `dimension_value_not_allowed`, `source_refused`, `credential_unavailable`                                                                                                                                                                                                                        |
| `409`  | `plan_spans_too_many_sources`, `federation_not_executable`, `federation_link_ambiguous`, `federation_link_compound`, `federated_answer_not_well_formed`, `plan_tables_share_an_identifier`, `multi_metric_federation_not_executable`, `multi_metric_top_not_executable`, `cross_model_ratio_without_shared_calendar`, `cross_model_ratio_spans_sources` |
| `413`  | `result_too_large`: more rows than the cap, more data than the data system returns at once, or more bytes than a response may hold                                                                                                                                                                                                                      |
| `422`  | `metrics_span_different_models`, `too_many_metrics`, `duplicate_metric_name`, `grain_not_supported`, `time_range_too_long`, `too_many_dimensions`, `too_many_filters`, `duplicate_dimension`, `cross_model_ratio_without_shared_dimension`, `top_over_uncertified_rows`, `resources_exhausted`, `deadline_exceeded`                                     |
| `429`  | `budget_exhausted`                                                                                                                                                                                                                                                                                                                                      |
| `503`  | `source_unavailable`. This is the one refusal that is worth a retry                                                                                                                                                                                                                                                                                     |

`credential_unavailable` means the caller has no access at the data system, and sutura does not read it as the
deployment instead. The fix is a grant at the data system. A `403` with no `outcome` is `insufficient_scope`.

Other failures have one shape, `{"code", "status", "detail"}`. These codes are common: `unauthorized` (`401`),
`too_large` (`413`, the request body), `host_not_allowed` (`403`), `unavailable` and `identity_unavailable` (`503`),
and `at_capacity` (`503` with `Retry-After`). A request that waits longer than `server.request_timeout_seconds` gets
`408`. A body with a key that the question does not declare, such as `sql`, gets `400` and the name of the key. A
`500` has no detail.

## Capacity

| Setting                                                 | Default                        | Meaning                                                                                                   |
| ------------------------------------------------------- | ------------------------------ | --------------------------------------------------------------------------------------------------------- |
| `runtime.max_concurrent_queries`                        | `8`                            | Questions that run at the same time. At most 512                                                          |
| `runtime.admission_timeout_seconds`                     | `5`                            | How long a question waits for a slot before `503 at_capacity`. At most 300                                |
| `runtime.engine_worker_threads`                         | as many as the machine reports | The width of the in-process engine. At most 256. Set it in a container with a CPU quota                   |
| `runtime.working_set_max_bytes`                         | `1073741824`                   | The bytes that the engine may reserve at once. sutura refuses to start above the memory that it can reach |
| `rate_limit.enabled`                                    | on in production, else off     | The request limiter                                                                                       |
| `rate_limit.probe_per_second`, `rate_limit.probe_burst` | `2`, `5`                       | Liveness, protected-resource metadata and the interface description                                       |
| `rate_limit.api_per_second`, `rate_limit.api_burst`     | `10`, `20`                     | The versioned API                                                                                         |
| `rate_limit.client_address`                             | `peer`                         | `peer` or `forwarded`: what a bucket counts                                                               |
| `rate_limit.trusted_proxies`                            | none                           | The hops whose `X-Forwarded-For` sutura believes. `forwarded` needs it                                    |

`server.request_timeout_seconds`, less a one-second margin, is also the deadline that each data system adapter
receives. One concurrency bound covers `/v1` and the agent surface together.

## TLS

Usually an ingress or a sidecar ends TLS. `security.tls_termination` states where TLS ends, and so which hop carries the
bearer token in clear text:

| Value        | What ends TLS                    | The clear-text hop                      |
| ------------ | -------------------------------- | --------------------------------------- |
| `none`       | nothing                          | The whole path. Use it on loopback only |
| `sidecar`    | a proxy in the pod               | Loopback inside the pod                 |
| `ingress`    | an ingress controller or gateway | The pod network                         |
| `in-process` | this process                     | None                                    |

For `in-process`, set both `server.tls_certificate` and `server.tls_key` to PEM files:

```yaml
security:
  tls_termination: "in-process"
server:
  tls_certificate: "/tls/chain.pem"
  tls_key: "/tls/key.pem"
```

sutura checks the pair before it binds the socket and never falls back to plain text. It reads both files again every
30 seconds. When the bytes change and the new pair is valid, new handshakes use it and open connections stay open.
An invalid pair is logged at `error` and ignored. Let a certificate manager renew the files. sutura has no ACME client.

`security.outbound.transport_anchors` (a PEM bundle path, or `system`) sets the trust anchors for the DataHub and
OpenMetadata readers. These are re-read on the same interval. The old material stays in use when a new one does not load.

## The log

`telemetry.format` is `bunyan` (one JSON object per line) or `pretty`. The default is `bunyan` in production and `pretty`
elsewhere. `telemetry.filter` is `info` by default, and `RUST_LOG` overrides it. The log goes to standard output.

```yaml
telemetry:
  format: bunyan
```

Audit records (`answered` and `refused`, and `raw_answered` and `raw_refused` for raw SQL) use the same stream and format.
The start-up line shows `log_format` and whether the configuration set it.

## Stop

`SIGTERM` or an interrupt drains the open connections and the running questions, and logs why the process stopped.
`runtime.shutdown_grace_seconds` (default `15`, above 0 and at most 300) is the budget for the whole stop. A question
that still runs when the budget ends is left running, and the process exits. Keep the budget below the kill timer of
the orchestrator.
