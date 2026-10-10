---
title: Inbound identity
description: How sutura verifies who asks - a token from your identity provider, or an assertion from a gateway.
---

# Inbound identity

Inbound identity tells sutura who asks. sutura checks a signed token from your identity provider
and gets a verified subject, its scopes and its groups. sutura uses the subject for the audit
record and for the scope check. A source with `posture: impersonation-at-source` also
runs the query as the identity that its map names for that subject: a service account on BigQuery, a
user on ClickHouse. sutura refuses a subject that the map does not name. No other data system
supports this posture.

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
| `resource`              | `https` URI | required | The value that `aud` must equal exactly                                                                                       |
| `authorization_server`  | `https` URI | required | The value that `iss` must equal exactly                                                                                       |
| `key_set_file`          | path        | required | A JWK set file with the public keys of the identity provider                                                                  |
| `algorithms`            | list        | required | The accepted algorithms, of one key family: `RS256`, `RS384`, `RS512`, `PS256`, `PS384`, `PS512`, `ES256`, `ES384` or `EdDSA` |
| `token_type`            | string      | `at+jwt` | The required `typ` header. `any` turns the check off                                                                          |
| `accept_any_token_type` | boolean     | `false`  | Must be `true` beside `token_type: any`                                                                                       |

`direct` cannot be used with `security.access_token`, because both use `Authorization: Bearer`.

## Settings for `behind-gateway`

| Key                            | Type        | Default  | Meaning                                                  |
| ------------------------------ | ----------- | -------- | -------------------------------------------------------- |
| `mode`                         | string      | required | `behind-gateway`                                         |
| `transit_header`               | string      | required | The header that holds the assertion. Not `authorization` |
| `transit_issuer`               | `https` URI | required | The issuer that must sign the assertion                  |
| `transit_audience`             | `https` URI | required | The audience that the assertion must carry               |
| `key_set_file`                 | path        | required | A JWK set file with the public keys of the gateway       |
| `algorithms`                   | list        | required | The accepted algorithms, as for `direct`                 |
| `transit_token_type`           | string      | required | The `typ` that the gateway sets, or `any`                |
| `transit_max_lifetime_seconds` | integer     | `120`    | The longest accepted `exp - iat`, 1 to 3600              |
| `accept_long_transit_lifetime` | boolean     | `false`  | Must be `true` for a lifetime above 300                  |

The gateway and sutura must be on a trusted network path. An assertion is not bound to one
request, so keep its lifetime short.

## The key set

sutura reads `key_set_file` before it opens the listener, and it does not start if the file is not
a usable JWK set. It reads the file again when it is older than 60 seconds, and when a token names
an unknown `kid`. To rotate keys, replace the file.

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

A verified caller without one of these scopes gets `403 insufficient_scope`.

## Example

This is `conf/base.yaml` of the [multi player](../examples/multi-player.md) example. Keycloak is the
identity provider:

```yaml
--8<-- "examples/multi-player/conf/base.yaml"
```

[Serving over HTTP](../serving.md) has the complete serving settings: tokens, TLS and rate limits.
