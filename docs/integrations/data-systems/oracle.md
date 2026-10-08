---
title: Oracle
description: Answer questions over an Oracle Database as one declared user.
---

# Oracle

The `oracle` data system answers questions over an Oracle Database. sutura renders each plan as
Oracle SQL and runs it through Oracle's Rust driver. The crate is `sutura-exec-oracle`, and the
source kind is `oracle`. The adapter is built with the `oracle` feature.

## When to use it

- Your data is in an Oracle Database, and one database user may read it for all callers.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                 | Type          | Default  | Meaning                                                                                                    |
| ------------------- | ------------- | -------- | ---------------------------------------------------------------------------------------------------------- |
| `host`              | string        | required | The listener host. With `plaintext`, a loopback IP address, such as `127.0.0.1`; with `verified`, any host |
| `port`              | integer       | required | The listener port. Oracle uses `1521`                                                                      |
| `service_name`      | string        | required | The service name: letters, digits, `_` and `.`                                                             |
| `user`              | string        | required | The user that sutura connects as                                                                           |
| `password_file`     | absolute path | required | A file that holds the password. sutura reads it at startup                                                 |
| `transport_mode`    | string        | required | `plaintext` or `verified`                                                                                  |
| `transport_anchors` | absolute path | none     | For `verified`: a PEM bundle. Its certificates are the only ones the server is verified against            |

sutura connects at startup, so it does not start if the listener or the login fails. The connect to
the listener is bounded at 10 seconds. sutura refuses a listener that redirects the connection, before
it logs in, so declare the address that answers.

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

sutura connects as the one user in `user`. Every caller's query runs as that user. The source must
use `posture: shared-service-user`.
