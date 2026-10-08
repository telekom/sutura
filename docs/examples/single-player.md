---
title: Single player
description: Serve a markdown catalog over local CSV files, and ask it questions with the CLI, over HTTP and over MCP.
---

# Single player

This example runs sutura on one machine for one user. The metadata is a catalog of markdown files.
The data is a directory of CSV files. Sutura reads the files with the access of the user who runs
it. There is no identity provider and no impersonation.

The example is in
[`examples/single-player`](https://github.com/telekom/sutura/tree/main/examples/single-player):

| Path                     | Contents                                                              |
| ------------------------ | --------------------------------------------------------------------- |
| `catalog/models/`        | The tables, and the columns sutura may read                           |
| `catalog/relationships/` | The joins sutura may use                                              |
| `catalog/metrics/`       | The certified metrics                                                 |
| `catalog/knowledge/`     | Glossary, caveats, worked examples, and terms with no definition      |
| `data/`                  | One CSV file for each model                                           |
| `questions/`             | Questions as YAML files. The `refused-*` files are refused on purpose |

All data is synthetic.

## Get the files

```bash
git clone https://github.com/telekom/sutura
cd sutura
```

Run all commands on this page from the repository root.

## 1. Start the server

Use a second terminal for the server. It stays in the foreground.

=== "Docker"

    ```bash
    export SUTURA_VERSION={{ sutura_version }}
    docker compose -f examples/single-player/compose.yaml up
    ```

    `SUTURA_VERSION` selects the image. Without it, Docker uses `latest`.

    !!! note "Docker Desktop on macOS and Windows"

        The container uses the host network. Docker Desktop turns this off by default. Turn it on
        in **Settings > Resources > Network > Enable host networking**. If it is off, the server
        starts, but `127.0.0.1:8080` does not answer.

=== "Nix (current main)"

    ```bash
    cd examples/single-player
    SUTURA__SECURITY__IDENTITY=single-user \
    SUTURA__SECURITY__SINGLE_USER_BECAUSE="one operator reading their own files" \
    SUTURA__SOURCES__LOCAL__KIND=files \
    SUTURA__SOURCES__LOCAL__DATA_DIR="$PWD/data" \
    SUTURA__SOURCES__LOCAL__POSTURE=shared-service-user \
      nix run github:telekom/sutura -- serve
    ```

    Nix builds sutura from the `main` branch. The first build compiles from source and takes a
    long time. Your Nix must have the `nix-command` and `flakes` features enabled.

## 2. Check the health

```bash
curl http://127.0.0.1:8080/health
```

```json
{"status":"ok"}
```

## 3. Read the configuration

```yaml title="examples/single-player/compose.yaml"
--8<-- "examples/single-player/compose.yaml"
```

| Setting                                 | What it does                                                                                           |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `command: serve`                        | Starts the HTTP server.                                                                                |
| `network_mode: host`                    | The server binds `127.0.0.1:8080`. Only this machine can connect, so no token is necessary.            |
| `.:/examples:ro`                        | Mounts the example directory read-only. Sutura writes nothing.                                         |
| `working_dir: /examples`                | Sutura reads the catalog from `catalog/` in this directory.                                            |
| `SUTURA__SECURITY__IDENTITY`            | `single-user`: one user, with the access in this configuration.                                        |
| `SUTURA__SECURITY__SINGLE_USER_BECAUSE` | Your reason for single-user mode. Sutura does not start without it.                                    |
| `SUTURA__SOURCES__LOCAL__KIND`          | `files`: the source `local` is a directory of CSV or Parquet files. The in-process engine reads them.  |
| `SUTURA__SOURCES__LOCAL__DATA_DIR`      | The directory of the files. Use an absolute path.                                                      |
| `SUTURA__SOURCES__LOCAL__POSTURE`       | `shared-service-user`: every question reads the data as the sutura process. There is no impersonation. |

Each model in the catalog names its source, for example `source: local` in
`catalog/models/subscriptions.md`. Each variable `SUTURA__A__B` sets the key `a.b`. You can put the
same settings in a file, `conf/base.yaml`, and set `SUTURA_CONFIG_DIR=conf`:

```yaml
security:
  identity: single-user
  single_user_because: "one operator reading their own files"
sources:
  local:
    kind: files
    data_dir: /absolute/path/to/examples/single-player/data
    posture: shared-service-user
```

## 4. Ask with the CLI

The CLI reads the files directly. It does not use the server. There is no macOS binary, so use the
image:

```bash
alias sutura='docker run --rm -v "$PWD:/w:ro" -w /w ghcr.io/telekom/sutura:${SUTURA_VERSION:-latest}'
```

On Linux, you can also download a release binary. [Getting started](../getting-started.md) shows
how.

List the metrics, then show one:

```bash
sutura catalog examples/single-player/catalog
sutura describe examples/single-player/catalog recurring_revenue
```

```text
recurring_revenue
  model      subscriptions
  measure    sum(mrr_cents)
  filters    status = "active"
  time       month
  dimension  contract_term -> contract_term (filterable)
  dimension  product_family -> product_family via subscription_product (filterable)
  dimension  product_name -> product_name via subscription_product (group-by only)
  dimension  region -> region via subscription_customer (filterable)
  dimension  sales_area -> sales_area via subscription_customer -> customer_region (filterable)
  dimension  segment -> segment via subscription_customer (filterable)
```

A question is a YAML file. It names metrics, a grain, a date range and, optionally, dimensions and
filters. It has no field for SQL.

```yaml title="questions/recurring-revenue-by-month.yaml"
--8<-- "examples/single-player/questions/recurring-revenue-by-month.yaml"
```

```bash
sutura query examples/single-player/catalog examples/single-player/questions/recurring-revenue-by-month.yaml examples/single-player/data
```

```text
-- definitions local-working-tree bbc08b0aadb05419d46c0855edcf9db6c1b204fead86569bc4c872377cd55af4
period	recurring_revenue
2026-01-01	237320
2026-02-01	232822
2026-03-01	216700
2026-04-01	206160
2026-05-01	202994
2026-06-01	202121
```

The first line names the catalog version and its digest. A change to a definition changes the
digest. The values are in minor units: `202121` is 2021.21.

A filter must use a value that the metric declares for that dimension:

```yaml title="questions/recurring-revenue-business-only.yaml"
--8<-- "examples/single-player/questions/recurring-revenue-business-only.yaml"
```

```bash
sutura query examples/single-player/catalog examples/single-player/questions/recurring-revenue-business-only.yaml examples/single-player/data
```

```text
-- definitions local-working-tree bbc08b0aadb05419d46c0855edcf9db6c1b204fead86569bc4c872377cd55af4
region	period	recurring_revenue
central	2026-06-01	13246
east	2026-06-01	9727
north	2026-06-01	22765
west	2026-06-01	20892
```

## 5. Ask over HTTP

The server must be running (step 1).

```bash
curl -s http://127.0.0.1:8080/v1/catalog
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}'
```

```json
{
  "outcome": "answer",
  "provenance": { "...": "the catalog version and digest" },
  "executed_as": [{ "source": "local", "posture": "shared-service-user" }],
  "columns": ["region", "period", "recurring_revenue"],
  "rows": [
    ["central", "2026-06-01", "51739"], ["east", "2026-06-01", "32598"],
    ["north", "2026-06-01", "42157"], ["south", "2026-06-01", "21203"],
    ["west", "2026-06-01", "49425"], ["null", "2026-06-01", "4999"]
  ]
}
```

`executed_as` tells you which identity read each source. `GET /v1/catalog` lists the metrics and
carries the glossary, caveats, worked examples and undefined terms in its `knowledge` field, the
same text MCP returns. Open <http://127.0.0.1:8080/docs> for the API in a browser.

## 6. Ask over MCP

### Claude Code

Claude Code starts sutura as a subprocess and talks MCP over its standard input and output:

```bash
claude mcp add sutura -- docker run -i --rm \
  -v "$PWD/examples/single-player:/examples:ro" -w /examples \
  -e SUTURA__SECURITY__IDENTITY=single-user \
  -e SUTURA__SECURITY__SINGLE_USER_BECAUSE="one operator reading their own files" \
  -e SUTURA__SOURCES__LOCAL__KIND=files \
  -e SUTURA__SOURCES__LOCAL__DATA_DIR=/examples/data \
  -e SUTURA__SOURCES__LOCAL__POSTURE=shared-service-user \
  ghcr.io/telekom/sutura:${SUTURA_VERSION:-latest} mcp
```

Start `claude` and ask *"What was the recurring revenue by region in June 2026?"*. Claude calls two
tools: `describe_catalog` to read the metrics and the glossary, then `ask_metric` to ask the
question.

The MCP process gives full access to each client that can start it. Over HTTP, `/mcp` requires an
identity provider today. Refer to [Serving over HTTP](../serving.md#the-agent-surface-over-http).

### A chat interface

`just demo` starts sutura with [Open WebUI](https://github.com/open-webui/open-webui) in one
container. It needs a clone with the development shell, and a model that can call tools:

```bash
export SUTURA_DEMO_MODEL_ENDPOINT="http://host.docker.internal:11434/v1"
export SUTURA_DEMO_MODEL="qwen3"
export SUTURA_DEMO_ACKNOWLEDGE="one person, one host, one local example"
just demo
```

The chat client calls the HTTP API, not MCP. [The local chat demo](../demo.md) has the details.

## 7. Join across two relationships

`sales_area` is a column of `regions`. A subscription has a customer, and a customer has a region.
The fact table has no region and no sales area. So the question needs two joins:
`subscription_customer`, then `customer_region`.

```yaml title="questions/recurring-revenue-by-sales-area.yaml"
--8<-- "examples/single-player/questions/recurring-revenue-by-sales-area.yaml"
```

```bash
sutura query examples/single-player/catalog examples/single-player/questions/recurring-revenue-by-sales-area.yaml examples/single-player/data
```

```text
-- definitions local-working-tree bbc08b0aadb05419d46c0855edcf9db6c1b204fead86569bc4c872377cd55af4
sales_area	period	recurring_revenue
central	2026-06-01	51739
north_east	2026-06-01	74755
south_west	2026-06-01	70628
null	2026-06-01	4999
```

`compile` shows the statement and does not read data:

```bash
sutura compile examples/single-player/catalog examples/single-player/questions/recurring-revenue-by-sales-area.yaml
```

```sql
SELECT "dim_region"."sales_area" AS "sales_area", ..., SUM("fct_subscription_monthly"."mrr_cents") AS "recurring_revenue"
FROM "fct_subscription_monthly"
LEFT JOIN "dim_customer" ON "fct_subscription_monthly"."customer_key" = "dim_customer"."customer_key"
LEFT JOIN "dim_region" ON "dim_customer"."region" = "dim_region"."region"
WHERE "fct_subscription_monthly"."month" >= ? AND "fct_subscription_monthly"."month" < ?
  AND "fct_subscription_monthly"."status" = ?
...
```

All values are bind parameters. `status = 'active'` comes from the metric definition, not from the
question. The joins are `LEFT JOIN`s: one subscription has a customer that `dim_customer` does not
contain, and its revenue stays in the total as the `null` row.

## 8. Be refused

Sutura refuses a question that the catalog does not permit. It refuses before it reads data. A
refusal is a result, so the CLI exits with `0`.

A value the dimension does not declare:

```bash
sutura query examples/single-player/catalog examples/single-player/questions/refused-value-not-allowed.yaml examples/single-player/data
```

```text
refused: DimensionValueNotAllowed
  the value is not one the definitions declare for that dimension
  metric: recurring_revenue
  dimension: region
  remedy: Use a value from that dimension's list below - the refusal will not repeat yours back.
```

A grain the metric does not declare:

```bash
sutura query examples/single-player/catalog examples/single-player/questions/refused-grain-not-supported.yaml examples/single-player/data
```

```text
refused: GrainNotSupported
  the metric exists and does not declare that time resolution
  metric: recurring_revenue
  grain: day
  remedy: Ask at a grain the metric lists - a finer grain is a number nobody certified.
```

A metric the catalog does not define:

```bash
sutura query examples/single-player/catalog examples/single-player/questions/refused-metric-unknown.yaml examples/single-player/data
```

```text
refused: MetricUnknown
  no metric of that name is defined here
  metric: customer_lifetime_value
  remedy: Use a name from the metric list below, exactly as spelled. Do not try a variant spelling, a plural, or a name you remember from another deployment: the list is the whole of what exists.
```

Over HTTP, a refusal has its own status. For example, `"grain": "week"` returns `422` with
`"code": "grain_not_supported"`. The other `questions/refused-*.yaml` files show the other refusals.

## How it works

1. Sutura reads the catalog and calculates a digest of the definitions.
2. A metric can declare an `anchor`: a date range and the value the metric must return for it.
   Sutura runs each anchor at startup and does not start if a value is different. Anchors are
   optional. This catalog declares six.
3. Sutura checks the question against the catalog. A question that the catalog does not permit is
   refused here.
4. Sutura compiles the question into one SQL statement with bind parameters. The in-process engine
   runs it over the CSV files.

## Related

- [Serving over HTTP](../serving.md): all settings, tokens, postures and refusal statuses.
- [Concepts](../concepts.md): metrics, grains, dimensions and refusals.
- [Inbound identity](../integrations/identity.md): how a multi-user deployment verifies callers.
- [Multi player](multi-player.md): Keycloak, DataHub and BigQuery, with an identity for each caller.
