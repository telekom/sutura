---
title: Oracle
description: Answer questions over an Oracle Database as one declared user, or as each caller with their own token.
---

# Oracle

The `oracle` data system answers questions over an Oracle Database. sutura renders each plan as
Oracle SQL. It connects through [Oracle's own pure-Rust driver](https://github.com/oracle/rust-oracledb), so the Oracle Instant Client and the Oracle Client C libraries are not needed. The crate is `sutura-exec-oracle`, and the
source kind is `oracle`. The adapter is built with the `oracle` feature.

## When to use it

- Your data is in an Oracle Database, and one database user may read it for all callers.
- Your data is in an Oracle Database that accepts OAuth 2.0 access tokens, and each query must run
  as the caller.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                 | Type          | Default  | Meaning                                                                                                                             |
| ------------------- | ------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `host`              | string        | required | The listener host. With `plaintext`, a loopback IP address, such as `127.0.0.1`; with `verified`, a valid DNS name or an IP address |
| `port`              | integer       | required | The listener port. Oracle uses `1521`                                                                                               |
| `service_name`      | string        | required | The service name: letters, digits, `_` and `.`                                                                                      |
| `user`              | string        | required | The user that sutura connects as                                                                                                    |
| `password_file`     | absolute path | required | A file that holds the password. sutura reads it at startup                                                                          |
| `transport_mode`    | string        | required | `plaintext` or `verified`. `mutual` is refused                                                                                      |
| `transport_anchors` | absolute path | none     | For `verified`: a PEM bundle. Its certificates are the only ones the server is verified against. `system` is refused                |
| `subjects`          | list          | none     | For `impersonation-at-source`: the caller subjects (`sub`) that may ask this source. A caller who is not in the list is refused     |

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

Each caller with their own token:

```yaml
sources:
  warehouse:
    kind: "oracle"
    host: "db.example.com"
    port: 2484
    service_name: "orcl.example.com"
    user: "sutura"
    password_file: "/run/secrets/oracle-password"
    transport_mode: "verified"
    transport_anchors: "/run/secrets/oracle-ca.pem"
    posture: "impersonation-at-source"
    subjects:
      - "0d6f2a1e-5b3c-4e8a-9f21-7c4b8e2d1a01"
      - "0d6f2a1e-5b3c-4e8a-9f21-7c4b8e2d1a02"
```

## Identity

This data system supports `shared-service-user` and secure-impersonation.

With `shared-service-user`, sutura connects as the one user in `user`. Every caller's query runs as
that user.

With `impersonation-at-source`, sutura opens a session for each query with the token that it
verified for the caller, sent as an OAuth 2.0 access token. The database checks the token and runs
the query as the database user that it maps the token to. sutura refuses an anonymous caller and a
caller who is not in `subjects`, and closes the session when the query ends. The source must use
`transport_mode: verified`. sutura also connects as `user` at startup to check the source, and no
caller's query runs on that connection. Configure the database to accept the tokens of your
identity provider.

sutura refuses to start when the Oracle driver's packet trace is switched on.
