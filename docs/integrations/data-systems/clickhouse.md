---
title: ClickHouse
description: Answer questions over a ClickHouse server through its HTTP interface, as one declared user or as a declared user per caller.
---

# ClickHouse

<span class="sutura-badge sutura-badge--recommended">Recommended</span>

The `clickhouse` data system answers questions over a ClickHouse server through its HTTP
interface. sutura renders each plan as ClickHouse SQL and sends the values as ClickHouse query
parameters. The crate is `sutura-exec-clickhouse`, and the source kind is `clickhouse`.

## When to use it

- Your data is in ClickHouse, and one ClickHouse user may read it for all callers, or each caller
  has a ClickHouse user whose grants and row policies the server applies.
- The question reads one data system. A ClickHouse source cannot be one leg of a federated answer.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                  | Type                      | Default  | Meaning                                                                                   |
| -------------------- | ------------------------- | -------- | ----------------------------------------------------------------------------------------- |
| `host`               | string                    | required | The host of the HTTP interface                                                            |
| `port`               | integer                   | required | The HTTP port. ClickHouse uses `8123` for HTTP and `8443` for HTTPS                       |
| `user`               | string                    | required | The user for HTTP Basic authentication                                                    |
| `password_file`      | absolute path             | required | A file that holds the password. sutura reads it at startup                                |
| `transport_mode`     | string                    | required | `plaintext` (HTTP), `verified` or `mutual` (HTTPS)                                        |
| `transport_anchors`  | `system` or absolute path | none     | The CA file, or `system` for the host trust store. Required for `verified` and `mutual`   |
| `client_certificate` | absolute path             | none     | The client certificate. `mutual` only                                                     |
| `client_key`         | absolute path             | none     | The client key. `mutual` only                                                             |
| `impersonate`        | map                       | none     | Each verified subject, and the ClickHouse user it runs as. `impersonation-at-source` only |

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

This data system supports `shared-service-user` and secure-impersonation (`EXECUTE AS`).

With `posture: shared-service-user`, sutura signs in as the one user in `user`, and every caller's
query runs as that user.

With `posture: impersonation-at-source`, sutura still signs in as `user`, and it runs each statement
as the ClickHouse user that `impersonate` declares for the caller:
`EXECUTE AS "<user>" <statement>`. It never uses the session form of `EXECUTE AS`. A caller that the
map does not name is refused, and is never run as `user`. A user name can contain only letters,
digits and `_ . - @`, and it cannot be the user in `user`. Unqualified table names still resolve
in the default database of `user`.

```yaml
posture: "impersonation-at-source"
impersonate:
  "analyst-a@example.com": "analyst_a"
  "analyst-b@example.com": "analyst_b"
```

Requirements:

- The server setting `access_control_improvements.allow_impersonate_user = 1`.
- `GRANT IMPERSONATE ON <user> TO <the user in user>` for each declared user. `ALL ON *.*` includes
  `IMPERSONATE` on every user, so do not grant `ALL` to the user in `user`.
- ClickHouse Cloud does not support `EXECUTE AS`.
- At startup, `sutura serve` runs `EXECUTE AS "<user>" SELECT currentUser(), authenticatedUser()`
  for each declared user, and does not start if one fails. A grant revoked after startup makes the
  caller's question fail; it does not run as `user`.
- Only `sutura serve` serves an impersonating ClickHouse source. The `sutura` command refuses it.

The caller does not sign in to ClickHouse; the server decides which users the user in `user` may become.
