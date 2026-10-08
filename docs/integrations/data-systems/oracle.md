---
title: Oracle
description: Answer questions over an Oracle Database as one declared user.
---

# Oracle

The `oracle` data system answers questions over an Oracle Database. sutura renders each plan as
Oracle SQL. It connects through [Oracle's own pure-Rust driver](https://github.com/oracle/rust-oracledb), so the Oracle Instant Client and the Oracle Client C libraries are not needed. The crate is `sutura-exec-oracle`, and the
source kind is `oracle`. The adapter is built with the `oracle` feature.

## When to use it

- Your data is in an Oracle Database, and one database user may read it for all callers.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key              | Type          | Default  | Meaning                                                       |
| ---------------- | ------------- | -------- | ------------------------------------------------------------- |
| `host`           | string        | required | The listener host. A loopback IP address, such as `127.0.0.1` |
| `port`           | integer       | required | The listener port. Oracle uses `1521`                         |
| `service_name`   | string        | required | The service name: letters, digits, `_` and `.`                |
| `user`           | string        | required | The user that sutura connects as                              |
| `password_file`  | absolute path | required | A file that holds the password. sutura reads it at startup    |
| `transport_mode` | string        | required | `plaintext`                                                   |

sutura connects at startup, so it does not start if the listener or the login fails.

## Example

```yaml
sources:
  warehouse:
    kind: "oracle"
    host: "127.0.0.1"
    port: 1521
    service_name: "FREEPDB1"
    user: "sutura"
    password_file: "/run/secrets/oracle-password"
    transport_mode: "plaintext"
    posture: "shared-service-user"
    acknowledged_because: "one read-only Oracle user for every caller"
```

## Identity

This data system supports `shared-service-user` only, and [#923](https://github.com/telekom/sutura/issues/923) and [#1217](https://github.com/telekom/sutura/issues/1217) track secure-impersonation.

sutura connects as the one user in `user`. Every caller's query runs as that user. The source must
use `posture: shared-service-user`.
