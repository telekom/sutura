# Metrics layer

DataHub as the catalog, Cube as the only data source, and Keycloak as the identity provider. Cube
computes each metric, and Cube decides which rows each caller reads. Read the walkthrough in
[`docs/examples/metrics-layer.md`](../../docs/examples/metrics-layer.md).

| File                                | What it holds                                                                     |
| ----------------------------------- | --------------------------------------------------------------------------------- |
| `conf/impersonation-at-source.yaml` | Cube receives each caller's own token                                             |
| `conf/shared-service-user.yaml`     | Cube receives the deployment's token                                              |
| `keycloak/realm.json`               | Two callers, `alice` (region `north`) and `bob` (region `south`), and the clients |
| `cube/model.yml`                    | The `orders` and `customers` cubes, and the access policy on `orders`             |
| `clickhouse/init.sql`               | The tables that Cube reads                                                        |
| `compose.yaml`                      | A live DataHub, and the loader that writes the catalog into it                    |
| `loader/load.py`                    | The datasets, the relationship and the metric that the loader writes              |
| `setup.sh`                          | Writes the secrets of the local DataHub to `.env`                                 |

`just dev-up-cube` starts the services in the `cube` profile of `compose.services.yaml`:
`cube-keycloak` with this realm, and `cube-metrics`, which serves `cube/model.yml` and verifies each
token with the keys of the realm.

A live DataHub, from this directory:

```sh
./setup.sh
docker compose up -d --wait datahub
docker compose run --rm loader
```

The loader writes a DataHub token for sutura to `.local/datahub-token`. Give sutura that file as
`/run/sutura/datahub-token`. Both settings files read DataHub at `http://127.0.0.1:8081`.

Start sutura with one settings file. sutura reads `base.yaml` in `SUTURA_CONFIG_DIR`, so copy the
file you chose to that name:

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
