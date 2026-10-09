---
title: PostgreSQL
description: Answer questions over a PostgreSQL database as one declared role, with or without TLS.
---

# PostgreSQL

The `postgres` data system answers questions over a PostgreSQL database. sutura renders each plan
as PostgreSQL SQL and runs it through the ADBC PostgreSQL driver, which uses libpq. The crate is
`sutura-exec-postgres`, and the source kind is `postgres`.

## When to use it

- Your data is in PostgreSQL, and one database role may read it for all callers.
- You want the [raw SQL tool](../../serving.md#the-raw-sql-tool-over-the-postgres-source-above)
  over a database. Each raw statement runs in a read-only transaction that sutura rolls back.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                  | Type          | Default  | Meaning                                                              |
| -------------------- | ------------- | -------- | -------------------------------------------------------------------- |
| `host`               | string        | none     | The host name or IP address. Write `host` or `unix_socket`, not both |
| `unix_socket`        | absolute path | none     | The socket directory, instead of `host`                              |
| `port`               | integer       | required | The TCP port, or the port in the socket file name                    |
| `database`           | string        | required | The database                                                         |
| `user`               | string        | required | The role that sutura signs in as                                     |
| `password_file`      | absolute path | required | A file that holds the password. sutura reads it at startup           |
| `transport_mode`     | string        | required | `plaintext`, `verified` or `mutual`                                  |
| `transport_anchors`  | absolute path | none     | The CA file. Required for `verified` and `mutual`                    |
| `client_certificate` | absolute path | none     | The client certificate. `mutual` only                                |
| `client_key`         | absolute path | none     | The client key. `mutual` only                                        |

`plaintext` needs a loopback IP address as `host`, such as `127.0.0.1` or `::1`. `localhost` is not
accepted as loopback. `verified` and `mutual` need `host`, not `unix_socket`.

Related settings:

| Setting                       | Default | Meaning                                                                 |
| ----------------------------- | ------- | ----------------------------------------------------------------------- |
| `tools.run_sql.enabled`       | `false` | Turns on the raw SQL tool. Refused with `security.identity: multi-user` |
| `SUTURA_POSTGRES_ADBC_DRIVER` | not set | An absolute path to the driver, for a build that links no driver        |

The musl release binaries link the PostgreSQL driver with libpq. Other builds load the driver that
`SUTURA_POSTGRES_ADBC_DRIVER` names. `sutura doctor` shows which driver the process opens.

## Example

This source is from the [raw SQL example](https://github.com/telekom/sutura/tree/main/examples/raw-sql):

```yaml
security:
  identity: "single-user"
  single_user_because: "one operator, asking one database, no subject to impersonate"
sources:
  local:
    kind: "postgres"
    host: "127.0.0.1"
    port: 5432
    database: "analytics"
    user: "sutura_reader"
    password_file: "/etc/sutura/postgres-password"
    transport_mode: "verified"
    transport_anchors: "/etc/sutura/database-ca.pem"
    posture: "shared-service-user"
    acknowledged_because: "single-user deployment; the raw tool runs under this one role"
```

## Identity

sutura signs in as the one role in `user`, with the password or, for `mutual`, with the client
certificate. Every caller's query runs as that role. The source must use
`posture: shared-service-user`. Grants and row-level security of that role apply to every answer.
