---
title: Transport security for a source, and the certificate that already rotates
description: Optional mutual TLS to a metadata source and to a data source, configured per source and never globally - what it authenticates and what it emphatically does not, why the client trust store is a decision rather than a default, why serving-side rotation needs no work because polling the certificate bytes and swapping one Arc is already how it happens, and why the client side needs one thing more than that reuse - a pool drain, because a connection already open keeps the identity it was established under.
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

Four rules, each with the failure it prevents:

1. **A partial declaration refuses at load, naming the missing half.** A certificate with no key, or a
   key with no certificate, is the same class as `server.tls_certificate` without `server.tls_key`,
   which already refuses. Silence would mean starting up with mTLS quietly disabled.
2. **The trust store is stated, not inherited.** The serving config deliberately pulls in neither
   `webpki-roots` nor `rustls-native-certs`, because it presents a chain and verifies none. A source
   connection is the first thing here that must VERIFY one, so which anchors are trusted becomes a
   decision with a name: the system store, a pinned bundle, or a per-source anchor. Defaulting to
   whatever the host happens to trust is how a source is silently accepted from the wrong issuer.

   **So the commonest configuration is decided here rather than left to a default:** a source that
   asks for TLS and names no anchors **refuses at load, naming the source**, in the same class as
   rule 1. The system store remains available and is reached by writing it - an operator who wants
   the host's own anchors says so, and the startup line then prints that they chose it. That is
   rule 2 as a configuration rule; rule 4 is the same rule as a type, and the two agree because
   `TrustAnchors` has no default and appears in every TLS variant, so there is no value the loader
   could have filled in on the operator's behalf.
3. **Rotation applies here too, and it is the same POLL rather than the same mechanism.** The
   detection half is genuinely shared and should be reused: compare the certificate and key BYTES on
   an interval for the reasons above, validate the new pair before adopting it, and leave the old one
   in use with a loud complaint if it does not parse or does not match.

   **What is not shared is the swap, and "the same mechanism" hid the difference.** A serving
   resolver hands the new `Arc<CertifiedKey>` to the next HANDSHAKE, and a connection already
   established is untouched - that is precisely the property that makes serving-side rotation cost
   nothing. A client certificate is presented at CONNECTION ESTABLISHMENT, so a pooled connection
   that is already open keeps the identity it was established under for as long as it stays open, and
   a pool whose idle timeout is measured in hours keeps the old certificate long past the swap.
   Reusing the resolver alone would therefore rotate the material and not the identity, which is a
   rotation that reads as done and is not.

   **So a client-side swap DRAINS THE POOL, and that is a second thing to build.** After a swap, new
   connections are established with the new material and connections holding the old material are
   retired rather than reused: idle ones closed at once, in-flight ones allowed to finish. Two
   properties it has to have, because the alternative is worse than not rotating at all - draining
   must not close a connection mid-query, and a source whose material has become invalid must fail
   its next connection loudly instead of falling back to an unauthenticated one.

   **The same drain is what an expiring subject credential needs**, which is the argument for building
   it once as a pool primitive rather than per adapter:
   [a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) establishes a leg
   under a credential minted for the asking subject, and a connection established under one subject's
   credential is not a connection the next subject may reuse. Whether a connection is poolable across
   subjects at all is that record's question and not this one's; what this record contributes is that
   the answer needs retire-and-replace either way, so it is one primitive with two callers.

4. **TLS without verification has no representation, and that is a type rather than a rule.** Every
   library in this space offers the escape hatch - `danger_accept_invalid_certs`,
   `sslmode=require` versus `verify-full`, a custom verifier that returns success - and every one of
   them turns a mutually authenticated channel into an encrypted one with an unknown peer. A `bool`
   named `verify` makes that state reachable from a config file, and a `bool` defaulted to `true` makes
   it reachable from a typo. So the shape is closed and carries the anchors in the variant that needs
   them:

   ```rust
   /// How sutura secures the channel to one source. Printed at startup, per source.
   pub enum SourceTransport {
       /// No transport security. For a local file or a process-local socket.
       Plaintext,
       /// TLS, verified. There is no unverified variant, so there is no way to ask for one.
       Verified { anchors: TrustAnchors },
       /// TLS, verified, and sutura presents a client certificate.
       Mutual { anchors: TrustAnchors, identity: ClientIdentity },
   }
   ```

   Three consequences, each load-bearing. `TrustAnchors` is **required** in both TLS variants, which is
   rule 2 enforced by construction rather than by review - a source cannot silently inherit whatever the
   host happens to trust, because there is no variant that omits the anchors. `Plaintext` is a **named
   choice** an operator wrote and the startup log prints, not the absence of a setting. And a driver
   whose own API only exposes a verification flag is adapted to this enum at the boundary, so the flag
   has exactly one call site per adapter and its value is derived from the variant rather than read from
   configuration. Where a driver cannot verify at all, the adapter offers no `Verified` construction and
   the deployment refuses - which is the same shape as a source declaring an impersonation the
   deployment has no way to perform being refused at startup.

## What mutual TLS is not

**It is not impersonation, and it must never be recorded as though it were.** mTLS says "this is
sutura" to the source. The subject's identity is a separate mechanism per source, decided in
[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md): a token that the source
maps to a role, a federated exchange whose principal is the person, or proxy authentication that
records the chain. A source reached over mTLS under a shared service identity still answers the same
rows to every caller, and the answer says so, because the mode is recorded per leg.

The two compose, and that is the intended shape: the channel is mutually authenticated, and the
subject travels inside it. Writing them into the same configuration block without saying which does
what is how a deployment ends up believing it has per-user access because it has certificates.

**It is also not a substitute for the mode declaration.** Whether a source executes as the asking
subject is a declared property of that source, and mutual TLS does not change it. A source declaring
an impersonation the deployment cannot perform still refuses at startup, however well the transport is
authenticated.

## Consequences

- `sutura-config` grows per-source transport material, which is the first configuration to carry a
  secret whose value is a file path rather than a string. The path is not the secret; what it points
  at is, and the redaction must sit on the loaded material rather than on the setting.
- A client trust store enters the dependency graph for the first time. That is a supply-chain change
  and belongs in the same review as the source adapter that needs it, not ahead of it.

  **Corrected: it already entered, and not in the same review as this record.** `Cargo.lock` carries
  `rustls`, `rustls-webpki` and `webpki-roots`, pulled in by `ureq` behind
  `sutura-exec-bigquery`'s `wire` feature. So the supply-chain change this bullet schedules has
  happened, and it happened under a different record.
- **A connection pool that can be drained is a thing to build, and it is not part of the rotation code
  that already exists.** Rule 3 is the reason and it has two callers, so it is one primitive rather
  than a per-adapter habit. It also means the first pooled source adapter cannot treat pooling as an
  internal detail: retiring a connection on demand is part of what the adapter has to offer.
- Three states per source and not two: `Plaintext`, `Verified`, `Mutual`. Each needs a test, and the
  middle one is the one that gets forgotten - which is also the one whose absence used to be spelled
  "TLS with verification turned off".
- Nothing here is reachable until a network source exists. This record is written now because the
  configuration shape it implies is easier to get right before there is a source than after.

  **Corrected, and this is the most consequential correction in this record - read it as a finding
  rather than as bookkeeping.** A network source exists: `crates/sutura-serve/src/main.rs` dispatches
  `SourceKind::BigQuery` and opens an adapter over the wire. What has NOT arrived is this record's own
  shape - `SourceTransport`, `Plaintext`, `Verified`, `Mutual`, `TrustAnchors` and `ClientIdentity`
  appear nowhere under `crates/`. **So rule 2 cannot fire.** *A source that asks for TLS and names no
  anchors refuses at load* is a decided refusal with no mechanism, because the source kind that dials
  out has no anchors field to be missing, and nothing in `sutura-config` can refuse over one. The
  sentence corrected here is what hid that: while nothing was reachable, an unenforceable rule cost
  nothing. #125 is where it gets fixed, and it is one pull request with #124 - the declarable
  Postgres source - because #124 alone ships a source an operator can declare over a connection on
  which nothing verifies a certificate. `crates/sutura-exec-postgres/src/lib.rs` connects with
  `NoTls` unconditionally and says so on the module.
