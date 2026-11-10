---
title: Oracle
description: Answer questions over an Oracle Database as one declared user. Running as each caller with their own token is built and not verified.
---

# Oracle

The `oracle` data system answers questions over an Oracle Database. sutura renders each plan as
Oracle SQL. It connects through [Oracle's own pure-Rust driver](https://github.com/oracle/rust-oracledb), so the Oracle Instant Client and the Oracle Client C libraries are not needed. The crate is `sutura-exec-oracle`, and the
source kind is `oracle`. The adapter is built with the `oracle` feature.

## When to use it

- Your data is in an Oracle Database, and one database user may read it for all callers. This is
  the supported posture, `shared-service-user`.

`impersonation-at-source` is built and not verified, so it is not supported yet (see
[Identity](#identity)). It is for an Oracle Database that accepts OAuth 2.0 access tokens, where
each caller has their own database user and each query must run with the permissions of that user.
Oracle Database 19.18 or later (not 21c) accepts these tokens.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                                             | Type          | Default          | Meaning                                                                                                                                                                                  |
| ----------------------------------------------- | ------------- | ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `host`                                          | string        | required         | The listener host: a valid DNS name or an IP address, the name the TLS handshake verifies                                                                                                |
| `port`                                          | integer       | required         | The listener port, often `2484` for TLS or `1521` for native encryption                                                                                                                  |
| `service_name`                                  | string        | required         | The service name: letters, digits, `_` and `.`                                                                                                                                           |
| `user`                                          | string        | required         | The user that sutura connects as                                                                                                                                                         |
| `password_file`                                 | absolute path | required         | A file that holds the password. sutura reads it at startup                                                                                                                               |
| `transport_mode`                                | string        | required         | `verified` or the explicit weaker `native-encryption` mode below. `plaintext` and `mutual` are refused                                                                                   |
| `transport_anchors`                             | absolute path | for `verified`   | A PEM bundle. Its certificates are the only ones the server is verified against. `system` is refused; anchors are refused for native encryption                                          |
| `native_encryption_acknowledged_because`        | string        | none             | Required, nonblank reason for `native-encryption`; refused for other modes                                                                                                               |
| `legacy_password_verifier`                      | boolean       | `false`          | Explicitly permits 10G and 11G password logins, only over `verified` or `native-encryption`                                                                                              |
| `legacy_password_verifier_acknowledged_because` | string        | none             | Required, nonblank reason when the legacy switch is `true`; refused otherwise                                                                                                            |
| `split`                                         | string        | none             | `flashback`: opts the source's lookup legs into a split at one snapshot; sutura checks the flashback privileges at startup. Refused on `impersonation-at-source`                         |
| `delegation`                                    | list          | none             | For `impersonation-at-source`, required: the hops that exchange the caller's token, in order. Must not be present without a value                                                        |
| `delegation[].token_endpoint`                   | URL           | required         | The token endpoint of the identity provider for this hop                                                                                                                                 |
| `delegation[].client_id`                        | string        | required         | The client ID that sutura uses at this identity provider                                                                                                                                 |
| `delegation[].client_secret_file`               | absolute path | required         | A file that holds the client secret. sutura reads it at startup                                                                                                                          |
| `delegation[].grant`                            | string        | `token-exchange` | `token-exchange` (RFC 8693), `on-behalf-of` (Microsoft Entra ID), or `broker-token` (Keycloak sends back the token of an external identity provider). The last hop is not `broker-token` |
| `delegation[].audience`                         | string        | required         | The audience that the token from this hop must carry. With `on-behalf-of`, sutura asks for the scope `<audience>/.default`.                                                              |

A serving deployment does not start when a source with `split: flashback` is its only source, or
when no `server.nodes` peer lists the source in its `sources`. A reading worker does not start when
it declares an Oracle source without `split: flashback`.

sutura connects at startup, so it does not start if the listener or the login fails. The connect to
the listener is bounded at 10 seconds. sutura refuses a listener that redirects the connection, before
it logs in, so declare the address that answers.

### Native encryption and legacy passwords

`verified` uses TLS to encrypt the connection and verify the listener's identity against the
declared trust anchors. Prefer it when the database provides a TCPS listener.

`native-encryption` uses Oracle Native Network Encryption and Data Integrity over TCP. It requires
both services, offers only AES-256/192/128 and SHA-512/384/256, and refuses a missing service or an
algorithm outside those lists. There is no algorithm setting or fallback to plaintext. The
Diffie-Hellman exchange authenticates neither endpoint: encryption and integrity do **not** prove
the listener's identity or protect against an active attacker establishing separate sessions with
each endpoint. This mode is an explicit operator decision for a listener that provides NNE rather
than TCPS. It is never selected automatically, and startup logs it at `WARN`.

The native reason is separate from the shared-service-user posture's `acknowledged_because`.
Native encryption accepts password authentication only; the built, unverified token path still
requires `verified`.

Native mode requires the patched NNE protocol, including authentication-key folding. The older
protocol is refused even when it advertises AES and SHA-2. A pre-patch database can still use
`verified` if it offers TCPS; native mode does not enable its obsolete integrity scheme.

An account with only a 10G or 11G password verifier needs a second opt-in,
`legacy_password_verifier: true`, and its own reason. Without it, the connection refuses the legacy
verifier and names its type. An unknown verifier is always refused. A legacy login exposes material
for offline password guessing if the login exchange is visible; the encrypted transport is
required, and the native mode's unauthenticated-peer limit still applies. A password reset can
provide a modern verifier where the database's policy permits it; the switch is for accounts that
cannot migrate yet. Startup logs the legacy opt-in at `WARN`.

## Example

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

For a TCP listener that requires native encryption and integrity:

```yaml
sources:
  warehouse:
    kind: "oracle"
    host: "db.example.com"
    port: 1521
    service_name: "orcl.example.com"
    user: "sutura"
    password_file: "/run/secrets/oracle-password"
    transport_mode: "native-encryption"
    native_encryption_acknowledged_because: "the listener requires NNE and provides no TCPS endpoint"
    legacy_password_verifier: true
    legacy_password_verifier_acknowledged_because: "this account cannot yet migrate its password verifier"
    posture: "shared-service-user"
    acknowledged_because: "one read-only Oracle user for every caller"
```

Omit the two legacy fields for an account with a modern verifier. Native encryption and the legacy
fields are Oracle-only; other data systems refuse them.

Each caller with their own database user, which is built and not verified (see
[Identity](#identity)). The caller signs in to Keycloak through Microsoft Entra ID. Keycloak sends
back the caller's Entra ID token, and Entra ID exchanges it for a token for the database:

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

## Identity

The supported posture is `shared-service-user`. sutura connects as the one user in `user`, and
every caller's query runs with the permissions of that user.

sutura refuses to start when the Oracle driver's packet trace is switched on.

### Each caller with their own token

!!! warning "Built, not verified"

    sutura accepts `impersonation-at-source` on Oracle and does what this section describes. No run
    has shown an Oracle database executing a query as the caller: the tests record the token that
    sutura sends to a fake listener, and no test environment runs an Oracle that accepts a token.
    Until a run shows it, this posture is not supported.

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
- sutura does not start when the last hop asks for the audience of sutura itself.
- sutura also connects as `user` at startup to check the source. No caller's query runs on that
  connection.

The database can also map the tokens of many callers to one shared schema. The query then runs with
the shared schema's grants plus the global roles that the database maps from the caller's token.
