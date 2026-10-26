# Federated

DataHub as the catalog, Postgres and ClickHouse as the data sources, one user. The metric `recurring_revenue` is on Postgres and the metric `active_customers` is on ClickHouse;
both are grouped by the shared `segment` dimension in one question. Read the walkthrough in [`docs/examples/federated.md`](../../docs/examples/federated.md).

| File                  | What it holds                                                              |
| --------------------- | -------------------------------------------------------------------------- |
| `conf/base.yaml`      | The settings, with `platforms` mapping each DataHub platform to a source   |
| `compose.yaml`        | Postgres with TLS, ClickHouse, a live DataHub, and the catalog loader      |
| `postgres/init.sql`   | The fact table `fct_subscription_monthly`                                  |
| `clickhouse/init.sql` | The dimension table `dim_customer` and metric table `fct_customer_monthly` |
| `loader/load.py`      | The datasets, the relationship and both metrics that the loader writes     |
| `setup.sh`            | Writes the secrets of the local services to `.env` and `.local`            |

The subscription rows are the CSV files in [`../single-player/data`](../single-player/data). The
ClickHouse customer metric uses a four-row synthetic monthly fact table.

From this directory:

```sh
./setup.sh
docker compose up -d --wait postgres clickhouse datahub
docker compose run --rm federated-loader
```

Every port binds `127.0.0.1` only. Start sutura in a second terminal. The image tag ends in `-musl`:
that image carries the Postgres driver, and the plain tag does not. `conf/base.yaml` reads the files
in `.local` from `/run/sutura`, and `SUTURA_CONFIG_DIR` names the directory that holds `base.yaml`:

```sh
docker run --rm --network host --user "$(id -u):$(id -g)" \
  -v "$PWD/conf:/conf:ro" -v "$PWD/.local:/run/sutura:ro" -e SUTURA_CONFIG_DIR=/conf \
  ghcr.io/telekom/sutura:${SUTURA_VERSION:-latest}-musl serve
```

Post the question:

```sh
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue","active_customers"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["segment"]}'
```
