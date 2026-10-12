---
title: ClickHouse
description: Answer questions over a ClickHouse server through its HTTP interface, as one declared user or as each caller's own ClickHouse user.
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

| Key                  | Type                      | Default  | Meaning                                                                                                                  |
| -------------------- | ------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------ |
| `host`               | string                    | required | The host of the HTTP interface                                                                                           |
| `port`               | integer                   | required | The HTTP port. ClickHouse uses `8123` for HTTP and `8443` for HTTPS                                                      |
| `user`               | string                    | required | The user for HTTP Basic authentication                                                                                   |
| `password_file`      | absolute path             | required | A file that holds the password. sutura reads it at startup                                                               |
| `transport_mode`     | string                    | required | `plaintext` (HTTP), `verified` or `mutual` (HTTPS)                                                                       |
| `transport_anchors`  | `system` or absolute path | none     | The CA file, or `system` for the host trust store. Required for `verified` and `mutual`                                  |
| `client_certificate` | absolute path             | none     | The client certificate. `mutual` only                                                                                    |
| `client_key`         | absolute path             | none     | The client key. `mutual` only                                                                                            |
| `delegation`         | list                      | none     | `impersonation-at-source` only: the hops that exchange the caller's token, in order. Must not be present without a value |

Each hop in `delegation` has the keys of an [Oracle](oracle.md#settings) hop.

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

This data system supports `shared-service-user` and two forms of `impersonation-at-source`:
`EXECUTE AS`, and token sign-in.

With `posture: shared-service-user`, sutura signs in as the one user in `user`, and every caller's
query runs as that user.

### `EXECUTE AS`

With `posture: impersonation-at-source` and no `delegation`, sutura still signs in as `user`, and it runs each statement
as the ClickHouse user named by the caller's verified subject (the `sub` claim of the caller's
token): `EXECUTE AS "<sub>" <statement>`. It never uses the session form of `EXECUTE AS`. There is
no map of callers: each caller's own ClickHouse user, its grants and its row policies decide what
the caller may read. A user name can contain only letters, digits and `_ . - @`. Unqualified table
names still resolve in the default database of `user`.

```yaml
posture: "impersonation-at-source"
```

Requirements:

- The server setting `access_control_improvements.allow_impersonate_user = 1`.
- A ClickHouse user for each caller, named by the caller's `sub`.
- `GRANT IMPERSONATE ON <user> TO <the user in user>` for each caller's user. `ALL ON *.*` includes
  `IMPERSONATE` on every user, so do not grant `ALL` to the user in `user`. Grant `IMPERSONATE` on
  each caller's user by name and never `IMPERSONATE ON *`, because a wildcard lets a verified caller
  run as any user their `sub` names.
- ClickHouse Cloud does not support `EXECUTE AS`.
- Only `sutura serve` serves an impersonating ClickHouse source. The `sutura` command refuses it.

A caller with no ClickHouse user, or one that the user in `user` may not become, is refused; the
query never runs as `user`. A caller whose `sub` is the user in `user` is refused.

### Token sign-in

With `posture: impersonation-at-source` and a `delegation` list, sutura exchanges the caller's token
through the hops of `delegation`. Then it signs in to ClickHouse as the caller for each query, with
HTTP Basic: the user is the caller's `sub`, and the password is the exchanged token. sutura never
sends the caller's own inbound token. sutura signs in as `user` only for its own startup queries.

```yaml
posture: "impersonation-at-source"
delegation:
  - token_endpoint: "https://idp.example.com/token"
    client_id: "sutura"
    client_secret_file: "/run/secrets/idp-client-secret"
    audience: "clickhouse"
```

A `host` that is not loopback needs `transport_mode: verified` or `mutual`, because each request
sends the caller's token. sutura does not start without it.

ClickHouse does not verify the token itself. It sends the user and the token to a token checker
that you run. sutura does not supply a checker. For each query, ClickHouse sends one request to the
checker:

```http
GET <uri>
Authorization: Basic base64(<sub>:<token>)
```

The request has no body. For each request, the checker:

1. Reads the user and the token from `Authorization: Basic`.
2. Verifies the signature of the token with the JWKS of the issuer.
3. Verifies the `iss` and `exp` claims, and that `aud` is the ClickHouse audience: the `audience`
   of the last hop in `delegation`.
4. Makes sure that the `sub` of the token is the Basic user.
5. Sends `200` if all checks pass, or `401` if one fails.

For Keycloak, see [ClickHouse token checker](../identity.md#clickhouse-token-checker).

Declare the checker in the ClickHouse server configuration:

```yaml
http_authentication_servers:
  token_checker:
    uri: "https://checker.example.com/"
```

And for each caller, a ClickHouse user named by the caller's `sub`, with its own grants and row
policies:

```sql
CREATE USER "<sub>" IDENTIFIED WITH http SERVER 'token_checker';
```

A caller with no verified subject, or whose `sub` is the user in `user`, is refused before sutura
asks the identity provider.
