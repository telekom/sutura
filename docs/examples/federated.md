---
title: Federated
description: Ask one metric on Postgres and one on ClickHouse, with DataHub as the catalog.
---

# Federated

This example runs sutura for one user over two data systems. The metric `recurring_revenue` is in Postgres, and the metric `active_customers` is in ClickHouse.
DataHub is the catalog. One question reads both metrics and groups them by `segment`.

The example is in
[`examples/federated`](https://github.com/telekom/sutura/tree/main/examples/federated):

| Path                  | Contents                                                                   |
| --------------------- | -------------------------------------------------------------------------- |
| `conf/base.yaml`      | The sutura settings                                                        |
| `compose.yaml`        | Postgres with TLS, ClickHouse, a live DataHub, and the catalog loader      |
| `postgres/init.sql`   | The fact table `fct_subscription_monthly`                                  |
| `clickhouse/init.sql` | The dimension table `dim_customer` and metric table `fct_customer_monthly` |
| `loader/load.py`      | The catalog that the loader writes into DataHub                            |
| `setup.sh`            | Writes the secrets of the local services to `.env` and `.local`            |

The subscription rows are the CSV files of the [single player](single-player.md) example. The ClickHouse
customer metric uses a four-row synthetic monthly fact table. All data is synthetic.

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
  "metrics": ["recurring_revenue", "active_customers"],
  "grain": "month",
  "range": { "start": "2026-06-01", "end": "2026-07-01" },
  "dimensions": ["segment"]
}
```

The metric `recurring_revenue` is on `postgres__fct_subscription_monthly`, while `active_customers`
is on `clickhouse__fct_customer_monthly`. Both models expose `segment`; the first reaches it through
`subscription_customer`. sutura reads one metric from each source and joins the two arms.

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
  -d '{"metrics":["recurring_revenue","active_customers"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["segment"]}'
```

## Distributed execution

`server.nodes` lets several sutura processes plan one combine together: the node that receives a
question still answers it, but when the combine is large enough it spreads the work over the
workers it declares. A question that reads one data system never goes across, even one asking for
several metrics there - that is one statement at that system. A question over two or more systems,
like this example's, goes across only once its legs hold more than `server.nodes.across_above_bytes`
(or a working-set ceiling, if that key is unset). See [Serving](../serving.md#tls) for the full
`server.nodes` rule; the decision to spread a combine this way, and what it trades away, is
summarized under Limits below.

This feature merged to `main` after the last tagged release, so `ghcr.io/telekom/sutura:{{
sutura_version }}-musl` - the image the rest of this page runs - predates it. Until a release ships
it, build the image from this checkout instead:

```sh
nix build .#oci
docker load < result
```

That produces a local `sutura:latest` image with `server.nodes` and the worker role compiled in.

### Run the coordinator and two workers

Nodes talk mutual TLS: every node's certificate must be signed by the one `tls_node_authority`
this deployment declares, or a worker call is refused. `conf/distributed/` holds three settings
directories - `coordinator`, `worker-1`, `worker-2` - and `compose.distributed.yaml` adds a
one-shot service that writes a throwaway demo CA and one leaf certificate per node to
`.local/distributed`. It is demo-only: the certificates last one day, `.gitignore` already covers
`.local/`, and nothing here is fit to reuse.

With `postgres`, `clickhouse` and `datahub` already up from [Run it](#run-it):

```sh
docker compose -f compose.yaml -f compose.distributed.yaml run --rm distributed-certs
```

Start both workers, then the coordinator, each in its own terminal from `examples/federated`:

```sh
docker run --rm --network host --user "$(id -u):$(id -g)" \
  -v "$PWD/conf/distributed/worker-1:/conf:ro" -v "$PWD/.local/distributed:/run/sutura/distributed:ro" \
  -e SUTURA_CONFIG_DIR=/conf sutura:latest serve
```

```sh
docker run --rm --network host --user "$(id -u):$(id -g)" \
  -v "$PWD/conf/distributed/worker-2:/conf:ro" -v "$PWD/.local/distributed:/run/sutura/distributed:ro" \
  -e SUTURA_CONFIG_DIR=/conf sutura:latest serve
```

```sh
docker run --rm --network host --user "$(id -u):$(id -g)" \
  -v "$PWD/conf/distributed/coordinator:/conf:ro" -v "$PWD/.local:/run/sutura:ro" \
  -e SUTURA_CONFIG_DIR=/conf sutura:latest serve
```

The coordinator reads the same secrets as the single-node example from `.local`, plus the demo
certificates nested under `.local/distributed`. Post the same question, now over TLS:

```sh
curl -sk -X POST https://127.0.0.1:8443/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue","active_customers"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["segment"]}'
```

The answer is the same JSON as the single-node run - going across changes how the combine was
computed, not its shape. To see that it actually crossed, watch the coordinator's own metrics while
the request is in flight, in a third terminal:

```sh
curl -sk https://127.0.0.1:8443/metrics | grep sutura_worker_slots_wanted
```

`sutura_worker_slots_wanted` is a gauge the coordinator only moves while a combine is currently
running across nodes; each worker's own `/metrics` carries `sutura_worker_requests_in_flight`,
which moves only while that worker is answering a call. Because this example's combine finishes in
well under a second, catching either gauge away from zero may take a couple of retries - there is
no log line or answer field for this, by design (see Serving's limits).

### Limits

- The certificates are a demo CA generated fresh by `distributed-certs`, good for one day, with no
  use beyond this example. All three node certificates carry the same `subjectAltName:IP:127.0.0.1`,
  so on this loopback setup any one of them would verify for any of the three addresses - only the
  port tells the nodes apart here. A real deployment's nodes have distinct hostnames, each named in
  its own certificate.
- `across_above_bytes: 1` is set far below any realistic ceiling so this example's small legs cross
  at all; left unset, sutura's own working-set ceiling would keep a combine this size on one node.
- Decision 5 of the ADR - a per-question task grant - is not built yet. A link today carries no
  restriction beyond node-certificate identity: any certificate this demo CA signed can ask either
  worker to run any stage, not just this question's.
- One lost node costs the whole question (`503 node_lost`); nothing resumes a stage or re-plans
  onto the nodes that remain.
- This two-worker setup has not been run end to end by anyone or anything - not CI, not by hand.
  The steps above are untested instructions, not a verified procedure.

## Related

- [DataHub](../integrations/catalogs/datahub.md): the catalog settings, including `platforms`.
- [PostgreSQL](../integrations/data-systems/postgres.md) and
  [ClickHouse](../integrations/data-systems/clickhouse.md): the source settings.
- [Metrics layer](metrics-layer.md): the same DataHub loader, with Cube.
