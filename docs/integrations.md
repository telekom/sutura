---
title: Kinds and settings
description: The catalog kinds, the data system kinds and the settings that every catalog and every data system reads.
---

# Kinds and settings

sutura connects to two kinds of system. A **catalog** supplies the definitions: models, joins and
metrics. A **data system** holds the data and runs the queries. Each one has an adapter crate and
a page here. [Inbound identity](integrations/identity.md) describes how sutura verifies callers.

## Catalogs

| Catalog                                                | `kind`                  | Supplies                                      |
| ------------------------------------------------------ | ----------------------- | --------------------------------------------- |
| [Markdown catalog](integrations/catalogs/markdown.md)  | `markdown`              | Everything: models, joins, metrics, knowledge |
| [DataHub](integrations/catalogs/datahub.md)            | `datahub`               | Models, descriptions, joins, metrics          |
| [OKF](integrations/catalogs/okf.md)                    | `okf`                   | Tables and descriptions                       |
| [Data Contract](integrations/catalogs/datacontract.md) | `datacontract`          | Tables, types, descriptions, joins            |
| [OpenMetadata](integrations/catalogs/openmetadata.md)  | `openmetadata`          | Tables, types, descriptions, joins            |
| [RDBMS dictionary](integrations/catalogs/rdbms.md)     | `rdbms`                 | Tables, types, descriptions                   |
| [Wren](integrations/catalogs/wren.md)                  | none: an import command | A markdown catalog to review                  |

A catalog that supplies part of the model declares the rest as absent. A catalog without metrics
answers no certified question. One deployment uses catalogs of one kind. Several entries of that
kind make one catalog.

### Catalog settings

The catalogs are a list under `catalogs:`. Every entry has these keys:

| Key               | Type    | Default    | Meaning                                                                  |
| ----------------- | ------- | ---------- | ------------------------------------------------------------------------ |
| `name`            | string  | required   | The name of the entry                                                    |
| `kind`            | string  | `markdown` | The catalog kind                                                         |
| `dir`             | path    | required   | The directory that a file catalog reads. Optional for `rdbms`            |
| `data_dir`        | path    | required   | Required, and not read by any catalog. Optional for `rdbms`              |
| `version`         | string  | required   | The label of the snapshot, for example a commit ID. Up to 128 characters |
| `refresh_seconds` | integer | not set    | `sutura serve` reads the catalog again at this interval. `0` is refused  |

All entries must have the same `version`. A key that sutura does not know is an error.

## Data systems

| Data system                                                   | `kind`       | Identity of the query                            |
| ------------------------------------------------------------- | ------------ | ------------------------------------------------ |
| [DataFusion (files)](integrations/data-systems/datafusion.md) | `files`      | The operating-system user of sutura              |
| [DuckDB](integrations/data-systems/duckdb.md)                 | `duckdb`     | The operating-system user of sutura              |
| [BigQuery](integrations/data-systems/bigquery.md)             | `bigquery`   | One service account, or the caller's own account |
| [PostgreSQL](integrations/data-systems/postgres.md)           | `postgres`   | One declared role                                |
| [ClickHouse](integrations/data-systems/clickhouse.md)         | `clickhouse` | One declared user                                |
| [Oracle](integrations/data-systems/oracle.md)                 | `oracle`     | One declared user                                |

A federated question reads two data systems. Each one runs its part, and DataFusion joins the
parts. ClickHouse cannot run a part of a federated question.

### Data system settings

The data systems are a map under `sources:`. The key is the alias that a model names in `source:`.
Every entry has these keys:

| Key                     | Type   | Default  | Meaning                                                                                                                    |
| ----------------------- | ------ | -------- | -------------------------------------------------------------------------------------------------------------------------- |
| `kind`                  | string | required | `files`, `duckdb`, `bigquery`, `postgres`, `clickhouse` or `oracle`                                                        |
| `posture`               | string | required | `shared-service-user`: one identity for every caller. `impersonation-at-source`: the caller's own identity (BigQuery only) |
| `acknowledged_because`  | text   | not set  | Your reason to serve one identity to every caller. Required for a shared source when `security.identity` is `multi-user`   |
| `verification_identity` | text   | not set  | The identity that checks anchors at startup. Only on an `impersonation-at-source` source                                   |
| `workload_identity`     | block  | not set  | The token exchange of an `impersonation-at-source` source. See [BigQuery](integrations/data-systems/bigquery.md)           |

`security.identity` is required as soon as one source is declared: `single-user` with
`security.single_user_because`, or `multi-user`. In `single-user` mode, the reason in
`single_user_because` is the acknowledgement for every shared source.

## Environment variables

Every key has an environment variable: `SUTURA__` and the key path in capitals, with `__` between
the parts. For example, `sources.local.data_dir` is `SUTURA__SOURCES__LOCAL__DATA_DIR`.
[Configuration](configuration.md) describes how sutura layers files and variables.
