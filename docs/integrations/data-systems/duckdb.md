---
title: DuckDB
description: Answer questions over one DuckDB database file, opened read-only.
---

# DuckDB

The `duckdb` data system answers questions over one DuckDB database file. sutura opens the file
read-only, inside the process, through the DuckDB ADBC driver. The crate is `sutura-exec-duckdb`,
and the source kind is `duckdb`. sutura renders each plan as DuckDB SQL.

## When to use it

- Your data is in one DuckDB file.
- You want the [raw SQL tool](../../serving.md#the-raw-sql-tool) without a
  database server.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and this key:

| Key             | Type          | Default  | Meaning                                                           |
| --------------- | ------------- | -------- | ----------------------------------------------------------------- |
| `database_file` | absolute path | required | The database file. sutura opens it read-only and never creates it |

Related settings:

| Setting                         | Default      | Meaning                                                                 |
| ------------------------------- | ------------ | ----------------------------------------------------------------------- |
| `tools.run_sql.enabled`         | `false`      | Turns on the raw SQL tool. Refused with `security.identity: multi-user` |
| `runtime.working_set_max_bytes` | `1073741824` | The memory budget for one result                                        |
| `SUTURA_DUCKDB_ADBC_DRIVER`     | not set      | An absolute path to `libduckdb`, for a build that links no driver       |

The musl release binaries link the DuckDB driver. Other builds load the library that
`SUTURA_DUCKDB_ADBC_DRIVER` names.

## Example

```yaml
security:
  identity: "single-user"
  single_user_because: "one operator, one database file"
sources:
  local:
    kind: "duckdb"
    database_file: "/srv/sutura/warehouse.duckdb"
    posture: "shared-service-user"
tools:
  run_sql:
    enabled: true
```

## Identity

This data system supports `shared-service-user` only: DuckDB runs inside the sutura process, so no per-caller identity reaches the source.

The process opens the file with its own operating-system identity. The source must use
`posture: shared-service-user`. sutura opens the file with external access off and the DuckDB
configuration locked, so a statement cannot read other files.
