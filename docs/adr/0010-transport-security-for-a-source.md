---
title: Transport security for a source, and the certificate that already rotates
description: Optional mutual TLS to a metadata source and to a data source, configured per source and never globally - what it authenticates and what it emphatically does not, why the client trust store is a decision rather than a default, why serving-side rotation needs no work because polling the certificate bytes and swapping one Arc is already how it happens, and why the client side needs one thing more than that reuse - a pool drain, because a connection already open keeps the identity it was established under.
---

# Transport security for a source, and the certificate that already rotates

Status: **accepted. The configuration half and the Postgres channel are built; rotation and the HTTP
adapters are not.**

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

|                              |                                                                                                       |
| ---------------------------- | ----------------------------------------------------------------------------------------------------- |
| client certificate and key   | What sutura presents. The key is a secret and travels as one, with the redaction that implies         |
| trust anchors for the source | Which certificate authority signs the SOURCE's chain, so verification is explicit rather than ambient |
| server name                  | Verified against the presented chain, and separate from the host actually dialled                     |

Four rules, each with the failure it prevents:

1. **A partial declaration refuses at load, naming the missing half.** A certificate with no key, or a
   key with no certificate, is the same class as `server.tls_certificate` without `server.tls_key`,
   which already refuses. Silence would mean starting up with mTLS quietly disabled.
2. **The trust store is stated, not inherited.** The SERVING side verifies nothing, so it declares no
   client trust store of its own - `rustls-native-certs` is absent from the graph for exactly that
   reason. (`webpki-roots` is resolved, and it arrived with the networked BigQuery adapter rather
   than with this configuration - see the correction under Consequences.) A source
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

  **Followed through by `telekom/sutura#124` and #125 as one change, and what it did NOT reach is the
  part to read.** `sutura_config::sources::transport` now owns `SourceTransport`, `Plaintext`,
  `Verified`, `Mutual`, `TrustAnchors` and `ClientIdentity`, so rule 2 fires at load: a TLS mode with
  no `transport_anchors` refuses naming the source, and the same parse refuses a source with no
  transport security and a host a network can reach - `plaintext` is a word a unix socket or a
  loopback host may write and nothing else. `Postgres` is a declarable `SourceKind` behind
  `sutura-serve`'s and `sutura-cli`'s default-off `postgres` feature;
  `sutura-exec-postgres::connect_secured` connects over `tokio-postgres-rustls` with the
  `rustls::ClientConfig` built from the declared anchor bundle, with the handshake mandatory; and
  `sutura-runtime`'s `announce_surface` prints each declared source's channel, so the mode is
  readable in the log rather than only in the tree.

  `TrustAnchors::System` is reached only by writing `transport_anchors: system`; the Postgres
  adapter then reads the host store once and refuses a partial or empty read rather than silently
  reducing it. The Postgres tier holds all three transport states: the ordinary corpus uses its
  unix socket as declared plaintext, the password role connects over verified TLS, and a dedicated
  certificate-authenticated role refuses a TLS client that presents no certificate before accepting
  the client identity the tier signed. That certificate authenticates the DEPLOYMENT as one static
  role, not the caller; it is no evidence for identity leg 2.

  **What is still not this record.** Rotation (rule 3) has no client-side implementation - the pool
  drain it needs is unbuilt. The HTTP adapters do not honour the declaration, so
  `sutura-exec-bigquery`'s wire still verifies against whatever `ureq` is configured to trust.

- **Amendment, `github.com/telekom/sutura#125`'s remainder, split across two PRs: a shared read first,
  then a second declaration and the adapter that reads it.**

  A per-source `transport_anchors` only means something when the source's own entry names the host
  it dials - Postgres, and a `datahub` catalog reader whenever one is built. The BigQuery wire and the
  STS token exchange dial a host that is a COMPILE-TIME CONSTANT (`wire.rs`'s `HOST`), shared by every
  `bigquery` entry a deployment writes, so a per-entry anchor would be exactly the "a declaration that
  does nothing" failure this issue opened against - `transport_*` STAYS refused on `bigquery` and
  `files` entries; it is not lifted. What covers a fixed-host client instead is a deployment-wide
  `security.outbound.transport_anchors` - landed with PR 2 below.

  **PR 1 lands the shared read and one refusal, with one consumer.** The bundle-or-system-store read
  and the client-identity read moved out of `sutura-exec-postgres::tls` into `sutura-tls`, a new leaf
  crate with no dependency on a crypto provider, a network client, or `sutura-config`. It returns raw
  `CertificateDer`/`PrivateKeyDer`, never a `RootCertStore` or a `rustls::ClientConfig`: which
  certificates a client trusts and how it builds its connector stays each adapter's own decision.
  `sutura-exec-postgres::tls` is migrated onto it in the same PR - its `client_config` keeps its exact
  signature and every `PostgresError` variant it can produce, and only WHERE the bytes are read moved;
  the three tests that exercised the removed private loader functions directly moved to `sutura-tls`'s
  own suite, and the tests that exercise `client_config` end to end stayed, because they are this
  crate's own claim that a refusal still comes back as `PostgresError`. **This is deliberate, not
  incidental**: landing a crate with no consumer would itself be an unread declaration -
  `cargo xtask unused-deps` and a dead-code lint would both say so - so the migration is the same PR as
  the extraction rather than a promise for later. `sutura-tls` had exactly one consumer (Postgres)
  until PR 2 added the BigQuery wire. The `InvalidSourceRegistry::TlsOverUnixSocket` parse-time refusal for a `verified`/`mutual` transport
  declared over a `unix_socket` dial lands in this PR too - unrelated to the shared crate, but the same
  "a declaration that does nothing" argument in the other direction: today it fails only at
  `PostgresWarehouse::connect_secured`'s own connect-time error, which names neither key.

  **PR 2 landed the second declaration together with its only reader**, for the identical reason PR 1's
  migration was not deferred: `security.outbound.transport_anchors` (`sutura_config::security::
  OutboundAnchors`) - a PEM bundle path or `system`, deployment-wide, anchors only, no client identity,
  because every endpoint it covers takes a bearer token and not a certificate - landed WITH the
  BigQuery wire's `WireAgent::secured` reader, the composition-root wiring (`sutura-serve`,
  `sutura-cli`), and the hermetic fake-TLS cells, so nothing merged that a deployment could write and
  have silently do nothing. A `ureq`-based adapter turns `sutura-tls`'s loaded certificates into
  `ureq::tls::Certificate` for `RootCerts::Specific` - checked against the workspace's own pinned
  `ureq`: `TlsConfig`/`RootCerts`/`ClientCert` already exist, so this needed no new HTTP client and no
  `deny.toml` change. Absent `security.outbound` is not a refusal (these clients always speak TLS
  regardless, and absence means "verify against the compiled-in roots", unchanged from every prior
  release); a PRESENT block naming no anchors is, the same argument `security.inbound` with no `mode`
  already makes.

  **A following PR adds the DataHub catalog reader as `security.outbound`'s THIRD consumer** (after the
  `BigQuery` wire and the STS exchange), reaching it through the same boot-time resolution:
  `sutura-serve`'s `main::outbound_anchors` hands its ONE loaded value to `catalog::open_catalog`
  → `open_one_datahub_catalog` → `HttpAspectReader::new(.., anchors)`, and the reader makes the same
  `ureq` fold into `RootCerts::Specific` (`sutura_catalog_datahub::tls_roots`) over the same
  `sutura_tls::LoadedAnchors` - anchors only, since a bearer takes no client certificate. So a
  deployment's declared CA now governs three fixed-host outbound clients, not two, and the same
  "never a second read, never a union" property holds for the catalog reader as for the source wire.

  `sutura-runtime` was considered and rejected as the loader's home: no `sutura-exec-*` crate depends
  on it today (`grep -l sutura-runtime crates/*/Cargo.toml` names only `sutura-app`, `sutura-mcp`,
  `sutura-cli`, `sutura-http`, `sutura-serve` - transports and composition roots, never a data-system
  adapter), and its own module header frames it as process-*global* machinery "a composition root
  calls deliberately" - the subscriber, the panic hook, the shutdown signal, the banner, the audit
  sink. A PEM/system-store read for one adapter's own outbound connection is not that, and giving a
  leaf adapter a dependency on `tracing-subscriber`/`tokio`'s signal handling to reach a loader would
  be a new, backwards edge for a five-function module. `xtask/src/boundaries.rs`'s `FORBIDDEN_EDGES`
  and `adapters.rs`'s "data systems" class name no rule against `sutura-tls` joining the graph: it is a
  leaf no `-exec-*`/`-catalog-*`/`-domain` prefix claims, so it starts in no forbidden class.

  **The inbound JWKS/discovery fetch does not exist to bring under this, and `security.outbound`'s
  generic shape is the reuse case for whenever it does.** `crates/sutura-http/src/inbound/keys.rs`'s
  own header: *"[`FileKeySet`] is the only source that ships. There is no HTTPS fetcher, and that is
  stated here rather than left to be discovered: an outbound HTTP client is a supply-chain change with
  its own review, and `docs/adr/0014` says plainly that the authorization server then becomes a hard
  runtime dependency whose outage must stay distinguishable from a dead data system. None of that is
  built."* `router.rs`'s and `protected_resource.rs`'s own "discovery" hits are this deployment
  PUBLISHING RFC 9728 metadata, not fetching anything. So an enterprise IdP behind a private CA is a
  real product case for `security.outbound`, and there is no client for it to govern yet - building the
  fetcher itself is ADR 0014's unbuilt half and a larger, separate change than #125's scope of "make an
  EXISTING client honour the declaration". `security.outbound`'s key stays generic (a plain anchors
  declaration, not named or scoped to BigQuery) for exactly this reuse: a JWKS/discovery fetcher, when
  built, reads it with no second mechanism. No hermetic cell is added against it here, since there is
  no client yet to point one at - that would be coverage for code that does not exist.

## Amendment, 2026-09-16: `sutura-serve` named four sites that are `sutura-cli` now

`sutura-serve` folded into `sutura-cli`'s `serve` module (`github.com/telekom/sutura#685` step 2),
after every one of these was written.

- `Postgres` is behind "`sutura-serve`'s and `sutura-cli`'s default-off `postgres` feature" - one
  feature, one crate, `sutura-cli`'s.
- "the composition-root wiring (`sutura-serve`, `sutura-cli`)" - one composition root.
- "`sutura-serve`'s `main::outbound_anchors`" - `sutura-cli::serve::outbound_anchors`; there is no
  `main` module carrying it.
- `sutura-runtime`'s dependents were listed as "`sutura-app`, `sutura-mcp`, `sutura-cli`,
  `sutura-http`, `sutura-serve`" - `sutura-app` was never a real one: its `Cargo.toml` names
  `sutura-runtime` only in a comment (explaining a DIFFERENT crate's feature), never in a
  `[dependencies]` entry, so the list was already off by one before the fold. Three crates depend on
  it today: `sutura-cli`, `sutura-http`, `sutura-mcp`. `grep -l sutura-runtime crates/*/Cargo.toml`
  is not a check a reader can run and get that number - it self-matches
  `sutura-runtime/Cargo.toml`'s own package-name line and also counts `sutura-app`'s comment;
  `grep -l '^sutura-runtime = ' crates/*/Cargo.toml` names exactly the three. The point the sentence
  makes - transports and composition roots, never a data-system adapter - is unaffected by the count
  moving.

## Second amendment, 2026-09-16: rotation is built, and the client side is *left-until-closed*, not drained

Rule 3's pool drain is now tested against the reality of the code that exists, and reality wins:
**there is no connection pool to drain.** `PostgresWarehouse` holds ONE connection, opened at connect
and kept for the adapter's life; `connect_secured` resolves the declared pair at connect time and a
live connection keeps the identity it was established under until it closes. Draining would close a
live connection with nothing to retire *to* - the primitive rule 3 names would be built to serve a
user that does not exist. So the Postgres swap is **left-until-closed and recorded as its own decision,
not inherited as a drain** (the "stated, not inherited" rule applied to the swap: this consumer's
choice is not rule 3's because the pool rule 3 was written for is unbuilt). When a pool lands, the
drain primitive rule 3 argues for is its own future concern.

The HTTP adapters have no drain question at all, which is what makes #125 item 3 tractable: a
`ureq` agent is **per-request**, so the next request naturally adopts whatever the rotating handle's
`current()` resolves to. BigQuery wire, the STS exchange, `iamcredentials` and the DataHub reader all
hold that shape, so all four rotate by the next request, per `docs/adr/0010`'s own swap-is-the-code's
mood.

`github.com/telekom/sutura#125` item 3 lands it with **one shared `Rotating` handle in `sutura-tls`**
(`sutura_tls::rotating`), reusing rule 3's POLL verbatim: re-read the declared material's bytes,
compare to what was last examined, and only on a difference parse-and-rebuild; adopt the new material
with one `info!` line on `Ok`, keep the old with **one** `error!` naming the source class on a load or
rebuild refusal - so a malformed replacement is loud exactly once and then silent until it changes.
`poll_once` is synchronous like the serving poller; *starting* the loop is the composition root's
choice and the interval is the same constant as the serving side (`sutura-tls::POLL_INTERVAL`, 30 s),
for the same "deployment shape, not a knob" reason. It rotates whatever was declared -
`security.outbound.transport_anchors` for the HTTP adapters, the per-source `transport_*` for
Postgres - and an absent declaration leaves `compiled-in` roots untouched. The DataHub reader is the
FOURTH consumer (`telekom/sutura#768` made it read the declaration); it was never review-held.

## Third amendment, 2026-09-19: `security.outbound` grows an optional client identity, correcting PR 2

PR 2's own text above says the deployment-wide declaration is "anchors only, no client identity,
because every endpoint it covers takes a bearer token and not a certificate" - true of the endpoint,
and stated too broadly: `github.com/telekom/sutura#911` (owner-decided 2026-09-18, on #911 and #125)
adds an optional `client_certificate`/`client_key` pair beside `transport_anchors`, declared as
PATHS TO FILES ON DISK rather than inline material, for a certificate manager that writes into a
mounted secret and rotates it in place. What it is for is never the fixed-host endpoint itself -
`HOST` still takes a bearer token on every wire this covers - it is a peer IN FRONT of one (a
gateway, a proxy) that a deployment configures to demand a certificate from the connection. Absent
is unchanged: no certificate presented, exactly as every release before this.

**The mechanism already existed; this fills in a `None`.** `sutura_tls::Rotator::new` already took
`identity: Option<Identity>` and already handed a rebuild closure `Option<LoadedIdentity>` - both
outbound HTTP call sites (`sutura_exec_bigquery::wire::WireAgent::rotating_agent`,
`sutura_catalog_datahub::http::HttpAspectReader::rotating_agent`) passed `None` for it and their
`rebuild` closures ignored the parameter they were handed. Rotation therefore comes for the identity
half the same way it already did for anchors: `sutura_tls::Declared` pairs the two (identity can
never be declared without anchors - the schema nests both under one `security.outbound:` block that
already requires `transport_anchors` whenever it is written), so a composition root threads one
`Option<Declared>` rather than two independently-optional values a call site could disagree on.

**Deployment-wide only, per PR 2's own reasoning restated rather than revisited**: the two wires this
covers dial a compile-time-constant host, so a per-entry `bigquery` identity would have nothing to
attach to - the same argument that keeps `transport_*` refused on `files`/`bigquery` source entries.

**`ureq`'s own `PrivateKey::from_der` cannot be called from outside `ureq`** - measured against
`ureq-3.4.2`'s `pub use cert::{Certificate, PemItem, PrivateKey, parse_pem}`, which never re-exports
`KeyKind`, the type that constructor's first argument needs. Both wires' `client_cert` conversion
re-armors the already-loaded DER as PEM instead (`sutura_tls::LoadedIdentity::key_kind` chooses the
label) and calls `Certificate::from_pem`/`PrivateKey::from_pem`, the pair `ureq` does expose.

**The limit stated where the claim is made, twice.** First, presenting a certificate is not a peer
verifying it: no shipped source is configured to demand one, so the new cells prove presentation and
nothing downstream. Second, this is not a leg-2 claim - it authenticates the DEPLOYMENT's transport,
not the asking subject; `AGENTS.md`'s leg-2 sentence is unchanged.
