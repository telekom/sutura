# Federated

DataHub as the catalog, Postgres and ClickHouse as the data sources, one user. The metric
`recurring_revenue` is on Postgres and its dimension `segment` is on ClickHouse, and one question
reads both. Read the walkthrough in [`docs/examples/federated.md`](../../docs/examples/federated.md).

| File                  | What it holds                                                            |
| --------------------- | ------------------------------------------------------------------------ |
| `conf/base.yaml`      | The settings, with `platforms` mapping each DataHub platform to a source |
| `compose.yaml`        | Postgres with TLS, ClickHouse, a live DataHub, and the catalog loader    |
| `postgres/init.sql`   | The fact table `fct_subscription_monthly`                                |
| `clickhouse/init.sql` | The dimension table `dim_customer`                                       |
| `loader/load.py`      | The datasets, the relationship and the metric that the loader writes     |
| `setup.sh`            | Writes the secrets of the local services to `.env` and `.local`          |

The rows are the CSV files in [`../single-player/data`](../single-player/data).

From this directory:

```sh
./setup.sh
docker compose up -d --wait postgres clickhouse datahub
docker compose run --rm federated-loader
```

Every port binds `127.0.0.1` only. Start sutura in a second terminal. `conf/base.yaml` reads the files
in `.local` from `/run/sutura`, and `SUTURA_CONFIG_DIR` names the directory that holds `base.yaml`:

```sh
docker run --rm --network host --user "$(id -u):$(id -g)" \
  -v "$PWD/conf:/conf:ro" -v "$PWD/.local:/run/sutura:ro" -e SUTURA_CONFIG_DIR=/conf \
  ghcr.io/telekom/sutura:${SUTURA_VERSION:-latest} serve
```

Post the question:

```sh
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["segment"]}'
```
