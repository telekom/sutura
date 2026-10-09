---
title: RDBMS dictionary
description: Read tables, columns and their descriptions from a documentation view in a PostgreSQL or Oracle database.
---

# RDBMS dictionary

The RDBMS dictionary catalog reads the description of your tables from a documentation view in a
PostgreSQL or Oracle database. The crate is `sutura-catalog-rdbms`, and the catalog kind is
`rdbms`. It supplies the structure, the column types and the descriptions. It supplies no metrics,
so it gives an agent a map of the tables before anyone defines a metric.

## When to use it

- Use it for multi-player governance. Many owners document their tables in one shared database, and
  that database is the source of truth. The [multi player](../../examples/multi-player.md) example shows this model with
  DataHub.
- Your database team documents tables and columns in the database itself.
- Later, you define metrics in a [markdown catalog](markdown.md) over the same tables.

## The documentation view

sutura reads one view named `columns` in the schema `dictionary_schema`. It has one row for each
documented column:

| Column               | Meaning                                                            |
| -------------------- | ------------------------------------------------------------------ |
| `environment`        | The deployment that the row applies to                             |
| `catalog_name`       | The physical catalog above the schema, if statements need one      |
| `schema_name`        | The physical schema of the table                                   |
| `table_name`         | The physical table                                                 |
| `model_name`         | The model name in sutura                                           |
| `table_description`  | The table description, or `NULL`                                   |
| `column_name`        | The physical column                                                |
| `column_ordinal`     | The order of the column in the table                               |
| `column_type`        | The data type, as `information_schema` reports it                  |
| `column_description` | The column description, or `NULL`                                  |
| `is_primary_key`     | `true` if the column is the only column of a primary or unique key |
| `is_deleted`         | `false` for a live row. sutura reads only live rows                |

The view has no foreign-key columns, so this catalog supplies no joins.

## Settings

This catalog reads `name` and `version` from the
[settings that every catalog has](../../integrations.md#catalog-settings). `dir` and `data_dir`
are optional here. The other keys are for this kind only, and sutura refuses them on another kind.

| Key                                                      | Type    | Default             | Meaning                                                              |
| -------------------------------------------------------- | ------- | ------------------- | -------------------------------------------------------------------- |
| `environment`                                            | string  | required            | Selects the rows of the view for this deployment                     |
| `source_alias`                                           | string  | required            | The `sources:` alias of the data system that holds the tables        |
| `dictionary_schema`                                      | string  | `sutura_dictionary` | The schema of the `columns` view. Letters, digits and `_` only       |
| `max_dictionary_rows`                                    | integer | `10000`             | The row limit for one read. `0` is refused                           |
| `max_dictionary_bytes`                                   | integer | `8388608`           | The size limit for one read. `0` is refused                          |
| `live_row_predicate.column`                              | string  | none                | An extra column that selects live rows                               |
| `live_row_predicate.operator`                            | string  | none                | `is_null`, `is_not_null` or `equals`                                 |
| `live_row_predicate.value`                               | string  | none                | The value for `equals`. Refused for the other two operators          |
| `connection.dialect`                                     | string  | `postgres`          | `postgres` or `oracle`                                               |
| `connection.host`                                        | string  | none                | The database host. Oracle requires it                                |
| `connection.unix_socket`                                 | path    | none                | PostgreSQL only: a socket directory, instead of `host`               |
| `connection.port`                                        | integer | required            | The database port                                                    |
| `connection.database`                                    | string  | none                | PostgreSQL: the database. Required for PostgreSQL                    |
| `connection.service_name`                                | string  | none                | Oracle: the service name. Required for Oracle                        |
| `connection.user`                                        | string  | required            | The login of the catalog                                             |
| `connection.password_file`                               | path    | required            | An absolute path to a file that holds the password                   |
| `connection.transport_mode`                              | string  | required            | `plaintext`, `verified` or `mutual`. Oracle accepts `plaintext` only |
| `connection.transport_anchors`                           | path    | none                | The CA file for `verified` and `mutual`                              |
| `connection.client_certificate`, `connection.client_key` | path    | none                | The client identity for `mutual`                                     |

`plaintext` needs a loopback IP address as `host`. `verified` and `mutual` need `host`, not
`unix_socket`. A build with the `rdbms` feature reads PostgreSQL dictionaries; the Oracle
dictionary needs a build with the `oracle` feature.

## Example

```yaml
catalogs:
  - name: "dictionary"
    kind: "rdbms"
    version: "dictionary-1"
    environment: "prod"
    source_alias: "warehouse"
    connection:
      host: "db.example.com"
      port: 5432
      database: "dictionary"
      user: "sutura_dictionary_reader"
      password_file: "/run/secrets/dictionary-password"
      transport_mode: "verified"
      transport_anchors: "/etc/sutura/db-ca.pem"
```

## Identity

The catalog signs in with its own login, `connection.user`. It never uses the credential of a data
system, because a catalog read has no caller. Each read opens one connection and one read-only
transaction, and rolls it back.
