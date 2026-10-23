---
title: Oracle
description: Answer questions over an Oracle Database as one declared user, or as each caller with their own token or end-user security context.
---

# Oracle

The `oracle` data system answers questions over an Oracle Database. sutura renders each plan as
Oracle SQL. It connects through [Oracle's own pure-Rust driver](https://github.com/oracle/rust-oracledb), so the Oracle Instant Client and the Oracle Client C libraries are not needed. The crate is `sutura-exec-oracle`, and the
source kind is `oracle`. The adapter is built with the `oracle` feature.

## When to use it

- Your data is in an Oracle Database that accepts OAuth 2.0 access tokens, each caller has their
  own database user, and each query must run with the permissions of that user.
- Your data is in an Oracle AI Database with Deep Data Security, its data grants decide what each
  caller may read, and callers have no database user of their own.
- Your data is in an Oracle Database, and one database user may read it for all callers.

Oracle Database 19.18 or later (not 21c) accepts these tokens.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                               | Type          | Default          | Meaning                                                                                                                                                                                                                                                              |
| --------------------------------- | ------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `host`                            | string        | required         | The listener host: a valid DNS name or an IP address, the name the TLS handshake verifies                                                                                                                                                                            |
| `port`                            | integer       | required         | The TCPS listener port, often `2484`                                                                                                                                                                                                                                 |
| `service_name`                    | string        | required         | The service name: letters, digits, `_` and `.`                                                                                                                                                                                                                       |
| `user`                            | string        | required         | The user that sutura connects as                                                                                                                                                                                                                                     |
| `password_file`                   | absolute path | required         | A file that holds the password. sutura reads it at startup                                                                                                                                                                                                           |
| `transport_mode`                  | string        | required         | `verified`. `plaintext` and `mutual` are refused                                                                                                                                                                                                                     |
| `transport_anchors`               | absolute path | required         | A PEM bundle. Its certificates are the only ones the server is verified against. `system` is refused                                                                                                                                                                 |
| `caller_session`                  | string        | `own-login`      | For `impersonation-at-source`: `own-login` sends the token from `delegation` as the login of each query's session; `end-user-context` logs each session in as `user` and sends that token as its end-user security context. Not allowed on any other posture or kind |
| `delegation`                      | list          | none             | For `impersonation-at-source`, required: the hops that exchange the caller's token, in order. Must not be present without a value                                                                                                                                    |
| `delegation[].token_endpoint`     | URL           | required         | The token endpoint of the identity provider for this hop                                                                                                                                                                                                             |
| `delegation[].client_id`          | string        | required         | The client ID that sutura uses at this identity provider                                                                                                                                                                                                             |
| `delegation[].client_secret_file` | absolute path | required         | A file that holds the client secret. sutura reads it at startup                                                                                                                                                                                                      |
| `delegation[].grant`              | string        | `token-exchange` | `token-exchange` (RFC 8693), `on-behalf-of` (Microsoft Entra ID), or `broker-token` (Keycloak sends back the token of an external identity provider). The last hop is not `broker-token`                                                                             |
| `delegation[].audience`           | string        | required         | The audience that the token from this hop must carry. With `on-behalf-of`, sutura asks for the scope `<audience>/.default`.                                                                                                                                          |

sutura connects at startup, so it does not start if the listener or the login fails. The connect to
the listener is bounded at 10 seconds. sutura refuses a listener that redirects the connection, before
it logs in, so declare the address that answers.

## Example

Each caller with their own database user. The caller signs in to Keycloak through Microsoft Entra
ID. Keycloak sends back the caller's Entra ID token, and Entra ID exchanges it for a token for the
database:

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
    delegation:
      - token_endpoint: "https://keycloak.example.com/realms/example/broker/entra/token"
        client_id: "sutura"
        client_secret_file: "/run/secrets/keycloak-client-secret"
        grant: "broker-token"
        audience: "11111111-1111-4111-8111-111111111111"
      - token_endpoint: "https://login.microsoftonline.com/00000000-0000-4000-8000-000000000000/oauth2/v2.0/token"
        client_id: "11111111-1111-4111-8111-111111111111"
        client_secret_file: "/run/secrets/entra-client-secret"
        grant: "on-behalf-of"
        audience: "22222222-2222-4222-8222-222222222222"
```

- Start Keycloak with the feature `identity-brokering-api:v2`. The `broker-token` hop uses the
  Identity Brokering API v2 of Keycloak.
- In Keycloak, the client `sutura` has `external.token.enabled` set to `true` and
  `external.token.idp` set to `entra`. The identity provider `entra` has `storeTokenInSession` set
  to `true`.
- The `entra` identity provider asks for a scope of the Entra ID app `11111111-…`. Thus the token
  of each caller has that audience, and Entra ID accepts it for the `on-behalf-of` hop.
- `22222222-…` is the Entra ID app of the database. Each caller has their own database user, for
  example `CREATE USER caller IDENTIFIED GLOBALLY AS 'AZURE_USER=user@example.com';`.

One database user for every caller:

```yaml
sources:
  warehouse:
    kind: "oracle"
    host: "127.0.0.1"
    port: 2484
    service_name: "FREEPDB1"
    user: "sutura"
    password_file: "/run/secrets/oracle-password"
    transport_mode: "verified"
    transport_anchors: "/run/secrets/oracle-ca.pem"
    posture: "shared-service-user"
    acknowledged_because: "one read-only Oracle user for every caller"
```

## Identity

This data system supports `impersonation-at-source` and `shared-service-user`.

With `impersonation-at-source`, each caller must exist in Oracle. Each caller has their own
database user, and the query runs with the permissions of that user. sutura exchanges the token
that it verified for the caller through the hops in `delegation`, in order. The first hop sends the
caller's token, and each next hop sends the token from the hop before it. sutura opens a session
for each query with the token from the last hop, sent as an OAuth 2.0 access token. The database
checks the token and maps it to the caller's database user. sutura closes the session when the
query ends.

- If a hop fails, sutura refuses the query. It sends no later hop and opens no session.
- sutura sends every verified caller's token. The database decides who may read what.
- sutura does not start without `delegation`, when the last hop is `broker-token`, or when the
  source has `subjects`.
- The source must use `transport_mode: verified`, and `delegation` needs
  `security.inbound.mode: direct`.
- sutura also connects as `user` at startup to check the source. No caller's query runs on that
  connection.

With `caller_session: end-user-context`, sutura opens a session for each query as `user` and
attaches the token from the last hop as the session's end-user security context, both as the
end-user token and as the database access token. The database checks the token and applies the
data grants of the caller it names. sutura closes the session when the query ends, so no session
carries the context of two callers.

- The database uses an identity provider that it trusts for the token: `IDENTITY_PROVIDER_TYPE`
  and `IDENTITY_PROVIDER_CONFIG`. The audience of the last hop is the audience that this
  configuration names.
- A table that the source reads needs `SET USE DATA GRANTS ONLY`. Without it, a query reads the
  table with the permissions of `user`.
- sutura sends no data roles and no attributes.
- sutura does not start when the database does not offer end-user security contexts, or when the
  source is not `transport_mode: verified`.

The database can also map the tokens of many callers to one shared schema. The query then runs with
the shared schema's grants plus the global roles that the database maps from the caller's token.

With `shared-service-user`, sutura connects as the one user in `user`. Every caller's query runs
with the permissions of that user.

sutura refuses to start when the Oracle driver's packet trace is switched on.
