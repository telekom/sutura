# Serving behind a gateway

Use this page when a gateway authenticates the calling application and replaces its token with an assertion that the
gateway signs. The gateway sends the assertion in `Authorization: Bearer`. sutura verifies it and takes the caller from
it. This is `security.inbound.mode: behind-gateway` with `transit_header: authorization`.
[Inbound identity](integrations/identity.md#settings-for-behind-gateway) lists every setting of the mode.

sutura checks the signature, the issuer, the audience, the `typ` (unless `transit_token_type: any`), and the lifetime of
the assertion. It cannot check that the keys are the keys of the gateway, or that the audience belongs to the gateway
alone. Two settings decide whether the verified subject is true, and only the operator can keep them true.

## Two rules

1. **The key set holds only the signing keys of the gateway.** A key of any other signer lets that signer write an
   assertion that sutura accepts, for any subject.
2. **`transit_audience` is an audience that only the gateway issues.** If the issuer that signs for the gateway also
   gives a token with this audience to anyone else, such as the application itself, sutura accepts that token as the word
   of the gateway.

## Who the caller is

The subject of a request is the application that the assertion of the gateway names. It is not a person. Every request
from one application has the same subject in the audit record, whoever uses the application. Grant that application the
[scopes](integrations/identity.md#scopes) that it needs and no more.

`security.access_token` cannot be set in this mode. Both it and the assertion use `Authorization: Bearer`, and sutura
refuses to start with both.

The assertion is a bearer that is not bound to one request. Keep the path from the gateway to sutura trusted and the
lifetime short. With `tlsTermination: ingress`, the assertion crosses the pod network in clear text. See
[Inbound identity](integrations/identity.md#settings-for-behind-gateway).

## Single use

`transit_single_use: true` makes sutura admit each assertion once. An assertion must then carry a `jti` of 1 to 128
bytes, and sutura refuses an assertion with a `jti` that it already admitted, until that assertion expires. The gateway
must sign a new assertion, with a unique `jti`, for each request. If it uses one assertion for more than one request,
sutura refuses each request after the first. Before you set it, make sure that your gateway does this.

The setting is `false` by default. Then sutura accepts one assertion again until its `exp` plus 30 seconds: at most
`transit_max_lifetime_seconds` plus 30 seconds after its `iat`, which is 150 seconds at the default lifetime.

With the setting `true`:

- Each sutura process keeps its own list of the `jti` values that it admitted. With more than one replica, each replica
  can admit the same assertion once. After a restart, sutura can admit an assertion again that it admitted before.
- A process keeps at most 262,144 assertions that have not expired. When the list is full, sutura refuses a new
  assertion with `503 identity_unavailable` until some expire. It logs this once in each `transit_max_lifetime_seconds`
  plus 60 seconds. A shorter
  `transit_max_lifetime_seconds` lets a process admit more requests in the same time.
- Single use does not stop the first use of a stolen assertion. Keep the path from the gateway to sutura trusted.

## Rotating the keys

The gateway rotates its signing keys. The key set that sutura reads must follow the rotation:

- Add the new key to the key set before the gateway signs with it. Wait until the mounted file holds the new key, then
  60 seconds more.
- Remove an old key after the last assertion that it signed has expired. `transit_max_lifetime_seconds` bounds that time.
- Do not add a key that is not a signing key of the gateway.

sutura reads `key_set_file` again 60 seconds after its last read, and when an assertion names an unknown `kid`, at most
once every 30 seconds. It keeps the last key set that loaded when the new file does not load.

The key set can also be one PEM public key or certificate, such as the `tls.crt` that cert-manager writes. The key id
is the RFC 7638 SHA-256 thumbprint of the key, so the gateway puts that thumbprint in the `kid` of each assertion it
signs with that key. A PEM file holds one key, so it cannot hold the old key and the new key at the same time.

So sutura keeps the key set that a rotation replaced. After sutura reads a new key set, the keys that left it verify for
`transit_max_lifetime_seconds` plus 30 seconds more, and then stop. While the new file loads, a removed key therefore
stops verifying within 60 seconds, plus one read, plus that time: 210 seconds at the default lifetime of 120 seconds.
Only the key set just replaced is kept, so a second rotation in that time stops the oldest key at once. To stop a key
at once, restart sutura with the new key set.

sutura asks the new key set first. So a JWK set whose new key keeps the `kid` of the old key gives the old key no extra
time: an assertion that the old key signed is refused from the read of the new set. A PEM file cannot do this, because
its `kid` is the thumbprint of the key.

## With the chart

The chart mounts the key set from a Secret and sets `security.inbound.key_set_file` to the mounted file. Update the
Secret to rotate. Kubernetes writes the new content into the mounted file after a short delay.

The chart mounts only the `security.inbound.keySetSecretKey` entry of the Secret. Keep the gateway's own Secret in the
gateway's namespace, and copy only its certificate or public key into a Secret of sutura's own, for example with
trust-manager or a copy job.

`security.inbound.mode` in `config.environment` is enough for the chart: it asks for no access token then.

```yaml
environment: production
security:
  metricsToken:
    secretName: sutura-metrics-token
  tlsTermination: ingress
  inbound:
    keySetSecretName: sutura-gateway-keys
config:
  environment:
    security:
      inbound:
        mode: behind-gateway
        transit_header: authorization
        transit_issuer: https://gateway.example.com
        transit_audience: https://sutura.example.com
        transit_token_type: at+jwt
        algorithms: [ES256]
```

Leave `security.accessToken.secretName` empty. `security.tlsTermination` names the hop that ends TLS between the gateway
and the pod ([A published container port](serving.md#a-published-container-port) and [TLS](serving.md#tls)).
