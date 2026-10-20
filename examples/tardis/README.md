# Behind a gateway of the Open Telekom Integration Platform

sutura as the upstream of the API gateway of the
[Open Telekom Integration Platform](https://github.com/telekom/Open-Telekom-Integration-Platform). The gateway is Kong
([gateway-kong-charts](https://github.com/telekom/gateway-kong-charts)) with Jumper, a sidecar of Kong
([gateway-jumper](https://github.com/telekom/gateway-jumper)), and an Issuer Service. In its One Token scenario, Jumper
creates a new token from the incoming token and the headers that Kong sends, and sends it upstream in `Authorization`
([gateway-jumper](https://github.com/telekom/gateway-jumper#one-token)). That token is the assertion that sutura
verifies in `security.inbound.mode: behind-gateway`. The settings are in [`conf/base.yaml`](conf/base.yaml).

[Serving behind a gateway](../../docs/serving-behind-a-gateway.md) has the two rules for the key set and the audience,
and [Inbound identity](../../docs/integrations/identity.md#settings-for-behind-gateway) lists every setting.

## What each setting takes from the gateway

| Setting                        | Value                    | Where it comes from                                                                                           |
| ------------------------------ | ------------------------ | ------------------------------------------------------------------------------------------------------------- |
| `transit_header`               | `authorization`          | Jumper sends the new token in `Authorization`.                                                                |
| `transit_issuer`               | `<gateway-issuer>`       | The `iss` claim of the new token: an issuer address that Jumper composes.                                     |
| `transit_audience`             | `<audience>`             | The `aud` claim. A provider can configure one.                                                                |
| `key_set_file`                 | a JWK set of the gateway | The `kid` in the token header names a certificate of the Issuer Service.                                      |
| `algorithms`                   | `["RS256"]`              | The `alg` in the token header.                                                                                |
| `transit_token_type`           | `JWT`                    | The `typ` in the token header. The body also has a `typ` claim, `Bearer`. sutura reads only the header `typ`. |
| `granted_scopes`               | the scopes to grant      | The One Token claims do not include `scope`.                                                                  |
| `transit_max_lifetime_seconds` | not set (120)            | `exp` and `iat` come from the incoming token.                                                                 |
| `transit_single_use`           | not set (`false`)        | The One Token claims do not include `jti`, which single use needs.                                            |

The claims are in [One Token](https://github.com/telekom/gateway-jumper#one-token) of the Jumper README, and its
[audience claim](https://github.com/telekom/gateway-jumper#audience-claim) section shows how a provider sets `aud`.

- **Audience.** sutura accepts an `https` URI, or a path that starts with one `/`. Configure that value as the provider
  audience of sutura, and write the same value for `<audience>`. An assertion with no `aud` is refused. For a provider
  that configures no audience, Jumper uses the audience values of the incoming token
  ([audience claim](https://github.com/telekom/gateway-jumper#audience-claim)). So use a value that no incoming token
  carries. If an incoming token can carry it, an assertion that the gateway signs for another provider has the audience
  of sutura, and sutura accepts it.
- **Lifetime.** A token has the `exp - iat` of the incoming token. If that is more than 120 seconds, set
  `transit_max_lifetime_seconds` to at least that. The limit is 3600: sutura cannot accept a token that lives longer.
  Above 300 it also needs `accept_long_transit_lifetime: true`.
- **Key set.** The signing-key Secret of the gateway holds the current key in `tls.*`, the previous key in `prev-tls.*`
  and the next key in `next-tls.*`, each with a `kid`
  ([gateway-kong-charts](https://github.com/telekom/gateway-kong-charts#automatic-certificate-and-key-rotation)). Write
  the public keys into a JWK set, each with the `kid` that the gateway puts in the header of its tokens, and no other
  key. [Rotating the keys](../../docs/serving-behind-a-gateway.md#rotating-the-keys) says
  when to add and to remove a key.

## The gateway side

How the gateway is configured is in the repositories of the platform. Its declarative configuration tool is Rover, named
in the [platform README](https://github.com/telekom/Open-Telekom-Integration-Platform), and the Control Plane is in
[controlplane](https://github.com/telekom/controlplane). The Jumper settings are in
[gateway-jumper](https://github.com/telekom/gateway-jumper).
