---
title: Cube
description: Answer questions through a Cube deployment's REST API, and read its data model as a catalog.
---

# Cube

The `cube` data system answers questions through the REST API of a [Cube](https://cube.dev)
deployment. sutura plans each question from the catalog, and Cube computes each metric from its own
data model. sutura does not compute a Cube metric itself. The crate is `sutura-exec-cube`, and the
source kind is `cube`. The adapter is built with the `cube` feature.

The `cube` catalog kind reads the same deployment's metadata API. The crate is `sutura-catalog-cube`.

## When to use it

- Your metrics are computed in Cube, and BI tools, REST clients and sutura must give the same
  numbers.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key          | Type          | Default  | Meaning                                                                                                                           |
| ------------ | ------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `endpoint`   | string        | required | `scheme://host[:port]`. `https` for any host, `http` only for a loopback IP address. No path and no `user@`.                      |
| `token_file` | absolute path | required | A file that holds the Cube API token. sutura reads it at startup                                                                  |
| `delegation` | list          | none     | For `impersonation-at-source`, required: the hops that exchange the caller's token, in order. Must not be present without a value |

Each `delegation` hop has the keys of an [Oracle hop](oracle.md#settings).

sutura does not connect at startup. The first request is sent for the first question. One request
is bounded at 30 seconds, and one response at 8 MiB. For TLS, the adapter uses `security.outbound`.

## How a plan maps

Each table in the catalog is a cube or a view in Cube, with the same name. A metric is the measure
`<table>.<metric>`, and a column is the dimension `<table>.<column>`. Every filter of the plan is a
Cube filter on `/cubejs-api/v1/load`. Before a query is sent, sutura reads `/cubejs-api/v1/meta`
with the same token: a member that Cube does not serve refuses the question.

Cube cannot run a part of a federated question.

## Identity

This data system supports `impersonation-at-source` and `shared-service-user`. In both, Cube runs
its warehouse query with the database identity in its own driver settings (`CUBEJS_DB_USER` in the
example), not with the caller's identity. The two differ in the token that Cube receives: each
caller's own token, or one token for every caller.

### impersonation-at-source

Each caller must exist in the identity provider that Cube trusts, and the claims of the caller's
own token decide what the caller may read. Cube applies its access policy from those claims.

sutura exchanges the token that it verified for the caller through the hops in `delegation`, in
order. The first hop sends the caller's token, and each next hop sends the token from the hop
before it. Each request of the caller's query sends the token from the last hop as its bearer.

- If a hop fails, sutura refuses the query. It sends no later hop and no request to Cube.
- sutura does not start without `delegation`, or when the last hop is `broker-token`.
- The `endpoint` must be `https`, and `delegation` needs `security.inbound.mode: direct`. Cube
  serves plain HTTP, so put a TLS proxy in front of it, and give its CA in `security.outbound`.
- sutura sends the token in `token_file` only for the checks it runs at startup. No caller's query
  sends it.

The operator sets these in Cube, so that Cube verifies the token from the last hop:

| Cube setting          | Value                                            |
| --------------------- | ------------------------------------------------ |
| `CUBEJS_JWK_URL`      | The key set of the identity provider             |
| `CUBEJS_JWT_ALGS`     | The algorithm of its tokens, for example `RS256` |
| `CUBEJS_JWT_ISSUER`   | Its issuer                                       |
| `CUBEJS_JWT_AUDIENCE` | The `audience` of the last hop                   |

Cube then uses the claims of that token as the security context. An `access_policy` in the data
model filters rows by those claims, for example by a claim that holds the caller's groups. A
caller whose token does not carry the claim reads no rows.

Key Cube's result cache and pre-aggregations on each caller's security context:
`context_to_app_id`, `context_to_orchestrator_id` and `pre_aggregations_schema` in `cube.py`
(`contextToAppId`, `contextToOrchestratorId` and `preAggregationsSchema` in `cube.js`).

### shared-service-user

One token for every caller. sutura sends the token in `token_file` on every request, so every
caller's query receives the security context of that token.

## Example

`examples/cube` declares the markdown catalog of `examples/single-player` over a `cube` source.
`just dev-up-cube` starts a Cube that serves `examples/cube/model` over the same data.

```yaml
catalogs:
  - name: "model"
    kind: "markdown"
    dir: "examples/single-player/catalog"
    data_dir: "examples/single-player/data"
    version: "example"
sources:
  local:
    kind: "cube"
    endpoint: "http://127.0.0.1:4000"
    token_file: "/run/sutura/cube-token"
    posture: "shared-service-user"
```

[Metrics layer](../../examples/metrics-layer.md) is the same with a DataHub catalog, Keycloak, and
both identity modes. Its source for `impersonation-at-source`:

```yaml
sources:
  warehouse:
    kind: "cube"
    endpoint: "https://cube.example.com"
    token_file: "/run/sutura/cube-token"
    posture: "impersonation-at-source"
    verification_identity: "sutura-deployment"
    delegation:
      - token_endpoint: "http://127.0.0.1:8180/realms/metrics-layer/protocol/openid-connect/token"
        client_id: "https://sutura.example.com"
        client_secret_file: "/run/sutura/exchange-secret"
        audience: "cube"
```

## Catalog

The `cube` catalog kind reads `/cubejs-api/v1/meta`. Each cube or view becomes a model on the source
that has the name of the catalog entry, and each dimension a column with Cube's type word
(`string`, `number`, `time` or `boolean`). A cube's description is required. A measure is read
but does not become a metric, because its computation stays in Cube. Joins stay in Cube too.

It reads the [settings that every catalog has](../../integrations.md#catalog-settings) and the
`endpoint`, `token_file`, `deadline_seconds` and `max_response_bytes` keys of the
[OpenMetadata catalog](../catalogs/openmetadata.md#settings). It requires `dir` and `data_dir`, but
does not read them.
