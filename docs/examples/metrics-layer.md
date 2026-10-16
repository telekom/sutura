---
title: Metrics layer
description: Serve a DataHub catalog over Cube for several users, and let Cube give each caller only their own rows.
---

# Metrics layer

This example runs sutura for several users with Cube as the only data system. Keycloak is the
identity provider. DataHub is the catalog, read with one token for every caller. Cube computes each
metric, and Cube decides which rows each caller reads.

The example is in
[`examples/metrics-layer`](https://github.com/telekom/sutura/tree/main/examples/metrics-layer):

| Path                                | Contents                                                       |
| ----------------------------------- | -------------------------------------------------------------- |
| `conf/impersonation-at-source.yaml` | The sutura settings: Cube receives each caller's own token     |
| `conf/shared-service-user.yaml`     | The sutura settings: Cube receives the deployment's token      |
| `keycloak/realm.json`               | The realm with the callers `alice` and `bob`, and four clients |
| `cube/model.yml`                    | The Cube data model, with an access policy on `orders`         |
| `clickhouse/init.sql`               | The tables that Cube reads                                     |
| `compose.yaml`                      | A live DataHub, and the loader that writes the catalog into it |
| `loader/load.py`                    | The catalog that the loader writes into DataHub                |
| `setup.sh`                          | Writes the secrets of the local DataHub to `.env`              |

All data is synthetic.

## The parts

`just dev-up-cube` starts the `cube` profile of `compose.services.yaml`. Three services belong to
this example:

- `cube-keycloak` imports `keycloak/realm.json`.
- `cube-clickhouse` loads `clickhouse/init.sql`.
- `cube-metrics` serves `cube/model.yml` over `cube-clickhouse`. It verifies each token with the
  keys of the realm, and accepts only the issuer of the realm and the audience `cube`.

The DataHub catalog holds the `orders` and `customers` datasets and the certified metric `revenue`.
Each dataset is a cube with the same name, and `revenue` is a measure of `orders`.

## Who reads what

Each caller exists in Keycloak, in one group:

| Caller                         | Group            | Revenue in June 2026 |
| ------------------------------ | ---------------- | -------------------- |
| `alice`                        | `north`          | `262345`             |
| `bob`                          | `south`          | `150000`             |
| the `sutura-deployment` client | `north`, `south` | `412345`             |

The client scope `cube` writes these groups into the claim `regions`. The access policy in
`cube/model.yml` gives each token only the rows of `orders` whose `region` is in its own `regions`
claim.

## impersonation-at-source

A caller signs in at the `sutura` client and sends the access token to sutura. sutura verifies it,
then exchanges it at Keycloak as the client `https://sutura.example.com` for a token with the
audience `cube`. Cube receives the caller's own token, so `alice` reads `262345` and `bob` reads
`150000`.

The `delegation` of the source declares the exchange:

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

Cube serves plain HTTP, and sutura sends a caller's token only over `https`. Put a TLS proxy in
front of `cube-metrics`, and give its CA in `security.outbound.transport_anchors`.

`token_file` holds a token of the `sutura-deployment` client. sutura sends it only for the checks
it runs at startup. `verification_identity` names the identity of those checks, because the
`revenue` metric has an anchor.

## shared-service-user

sutura sends the token of the `sutura-deployment` client on every request. Every caller reads
`412345`, the answer of the deployment. `acknowledged_because` on the source records why every
caller reads as one identity.

## A live DataHub

`compose.yaml` in the example runs DataHub on `127.0.0.1:8081`, the catalog `endpoint` in both
settings files. Its loader writes the `orders` and `customers` datasets, the relationship
`orders_to_customer` and the metric `revenue` into DataHub. From `examples/metrics-layer`:

```sh
./setup.sh
docker compose up -d --wait datahub
docker compose run --rm loader
```

`setup.sh` writes the secrets of this DataHub to `.env`. The loader writes a DataHub token for
sutura to `.local/datahub-token`. Give sutura that file as `/run/sutura/datahub-token`, the catalog
`token_file` in both settings files. Every port of this DataHub binds to `127.0.0.1` only.

## Run sutura

sutura reads `base.yaml` in `SUTURA_CONFIG_DIR`, so a settings file under another name is not read.
From `examples/metrics-layer`, give sutura the file you chose under that name and start it:

```sh
mkdir -p .local/conf
cp conf/impersonation-at-source.yaml .local/conf/base.yaml
SUTURA_CONFIG_DIR="$PWD/.local/conf" sutura serve
```

Ask the example question as `alice`. `$TOKEN` is her access token from the `sutura` client of the
realm (password grant, user and password `alice`):

```sh
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"metrics":["revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'
```

## Related

- [Inbound identity](../integrations/identity.md): how sutura verifies a caller's token.
- [Cube](../integrations/data-systems/cube.md): the source settings, and what Cube verifies.
- [DataHub](../integrations/catalogs/datahub.md): the catalog settings.
- [Multi player](multi-player.md): the same identity provider, with BigQuery.
