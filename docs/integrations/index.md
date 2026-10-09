---
title: Integrations
description: The catalogs sutura reads definitions from, the data systems it runs questions on, and how it verifies callers.
---

# Integrations

sutura connects to two kinds of system. A **catalog** supplies the definitions: models, joins and
metrics. A **data system** holds the data and runs the queries. Each adapter has one page.
[Kinds and settings](../integrations.md) lists the keys that every catalog and every data system
reads.

## Catalogs

A catalog serves one governance model. In **multi-player** governance, many owners keep one reviewed
source of truth in a shared catalog: see the [multi player](../examples/multi-player.md) example. In
**single-player** governance, one owner keeps the files in the repository: see the
[single player](../examples/single-player.md) example.

<!-- dprint-ignore-start -->

<div class="grid cards" markdown>

-   :simple-markdown:{ .lg .middle } __Markdown catalog__

    ---

    <span class="sutura-badge">Catalog · single-player governance</span>

    Write the whole model as markdown files and review it in git.

    [:octicons-arrow-right-24: Markdown catalog](catalogs/markdown.md)

-   :material-database-search:{ .lg .middle } __DataHub__

    ---

    <span class="sutura-badge sutura-badge--recommended">Recommended</span>
    <span class="sutura-badge">Catalog · multi-player governance</span>

    Read models, joins and certified metrics from a shared DataHub.

    [:octicons-arrow-right-24: DataHub](catalogs/datahub.md)

-   :material-table-large:{ .lg .middle } __OKF__

    ---

    <span class="sutura-badge">Catalog · single-player governance</span>

    Read table structure and descriptions from Frictionless Table Schema files.

    [:octicons-arrow-right-24: OKF](catalogs/okf.md)

-   :material-file-sign:{ .lg .middle } __Data Contract__

    ---

    <span class="sutura-badge">Catalog · single-player governance</span>

    Read tables, columns and joins from Open Data Contract Standard v3 documents.

    [:octicons-arrow-right-24: Data Contract](catalogs/datacontract.md)

-   :material-book-open-page-variant:{ .lg .middle } __OpenMetadata__

    ---

    <span class="sutura-badge">Catalog · multi-player governance</span>

    Read tables, descriptions and joins from a shared OpenMetadata.

    [:octicons-arrow-right-24: OpenMetadata](catalogs/openmetadata.md)

-   :material-database-outline:{ .lg .middle } __RDBMS dictionary__

    ---

    <span class="sutura-badge">Catalog · multi-player governance</span>

    Read table descriptions from a documentation view in PostgreSQL or Oracle.

    [:octicons-arrow-right-24: RDBMS dictionary](catalogs/rdbms.md)

-   :material-file-import-outline:{ .lg .middle } __Wren__

    ---

    <span class="sutura-badge">Catalog · single-player governance</span>

    Convert a WrenAI manifest into a markdown catalog that you review.

    [:octicons-arrow-right-24: Wren](catalogs/wren.md)

</div>

<!-- dprint-ignore-end -->

## Data systems

A data system runs in one of two identity modes. With `shared-service-user`, every caller's query
runs as the one identity that the deployment declares. With secure-impersonation, each caller's
query runs as that caller's own identity at the source.

<!-- dprint-ignore-start -->

<div class="grid cards" markdown>

-   :material-file-table-outline:{ .lg .middle } __DataFusion (files)__

    ---

    <span class="sutura-badge">Data system</span>
    <span class="sutura-badge">shared-service-user</span>

    Run questions over Parquet, CSV and NDJSON files, inside the sutura process.

    [:octicons-arrow-right-24: DataFusion](data-systems/datafusion.md)

-   :simple-duckdb:{ .lg .middle } __DuckDB__

    ---

    <span class="sutura-badge">Data system</span>
    <span class="sutura-badge">shared-service-user</span>

    Run questions over one DuckDB database file, opened read-only.

    [:octicons-arrow-right-24: DuckDB](data-systems/duckdb.md)

-   :simple-googlebigquery:{ .lg .middle } __BigQuery__

    ---

    <span class="sutura-badge sutura-badge--recommended">Recommended</span>
    <span class="sutura-badge">Data system</span>
    <span class="sutura-badge">shared-service-user</span>
    <span class="sutura-badge">secure-impersonation</span>

    Run questions as one service account or as each caller's own account.

    [:octicons-arrow-right-24: BigQuery](data-systems/bigquery.md)

-   :simple-postgresql:{ .lg .middle } __PostgreSQL__

    ---

    <span class="sutura-badge">Data system</span>
    <span class="sutura-badge">shared-service-user</span>

    Run questions over a PostgreSQL database as one declared role.

    [:octicons-arrow-right-24: PostgreSQL](data-systems/postgres.md)

-   :simple-clickhouse:{ .lg .middle } __ClickHouse__

    ---

    <span class="sutura-badge sutura-badge--recommended">Recommended</span>
    <span class="sutura-badge">Data system</span>
    <span class="sutura-badge">shared-service-user</span>
    <span class="sutura-badge">secure-impersonation</span>

    Run questions over a ClickHouse server through its HTTP interface.

    [:octicons-arrow-right-24: ClickHouse](data-systems/clickhouse.md)

-   :material-database-cog:{ .lg .middle } __Oracle__

    ---

    <span class="sutura-badge">Data system</span>
    <span class="sutura-badge">shared-service-user</span>

    Run questions over an Oracle Database as one declared user.

    [:octicons-arrow-right-24: Oracle](data-systems/oracle.md)

</div>

<!-- dprint-ignore-end -->

### Identity mode of each data system

| Data system                                      | Identity mode                                               |
| ------------------------------------------------ | ----------------------------------------------------------- |
| [DataFusion (files)](data-systems/datafusion.md) | `shared-service-user` only                                  |
| [DuckDB](data-systems/duckdb.md)                 | `shared-service-user` only                                  |
| [BigQuery](data-systems/bigquery.md)             | `shared-service-user` and secure-impersonation              |
| [PostgreSQL](data-systems/postgres.md)           | `shared-service-user` only                                  |
| [ClickHouse](data-systems/clickhouse.md)         | `shared-service-user` and secure-impersonation (EXECUTE AS) |
| [Oracle](data-systems/oracle.md)                 | `shared-service-user` only                                  |

## Identity

<!-- dprint-ignore-start -->

<div class="grid cards" markdown>

-   :material-shield-account:{ .lg .middle } __Inbound identity__

    ---

    <span class="sutura-badge">Identity</span>

    Verify who is asking with a token from your identity provider or an assertion from a gateway.

    [:octicons-arrow-right-24: Inbound identity](identity.md)

</div>

<!-- dprint-ignore-end -->
