---
title: Inbound identity
description: How sutura verifies who asks - a token from your identity provider, or an assertion from a gateway.
---

# Inbound identity

Inbound identity tells sutura who asks. sutura checks a signed token from your identity provider
and gets a verified subject, its scopes and its groups. sutura uses the subject for the audit
record and for the scope check. A source with `posture: impersonation-at-source` also runs the
query as the caller: on BigQuery as the caller's own principal in a workload identity pool, on
ClickHouse as the user that the caller's verified subject names, and on Oracle in a session
opened with the caller's own token. No other data system supports this posture.

The settings are under `security.inbound`. There are two modes, and there is no default:

| Mode             | Use it when                                                                        |
| ---------------- | ---------------------------------------------------------------------------------- |
| `direct`         | Callers send their own access token to sutura in `Authorization: Bearer`           |
| `behind-gateway` | A gateway authenticates the caller and sends sutura a signed assertion in a header |

Without a `security.inbound` block, sutura has no caller identity. Then `security.access_token`
authenticates the deployment, and every question runs with the access of the deployment.

## Settings for `direct`

| Key                     | Type        | Default  | Meaning                                                                                                                       |
| ----------------------- | ----------- | -------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `mode`                  | string      | required | `direct`                                                                                                                      |
| `resource`              | `https` URI | required | The identifier that sutura publishes. `aud` must equal it exactly, unless `audience` is set                                   |
| `audience`              | string      | none     | The value that `aud` must equal exactly instead: an `https` URI, or a client ID of letters, digits, `-`, `.` and `_`          |
| `extra_scope_claims`    | list        | none     | More claims that grant scopes, beside `scope`: `scp`, `roles`, or both                                                        |
| `authorization_server`  | `https` URI | required | The value that `iss` must equal exactly                                                                                       |
| `key_set_file`          | path        | required | A JWK set, or one PEM public key or certificate, of the identity provider                                                     |
| `algorithms`            | list        | required | The accepted algorithms, of one key family: `RS256`, `RS384`, `RS512`, `PS256`, `PS384`, `PS512`, `ES256`, `ES384` or `EdDSA` |
| `token_type`            | string      | `at+jwt` | The required `typ` header. `any` turns the check off                                                                          |
| `accept_any_token_type` | boolean     | `false`  | Must be `true` beside `token_type: any`                                                                                       |

`direct` cannot be used with `security.access_token`, because both use `Authorization: Bearer`.

## Settings for `behind-gateway`

| Key                            | Type        | Default  | Meaning                                                                       |
| ------------------------------ | ----------- | -------- | ----------------------------------------------------------------------------- |
| `mode`                         | string      | required | `behind-gateway`                                                              |
| `transit_header`               | string      | required | The header that holds the assertion                                           |
| `transit_issuer`               | `https` URI | required | The issuer that must sign the assertion                                       |
| `transit_audience`             | string      | required | The `aud` of the assertion: an `https` URI or a path that starts with one `/` |
| `granted_scopes`               | list        | none     | The scopes of an assertion that has no `scope` claim                          |
| `key_set_file`                 | path        | required | A JWK set, or one PEM public key or certificate, of the gateway               |
| `algorithms`                   | list        | required | The accepted algorithms, as for `direct`                                      |
| `transit_token_type`           | string      | required | The `typ` that the gateway sets, or `any`                                     |
| `transit_max_lifetime_seconds` | integer     | `120`    | The longest accepted `exp - iat`, 1 to 3600                                   |
| `accept_long_transit_lifetime` | boolean     | `false`  | Must be `true` for a lifetime above 300                                       |

Some gateways authenticate the calling application, for example with its OAuth 2.0 client
credentials, and replace its token with their own assertion in `Authorization: Bearer`. For such a
gateway, set `transit_header: authorization`. sutura then reads the assertion after the `Bearer`
scheme, and `security.access_token` cannot be set, because both use `Authorization: Bearer`. Behind
such a gateway, the subject is the application that the gateway's assertion names.

The gateway and sutura must be on a trusted network path. An assertion is not bound to one
request, so keep its lifetime short.

## The key set

sutura reads `key_set_file` before it opens the listener, and it does not start if the file is not
a usable key set. It reads the file again 60 seconds after its last read, and when a token names an
unknown `kid`, at most once every 30 seconds. To rotate keys, replace the file, and wait 60 seconds
after the file holds the new key before the gateway signs with it. Behind a gateway, a key that leaves
the file verifies for `transit_max_lifetime_seconds` plus 30 seconds more, unless the new key keeps
its `kid` - see [Rotating the keys](../serving-behind-a-gateway.md#rotating-the-keys).

The file is a JWK set, or one PEM key: a single `PUBLIC KEY` block, or a certificate followed by its
chain. A file that starts with `-----BEGIN ` is read as PEM. Only the first certificate is a key,
and its validity period is not checked. The key id of a PEM key is its RFC 7638 SHA-256 thumbprint,
so a token must carry that thumbprint as its `kid`. A file with a private key, or with a second
public key, does not load.

## Related settings

| Key                            | Default | Meaning                                                                                                                    |
| ------------------------------ | ------- | -------------------------------------------------------------------------------------------------------------------------- |
| `security.identity`            | none    | `single-user` or `multi-user`. Required when a source is declared                                                          |
| `security.single_user_because` | none    | Your reason for `single-user`. Required with it                                                                            |
| `security.audience_mapping`    | empty   | Maps a `groups` claim value to the catalog audiences that it can see                                                       |
| `server.agent_surface.enabled` | `false` | Mounts `/mcp`. Needs `security.inbound`, except on a guarded `single-user` deployment ([Serving over HTTP](../serving.md)) |
| `server.allowed_hosts`         | empty   | More `Host` names to answer, beyond loopback and the host of `resource`                                                    |

## Scopes

The `scope` claim of a verified token decides what the caller may do:

| Scope                 | Allows               |
| --------------------- | -------------------- |
| `sutura:catalog.read` | Read the catalog     |
| `sutura:metrics.ask`  | Ask a question       |
| `sutura:sql.run`      | Use the raw SQL tool |

A verified caller without one of these scopes gets `403 insufficient_scope`. A scope that names
none of these grants nothing.

In `direct`, `extra_scope_claims` also reads `scp` and `roles`. Each is a space-delimited string or
a list of strings. sutura adds their scopes to the scopes of `scope`. When the setting is not set,
sutura reads only `scope`.

Behind a gateway, `granted_scopes` gives scopes to an assertion that has no `scope` claim. All
subjects that the gateway signs an assertion for without `scope` get these scopes, so sutura writes
them to the start-up log at `WARN`. An assertion with a `scope` claim, also an empty one, gets only
the scopes of that claim. `granted_scopes` cannot hold `sutura:sql.run`.

## Microsoft Entra ID

An Entra access token has `typ: JWT` and the application ID of your API in `aud`. Set:

- `authorization_server`: `https://login.microsoftonline.com/<tenant>/v2.0`.
- `key_set_file`: a copy of `https://login.microsoftonline.com/<tenant>/discovery/v2.0/keys`. Entra
  rotates its keys, so refresh the file on a schedule.
- `token_type: any` and `accept_any_token_type: true`.
- `audience`: the application ID of the API.
- `extra_scope_claims: [scp, roles]`: `scp` holds the delegated scopes of a user, and `roles` holds
  the app roles assigned to the caller, an application or a user.

In the app registration of the API, set `accessTokenAcceptedVersion` to `2`. A version 1 token has
a different issuer, and sutura refuses it. Register the API separately from every client: with
`extra_scope_claims` set, a token must name its client in `azp`, and the client must differ from the
API.

## ClickHouse token checker

A [ClickHouse source with token sign-in](data-systems/clickhouse.md#token-sign-in) needs a token
checker that you run. Keycloak cannot answer the request of ClickHouse itself: ClickHouse sends
`GET` with `Authorization: Basic`, the userinfo endpoint of Keycloak expects `Authorization: Bearer`,
and its introspection endpoint expects `POST` with client authentication.

Run a small checker in front of Keycloak. For each request, the checker:

1. Reads the user and the token from `Authorization: Basic`.
2. Verifies the signature of the token with the JWKS of the issuer.
3. Verifies the `iss` and `exp` claims, and that `aud` is the ClickHouse audience: the `audience`
   of the last hop in `delegation`.
4. Makes sure that the `sub` of the token is the Basic user.
5. Sends `200` if all checks pass, or `401` if one fails.

Keycloak puts the ID of the user in `sub`, so each ClickHouse user is named by that ID.

## Example

This is `conf/base.yaml` of the [multi player](../examples/multi-player.md) example. Keycloak is the
identity provider:

```yaml
--8<-- "examples/multi-player/conf/base.yaml"
```

[Serving over HTTP](../serving.md) has the complete serving settings: tokens, TLS and rate limits.
