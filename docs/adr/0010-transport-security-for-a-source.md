---
title: Transport security for a source, and the certificate that already rotates
description: Optional mutual TLS to a metadata source and to a data source, configured per source and never globally - what it authenticates and what it emphatically does not, why the client trust store is a decision rather than a default, and why serving-side rotation needs no work because polling the certificate bytes and swapping one Arc is already how it happens.
---

# Transport security for a source, and the certificate that already rotates

Status: **accepted. The serving half is already built; the source half is not.**

Two separate things share this record because they are constantly confused for each other, and the
confusion is the dangerous part: **mutual TLS authenticates the SERVICE to a source. It does not
authenticate the subject, and it delivers no part of impersonation.**

## What is already built, so nobody rebuilds it

Serving-side certificate rotation with no dropped connection exists in `sutura-http`'s TLS module and
needs nothing added:

- A `rustls::server::ResolvesServerCert` over a `tokio::sync::watch` channel. `ServerConfig` is built
  **once** and never rebuilt; what rotates is the `Arc<CertifiedKey>` the resolver hands out, so a
  connection already established is untouched and the next handshake gets the new chain.
- The renewal poll compares the certificate and key **bytes** against the ones in use, on an
  interval. That is deliberately not a filesystem watch, and the reason is exactly the deployment
  shape this record cares about: a Kubernetes secret mount and certbot both rotate by replacing a
  **symlink**, so an `inotify` watch registered on the file path follows the old inode and never
  fires. Watching the directory and interpreting rename events is the correct alternative and it is
  more code for the same answer. Comparing content rather than `mtime` also survives a writer that
  preserved the timestamp.
- The new pair is validated **before** the swap, because reloading into a broken state is worse than
  not reloading: every new handshake would fail. A pair that does not parse or does not match leaves
  the old one serving and says so loudly.

The cost is bounded staleness, up to one interval between the write and the swap. That is the trade,
it is written down where the constant is, and it is the right one for a certificate whose lifetime is
measured in weeks.

**So the only serving-side question left is inbound client certificates**, which is a different
feature and is not decided here. Note that it would sit beside OAuth rather than replace it: a client
certificate proves which deployment is calling, and leg 1 proves which subject is asking.

## Mutual TLS to a source, optional and per source

**Configured per source, never globally.** A metadata source and a data source each get the same
optional shape, because a deployment will genuinely mix them: one warehouse behind mTLS, one catalog
on public TLS, one local file with no transport at all.

What a source declaration may carry:

| | |
| --- | --- |
| client certificate and key | What sutura presents. The key is a secret and travels as one, with the redaction that implies |
| trust anchors for the source | Which certificate authority signs the SOURCE's chain, so verification is explicit rather than ambient |
| server name | Verified against the presented chain, and separate from the host actually dialled |

Three rules, each with the failure it prevents:

1. **A partial declaration refuses at load, naming the missing half.** A certificate with no key, or a
   key with no certificate, is the same class as `server.tls_certificate` without `server.tls_key`,
   which already refuses. Silence would mean starting up with mTLS quietly disabled.
2. **The trust store is stated, not inherited.** The serving config deliberately pulls in neither
   `webpki-roots` nor `rustls-native-certs`, because it presents a chain and verifies none. A source
   connection is the first thing here that must VERIFY one, so which anchors are trusted becomes a
   decision with a name: the system store, a pinned bundle, or a per-source anchor. Defaulting to
   whatever the host happens to trust is how a source is silently accepted from the wrong issuer.
3. **Rotation applies here too, and it is the same mechanism.** A client certificate expires exactly
   as a serving certificate does. Reuse the polling-and-swap already built rather than writing a
   second rotation path, and a source whose material becomes invalid must fail its next connection
   loudly instead of falling back to an unauthenticated one.

## What mutual TLS is not

**It is not impersonation, and it must never be recorded as though it were.** mTLS says "this is
sutura" to the source. The subject's identity is a separate mechanism per source, decided in
[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md): a token that the source
maps to a role, a federated exchange whose principal is the person, or proxy authentication that
records the chain. A source reached over mTLS under a shared service identity is still a source that
cannot carry critical data in multi-user mode.

The two compose, and that is the intended shape: the channel is mutually authenticated, and the
subject travels inside it. Writing them into the same configuration block without saying which does
what is how a deployment ends up believing it has per-user access because it has certificates.

**It is also not a substitute for the posture declaration.** Whether a source can execute as the
asking subject is a declared property of that source, and mTLS does not change it. The startup refusal
for a critical dataset behind a non-impersonating source fires regardless of how well the transport is
authenticated.

## Consequences

- `sutura-config` grows per-source transport material, which is the first configuration to carry a
  secret whose value is a file path rather than a string. The path is not the secret; what it points
  at is, and the redaction must sit on the loaded material rather than on the setting.
- A client trust store enters the dependency graph for the first time. That is a supply-chain change
  and belongs in the same review as the source adapter that needs it, not ahead of it.
- Optional means three states per source and not two: no transport security, TLS with verification,
  and mutual TLS. Each needs a test, and the middle one is the one that gets forgotten.
- Nothing here is reachable until a network source exists. This record is written now because the
  configuration shape it implies is easier to get right before there is a source than after.
