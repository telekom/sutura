---
title: ClickHouse
description: Answer questions over a ClickHouse server through its HTTP interface, as one declared user.
---

# ClickHouse

The `clickhouse` data system answers questions over a ClickHouse server through its HTTP
interface. sutura renders each plan as ClickHouse SQL and sends the values as ClickHouse query
parameters. The crate is `sutura-exec-clickhouse`, and the source kind is `clickhouse`.

## When to use it

- Your data is in ClickHouse, and one ClickHouse user may read it for all callers.
- The question reads one data system. A ClickHouse source cannot be one leg of a federated answer.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                  | Type                      | Default  | Meaning                                                                                 |
| -------------------- | ------------------------- | -------- | --------------------------------------------------------------------------------------- |
| `host`               | string                    | required | The host of the HTTP interface                                                          |
| `port`               | integer                   | required | The HTTP port. ClickHouse uses `8123` for HTTP and `8443` for HTTPS                     |
| `user`               | string                    | required | The user for HTTP Basic authentication                                                  |
| `password_file`      | absolute path             | required | A file that holds the password. sutura reads it at startup                              |
| `transport_mode`     | string                    | required | `plaintext` (HTTP), `verified` or `mutual` (HTTPS)                                      |
| `transport_anchors`  | `system` or absolute path | none     | The CA file, or `system` for the host trust store. Required for `verified` and `mutual` |
| `client_certificate` | absolute path             | none     | The client certificate. `mutual` only                                                   |
| `client_key`         | absolute path             | none     | The client key. `mutual` only                                                           |

`plaintext` needs a loopback IP address as `host`. There is no `database` key: unqualified table
names resolve in the default database of the user.

`runtime.working_set_max_bytes` (default `1073741824`) is also the size limit of one response.
For HTTPS to a host that is not loopback, sutura uses the proxy in `HTTPS_PROXY` or `ALL_PROXY`,
unless `NO_PROXY` names the host.

## Example

```yaml
sources:
  warehouse:
    kind: "clickhouse"
    host: "clickhouse.example.com"
    port: 8443
    user: "sutura"
    password_file: "/run/secrets/clickhouse-password"
    transport_mode: "verified"
    transport_anchors: "system"
    posture: "shared-service-user"
    acknowledged_because: "one read-only ClickHouse user for every caller"
```

## Identity

sutura signs in as the one user in `user`. Every caller's query runs as that user. The source must
use `posture: shared-service-user`.
