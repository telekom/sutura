---
title: Federated
description: Ask one metric on Postgres, grouped by a dimension on ClickHouse, with DataHub as the catalog.
---

# Federated

This example runs sutura for one user over two data systems. The metric `recurring_revenue` is in
Postgres, and its dimension `segment` is in ClickHouse. DataHub is the catalog. One question reads
both.

The example is in
[`examples/federated`](https://github.com/telekom/sutura/tree/main/examples/federated):

| Path                  | Contents                                                              |
| --------------------- | --------------------------------------------------------------------- |
| `conf/base.yaml`      | The sutura settings                                                   |
| `compose.yaml`        | Postgres with TLS, ClickHouse, a live DataHub, and the catalog loader |
| `postgres/init.sql`   | The fact table `fct_subscription_monthly`                             |
| `clickhouse/init.sql` | The dimension table `dim_customer`                                    |
| `loader/load.py`      | The catalog that the loader writes into DataHub                       |
| `setup.sh`            | Writes the secrets of the local services to `.env` and `.local`       |

The rows are the CSV files of the [single player](single-player.md) example. All data is synthetic.

## The settings

```yaml
--8<-- "examples/federated/conf/base.yaml"
```

`platforms` maps the platform of each DataHub dataset to the source that serves it. The dataset
`fct_subscription_monthly` is on the platform `postgres`, so sutura reads it from `billing`. The
dataset `dim_customer` is on `clickhouse`, so sutura reads it from `crm`. Both sources run as the
deployment's own user, and `single_user_because` records why.

## The question

```json
{
  "metrics": ["recurring_revenue"],
  "grain": "month",
  "range": { "start": "2026-06-01", "end": "2026-07-01" },
  "dimensions": ["segment"]
}
```

The metric is on the model `postgres__fct_subscription_monthly`. The dimension `segment` is on
the model `clickhouse__dim_customer`, reached through the relationship `subscription_customer`.
sutura reads the fact rows from Postgres and the dimension rows from ClickHouse, and joins them.

## Run it

From `examples/federated`:

```sh
./setup.sh
docker compose up -d --wait postgres clickhouse datahub
docker compose run --rm federated-loader
```

The files in `.local` are the files that `conf/base.yaml` reads from `/run/sutura`:
`datahub-token`, `postgres-password`, `postgres-ca.pem` and `clickhouse-password`. Every port of
these services binds to `127.0.0.1` only.

Start sutura in a second terminal. It stays in the foreground. `SUTURA_CONFIG_DIR` names the
directory that holds `base.yaml`, and the container reads the files in `.local` as its own user. It
uses the host network, as in the [single player](single-player.md) example. The image tag ends in
`-musl`: that image carries the Postgres driver, and the plain tag does not:

```sh
export SUTURA_VERSION={{ sutura_version }}
docker run --rm --network host --user "$(id -u):$(id -g)" \
  -v "$PWD/conf:/conf:ro" -v "$PWD/.local:/run/sutura:ro" -e SUTURA_CONFIG_DIR=/conf \
  ghcr.io/telekom/sutura:$SUTURA_VERSION-musl serve
```

Post the question:

```sh
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["segment"]}'
```

## Related

- [DataHub](../integrations/catalogs/datahub.md): the catalog settings, including `platforms`.
- [PostgreSQL](../integrations/data-systems/postgres.md) and
  [ClickHouse](../integrations/data-systems/clickhouse.md): the source settings.
- [Metrics layer](metrics-layer.md): the same DataHub loader, with Cube.
