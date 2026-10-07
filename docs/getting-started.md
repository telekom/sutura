---
title: Getting started
description: Install sutura with Docker Compose, a Linux binary or Nix, and ask one question.
---

# Getting started

This page installs sutura and asks one question about the example data. The
[single-player example](examples/single-player.md) then shows the HTTP API and MCP.

## Set the version

Set the version one time. All commands on this page use it.

```bash
SUTURA_VERSION={{ sutura_version }}
```

The [release page](https://github.com/telekom/sutura/releases) lists all versions.

## Get the example data

```bash
curl -fsSL "https://github.com/telekom/sutura/archive/refs/tags/v${SUTURA_VERSION}.tar.gz" | tar -xz
cd "sutura-${SUTURA_VERSION}"
```

Run all commands on this page from this directory. All data is synthetic.

## Run it with Docker Compose

```bash
docker compose -f examples/single-player/compose.yaml up -d
docker compose -f examples/single-player/compose.yaml exec sutura sutura doctor
curl http://127.0.0.1:8080/health
```

`doctor` shows how the binary was built: profile, engine and linked data systems. `/health` returns
`{"status":"ok"}`.

`compose.yaml` uses the image `ghcr.io/telekom/sutura:${SUTURA_VERSION}`. If `SUTURA_VERSION` is not
set, it uses `latest`. In version 0.6.1 and earlier, `compose.yaml` always uses `latest`.

!!! note "Docker Desktop on macOS and Windows"

    The container uses the host network. Turn it on in **Settings > Resources > Network > Enable
    host networking**. If it is off, `/health` does not answer.

To stop the server:

```bash
docker compose -f examples/single-player/compose.yaml down
```

## Install it

On Linux, you can install the binary and use the CLI without Docker. Each release has four assets.
Each asset contains one file: the `sutura` executable.

| Asset                                      | Platform                                         |
| ------------------------------------------ | ------------------------------------------------ |
| `sutura-x86_64-unknown-linux-musl.tar.gz`  | x86_64, static. Use this one if you are not sure |
| `sutura-aarch64-unknown-linux-musl.tar.gz` | aarch64, static                                  |
| `sutura-x86_64-unknown-linux-gnu.tar.gz`   | x86_64, glibc                                    |
| `sutura-aarch64-unknown-linux-gnu.tar.gz`  | aarch64, glibc                                   |

```bash
gh release download "v${SUTURA_VERSION}" --repo telekom/sutura \
  --pattern 'sutura-x86_64-unknown-linux-musl.tar.gz' \
  --pattern 'sutura-x86_64-unknown-linux-musl.tar.gz.sha256'
sha256sum -c sutura-x86_64-unknown-linux-musl.tar.gz.sha256
gh attestation verify sutura-x86_64-unknown-linux-musl.tar.gz --repo telekom/sutura
tar -xzf sutura-x86_64-unknown-linux-musl.tar.gz
install -m755 sutura ~/.local/bin/sutura
sutura doctor
```

`sha256sum -c` makes sure that the file is complete. `gh attestation verify` makes sure that the
release workflow of this repository built the file. [Verifying a release](verifying-a-release.md)
tells what these checks do not prove.

There is no binary for macOS or Windows. Use Docker Compose or Nix.

## Install it with Nix

Nix builds sutura and its drivers from source. Nix must have the `nix-command` and `flakes`
features. The first build takes a long time. On Apple silicon (aarch64-darwin), a Nix build of
`main` links the BigQuery driver, and mounts a PostgreSQL driver that it builds and names in
`SUTURA_POSTGRES_ADBC_DRIVER` unless you set that variable. Earlier releases do neither.

Install a release:

```bash
nix profile install "github:telekom/sutura/v${SUTURA_VERSION}"
```

Install the dev version, the `main` branch:

```bash
nix profile install github:telekom/sutura
```

## Ask a question

With Docker, set this alias first:

```bash
alias sutura='docker run --rm -v "$PWD:/w:ro" -w /w ghcr.io/telekom/sutura:${SUTURA_VERSION}'
```

List the metrics in the catalog:

```bash
sutura catalog examples/single-player/catalog
```

A question is a YAML file. It names metrics, a grain, a date range and, optionally, dimensions and
filters. It has no field for SQL.

```yaml title="examples/single-player/questions/recurring-revenue-by-region.yaml"
--8<-- "examples/single-player/questions/recurring-revenue-by-region.yaml"
```

```bash
sutura query examples/single-player/catalog examples/single-player/questions/recurring-revenue-by-region.yaml examples/single-player/data
```

```text
-- definitions local-working-tree fc8d41d77d8e71ae22442a29621015faf94c03ac2628e99c3bc2d8dd06c3da19
region	period	recurring_revenue
central	2026-06-01	51739
east	2026-06-01	32598
north	2026-06-01	42157
south	2026-06-01	21203
west	2026-06-01	49425
null	2026-06-01	4999
```

The first line names the catalog version and the digest of its definitions. The `null` row is one
subscription with a customer that `dim_customer.csv` does not contain. Its revenue stays in the
total.

`compile` shows the SQL statement. It does not read data:

```bash
sutura compile examples/single-player/catalog examples/single-player/questions/recurring-revenue-by-region.yaml
```

## Be refused

Sutura refuses a question that the catalog does not permit. Here, `region` has a value that the
metric does not declare:

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

A refusal is a result, so the CLI exits with `0`. The refusal does not repeat the value you sent.
The other `questions/refused-*.yaml` files show the other refusals.

## Write your own catalog

A catalog is a directory of markdown files with YAML front matter. Each file declares its `kind`.
The directory names are free.

A model names a table, its source and its columns:

```markdown
---
kind: model
name: subscriptions
source: local
table: fct_subscription_monthly
columns: [month, subscription_key, customer_key, status, mrr_cents]
---

One row per subscription per month.
```

A metric names a model, a measure, the grains and the dimensions:

```markdown
---
kind: metric
name: recurring_revenue
model: subscriptions
measure:
  simple: { aggregate: sum, column: mrr_cents }
required_filters:
  - equals: { column: status, value: active }
time_column: month
grains: [month]
dimensions:
  - name: contract_term
    column: contract_term
    values: [annual, monthly]
audience: open
---

Recurring revenue in the month, in minor units, from active subscriptions only.
```

| Key                   | What it does                                                                                    |
| --------------------- | ----------------------------------------------------------------------------------------------- |
| `measure`             | `simple` (one term) or `ratio` (two terms). A term is `aggregate` over a column, or `count_if`. |
| `required_filters`    | Sutura adds these filters to each question. A caller cannot see or remove them.                 |
| `dimensions[].values` | The values a filter may use. A dimension without `values` is group-by only.                     |
| `anchor`              | Optional. A date range and its certified value. Sutura checks it at startup.                    |

A catalog with no declaration must carry every kind the format can express, except column types
and column descriptions. A narrower one, which would be refused as `Unprovided`, states what it
supplies in one `kind: declaration` document (at most one per directory): `definitions:` must list
`structure`, `knowledge:` is required and may be `[]`, and `may_provide:` is optional.
`sutura import wren` writes one.

The data is one file for each model in the data directory: `<table>.parquet` or `<table>.csv`.

To declare the source in a file instead of on the command line, write `conf/base.yaml` and set
`SUTURA_CONFIG_DIR=conf`:

```yaml
security:
  identity: single-user
  single_user_because: "one analyst, one laptop, the files they already have"
sources:
  local:
    kind: files
    data_dir: "/absolute/path/to/data"
    posture: shared-service-user
```

Then `sutura query catalog/ question.yaml` takes no data directory. For BigQuery, Postgres and
ClickHouse sources, refer to [Serving over HTTP](serving.md#sources) and
[Integrations](integrations.md#data-sources).
