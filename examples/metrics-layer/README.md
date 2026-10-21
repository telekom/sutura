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
| `compose.cube.yaml`                 | Keycloak, ClickHouse, Cube and the TLS proxy in front of Cube                     |
| `proxy/nginx.conf`                  | The TLS proxy: `https://cube.example.com` to Cube                                 |
| `loader/load.py`                    | The catalog that the loader writes, and the files it takes from the realm         |
| `setup.sh`                          | Writes the secrets to `.env`, and the CA and certificate of the TLS proxy         |

The services, from this directory:

```sh
./setup.sh
docker compose up -d --wait datahub keycloak cube-tls
docker compose run --rm loader
```

`setup.sh` needs `openssl` on the host.

`setup.sh` writes `COMPOSE_FILE` to `.env`, so `docker compose` reads `compose.yaml` and
`compose.cube.yaml` together. It also issues the CA and certificate of the proxy. They are valid
for 30 days: delete `.tls`, run `./setup.sh` again, run `docker compose restart cube-tls`, and
start sutura again. Keycloak imports `keycloak/realm.json` on `127.0.0.1:8180`. Cube serves
`cube/model.yml` over ClickHouse, verifies each token with the keys of the realm, and answers
through the proxy on `127.0.0.1:443`. `just dev-up-cube` starts the same services for this
repository's tests, in the `cube` profile of `compose.services.yaml`.

The loader writes the files that sutura reads from `/run/sutura` to `.local`: `datahub-token`,
`jwks.json` (the keys of the realm), `exchange-secret`, and `cube-token`, a token of the
`sutura-deployment` client. `setup.sh` wrote `cube-ca.pem`, the CA of the proxy. The realm issues
tokens for an hour, so run the loader again when `cube-token` has expired. Then start sutura again:
it reads the token at startup. Both settings files read DataHub at `http://127.0.0.1:8081`.

Start sutura with one settings file. sutura reads `base.yaml` in `SUTURA_CONFIG_DIR`, so copy the
file you chose to that name. The container names Cube as `cube.example.com`, the host of the
certificate, and `--add-host` points that name at the proxy:

```sh
mkdir -p .local/conf
cp conf/impersonation-at-source.yaml .local/conf/base.yaml
docker run --pull always --rm --network host --add-host cube.example.com:127.0.0.1 \
  --user "$(id -u):$(id -g)" \
  -v "$PWD/.local:/run/sutura:ro" -e SUTURA_CONFIG_DIR=/run/sutura/conf \
  ghcr.io/telekom/sutura:${SUTURA_VERSION:-latest} serve
```

On Docker Desktop, turn on **Settings > Resources > Network > Enable host networking** first; with
it off, `127.0.0.1:8080` does not answer. See
[Single player](../../docs/examples/single-player.md).

Ask the example question as `alice`. `$TOKEN` is her access token from the `sutura` client of the
realm (password grant, user and password `alice`):

```sh
TOKEN=$(curl -s http://127.0.0.1:8180/realms/metrics-layer/protocol/openid-connect/token \
  -d grant_type=password -d client_id=sutura -d username=alice -d password=alice \
  | sed -n 's/.*"access_token":"\([^"]*\)".*/\1/p')
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"metrics":["revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'
```

`alice` reads `262345`, and `bob` (user and password `bob`) reads `150000`.
