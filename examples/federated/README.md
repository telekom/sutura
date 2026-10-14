# Federated

DataHub as the catalog, Postgres and ClickHouse as the data sources, one user. The metric
`recurring_revenue` is on Postgres and its dimension `segment` is on ClickHouse, and one question
reads both. Read the walkthrough in [`docs/examples/federated.md`](../../docs/examples/federated.md).

| File                  | What it holds                                                            |
| --------------------- | ------------------------------------------------------------------------ |
| `conf/sutura.yaml`    | The settings, with `platforms` mapping each DataHub platform to a source |
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

Give sutura the files in `.local` as `/run/sutura/datahub-token`, `postgres-password`,
`postgres-ca.pem` and `clickhouse-password`. Every port binds `127.0.0.1` only.
