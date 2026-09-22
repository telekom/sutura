---
title: What the BigQuery wire is built from
description: The dependency decision the BigQuery transport was gated on - four community and official clients priced against what each costs the release, the licence gate and Arrow, why every wrapper crate is refused on one transitive dependency, and why the REST endpoint over a client already in the graph cost zero new packages. Superseded by its own fifth and sixth amendments: the transport it chose was deleted for the ADBC driver, and per-subject execution is workload-identity federation of the asker's own assertion - the fifth amendment's principal switch was rejected and deleted, and the sixth says what replaced it.
---

# What the BigQuery wire is built from

Status: **superseded by this record's own fifth amendment.** The transport this record decided was
built, feature-gated, linted, tested and audited, and from 2026-08-30 it was **run against a real
project** - the first time anything in this repository had a statement accepted by `BigQuery`. It has
since been **deleted**: the ADBC driver is the adapter's only transport, and the `ureq` client, its
agent, its bounds and its credential and exchange machinery went with it. The DEPENDENCY reasoning
below is why every wrapper crate was refused and still holds as reasoning; the transport it chose
does not ship. Read the fifth amendment first, then *What is claimed, and what is not* - the two
hosted runs this line used to open on are runs of code that is no longer in the tree.

**And so are the CELLS the wire-era sections cite, which those sections state in the present tense.**
A cell named in a section above the fifth amendment lived under `src/wire/**` or in an integration
target that went with it - `exchanged_identity.rs` and the `#[ignore]`d dataset legs beside it are
gone, and `src/tests.rs` still exists but holds the ADBC transport's cells rather than the wire's - so
a reader who greps one of those names finds nothing. It is a record of what was built, not a map of
the tree; the amendments name what survived.

**Corrected:** this status block read *one hand-built `SUM` was accepted, not the corpus* while the
correction in *What is claimed, and what is not* records the corpus leg as built and run. A record
with two answers to one question is worse than one answer, and a status line is the half a reader
meets first. The limits that survive are the three `#[ignore]`d dataset legs and the single identity
the credential carries, both stated where that correction is rather than restated here.

[0017](0017-what-a-bigquery-test-runs-against.md) decided what a `BigQuery` test runs against and
left one seam deliberately empty: `sutura_exec_bigquery::transport::JobTransport`, with the sentence
that *the change that implements it is the change that can first run it against a project - which is
also where the dependency decision belongs.* This record is that dependency decision.

**It is a decision before it is code**, because the option that looks best on API surface is refused
on a transitive dependency, and an option priced only on "does the crate do what we need" would have
picked it. All four costs below are measured against this workspace on **2026-08-30** and the
commands are named, because a version number in a design record is stale before anybody builds from
it.

## What has to be priced, and why an unpriced option is not an option

| Cost                 | The mechanism that would fail                                                                                                                                                                                                                                                                                                                                                                          |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **The release**      | `sutura-cli` is the only crate any release package builds - `flake.nix` sets `cargoExtraArgs = "--package sutura-cli"` on both the native and the cross paths - across four target triples, two of which are musl. nixpkgs has no musl `libduckdb`, which is why a data system's driver is a dev-dependency here. A client with a native library repeats that problem in a source that is not optional |
| **The licence gate** | `deny.toml` runs an exact allowlist with `unused-allowed-license = "deny"`, so an allowed licence nothing uses is itself a failure. Every addition is a deliberate entry                                                                                                                                                                                                                               |
| **Arrow**            | `cargo xtask check-arrow` fails when the `arrow-*` family spans more than one major without a dated, reasoned entry in `devco/arrow-majors-allow`. The engine sets the type vocabulary and adapters conform                                                                                                                                                                                            |
| **`anyhow`**         | Not on the issue's list and it is the one that decides this. `AGENTS.md` states that *`anyhow` appears nowhere in this workspace - `Cargo.lock` included, so not even transitively*, and `cargo xtask check-boundaries` fails a dynamic-error crate in a library. A dependency that pulls it transitively spends a claim this repository makes in writing                                              |

## The options, priced

There is **no stable official Google `BigQuery` SDK for Rust.** There is an official repository -
`googleapis/google-cloud-rust` - whose `google-cloud-auth` is GA, and whose **query** client
`google-cloud-bigquery` is published as `0.16.1-preview` with its own documentation warning that it
is a preview release. Checked on 2026-08-30 against crates.io rather than recalled.

**And the obvious spelling resolves to the wrong crate.** `google-cloud-bigquery`'s highest *stable*
version is `0.15.0`, published in February 2025 by the crate's previous owner before the name was
handed to Google; that release pulls `openssl` and `arrow 53.4.1`. So `cargo add
google-cloud-bigquery` gets an eighteen-month-old release that breaks two of the four costs at once.
Written down because it is a trap a reader would otherwise walk into.

| Option                                                     | Packages added | `anyhow`             | `openssl` | Arrow      | Native build                  | Verdict     |
| ---------------------------------------------------------- | -------------- | -------------------- | --------- | ---------- | ----------------------------- | ----------- |
| `gcp-bigquery-client` 0.28.0                               | +71            | **yes**              | no        | no         | `zstd-sys` (`links = "zstd"`) | **refused** |
| `google-cloud-bigquery` 0.15.0 (what the name resolves to) | not counted    | **yes**              | **yes**   | **53.4.1** | `openssl-sys`                 | **refused** |
| `gcloud-bigquery` 1.7.0 (the same family, renamed)         | not counted    | **yes** (twice over) | no        | **58.4.0** | `aws-lc-sys`, cmake           | **refused** |
| `google-cloud-bigquery` 0.16.1-preview (official)          | +92            | **yes**              | no        | no         | `aws-lc-sys`, cmake           | **refused** |
| `reqwest` + `gcp_auth`, calling `jobs.query`               | +68            | no                   | no        | no         | `ring` only                   | viable      |
| **`ureq`, calling `jobs.query`**                           | **+0**         | no                   | no        | no         | `ring` only                   | **chosen**  |

**Every wrapper crate is refused on `anyhow`, and it is unavoidable rather than a feature flag.** It
arrives through `prost-derive` → `prost`, which every one of them pulls because they all bundle the
gRPC Storage and Write APIs beside the REST query surface. So the crate that would save us writing a
request body costs a claim this repository enforces with a gate.

Two of them are independently disqualified as well, which is worth recording because it means the
`anyhow` finding is not load-bearing on its own: `gcloud-bigquery` and the stale
`google-cloud-bigquery` each pull an Arrow major that is not the engine's, and the latter pulls
`openssl` into a workspace whose musl targets have no system OpenSSL.

### Why `ureq` and not `reqwest`

Two reasons, and the first is the port's shape rather than a preference.

**`JobTransport::run` is synchronous, because `Warehouse` is.** The service reaches it through
`sutura_runtime::spawn_carrying_span`, on a blocking-pool thread. `reqwest::blocking` satisfies that
signature by starting a runtime of its own inside a call that is already on a blocking thread;
`ureq` is natively blocking and needs none.

**And it costs nothing, which was measured rather than argued.** `ureq 3.4.0` with exactly the two
features this needs is **already resolved in `Cargo.lock`**: it arrives as a build-dependency of
`libduckdb-sys`, whose `build.rs` downloads a prebuilt library when nothing else supplied one -
`deny.toml` describes that at length and notes that the downloader never runs here, because
`nix/duckdb.nix` sets `DUCKDB_LIB_DIR` and the build sandbox has no network.

The measurement, reproducible with `cargo metadata --format-version 1 --all-features`:

```text
packages in Cargo.lock before:  446
packages in Cargo.lock after:   446
```

The only change to `Cargo.lock` is three dependency **names** added to `sutura-exec-bigquery`'s own
stanza - `serde`, `serde_json`, `ureq`. No package, no version, no licence.

`cargo tree -p sutura-exec-bigquery --features wire` says what the *compiled closure* of that crate
gains, which is the number that actually matters and is not zero: **33 packages to 52**, the
nineteen being `ureq`, `ureq-proto`, `rustls`, `rustls-webpki`, `rustls-pki-types`, `webpki-roots`,
`ring`, `untrusted`, `subtle`, `zeroize`, `http`, `httparse`, `bytes`, `base64`, `log`, `once_cell`,
`percent-encoding`, `getrandom` and `utf8-zero`. **Every one of them is already in the lock** - which
is the claim the +0 rests on and the one that was measured. How many are also already *compiled* by
some other crate is not counted here, because the number differs per feature set and per target and a
figure with neither attached would be the kind of stale number this repository's own dependency rule
tells a record not to write.

## The decision

**`jobs.query` over `ureq`, behind a default-off `wire` feature on `sutura-exec-bigquery`, with a
second narrow port for the credential.**

Four parts, each with its own reason:

1. **The REST endpoint, called directly.** `0017` already established what the call needs and the
   corpus already renders it: a `jobs.query`-shaped submission, `parameterMode` positional with an
   ordered array whose entries omit `name`, `defaultDataset` on the request, and an explicitly
   declared billing project. A `GeneratedQuery` is a statement plus an ordered parameter list, which
   is a closer fit to that request body than to any wrapper's typed builder.
2. **`ureq` with `default-features = false` and exactly `rustls` and `rustls-webpki-roots`**, chosen
   to match what `libduckdb-sys` already resolves so that nothing is added. No `platform-verifier`
   (the same binary would trust different roots on different machines), no `json` (`serde_json` is
   already a dependency), no `gzip`, `brotli`, `charset`, `cookies` or `socks-proxy` - a cookie jar
   on a client that presents a bearer token is a second credential store nobody asked for.
3. **A default-off feature**, so which side of the build the TLS stack is compiled on stays a
   decision a composition root makes in a manifest line a reviewer can see. It is not hidden from
   any gate: `just lint`, `just test`, the doctests and `deny.toml`'s `[graph] all-features = true`
   all compile it.
4. **`wire::credential::AccessTokens`, a second port.** The wire needs two things from a credential -
   a token usable now with the instant it stops being usable, and whether a request carrying it has to
   name a quota project - and everything else about how a deployment authenticates is somebody else's
   decision. A port on the first day rather than a `String` field, so that *which* credential shape a
   deployment holds is a choice of implementor.

   **What this is NOT, corrected by review because the first version of this bullet claimed it:** it
   is not yet the seam at which per-subject execution arrives as *merely another implementor*. Three
   signatures say so, and all three are in this repository rather than in a plan:
   `Warehouse::execute` receives a `&Presented`, and `BigQueryWarehouse` reads it only to call
   `deliverable` and then drops it; `JobTransport::run` receives a `JobRequest` and nothing else;
   and `AccessTokens::bearer` receives a clock and a `CallDeadline`. **So an implementation behind
   this port cannot select a credential for the presented subject, and cannot tell two concurrent
   subjects apart.** Per-subject identity is correctly out of scope here - `IMPERSONATION` is
   `NoPlaceForASubject` and the crate says so in four places - and the honest record of it is this
   paragraph rather than speculative API added now: **the step that builds it has to carry the leg's
   subject or its credential context through one of those three interfaces, and deciding which is
   part of that change.** Nothing here is a step towards it, which the *Consequences* section below
   also states.

   **Corrected: both the constant and the "reads it only to call `deliverable`" clause are stale.**
   `IMPERSONATION` is `ImpersonationCapability::PerSubjectCredential` now, and `BigQueryWarehouse`
   does more with a presented subject than call `deliverable` - `subject_bearer` extracts a
   `SubjectToken`'s material and rides it as the job's bearer. The three-signature argument about
   *this port* is otherwise unaffected: nothing here lets an implementation select a credential FOR
   the subject or tell two concurrent subjects apart, which is a property of `AccessTokens::bearer`'s
   signature rather than of the constant. What carries the leg's subject through per-leg execution is
   `sts.rs`'s `WorkloadIdentityBroker`, composed in `sutura-serve`
   (#284) - both deleted by the eighth amendment below. **Superseded 2026-09-16, and that supersession is itself WITHDRAWN 2026-09-22:** the hosted run of `bigquery-exchanged-identity` cited here did exchange each principal's own assertion against a real STS, but it ran `wire::StsOverHttp` and `wire::IamCredentialsOverHttp` - deleted by the fifth and eighth amendments below - so it is a run of code this tree does not contain and settles nothing about what ships. The `AGENTS.md` sentence quoted here no longer reads that way: leg 2 is *built and unproven* there, a source's declared map decides only WHETHER a caller may be served, and `docs/where-identity-is-proven.md` records the venue that would show a pool resolving one as `wired`.

### Which shipped artifact links what, per target

**The answer today is: none of them link either half of this crate**, and that is checkable rather
than asserted.

**Corrected: that overstates the manifest side.** `sutura-cli`'s own manifest declares the edge -
`Cargo.toml:55`'s `bigquery` feature and `:87`'s `sutura-exec-bigquery = { workspace = true, optional
= true }` - and `nix/shipped.nix:161-164` packages `sutura-serve` as a release artifact, published as
the tarball `.github/workflows/release.yml:447` uploads. What holds is narrower than "none of them
link either half":
no artifact in the table below LINKS `sutura-exec-bigquery` or `ureq` in its DEFAULT build, because
every one of them builds with the `bigquery` feature off.

| Artifact                                       | Built from                                                                                                             | Links `sutura-exec-bigquery`? | Links `ureq`? |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------- | ------------- |
| `sutura` (`x86_64-unknown-linux-gnu`, native)  | `--package sutura-cli`                                                                                                 | no                            | no            |
| `sutura` (`aarch64-unknown-linux-gnu`, cross)  | `--package sutura-cli --target …`                                                                                      | no                            | no            |
| `sutura` (`x86_64-unknown-linux-musl`, cross)  | `--package sutura-cli --target …`                                                                                      | no                            | no            |
| `sutura` (`aarch64-unknown-linux-musl`, cross) | `--package sutura-cli --target …`                                                                                      | no                            | no            |
| `oci-<triple>` for each of those four          | the unsuffixed cross package                                                                                           | no                            | no            |
| `sutura-serve`                                 | nothing - **it is built by no release package at all**, because the flake's release derivations name `sutura-cli` only | no                            | no            |

**Corrected: `sutura-serve` IS built by a release package.** `nix/shipped.nix:161-164` names it as a
shipped binary (`bin = "sutura-serve"`, `package = "sutura-serve"`), and
`.github/workflows/release.yml:447` uploads the published `sutura-serve-<target>.tar.gz`. The flake's
release derivations are not `sutura-cli`-only; the table's own "no" columns for this row still hold,
because that package builds with the `bigquery` feature off.

`crates/sutura-cli/Cargo.toml` declares no edge to `sutura-exec-bigquery`, and the root manifest
keeps the crate out of `[workspace.dependencies]` on purpose - `cargo xtask unused-deps` is what
keeps it out, because no member inherits it. So the four cross builds are byte-identical in their
dependency requirements before and after this change, and the `wire` feature is off in every
resolution any of them performs.

**Corrected: the edge exists, feature-gated.** `Cargo.toml:55` is `bigquery =
["dep:sutura-exec-bigquery", "sutura-exec-bigquery/wire"]` and `:87` is `sutura-exec-bigquery =
{ workspace = true, optional = true }` - an edge, inherited from the workspace and off unless a build
asks for the `bigquery` feature. The root manifest's own `[workspace.dependencies]` (`Cargo.toml:105`)
carries the crate too, with its own comment saying why: the entry left when nothing linked the crate
and is back because something does. `cargo xtask unused-deps` is what keeps that entry honest in
either direction, not what keeps it out.

**What it would cost when a composition root does link it**, stated now so the diff that does is not
the one working it out: `ureq` plus the eighteen crates above, compiled for that target. The musl
question is `ring`, which is the only one with a native component - and `ring` ships **pregenerated
assembly** for `linux64` on both `x86_64` and `aarch64`, so it needs a C compiler for its shims and
neither Perl nor nasm. `rustls` needs no system library at all, which is the same argument
`tokio-rustls` already carries in the root manifest for the inbound side.

**That prediction has since been run, and it held.** This paragraph said *no musl cross build was
run on this machine, so this is a prediction with its mechanism named rather than a measurement*.
`telekom/sutura#121` built it: the four `cross` jobs link `sutura-cli --features bigquery` for both
musl triples on every pull request, and the feature is twelve compiled units - `ring`, `untrusted`,
`rustls`, `rustls-pki-types`, `rustls-webpki`, `webpki-roots`, `ureq`, `ureq-proto`, `httparse`,
`getrandom 0.2`, `utf8-zero`, `sutura-exec-bigquery`. `docs/adr/0017` carries the numbers, the
derivation A/B behind them and the limits, including the one that matters here: the probe builds the
`ci` profile, so `release`'s thin LTO and `panic = "abort"` over that assembly are still unmeasured.
It remains true that no shipped artifact links the feature.

### What it costs the licence gate: nothing, and one thing changes anyway

**Zero new entries.** `deny.toml`'s allowlist already carries `ISC` (`rustls-webpki`, `untrusted`,
and the ISC half of `ring`'s `Apache-2.0 AND ISC`), `BSD-3-Clause` (`subtle`) and
`CDLA-Permissive-2.0` (`webpki-roots`), and its own comment says where they came from: *five of them
arrive under `libduckdb-sys`'s `ureq`.*

What changes is **why they are there.** Until now those five were justified by a build-script
downloader that never runs, under a dev-dependency. They are now justified by first-party code as
well, which makes them harder to remove by accident and easier to reason about - and it removes a
latent trap: had DuckDB ever been dropped as a dev-dependency, `unused-allowed-license = "deny"`
would have failed on five entries at once. The comment in `deny.toml` is updated to say both, because
a canonical comment that names one of two owners is the shape of thing that rots.

### What it costs Arrow: nothing

`ureq` names no Arrow crate, and neither does anything in its closure. `cargo xtask check-arrow`
reports the same two majors it reported before - 59 from the engine, 58 from the `duckdb` crate,
both explained in `devco/arrow-majors-allow` - and this change adds no third.

**This is also the reason the Storage Read API stays out of scope**, beyond the reason `0017` and the
issue both give. It returns Arrow, so it would put a third Arrow consumer in the graph and the
engine's major would have to win; and it *never submits the statement*, so it cannot answer a
semantic query, only read a table. If it has a role it is a later optimisation over a table we
already trust, not this wire.

## What is built

Everything below the socket, and one thing at it.

- **`wire::BigQueryWire`**, the one `JobTransport` implementor. `submit` builds the request, presents
  the bearer, reads the status and parses the answer; `validate` and `run` are the two callers with
  the two different conclusions.
- **`wire::WireAgent`**, which is what makes every claim on this page a property of a TYPE rather than
  of a call site. The client's settings used to live in a free function returning a bare
  `ureq::Agent`, and both the transport and the credential source accepted any agent - so a
  composition root writing `ureq::Agent::new_with_defaults()` got redirects on, plaintext allowed and
  no timeout, while every test passed because the tests all called the right builder. Private field,
  one constructor, a `compile_fail` doctest with a compiling twin. *A newtype parses rather than
  validates* is the rule; this was the gap.
- **`wire::JobBounds`**, and it is not a tidy-up. A job is bounded in **time** by `jobTimeoutMs` and in
  **money** by `maximumBytesBilled`, both required, both carried by the `WireAgent` so the socket
  timeout and the request body read the same value. Two corrections are folded in here, and each was a
  real defect:
  - **`timeoutMs` bounds nothing at the service.** The endpoint documents it as how long the CLIENT
    waits; when it expires the answer carries `jobComplete: false` with a `jobReference` and **the
    job keeps running and keeps billing.** The first version of this wire sent `timeoutMs: 55000`,
    returned `NotComplete`, discarded the reference and walked away from a live billable job.
    `jobTimeoutMs` is the field that is actually a deadline, and the two are now the same number so
    the client stops waiting at the instant the service cancels.
  - **Nothing in this repository bounded bytes SCANNED.** `LIMIT 10001` bounds rows returned, the
    one-page refusal bounds a page, and the 32 MiB response cap bounds what is read into memory - a
    question can satisfy all three and scan a partitioned table end to end. `maximumBytesBilled` is
    enforced at the service, which is why it beats comparing a dry run's estimate: a job that would
    exceed it fails **and is not charged.**
  - And the numbers reconcile now. `server.request_timeout_seconds` ships as **30**; the first
    version asked the endpoint to hold a job for 55 s behind a 70 s socket, so a blocking-pool
    thread could be held for up to **40 s after the request it served had gone.** The deadline is a
    parameter a composition root fills from that same setting, and the socket is the deadline plus
    five seconds of connection setup.
- **The quota project on every request.** `x-goog-user-project`, carrying the source's declared billing
  project. An application-default credential is an END-USER credential, and the endpoint's own
  direct-REST guidance requires a quota project for one - without it a valid token comes back refused
  with a message about user credentials not being supported, which reads as an authentication fault
  and is not one. The credential file's own `quota_project_id` is deliberately not read: two answers
  to *who pays* that can disagree silently is worse than one a reviewer can see in a settings file.
- **`wire::credential::ApplicationDefault`**, one `AccessTokens` implementor: it reads the file
  `gcloud auth application-default login` writes and exchanges its refresh token. That is exactly the
  fixture `0017` decided, and `just gcloud-login` is what produces it.
- **The decisions the module header states and the suite pins.** One page or a refusal - a
  `pageToken`, an incomplete job or a total the delivered count does not equal is refused, because to
  `answer()` a first page would read as *under the cap, not truncated*. The service's own result
  cache **off** - an anchor that reproduces from a cache has reproduced the cache, and a cached
  answer under a shared identity is shared across every asker. `max_redirects(0)`, so the bearer has
  no second host to follow a redirect to. The endpoint's `errors[].reason` is mapped to a closed
  `ReasonCode` before it reaches an error; textual diagnostics that remain strings - the credential
  file's `type`, an OAuth error code and an unusable page token - are bounded and character-filtered
  through **one** shared function. The free-text `message` is carried separately, bounded to a line,
  and redacted under ordinary rendering.
- **Failure is derived from the RESULT SHAPE, never from `errors` being non-empty**, and the first
  version got this wrong in the direction that matters. The endpoint documents that array as *"the
  first errors or warnings encountered"* and says entries *"do not necessarily mean that the job has
  completed or was unsuccessful"* - so refusing on it **declined successful queries that merely
  warned**, and answered a caller a `503` for a result the service had produced. What refuses is
  `jobComplete`, a `pageToken`, an absent `totalRows` and a delivered count that is not the reported
  total; the reported reason is mapped to a closed code and folded into whichever of those fires,
  which is also where a genuinely failed job lands, because the endpoint reports one as complete with
  no total.
- **The bearer's DESTINATION is a constant; its ROUTE is not.** `HOST` cannot be configured,
  `https_only` is on, `max_redirects` is `0` - so nothing a deployment writes changes which service
  receives the credential. What a deployment *can* change is the path: `ureq`'s default config is
  `Proxy::try_from_env()`, so `HTTPS_PROXY` routes these requests. That is left on deliberately, an
  egress proxy being a real deployment shape here, and it is safe because the tunnel is still TLS to
  the pinned host against a compiled-in root set - a proxy sees a hostname and no bytes. It is written
  out rather than inherited so it is a decision a reviewer can disagree with. **An earlier version of
  this record and of the module header claimed the stronger thing**, and *no deployment can choose
  where this goes* is true of the destination only.
- **The bearer does not reach a log through the client**, which was checked rather than assumed:
  `ureq 3.4.0` redacts every header outside its own `NON_SENSITIVE_HEADERS` allowlist, and
  `ureq-proto` strips `authorization` on a redirect. Recorded because `tracing-log` bridges `log`
  here, so the client's diagnostics land in the same stream as everything else.

### What is deliberately not built, each refused by name rather than mishandled

- **A service-account key.** It needs an `RS256` assertion signed with a private key. The signing is
  the dependency question, not the flow: `jsonwebtoken` is already here but with `use_pem` off, so a
  PEM key has nothing to parse it, and turning that on adds `pem` and `simple_asn1`. Deferred to the
  change that has a key to test against.
- **The metadata server**, which is how a deployment on the provider's own compute gets a token with
  no key at all. It is a plain unauthenticated `GET` and would cost **nothing** in dependencies -
  the cheapest option on this whole page. It is out because **nothing in this repository can verify
  it**: it exists only inside that provider's network, so building it would be adding an unexercised
  code path to a module whose whole point is that it does not claim more than it has.
- **Identity federation**, which is the per-subject step and an architecture decision with an owner
  outside this repository.
- **Paging.** `getQueryResults` needs the job's `location` for a dataset outside the two
  multi-regions, and `SourcePlacement::BigQuery` declares none. `0017` said the change adding the
  wire is the one that decides that field; **this change decides it by not needing it**, and the cost
  is that a result larger than one page is refused rather than assembled.
- **Retries.** A refused job arrives as `BigQueryError::Endpoint`, which is a `503` on the transport.
  Retrying inside an adapter spends a caller's request timeout on a decision the caller cannot see.
- **A token cache** - and both the cost of not having one and the reason for not having one were
  written wrongly here first, so both are corrected rather than quietly fixed.
  - **The cost is two exchanges per question, plus one per anchor.** `sutura_app::answer` calls
    `dry_run` and then `execute`; each goes through `submit` and each mints a token. The earlier
    wording, *"one extra round trip per job"*, was half the number and counted the wrong unit.
  - **The reason was wrong, and wrong in a way row 16 would have inherited.** This page said a token
    cache keyed by nothing is the credential-shaped version of the result cache this crate refuses.
    That is true of a cache shared across SUBJECTS and false for this implementor:
    `ApplicationDefault` **is** one identity, so a token held until its `not_after` is keyed by
    exactly the thing that matters and leaks to nobody. The `std::sync::Mutex` ban is not an
    argument either - `sutura-http`'s own key-set cache holds a lock.
  - **The honest reason is the small one: it is not needed until it is measured.** Nothing here has
    run against a real endpoint, minting is one round trip against a query that costs seconds and
    money, and the shape with nothing to reuse cannot get *a credential is not reused past its
    expiry* wrong - which is an assertion the per-subject step owes. **What that step must not
    inherit is a prohibition**, because caching per subject, keyed by subject, is a different
    question this decision does not answer.

## What is claimed, and what is not

**A statement this repository generated was accepted by `BigQuery` on 2026-08-30**, answered as one
complete page, and the numbers it returned were the fixture's - two bucketed sums, 42 and 99, over four
rows chosen so a wrong plan could not also produce them. Three tests green: a dry run accepted, a real
run whose values match, and a negative control (a table the dataset does not hold, refused rather than
panicking). Run from a developer's machine under a service-account key; `docs/adr/0017`'s amendment is
where the CI job that repeats it is decided.

**Two things that run did not establish, and the first is the one a badge would overstate.**

**It is ONE statement, not the corpus.** No join, no `COUNT(DISTINCT`, no `CASE WHEN`, no `NULLIF`
ratio, no `CAST(... AS FLOAT64)` and no `ISOWEEK` - and `ISOWEEK` plus `DATE_TRUNC`'s argument order
are precisely the two constructs `0017` MEASURED a parse check to be blind about, which makes them what
a live run is worth most for. The corpus-wide leg `0017` specifies - load the fixtures, run the
corpus's questions, compare rows with the engine - **is now built**, as
`corpus.rs`. **Corrected: this said the corpus-wide leg *is not
built*, which made this the THIRD answer in this file to one question** - the status block said it,
this bullet said it, and the correction further down this section contradicted both. What is still
true of the run this bullet is about is its own first sentence: that run was one statement. The count
that used to sit here is gone rather than corrected - it said *21 questions* against a corpus that
has grown twice since, and nothing here derives it.

**It says nothing about identity.** A service-account key is `SharedServiceUser`: one identity for
everybody who asks. So what is established is *accepted, and correct for that identity*.
`BigQueryWarehouse::IMPERSONATION` still reads `NoPlaceForASubject`, and nothing here is a step towards
per-subject execution.

**Corrected: the constant moved, and this section did not follow it.** `IMPERSONATION` is
`ImpersonationCapability::PerSubjectCredential` now. A leg presenting `Presented::SubjectToken` has
somewhere to go: the assertion becomes an `external_account` credential document the driver
federates against the declared pool (sixth amendment) - `Presented::SubjectPrincipal` is refused
(`BigQueryError::NoPrincipalSwitch`), since the mechanism that shape names was deleted. **What this section's premise still gets right:** the corpus leg tested here runs on the
service-account credential, so a green run says nothing about the per-subject path. What that path
needed was built, not unbuilt: `sts.rs`'s `WorkloadIdentityBroker`
minted the per-leg credential and `sutura-serve`'s `bigquery` composition attached it (#284) - both
deleted by the eighth amendment below. The
limit is narrower - **Superseded 2026-09-16, and that supersession is itself WITHDRAWN 2026-09-22:** the hosted run of `bigquery-exchanged-identity` cited here did exchange each principal's own assertion against a real STS, but it ran `wire::StsOverHttp` and `wire::IamCredentialsOverHttp` - deleted by the fifth and eighth amendments below - so it is a run of code this tree does not contain and settles nothing about what ships. The `AGENTS.md` sentence quoted here no longer reads that way: leg 2 is *built and unproven* there, a source's declared map decides only WHETHER a caller may be served, and `docs/where-identity-is-proven.md` records the venue that would show a pool resolving one as `wired`.

### The service-account flow, and what it cost

`0017`'s fixture decision named a developer's own login, and CI can only hold a key - so the credential
module reads **both** kinds, as a closed two-variant shape rather than a struct of `Option`s: a document
carrying both a refresh token and a private key is unrepresentable, so which flow runs is never
ambiguous. The service-account flow signs an assertion (`RS256`) and trades it under
`grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer`.

**It cost zero new packages, and that was verified rather than assumed.** `ring` is already in the
graph as `ureq`'s and `tokio-rustls`'s crypto provider, and it carries `RsaKeyPair::from_pkcs8` plus
`RSA_PKCS1_SHA256` - exactly the primitive and exactly the key encoding a service-account key uses, so
no `ASN.1` conversion is needed anywhere. `base64` was already resolved as a dev-dependency at the same
version and is now a real one. `Cargo.lock` is still **446 packages**.

**The alternative was priced and refused**, which is why this paragraph exists rather than a
`cargo add`: `jsonwebtoken` is already here for the inbound side and could sign, but only with its
`use_pem` feature - its DER path wants `PKCS#1` while a service-account key is `PKCS#8`. That was
measured at **one** new package (`simple_asn1` 0.6.4, ISC, already allowed) and would also have switched
the feature on for `sutura-http`, undoing half of a decision the workspace manifest states. So the
licence allowlist still needs no entry and `check-arrow` still reports the same two majors.

**What is first-party is the JWT's text and not the cryptography**, which is the line `docs/adr/0014`
draws when it argues for hand-writing a metrics exposition format and against hand-writing signature
verification in the same breath. `ring` computes the signature; this module base64url-encodes two JSON
documents and joins them with dots. And this side SIGNS rather than verifies, so the algorithm is a
constant rather than a field read off somebody else's document.

### One finding the live run produced that no local check could have

The first submission came back `400 invalidQuery`: *"Cannot access field day on a value with type
INT64"*. The fault was in the FIXTURE - the plan's metric label was the same word as the table name, and
`GoogleSQL` resolved the qualifier to the select-list alias instead of the table. Two consequences:

1. **`Definitions::assemble` refuses a dimension named after its metric; nothing refuses a metric label
   equal to the TABLE name**, and on this dialect that produces a statement the service rejects.
   Flagged rather than fixed, because it is a domain change.
2. **The endpoint's `message` is now carried on the refusal, bounded to 400 printable-ASCII
   characters.** It had been dropped on the argument that what is not read cannot be logged by accident,
   and a status plus a reason code that together say *your SQL is wrong* turned out to be
   undiagnosable. A bound answers the original concern; dropping the field answered it by removing the
   diagnostic too.

### What is still not claimed, and by what mechanism

The leg is a **smoke leg**, and it is:
`acceptance.rs`, five `#[ignore]`d tests, reached by
`bigquery-acceptance`, needing three variables a developer names in their own environment.

**And it is narrower than what 0017 and issue #70 ask for, which is stated here because a record that
promises the corpus over a test submitting one statement is the overstated-claim defect this
repository treats as a defect.** Those records ask for *the corpus's statements accepted and returning
rows* and *the rows agreeing with the engine's for the same plan*. This leg submits one hand-built
`SUM` over a two-column table a developer supplies. It therefore exercises no join, no
`COUNT(DISTINCT`, no `CASE WHEN`, no `NULLIF` ratio, no `CAST(... AS FLOAT64)` and no `ISOWEEK` - and
`ISOWEEK` and `DATE_TRUNC`'s argument order are precisely the two things 0017 MEASURED the parse check
to be blind about, which makes them what a live run is worth most for.

What it does prove on the day it runs: the endpoint accepts a statement this repository generated,
answers it as one complete page, the answer maps into domain values, and the composition fits
together - which no local test can show. **The leg the records ask for is #78's importer shape pointed
at a dataset** - load the example fixtures, run the corpus's questions, compare rows with the engine.

**Corrected:** this said *"and it is not built"*, and listed the sentences narrowed to match it -
0017's amendment, `AGENTS.md`, `docs/architecture.md`, both plan pages, the justfile recipe and the
leg's own header. **It is built**, as `corpus.rs`, and it has run
green against a real dataset; [0017](0017-what-a-bigquery-test-runs-against.md)'s third amendment is
the record of that run and says which of the four bullets it answered. **The limit next to that: the
three legs that reach a real dataset are `#[ignore]`d and outside `just validate`**, because a nix
check has no network - they run by binary selection in the acceptance tier, so a green local run
proves nothing about those three. **The file's other eight tests DO run in `checks.nextest`** - the
run-isolation, per-run naming and comparison-semantics rules, none of which needs a project - and
`corpus.rs` says *NOT `#[ignore]`d, unlike the three legs below* of one of them in as many words. An
earlier revision of this sentence said *every* test in the file is ignored and none is inside
`just validate`; it is 11 `#[test]` of which 3 are `#[ignore]`d. The correction landed there and was
not carried here, which is the sibling-drift class this record set exists to be checked against.
The narrowness that survives is the identity one, and it is stated in that file's own header rather
than restated here: the credential is one identity for everybody who asks, so a green there reads
*accepted, and correct for that identity*. The count that used to sit in this sentence is gone rather
than corrected - it said *21 questions* against a corpus that has grown twice since, and nothing here
derives it.

**Its own range was also wrong, and the fix is worth a line because of what it says about the claim.**
The first version asked for a hundred-year span, which this surface refuses as `TimeRangeTooLong`
before an adapter ever sees it - so it was asking a real endpoint a question no caller could ask.

**An unconfigured run of it FAILS rather than skipping, which is a reversal worth recording because
the first version got it wrong - and which earned its keep on its first real use, reporting
`0 passed, 3 failed` against a key the wire could not then read rather than three green ticks.** That version printed `SKIPPED - ... is not set` and
returned, and all three tests then reported PASS with no project anywhere - a green nobody asked for,
over exactly the claim the file exists to make. The compose tier does skip, correctly, because its
cells run inside `just test` and failing would break the suite on every machine with no docker; these
tests are `#[ignore]`d, so the only way to reach one is to ask for it by name. **That asymmetry is the
general rule and not a special case: skip where the runner had no choice, fail where somebody typed
the command.**

So the honest summary of `BigQuery` support in this repository is `0017`'s sentence with one word
changed once and then twice: **the statement is right as far as five mechanisms can tell, and one of
them is now a real endpoint.** The fifth mechanism is the wire's own suite, and it is worth being exact
about what it proves - because it is the part that still holds for the twenty statements nobody has
submitted:

- the request this adapter builds is the document it says it builds - asserted on the **serialized**
  body, so it is bytes and not a struct;
- the answer this adapter reads is read the way it says it is - asserted over response documents
  written here.

**Those documents are not the service's**, which is the same gap `0017` refuses to paper over one
size smaller: a response document written by the same person who wrote the decoder is not evidence
that the service sends that shape. The contract came from the endpoint's published reference.

**And the HTTP exchange itself is untested, deliberately.** The host is a compile-time constant and
the agent is built `https_only`, so there is no way to point the wire at a loopback listener. That is
a security property - no deployment can redirect the credential - paid for with a coverage hole, and
the trade is recorded rather than resolved: making the host configurable in order to test it would
remove the property the test would be checking around.

## Consequences

- `sutura-exec-bigquery` stays in AGENTS.md's *Built And Not Wired* section, one line further along.
  Nothing in it may be cited as an invariant.
  **Corrected: the section is `.agents/skills/sutura/query-surface/SKILL.md`'s *Built and not
  wired* now** - `AGENTS.md` carries no such section. Nothing named there may be cited as an
  invariant either.
- The `data_systems:` axis of `crates/sutura-app/tests/adapters/adapters.rs` still gains **no** entry.
  That registry's rule is that an entry is something somebody could deploy, and a cell that has never
  executed reads as coverage. It gains one in the change that pastes a green acceptance run.
- `sutura-serve` still refuses `kind: bigquery` by name, and correctly: it links no `BigQuery`
  adapter. Un-refusing it is a composition change that also has to answer where the deployment's
  credential comes from, and the only source built today reads a developer's own login.

  **Corrected: `sutura-serve` links the adapter now**, behind the default-off `bigquery` feature -
  `crates/sutura-cli/src/serve.rs`'s `OpenedSources::BigQuery` and `BigQuerySource` type alias to
  `sutura_exec_bigquery::BigQueryWarehouse`. A default build (the feature off) still links none of
  it, which is the sense in which "refuses `kind: bigquery` by name" survives.
- **`just validate` does not cover the acceptance leg and cannot.** The nix check sandbox has no
  network, this repository is public so a workflow secret is unavailable to a fork's pull request,
  and a gate that fails for an environment reason gets disabled. The acceptance evidence for this
  dialect lives in a developer's terminal and nowhere else.
- **A future `just update` that moves `ureq` past what `libduckdb-sys` resolves would turn the +0 into
  a real number, and that is now a gate rather than this sentence.** `cargo xtask
  check-shared-client` reads `Cargo.lock` and fails on two things: more than one `ureq` version, and
  `libduckdb-sys` no longer depending on `ureq` - the second being the PREMISE of the measurement,
  which can stop holding without anything else breaking. It runs in `just hygiene`, it has its own
  unit tests, and both rules were proved red by breaking the lock deliberately before they were
  proved green. **What it deliberately does not check is the feature sets:** 0018's stronger claim -
  *same version, same features* - needs `cargo metadata`'s resolve graph rather than the lock, which
  is a process invocation and a JSON parser in a crate with one dependency. That absence is written
  in the gate's own header, because a gate that reads as if it covered something it does not is the
  failure this repository names as its canonical example.

## Amendment, 2026-09-02: a second call at the socket, and it is not a job

*What is built* above enumerates the wire as `submit` / `validate` / `run` - three names over one
endpoint, `jobs.query`. Issue #120 added a fourth call and it is a different KIND of call, so this
record says so rather than letting a reader infer that everything here submits a job.

**What was added.** `wire::tables::list`, reached through `JobTransport::list_tables`, is a paged
`GET` on `tables.list`: it issues no statement, reads no rows, and is billed for nothing. It exists
because **both serving composition roots** need to know, before they accept anything, whether a
dataset holds the tables a bundle names - a `files` source already refuses that case and a `bigquery`
one did not - and because asking per DATASET rather than per MODEL is what makes the check affordable
at all. `sutura-serve` asks before its listener opens and `sutura-cli`'s agent surface before it
announces itself on the pipe; the decision they share is `sutura_app::preflight::ask`, and each root
renders its own sentence through its own sink.

**What it inherits without re-arguing:** the host is the same `const`, the agent is the same pinned
`WireAgent`, redirects are refused, the answer is read under `MAX_ANSWER_BYTES`, and the bearer comes
from the same credential source through the same expiry guard - factored into `source_bearer` when
this became its second caller.

**What it decides for itself, and each is a decision rather than a default:**

- **One absolute deadline for the whole listing**, not one per page. `CallDeadline` is opened once
  and each page reads what is left, so a dataset that pages slowly shortens the pages after it. A
  budget spent before the next page is `DeadlineSpent` and never a partial listing, because a partial
  listing is a wrong ANSWER rather than a slow one - this call's output is *these tables are absent*.
- **A page bound that fails rather than truncates.** `MAX_PAGES` pages of `PAGE_SIZE` is 64,000
  tables; past it the call is `ListingDidNotFinish`, for the same reason.
- **A page token is VALIDATED and never filtered.** A token is opaque, so a stripped character
  addresses a different page rather than a safe one. It is checked against the URL-unreserved set
  plus `=`, and bounded - review had to point out that *validated* named the alphabet and not a
  length, and that the only bound on it was the 32 MiB answer cap.
- **The quota project is the SOURCE's billing project and not the dataset's.** This is the one live
  bug review found here, and it is worth the paragraph: a cross-project model at
  `partner-data.shared.dim_region` on a source declared `billing_project: acme-analytics` had its
  listing attributed to `partner-data`, which the caller holds no `serviceusage.services.use` on - so
  the listing would have `403`'d while a QUERY against the same table worked. `DatasetAddress` carries
  the two projects in two accessors named for their roles, because a newtype per id prevents an
  argument-ORDER mistake and permits a ROLE mistake, and the role mistake is the one that happened.
- **A refusal is told apart from an outage, and that is a port method rather than a status check at
  the call site.** `JobTransport::listing_was_refused` answers `true` for `401` and `403` only; the
  adapter forwards it as `Warehouse::preflight_was_refused`, and **both serving composition roots**
  refuse naming the grant - `sutura-serve` before its listener opens, and `sutura-cli`'s agent
  surface before it announces itself on the pipe. `sutura query` asks nothing, deliberately: it
  answers one question on a terminal and exits, so an absent table already reaches the person who
  typed the command. Everything else - unreachable, unreadable, a document that would not decode, a
  `404` - stays a warning and the deployment serves. Without that split, a missing
  `bigquery.tables.list` grant and a momentarily dead endpoint were the same permanent warning, which
  turned the check off in the deployment least likely to read a startup log. **A `404` is deliberately
  in the warning half:** a dataset that is not there cannot be told from a name somebody is about to
  fix, and the endpoint answers `404` for an invisible project too.

**The limits, in this record's own tradition of stating them next to the claim** - counted by nobody,
because a hand-maintained number over a list that grows is the same defect as the ordinal deleted two
bullets down:

- **A live dataset HAS now answered a listing, and the documents this suite decodes are still ours.**
  The three response documents were written here, which is this record's existing limit restated for
  a second endpoint; what closes the other half of #216 is a run and not a document.
  `the_dataset_really_answers_a_listing_and_names_only_the_table_it_does_not_hold` is `#[ignore]`d
  beside its neighbours and asks a real dataset about a set of one table it holds and one it does
  not, with the clean set asserted FIRST as the control. **It has never run on a developer machine
  here** - no dataset is named in this environment, so the (since-removed) `bigquery-acceptance` leg would fail on its own
  precondition rather than reporting green. **It has RUN, green, in the `bigquery-acceptance` job** -
  on #221's own branch on 2026-09-02 and again on `main` at the commit that merged it on 2026-09-03 -
  and it keeps running there, because `--run-ignored only` reaches every `#[ignore]`d test in the
  targets that app runs rather than a set somebody has to remember to extend. **Narrowed by
  `docs/adr/0017`'s tenth amendment**: the two-principal cell is a third target with its own app,
  and this one filters that BINARY out - so the property survives per target and *every ignored test
  in the crate* stopped being the right sentence. An earlier version of this sentence
  also placed it in that run's test list by ORDINAL, and the ordinal was wrong: it is deleted rather
  than corrected, because nextest reports in completion order and a position in that list is not a
  property of the suite.
- **A dataset that is not there is answered with a non-2xx, which is the one thing no fake can say -
  and it is NARROWER than the claim this bullet first made.** The first version said
  `a_dataset_the_credential_cannot_list_is_unverified_and_never_every_table_absent` *pins the warning
  half* of `preflight_was_refused`. It does not: that decision is already held hermetically, twice
  with controls - `wire::tables`' `was_refused` suite asserts `404` warns beside `401`, `403`, `500`
  and `503`, and `a_refused_listing_and_an_unreachable_one_are_not_the_same_outcome` asserts the same
  thing one port up. A predicate over a status needs no service. **What needs one is the status
  itself:** every field of `Listing` is `#[serde(default)]`, so a `200` with an empty body for a
  dataset that does not exist would read as *every table is absent* and refuse a deployment over a
  dataset name. The leg asks a real endpoint about a fictitious dataset in a project the credential
  can see, and requires `WireError::Refused { status: 404 }` - so the empty-decode path is measured
  not to be what a missing dataset produces.
  **The oracle names the status because review caught it green for the wrong reasons.** `expect_err`
  plus a surviving `#[source]` plus `!preflight_was_refused` is satisfied by `Unreachable`, by a `500`
  or `503`, by `DeadlineSpent`, and by the two `403`s this crate deliberately puts in the warning half
  (`rateLimitExceeded`, `quotaExceeded`) - in each of which the endpoint never answered about that
  dataset, while the leg reported *a dataset that is not there warns*. The clean set asked FIRST is a
  control on a second axis: this credential really can list this project, so the failure is
  dataset-specific rather than an identity that reads nothing.
  **The REFUSAL half still is not live:** it needs an identity holding no `bigquery.tables.list`,
  which the acceptance environment's identity is not, so `401`/`403` remains a fake-transport claim.
- **A document whose shape the service changes decodes to an EMPTY listing**, because every field is
  `#[serde(default)]` - and an empty listing means *every table is absent*. That fails toward
  refusing a deployment rather than serving one, which is the right direction, and a test pins the
  behaviour so the direction is a measured property rather than a hope. On its own it cannot be told
  from an empty dataset, which really does answer with no `tables` array. **`totalItems` is what
  tells the two apart, and it is now DECODED** - a non-zero total beside a document that carried no
  readable table id is a shape change and not an empty dataset. What is worth reading about HOW,
  because each of them is a way the obvious version would have been wrong:

  - **Read as raw JSON and turned into a `ListingTotal`, never as an `Option<u64>`.** An `Option`
    already tolerates the field's absence; what it would also do is fail the WHOLE decode on a value
    spelled some other way, and a failed decode here is `NotAListing` - which
    `listing_was_refused` puts in the warning half, so the absent-table check for that dataset is
    lost whole: `Verdict::Unverified`, a `WARN` naming the source, and the deployment serves. Loud,
    and serving anyway. **This bullet read *silently* and the mechanism does not support it** - a
    review correction, and the accurate cost carries the decision on its own: a field nothing yet
    decides on must not be able to switch off the check it exists to sharpen. This service already
    spells the sibling `totalRows` as a JSON string, so a count arriving quoted is its own habit
    rather than a hypothetical, and a string is read too.
  - **Four variants rather than a number, and the two that mean *nothing to compare* are separate.**
    *The service sent no total* and *the service sent something this crate could not read* are
    different findings; the second is itself evidence the document is being generated differently.
  - **The comparison is against the entries that carried a table id this crate could READ - neither
    the ids the listing named nor the entries it merely counted**, and both halves of that are a
    wrong claim avoided. An id outside `usable_table_id`'s accepted set is dropped from the named
    set, and `BigQuery` permits one - so a dataset holding such a table names fewer ids than its own
    total claims while nothing whatever is wrong, and comparing against the named set would report
    that ordinary dataset as short of its total. **The entry count is the mistake the other way, and
    it is a review finding on this change rather than a hypothetical:** a document whose
    `tableReference` the service renamed or nested carries entries and no readable id, and counting
    entries answered `Accounted { reported: 3 }` over zero ids - the pre-flight reporting every
    table in the bundle absent while the cross-check read clean, over exactly the ambiguity the
    field is decoded to remove. Reproduced before it was fixed, and
    `a_listing_whose_entries_carry_no_readable_id_is_short_of_its_own_total` is what holds it: an
    entry with no readable id is the shape signal, an id `usable_table_id` rejected is the
    legitimate drop, and the two tests are a pair.
  - **What the value still does not reach, stated where the claim is:** a service that re-spells the
    COUNT as well as the entry leaves `Unreported` or `Unreadable`, which say *nothing to compare*
    rather than *empty dataset*; a dataset every one of whose ids this crate drops is `Accounted`
    beside no ids by design; and a `Short` whose identified count is non-zero does not separate a
    shape change from a table created or deleted between the total and the array. The raw entry
    count is not kept, so an identified count of zero merges *the array was empty* with *no entry
    carried an id* - the same finding for the only caller there is, and a third number for a
    decision that needs more.
  - **The DATASET's number, not the page's, and that is measured rather than assumed.** In the
    endpoint's own discovery document, read on 2026-09-04 at revision `20260811`,
    `TableList.totalItems` is `{"format": "int32", "type": "integer"}` - a bare JSON number -
    described as *"The total number of tables in the dataset"*, beside the neighbouring `etag`'s *"A
    hash of this page of results"*. Nothing there calls it approximate. So it is compared against a
    whole FINISHED listing: the first page's total against every page's entries, with a listing that
    ran out of pages or budget staying an `Err` rather than a comparison against a count this
    transport knows is short.

  **The decision is taken now, and it is none of the three shapes this record deferred to.**
  telekom/sutura#275 offered a refusal-half `WireError`, a `WARN`, or deleting the value; the first
  two share a premise that does not survive being written down. A `WireError` in the refusal half
  makes `listing_was_refused` - documented as *the endpoint REFUSED* - answer `true` for something
  that is not a refusal, and BOTH roots render that verdict as *grant this identity
  `bigquery.tables.list`*, which is the same defect one sentence over: an operator sent to fix a
  thing that was never wrong. And a `WARN` is not the cheap option in this tree, it is a
  **loosening** - the case refuses TODAY, for the wrong reason, so warning would let a deployment
  serve that does not serve now, which is the direction this paragraph already named as worse.

  **So the refusal is a VALUE on the answer and not an `Err`** - the rule `ToolOutcome::Refusal`
  holds on the query path, applied one port down, and it is what makes the `Err`-is-the-warning-half
  trap irrelevant rather than worked around. `TablesPresent` gains a fourth variant,
  `Unaccounted { tables, shortfall }`; `sutura_app::preflight::ask` maps it to a sixth `Verdict` with
  no wildcard arm, so **both composition roots failed to compile until each decided**, and both
  refuse. Neither names a model behind the tables, deliberately: nothing here establishes that a
  `table:` is wrong.

  **And it is narrow, which is the half a blunt version would have got wrong.** A table a short
  listing NAMED is present - a listing cannot un-name an entry it carried - so a short listing that
  still named everything the bundle asks about answers `All`, and an ordinary create-or-delete race
  over tables nobody asked about reddens no boot. Only the tables the listing did not reach land in
  the new answer. `a_listing_short_of_its_own_total_still_answers_on_the_tables_it_named` was the
  pin on the non-decision and is gone, replaced by four cells: the gap, the short listing that is
  still clean, the accounted listing that still names an absence, and the precedence between a
  definite absence in one dataset and a gap in another.

  **Two things review broke before this was believable, and both are why the shape changed.** The
  first: `ListingTotal::Short`'s fields were public and `HeldTables::of` is a `pub const fn`, so a
  `Short` whose reported total sat BELOW its identified count was constructible - the pre-flight then
  saturated the subtraction to zero and **fell back to reporting the bundle's tables absent**, the
  defect itself, reached through the public API without mutating anything. `reported > identified`
  was held by an `if` one module away. It is `transport::Shortfall`'s now - `identified` plus a
  `NonZeroU64` gap, one fallible `parse`, no public fields - and the fallback that needed it is
  deleted rather than documented. **A type that forecloses a zero is worth nothing while a
  constructor can route around it.** The second: a gap of one was printed beside three unnamed
  tables, as though the set and the shortfall were one quantity. They are not - the gap BOUNDS how
  many of the set it can explain, so two of those three really were missing - and both roots say *at
  most N of these M* for that reason. **The third, a round later, is that bound running the other
  way:** a shortfall counts tables the data system did not account for anywhere in the DATASET while
  the set is only the part the bundle names, so the shortfall can be the larger number - and on this
  check's own shape it always is, since an identified count of zero makes it the dataset's whole
  table count. Both roots printed *at most 9 of the 2 table(s)*.
  `UnaccountedTables::explained_by` is the clamp, in the domain, because two roots each remembering
  a `min` is the rule held by recall this repository does not accept. It hid because every cell but
  the two that RENDER runs a gap larger than its set.

  **An unreadable count is a separate, count-free refusal (`telekom/sutura#443`).** `Unreadable`
  retains the whole listing's identified count, before unsupported names are filtered out. Beside
  zero readable IDs it now produces `UnreadableInventory(UnaccountedTables)`, carried through the
  application verdict to both roots. There is no invented shortfall and neither root names a model
  to fix. Readable-but-rejected IDs still count, so an ordinary unsupported name cannot trigger this
  refusal merely because the named set is empty. The counted `Unaccounted` shape and its nonzero
  bound remain unchanged.

  Among successful listing answers, a definite absence is reported first, then an unreadable
  inventory, then a counted gap. This is a diagnostic policy, not an ordering inherent in the
  evidence: all three refuse startup. Their table sets stay separate, so an unreadable dataset's
  message does not relabel tables from a counted-gap dataset. The existing transport-error `?`
  still stops the listing walk; this precedence makes no promise over a failed metadata request.

  **What remains outside the decision.** `Unreported` beside no ids is
  deliberately not in that issue, and the reason is the variant's own: *an empty listing and an empty
  dataset are one value* there, so nothing in the document tells them apart and a refusal would be
  taken on evidence that does not distinguish them - it is where every boot stood before the field
  was decoded. A dataset every id of which `usable_table_id` drops is `Accounted` beside no ids and
  is an ordinary dataset. An unreadable total with a nonzero identified count also retains its
  previous presence/absence reading. A gap is not a diagnosis, so a
  document whose shape changed and a table created or dropped mid-listing are one answer. A definite
  absence outranks a gap ACROSS datasets, so where a bundle has both the gap waits for the next boot;
  within one dataset the two numbers are carried side by side instead. Both refuse, so nothing serves
  that would not have. And the case has still **never been seen live** and cannot be provoked from
  the acceptance environment: what holds its meaning is the hermetic suite over documents, and the
  live leg establishes only that the field arrives.

  **The correction worth recording rather than quietly making:** the version of this bullet before
  #263 deferred the question to *the live run above*, and that run could not make the measurement in
  either direction - the leg asserts on `TablesPresent`, and the decoder read no such field, so no
  `totalItems` value reached an assertion, a panic message or a log line. A deferral pointing at
  evidence that cannot bear it is the overstatement this record is otherwise built to avoid.
  `a_real_listing_reports_a_total_and_it_accounts_for_the_entries_it_carried` is what measures it
  now, in the (since-removed) `bigquery-acceptance` leg, one rung BELOW the domain port because `TablesPresent` carries
  no count and should not. It prints the verdict and requires a total the crate could read - red, not
  silent, if the service populates nothing, because a cross-check whose input never arrives has no
  teeth.

  **And it has now RUN, green, in the `bigquery-acceptance` job on 2026-09-04**, which is what the
  earlier deferral was owed: a real `tables.list` answered `ListingTotal::Accounted`, so the service
  populates the field and its number agreed with the readable table ids the same document carried.
  The cross-check has a real input, and *whether it does* is no longer the open question. **What that
  run does not establish, counted by nobody:** it is ONE dataset at one moment, and a second
  deployment's service behaviour is not a property this run establishes; the leg
  deliberately does not require the total to be EXACT, because the corpus leg writes four tables to
  the same dataset and a moving number would be a leg failing for a reason outside the diff; and the
  case the cross-check exists for - a document carrying no readable table id beside a non-zero total
  - has never been seen live and cannot be provoked from here, so what holds its MEANING is the
    hermetic suite over documents. **The 2026-09-04 run predated the counting correction above**, so
    what it established is that the field arrives and is comparable at all - not which basis the
    comparison is made on, because `Accounted` over an entry count says nothing about readable ids.
    **The job has now answered that too, green on the head carrying the correction**: a real
    `tables.list` still answers `ListingTotal::Accounted`, and the total it reported equalled the
    usable ids the same document carried - so on that dataset every entry carried an id this crate can
    read, and the corrected count did not turn an ordinary listing into a shape change. One dataset at
    one moment, again, and the case the cross-check exists for is still not among the things a live run
    here has seen. It has never run on a developer machine either, for the reason the leg above
    it has not: no dataset is named in this environment.
- **A real listing DOES now reach the pre-flight decision, and what stays fake is each root's
  wording.** Issue #120's own verification asked for a run asserting the boot refusal, and until
  `a_real_listing_reaches_the_boot_decision_and_names_the_model_behind_the_absent_table` the two
  halves did not meet: the legs above assert the `TablesPresent` that `BigQueryWarehouse::preflight`
  returns, and every test of the decision above it ran against a `Warehouse` fake. That leg loads a
  bundle through `sutura_catalog_local::LocalCatalog` naming one table the dataset holds and one it
  does not, hands it to `sutura_app::preflight::ask` over a real warehouse, and requires
  `Verdict::Absent` naming the absent table and the **model** behind it - with the clean bundle
  answering `Verdict::Present` first, because an empty listing produces `Absent` too. **An earlier
  version of this bullet called the seam structurally unreachable from here, and it was wrong by one
  dependency edge:** `sutura-app` is already a dev-dependency of this crate and
  `sutura_app::preflight::ask` is the decision sequence *both* composition roots call, which
  `sutura_serve::boot::refuse_absent_tables`' own documentation states. **What genuinely stays out of
  reach is the words and the sink** - each root's own sentence for each verdict, through its own
  transport's sink. Those are `pub(crate)` in crates that depend ON this one, and they are rendering
  rather than decision; a run that asserted them would be an acceptance leg in a composition root,
  which is a different crate and its own change.
- **That leg's own harness has a control, and what holds it is prose rather than a gate.** `ask`
  **skips** a source the bundle names no model in, so a scratch bundle that reached this leg's source
  with nothing - a `source:` that stopped matching, a document the parse refused - would hand the
  decision an empty question rather than fail. So
  `a_scratch_bundle_really_names_the_models_this_legs_own_source_is_asked_about` is the one test in
  that file which is not `#[ignore]`d; the acceptance file's own header carries the venue argument.
  **Provoked both ways under `just test` on 2026-09-03, which is the venue and is named because a
  mutation result without one is the shape this record is about:** the document's `source:` pointed
  elsewhere gave `left: []` against the two models expected, and a harness writing no document at all
  failed on the catalog adapter's own `Empty { path: ... }`. Each edit reverted after. **The direction nothing holds is the reverse one** - `#[ignore]` added to
  that control drops it out of every gate into the credentialled venue alone, silently. The
  *dangerous* direction is already mechanical, because a live leg missing `#[ignore]` reaches
  `Fixture::required()`, which fails rather than skips. What would hold the other is a line-scan gate
  asserting the partition - every `#[test]` in those two targets that reaches `Connection::required()`
  is `#[ignore]`d, and every one that does not is not - on `check-boot-order`'s pattern. Not built
  here: it is an `xtask` module plus two classification-table rows, which is its own change.
- **`tables.list` reports existence and nothing else.** Not the columns a model names, and not
  whether the identity that will ask a question may read the rows: a listing grant and a read grant
  are two grants. An anchor is what covers both, for the metrics that have one.

## Second amendment, 2026-09-11: the refusal's `Display` no longer carries the endpoint's message

The finding above - the endpoint's `message` is kept on the refusal - is narrowed where it was
weakest. The message is still carried on the type and still bounded to 400 printable-ASCII
characters, and `Debug` still redacts it. What changed is `Display` on `WireError::Refused`: it
renders `{status}` and the closed local reason code and **no longer interpolates `detail`**. That was
the path a cause-chain walk takes - the transports' sinks flatten each link with `Display` - so a
deployment's own log used to carry the endpoint's free text, which on a `403` quotes the resource and
the principal it refused.

`EndpointMessage` also loses its `Display` implementation, so the raw sentence is reachable only
through `EndpointMessage::as_str`, named on purpose. Every rendering this error can meet is therefore
one of: status plus a closed reason code (`Display`), a redacted marker (`Debug`), or an explicit
accessor.

**The limit, stated next to the claim.** The endpoint's message is still a string on the error TYPE,
and a caller that deliberately calls `as_str` can render it. This removes the accident, not the
capability, and it says nothing about what the endpoint records on its own side. The tests are
`the_endpoints_own_message_is_redacted_under_debug_and_absent_from_display` in
`tests.rs`, and
`a_refusal_this_leg_dies_on_names_the_reason_and_never_the_message` in
`exchanged_identity.rs`, which holds that the leg goes through the
status-and-reason shape rather than rendering the error.

## Third amendment, 2026-09-12: provider reason text is closed before ordinary rendering

The endpoint's `errors[].reason` is provider-owned input. Bounding and filtering its characters still
allowed an arbitrary identifier to reach `WireError::Refused`'s `Display`, so the wire now maps it to
the closed `ReasonCode` vocabulary at decode time. Known decisions, including `responseTooLarge`,
`rateLimitExceeded` and `quotaExceeded`, retain their behavior; an absent or unrecognized provider
value renders only a static local marker. The same type is carried by incomplete-job and missing-total
errors, so no ordinary rendering of a shape-derived diagnostic can carry provider text either.

`an_unrecognized_provider_reason_cannot_reach_ordinary_error_rendering` in
`tests.rs` is the regression test for the boundary.

## Fourth amendment, 2026-09-16: `sutura-serve` folded into `sutura-cli`, and the artifact table's own row with it

`sutura-serve` folded into `sutura-cli`'s `serve` module (`github.com/telekom/sutura#685` step 2),
after every site below was written. Three, in the base body rather than in an earlier amendment:

- *The decision* named `WorkloadIdentityBroker` as "composed in `sutura-serve` (#284)". It is composed
  in `sutura-cli`'s `serve` module now. **Superseded 2026-09-16, and that supersession is itself WITHDRAWN 2026-09-22:** the hosted run of `bigquery-exchanged-identity` cited here did exchange each principal's own assertion against a real STS, but it ran `wire::StsOverHttp` and `wire::IamCredentialsOverHttp` - deleted by the fifth and eighth amendments below - so it is a run of code this tree does not contain and settles nothing about what ships. The `AGENTS.md` sentence quoted here no longer reads that way: leg 2 is *built and unproven* there, a source's declared map decides only WHETHER a caller may be served, and `docs/where-identity-is-proven.md` records the venue that would show a pool resolving one as `wired`.
- *Which shipped artifact links what, per target*'s table carried a whole row for `sutura-serve` as
  its own release artifact, and a `Corrected:` note beneath it saying `nix/shipped.nix:161-164` names
  it as a shipped binary. Both are spent: `nix/shipped.nix`'s `binaries` list has one entry now
  (`bin = "sutura"`, `package = "sutura-cli"`), so there is no second row - the four native artifacts
  and their `oci-<triple>` twins above are the whole table, and each still links neither
  `sutura-exec-bigquery` nor `ureq` in its default build, unchanged.
- *What is claimed, and what is not*'s `Corrected:` note said "`sutura-serve` links the adapter now",
  citing `crates/sutura-cli/src/serve.rs`'s `OpenedSources::BigQuery` - the citation already named the
  post-fold file; only the crate name in the sentence was `sutura-serve`. It is `sutura-cli` linking
  the adapter, through the same type alias, unchanged otherwise.

## Fifth amendment, 2026-09-20: ADBC replaces the wire, and impersonation becomes a principal switch

The `wire` transport is gone. `adbc-drivers/bigquery` (Go, Apache-2.0), self-built per release triple
by `nix/bigquery-adbc.nix` and pinned at `go/v1.13.0`, is the adapter's only transport, reached
through `adbc_core` + `adbc_driver_manager` over the C ABI. The driver owns its own HTTP and its own
authentication, so this crate ships no client and reads no credential file. Everything this record
prices about the four wrapper crates - `anyhow` in a library, an Arrow major this workspace cannot
carry - is a property of those crates and is unaffected; what is spent is the `ureq` conclusion it
ends on.

**The identity claim this record carried has changed mechanism, and the new one is weaker in a way
that has to be written down.** The wire forwarded the asking subject's exchanged access token as the
job's own bearer: Google's token service verified the caller's assertion against a workload-identity
pool, `iamcredentials.generateAccessToken` resolved it to the account the declared per-source map
named, and the job ran under that account. The pinned ADBC driver has **no option that accepts a
per-subject access token** - its auth types take a credential file, a credential JSON document, or an
OAuth client id/secret/refresh-token triple, and its `bigquery.impersonate.*` options build an
impersonated token source from the process's own application default credentials
(`go/connection.go`'s `newClient`, which passes no client options to
`impersonate.CredentialsTokenSource` and replaces any other auth option with it).

So `IMPERSONATION` stays `PerSubjectCredential` and what delivers it is
`bigquery.impersonate.target_principal`, set per job to the account the declared map names for the
asking subject. The domain already had the shape for that and already called it the weaker one:
`Presented::SubjectPrincipal` is *a principal the data system switches to, on a connection the
DEPLOYMENT authenticated.* The chain is now **leg 1 verifies the caller, this deployment maps the
verified subject to a declared account, and this deployment's own identity is authorized to become
it** - so the caller's possession of a credential is checked by sutura and by nobody else on the
path, and the deployment holds `roles/iam.serviceAccountTokenCreator` on every account it declares.
`docs/where-identity-is-proven.md` carries what a run would have to show; leg 2 is **not** proven on
this transport.

### The two alternatives, priced

**Let the driver do the whole federation.** `bigquery.auth_type = json_credential_string` with a
credential JSON of Google's `external_account` type reproduces the deleted chain exactly, inside the
driver: the pool audience, the token endpoint and a `service_account_impersonation_url` naming the
declared account, with the caller's assertion as the subject token. It keeps the property this
amendment gives up - Google verifies the caller's assertion - and every value it needs is already in
the settings tree. **The cost is that the subject token has to be on a filesystem path**: in
`cloud.google.com/go/auth v0.23.2` an external account's `credential_source` is a file, a URL, an
executable or an AWS metadata endpoint, and the programmatic supplier is a Go-level interface with no
JSON spelling. That means writing a verified caller's own assertion to disk, once per request, with a
lifetime nothing in the type system bounds - and `docs/adr/0020` is the record of a repository that
treats a credential reaching a log as the defect. Refused here, and it is the option to revisit if
the trust chain matters more than the exposure.

**Add the option upstream.** A driver option taking an already-minted access token
(`option.WithTokenSource(oauth2.StaticTokenSource(..))`) is about fifteen lines of Go and would let
sutura keep the exchange in process and hand the result to the driver. It needs the exchange back -
the deleted `StsOverHttp`/`IamCredentialsOverHttp` hops, or an HTTP client to rebuild them on - so it
is a larger change than the one this amendment makes, and it belongs upstream rather than as a vendor
patch (`VENDOR.md` records the three source-level adaptations this build already carries, and a forked
public option surface is a different kind of debt from a `go.mod` floor relax). An unpatched driver
refuses an unknown option key rather than ignoring it, so a deployment pointed at an upstream `.so`
would fail closed - which is what makes this safe to propose and unsafe to assume.

### The values are BOUND, and the first shape of this amendment refused every question

`adbc_core::Statement::bind` takes one Arrow `RecordBatch` and the driver reads a `bigquery`
query parameter per column (`record_reader.go`'s `getQueryParameter`). Between the transport's
adoption and this paragraph the transport did not call it: it answered
`Uncovered("bind statement parameters")` to any request carrying values. **That was the whole
surface, not a corner.** `sutura_domain::query::Question` makes its `range` a mandatory field, and
`sutura_sql::Dialect::BigQuery` renders `PlaceholderStyle::Question`, so every rendered statement
carries positional `?` and values to go with them - a served `bigquery` deployment booted clean and
answered nothing. Round-2 review of telekom/sutura#929 found it; `crates/sutura-exec-bigquery/src/adbc/bind.rs`
is the fix and carries the type map.

**One row, and the driver is why that is a property rather than a detail.** Inside each bound batch
the driver loops `for i := range int(rec.NumRows())` and runs the whole query once per row, appending
each result to the same stream - so a two-row batch is one question answered twice and concatenated,
which is a wrong number under a certified metric name and nothing would report it. `ONE_QUESTION` is
a named constant and `a_parameter_batch_carries_exactly_one_row` is its cell.

A date binds as Arrow `Date32` and not as text, because the driver derives the `GoogleSQL` type from
the ARROW type: a date sent as `STRING` compares against a `DATE` column through a coercion
`GoogleSQL` does not perform, and against a text column compares lexically. The domain's `Date`
already holds days since the epoch, so nothing is formatted on that path.

### The driver is not linked in: a four-way owner decision, priced

The owner asked for the driver to be INCLUDED in the release artefacts and verified in CI, for
normal CI and for the musl artefact. The mechanism that would resolve all three at once is known:
`-buildmode=c-archive` instead of `c-shared`, linking the archive into the binary, and
`adbc_driver_manager::ManagedDriver::load_static` instead of `load_dynamic_from_filename`. Then the
driver ships because it IS the binary, static musl works because no `dlopen` is reached, and the
`SUTURA_BIGQUERY_ADBC_DRIVER` startup requirement disappears.

**What stands in the way is first-party `unsafe`, and it is a CHOICE rather than a wall.** An
earlier draft of this amendment called `unsafe_code = "forbid"` the blocker; review refuted it with
this repository's own precedent, so the correction is here rather than deleted. `load_static` takes
an `&adbc_ffi::FFI_AdbcDriverInitFunc`, which is
`unsafe extern "C" fn(c_int, *mut c_void, *mut FFI_AdbcError) -> AdbcStatusCode`, and the only way
to obtain one for a linked archive is an `unsafe extern` block declaring the archive's
`AdbcDriverInit` symbol. Four routes reach that, and the owner picks one:

1. **Lift the workspace `forbid`.** `Cargo.toml`'s lint table sets `unsafe_code = "forbid"` and says
   in the same place that needing it *"is an architecture decision, and lifting a `forbid` is
   exactly the size of diff that decision deserves."* Cost: every crate in the workspace becomes a
   place `unsafe` may appear, and the property that no first-party `unsafe` exists stops being a
   fact about the tree. Cheapest to write, most expensive to give up.
2. **A vendored crate outside the workspace, owning the one `unsafe` block.**
   `Cargo.toml`'s `exclude = ["vendor/mimalloc_rust"]` is the established precedent - that path
   holds **21 lines carrying `unsafe`** today, outside this forbid's reach, and `VENDOR.md` is where
   such a thing is recorded. **The number is stated with its method because three different ones
   were in circulation** - ten, eleven and twenty-one, for one thing:
   `grep -rc unsafe vendor/mimalloc_rust --include='*.rs'` summed over its three files gives
   11 + 10 + 0 = 21, and eleven was `src/lib.rs` alone. Counting `unsafe fn`/`impl`/`{`/`extern`
   occurrences agrees at 21, because no line carries two. Cost: a second build unit and a `VENDOR.md` row, and the `unsafe` is real
   wherever it lives - what the exclusion buys is that it is CONTAINED and named rather than
   permitted everywhere.
3. **An upstream `adbc_driver_manager` API that takes a symbol NAME rather than a function
   pointer.** The manager already resolves symbols by name for the dynamic path; a `load_static`
   sibling accepting `&str` would keep the `unsafe` on their side of the ABI, where it already is.
   Cost: an upstream round trip on somebody else's release schedule, which is the slowest route and
   the only one that leaves this workspace unchanged. `AGENTS.md`'s *propose upstream* is this.
4. **Stay dynamic, and decide what musl means.** Keep `load_dynamic_from_filename`, ship the `.so`
   as a release asset beside each gnu artefact, and either drop the two musl triples or declare ADBC
   gnu-only. Cost: no `unsafe` at all and no upstream work, in exchange for `bigquery` being
   unavailable on the static artefacts - which is the status quo made explicit rather than a
   regression.

**Two routes review closed, recorded as closed rather than left open.** `sutura-domain`'s
`ALLOWED_IN_DOMAIN` does not extend to this: it is a dependency allowlist and says nothing about a
lint. And a workspace MEMBER cannot escape the lint by omitting `[lints] workspace = true`, because
`xtask/src/api_docs/lints.rs` requires every member to inherit it - measured at 22 of 22.

**What was built instead, because a link check would have been worse than nothing.** `sutura doctor`
loads the `.so` through the driver manager and reports the outcome, which runs the driver's Go
runtime inside the shipped binary beside tokio and the release allocator - the coexistence that is
the real risk and that linking alone cannot show. `just bigquery-driver-check` runs that command
against BOTH release binaries and asserts the gnu one loads and the static musl one does not, in
both directions: if musl ever loads, the check refuses and says this record is wrong rather than
going green. `ci.yml`'s `bigquery-driver-check` job invokes it on the `data_source_bigquery`
classification, and `ci-aggregate` holds run-or-fail for it.

**And the driver's own BUILD check was gated by nothing until the same change.** It sat only inside
the `continue-on-error: true` measure-host stage, with no unconditional re-run beneath it the way
`reuse` and `helm-chart` have - so its exit code was discarded on every push, which is how it stayed
green while its own script was failing on `Is a directory`. `ci.yml` now realises it in an
unconditional `ADBC driver` step as well.

### What else moved with the transport

- `WorkloadIdentityBroker` survived with its ports, its cache, its floor and its claim check, and had
  **no implementor a composition root could reach** - both hops were the wire's. It stopped being what
  a served deployment attaches, and the eighth amendment below deletes it outright.
- `sutura serve` attaches `sutura_exec_bigquery::DeclaredPrincipalBroker` instead: the declared
  subject-to-account map, presented as the identity each job runs as, with a startup refusal for a
  declaration naming nobody and for the pool expectations `telekom/sutura#817` added, which described
  an exchange this build does not perform.
- **Between the transport's deletion and this amendment, a served `impersonation-at-source` `bigquery`
  source booted clean and was refused on every question.** The posture cross-check passed on
  `PerSubjectCredential` while the only broker attached was the static one, which holds nothing for an
  impersonating source. That is the defect this amendment closes, and it is recorded rather than
  quietly fixed because the shape - a control that reads as present at boot and is absent at request
  time - is the one this repository treats as worse than an outage.
- **No release artefact carries a driver.** `nix/shipped.nix` and `nix/oci.nix` publish none, while
  `SUTURA_BIGQUERY_ADBC_DRIVER` is a hard startup requirement, so `bigquery` is non-functional out of
  the box on all four triples. And a static musl binary cannot `dlopen` a `.so` at all, so two of the
  four triples cannot load one however it is shipped - which is now ASSERTED by running the artefact
  rather than stated (see above). Both are open decisions, not conclusions of this record; the
  archive route that would close them is the `unsafe_code` section above.
  **Both are closed by the tenth amendment**, which takes exactly that route: every published
  artefact links the `c-archive` and no release artefact reads the variable at all.
- **The driver is loaded at BOOT now, not on the first question.** Both composition roots call
  `AdbcBigQuery::probe` after reading the path, so a missing or wrong-ABI `.so` stops the process.
  What that does not establish is that a question can be answered: the probe opens no connection.
- **`list_tables` warns and SERVES, and the comment claiming otherwise is corrected.** The transport
  cannot list a dataset (`GetObjects` is unbound), and it is not an authorization refusal - so
  `serve::boot` takes the WARN arm. The consequence, stated where it is: on a `bigquery` source a
  mistyped `table:` is not caught at boot; it fails the first question against that model, which a
  `files` deployment does not do. `the_listing_this_transport_cannot_do_is_not_an_authorization_refusal`
  pins that the override is READ - review measured that flipping it left the suite green - and **not
  "both directions", which this line claimed and a constant `false` does not have.** The second
  direction is a different cell one layer up:
  `adbc::tests::a_listing_this_transport_cannot_do_is_a_warning_and_not_a_startup_refusal` reads the
  outcome `serve::boot` acts on.

## Sixth amendment, 2026-09-20: the principal switch is deleted, and WIF replaces it

**The fifth amendment above decided a mechanism the owner rejected, and it is not kept beside this
one.** That amendment priced a PRINCIPAL SWITCH - `bigquery.impersonate.target_principal`, an
impersonated credential minted from the deployment's own application default credentials - and was
honest about its cost: the asking subject's credential was nowhere in the chain, leg 1 was the only
barrier, and the deployment held `roles/iam.serviceAccountTokenCreator` on every declared account.
The owner's instruction was *"indeed no fallback! we must work with impersonation!!"*, so the switch
is **deleted rather than demoted**: `crate::transport::JobIdentity` carries one subject arm, there
is no option list that names an impersonation target, and a `Presented::SubjectPrincipal` is refused
by the adapter.

**What ships is Workload Identity Federation, and the route the fifth amendment refused is the one
taken.** It said `external_account` was unavailable because its `credential_source` is
file/url/executable/aws only, so a caller's assertion would have to reach the driver on disk. That
enumeration was right and the conclusion was wrong - it did not read `url_provider.go`, whose source
kind is a plain `GET` with **arbitrary headers from the credential document**. So:

```text
bigquery.auth_type = json_credential_string
bigquery.auth.credentials_type = external_account
bigquery.auth.credentials = an `external_account` document naming a loopback URL
  -> go/connection.go: option.WithAuthCredentialsJSON(credType, bytes)
  -> credentials/filetypes.go: handleExternalAccount, which passes `credential_source` THROUGH
  -> externalaccount.NewTokenProvider: GET our loopback source, exchange at Google's STS
```

Three facts from the pinned `cloud.google.com/go/auth v0.23.2` make it work, each measured rather
than assumed: `filetypes.go` hands `f.CredentialSource` through whole; `Options::validate` performs
**no** scheme or host check, so a loopback address is accepted; and a non-empty
`service_account_impersonation_url` is what turns the second hop on, which is why omitting it leaves
the credential as the pool principal the subject resolved to - two subjects, two principals, with no
declared map in the middle.

**The four source kinds, priced, because the choice is the security decision.** `url` was taken:
nothing is written and nothing appears in `argv`, and the per-request document can carry a nonce in
the path and a secret in a header. `executable` needs `GOOGLE_EXTERNAL_ACCOUNT_ALLOW_EXECUTABLES=1`
on every deployment and its command is one whitespace-split string with no shell, so a per-request
assertion would travel in `argv` (world-readable) or through the same loopback machinery plus a fork.
`file` puts the assertion at rest and is the fallback this is preferred over. `certificate` is mTLS
workload identity with nowhere for a caller's assertion, and `environment_id` accepts only `aws1`.

**What the loopback port costs, stated where the claim is.** It is on `127.0.0.1` with a
kernel-assigned port, so any local process can connect. Two independent 128-bit values are required
together and both exist only inside the credential document this process built for one request, so
the bound is *a local process that can read that document already has the assertion*. That bound
rests on the document never reaching disk or a log - it is a `Secret` up to one named exposure at the
driver boundary, and `the_credential_document_is_redacted_under_debug` is the cell on it.

**And `IMPERSONATION` is now true by construction rather than by a doc amendment.** The subject's own
assertion is what Google verifies; the two postures are XOR
(`Impersonation::Disabled` for `shared-service-user`, which runs on the deployment's own application
default credentials and is mandatory there, against `Impersonation::ThroughPool` for
`impersonation-at-source`); and the pair *a subject at a source with no pool* is a REFUSAL rather
than a degradation. There is no arm that answers a subject's question as the deployment.

**Two things this amendment does not claim.** Nothing reachable from this repository shows Google
ACCEPTING the assertion - the document is built and the loopback source is asserted against a real
socket, and the exchange needs a pool, a project and a hosted run, so
`docs/where-identity-is-proven.md` keeps its venue at `wired` and **leg 2 is not proven.** And the
multi-hop case the owner named (Keycloak or Entra, then an RFC 8693 exchange, then the pool) is not
built: `WorkloadIdentityBroker` was the home for that hop and had no HTTP implementor, and the eighth
amendment deletes it - so what ships is the single-hop case where the caller's own verified document
is what the pool trusts, and a multi-hop exchange would start from nothing rather than from a type.

### Addendum to the sixth amendment, 2026-09-20: what review measured on the loopback source

Three corrections, all from breaking the thing rather than reading it. They do not change the
mechanism; they are the bounds the first cut of it did not have.

**The request bound was nominal.** `MOST_REQUEST_BYTES` was checked BETWEEN lines around a
`read_line`, which is unbounded WITHIN one line - a review measured 268 MB accepted on a single line
against a nominal 8 KiB, and raising the constant to `usize::MAX` killed no cell. The read is now
capped at one byte past the bound and a head that reaches the cap is refused rather than truncated
and matched, so a caller cannot present both correct values and then any amount of padding.

**One idle connection was a denial of service, and it hung shutdown.** With no credentials at all, a
local process could connect, send nothing, and the driver's own fetch was never served - the accept
loop is serial and the socket had no deadline - after which `Drop`'s join never returned. Every
accepted socket now carries a read and write deadline. **The limit that remains**: connections are
still handled one at a time, so a process that keeps opening them can DELAY a fetch by up to one
window each time. It cannot prevent one and it cannot read anything. Handling connections
concurrently would trade that for a detached handler holding the assertion after the request it
belonged to ended, which is the guarantee `Drop` exists to give.

**`scope` reaches nothing, and the fifth amendment's replacement did not change that.** Measured
against the pinned sources: `credsfile::ExternalAccountFile` (`cloud.google.com/go/auth@v0.23.2`)
has no `scopes` member, so the credential document cannot carry one; and the driver's only scope
option is `bigquery.impersonate.scopes`, which `connection.go`'s `hasImpersonationOptions` reads as a
request for service-account impersonation - it then demands a target principal and REPLACES the
federated credential with an impersonated token source. So `sources.<alias>.workload_identity.scope`
is declared and sent by nothing; it is not screened in the transport either, because a screened value
that goes nowhere reads as a control that is in place. `audience` is the one of the three keys the
transport sends, and `impersonate`'s keys are read for authorization while its values are not read at
all.

**And the hosted venue asserted the deleted mechanism.** `each_subject_executes_as_the_account_this_source_declared_for_it`
built a `Presented::SubjectPrincipal` - the shape this adapter now refuses - so it could not have
reached a dataset, and its comment said a `principal://` answer would mean the hop did not happen,
which is exactly what this mechanism produces. Rewritten as
`each_subject_executes_as_its_own_principal_at_the_declared_pool`: two subjects, two DISTINCT
principals, neither of them the deployment's own. Nothing predicts either string, because the pool
resolves it.

## Seventh amendment, 2026-09-21: the transport can answer at all, the Arrow read is bounded and checked, and execution does not move to DataFusion

**A configured ADBC source could not answer ANY question, and nothing in this crate showed it.**
`crate::adbc::AdbcBigQuery::validate` returned an error because ADBC has no call that prices a
statement without running it - which read as honest here and was fatal one crate up:
`sutura_app::answer` calls `Warehouse::dry_run` before `execute` and turns any `Err` that is neither
a spent deadline nor a source refusal into `ServiceError::Warehouse`. So every question against a
`bigquery` source over this transport was a service error, on a deployment that booted clean. Review
round 4 of `github.com/telekom/sutura#929` called the PR *adoption scaffolding, not an adopted
transport*; this is the half of that which was a live defect rather than a missing feature.

The fix is the shape this trait already uses for the same problem - `listing_was_refused`,
`job_was_refused`, `deadline_exceeded` are all predicates the adapter asks its transport about a
failure it already holds. `JobTransport::declined_to_dry_run` is the fourth, ADBC answers `true` for
its own `AdbcError::NoDryRun` variant and nothing else, and `BigQueryWarehouse::dry_run` answers
`PreFlight::NotAsked` for it. **`NotAsked` and never `Accepted { estimated_bytes: None }`**: the
port's own documentation is that a defaulted pre-flight reads as *this subject may run this plan*,
and `sutura_conformance::execute`'s pack compares `estimated_bytes.is_some()` against
`PRICES_DRY_RUN` on an accepted one. The variant is matched rather than `Uncovered`'s `&'static str`,
because `list_tables` answers `Uncovered` too and a predicate keyed on text would read a listing this
transport cannot do as a dry run it declined.

**The limit, and it is a control that got weaker rather than a gap that was always there.** Nothing
prices a statement on a shipped path now, so `sutura_app`'s spend ledger charges a `bigquery` source
nothing and `governance.per_replica_spend_ceiling` bounds no source at all - `docs/adr/0030`'s
counter is live code with no adapter feeding it. `BigQueryWarehouse::PRICES_DRY_RUN` stays `true`
because it declares what the ENDPOINT can do (a free, slotless `dryRun`, which is still true) and
because it cannot vary with the transport type; the correction is written at that constant and in
`.agents/skills/sutura/invariants/SKILL.md`'s spend row, whose *every adapter but BigQuery* is now
*every adapter including BigQuery*. Binding an ADBC call that prices, or reading the job statistics
the driver attaches after a run, is what would restore it - neither is built here.

**On DataFusion, the review asked for one of two things and this amendment takes the second
explicitly, so the absence is named rather than inferred.** The options were *move
execution and federation to a bounded DataFusion Arrow path*, or *scope this as a transport-only
change and do not claim the federation goal here*. This is the transport-only scope: the ADBC driver
replaces the wire and nothing else moves. A `RecordBatch` the driver hands back never enters a
DataFusion `SessionContext` or a `TableProvider`; it is decoded into `crate::transport::JobRows` and
a federation leg's rows are re-aggregated by `sutura_app` in memory, exactly as they were under the
wire. Whether execution belongs on a DataFusion Arrow path is a separate decision with its own
owner, and building it here would be that work done twice.

**What the same review did measure is a real defect, and it is fixed rather than scoped away.** The
read was an unbounded double materialisation: every `RecordBatch` was collected into a `Vec` and
then every row decoded beside it, so a result was held twice before anything downstream could look
at a working set - and a federation leg carries no `LIMIT` at all (`sutura_domain::plan::leg`'s own
header says so, because a leg is not an answer), so nothing in the statement bounded what a driver
could stream back. `crate::adbc::decode::Decoding` now takes one batch at a time off the driver's
reader, so there is one materialisation; `crate::adbc::decode::MOST_RESULT_ROWS` is a ceiling
enforced at the batch that crosses it, so the rows past it are never held; and the cast to text is
one vectorised `arrow_cast::cast` per COLUMN per batch, where it used to sit inside the row loop and
cast each column's whole array once per row of it.

**A width check is not a schema check, and this one was a wrong answer rather than a failure.** The
decode compared `batch.num_columns()` against the announced field count and nothing else, so a
driver handing back two columns of the SAME type in the wrong order produced a transposed answer
under a certified metric name, silently. Nothing in the pinned graph would have caught it, measured
rather than assumed:

- `arrow_array::RecordBatch::try_new` validates **positionally** - it zips columns against fields
  and compares type and nullability. Field names are never compared, which is why a
  differently-typed swap errors and a same-typed swap does not.
- DataFusion's name-based mechanism is **opt-in and on the datasource path**. `SchemaAdapter` and
  `SchemaMapper` are deprecated and their default implementation answers `not_impl_err!`; the live
  `PhysicalExprAdapter` does resolve by name, and only for a datasource.
- **Nothing validates that a custom plan's stream matches its declared schema.** The plan contract
  assumes it and consumers index positionally.

So for a foreign driver behind this transport the obligation is ours and it was unenforced.
`Decoding::push` now refuses a batch whose field at a position is not the field the schema announced
there - **name and type** - as a typed `Decode::Mislabelled` carrying the position and both
descriptors, before a single value is read. The comparison is not a bare `zip`: the width refusal
runs first and is what makes the pairing total, because a `zip` alone silently matches the shorter
prefix, which is both halves of the same defect.

**The limit beside it.** This makes the transport stricter than it was: if the pinned driver ever
emits a batch whose schema differs from the one its own reader announced, a question that used to
answer now refuses. That is the direction to fail in - a refusal an operator can read beats a
transposed aggregate nobody can see - but it is a behaviour change and not only a check. And the
ceiling is a bound on this process's memory, never a cap on an answer: `sutura_domain::plan::MAX_ROWS`
is the answer cap and travels in the statement's own `LIMIT`.

## Eighth amendment, 2026-09-21: the exchanging broker is deleted, not kept beside the shipping path

**`sts.rs`, `sts/cache.rs` and `sts/tests.rs` - 2,571 lines - are deleted, with the
`WorkloadIdentityBroker` they held and its three ports (`StsExchange`, `ImpersonateAsAccount`,
`UnixClock`).** The fifth and sixth amendments took the transport that implemented them; this one
takes the code that was left.

**Why, in the owner's own terms.** Leg 1 has real, independent evidence: a live Keycloak issues a
token, the composed binary verifies it and refuses a wrong audience
(`crates/sutura-cli/tests/served/keycloak_test.rs`), and `keycloak-served-test` is a required
context. What the `sts` tree added on top of that was *the verified bytes are the bytes offered to an
exchange* - and after the `wire` removal there was no exchange to offer them to. Every `StsExchange`
in the tree was a test fake, no composition root could reach the broker, and the ADBC path never
touched the module. So the tree was evidence about a leg that does not ship, propped up by fakes; the
owner's decision was to drop it rather than keep artificial scaffolding around a claim that already
has real evidence.

**What the shipping path does instead.** `DeclaredPrincipalBroker`
(`crates/sutura-exec-bigquery/src/principal.rs`) decides WHETHER a caller may be served at an
impersonating source, from a declared per-source map keyed on the full verified subject; the ADBC
transport then puts that caller's own assertion behind an `external_account` credential document
served over a loopback source (`crates/sutura-exec-bigquery/src/adbc/subject.rs`), and Google's token
service - not this process - performs the exchange. There is no second broker to pick and no arm
a misconfiguration can select back onto an exchange this tree cannot make.

### What went with it, each decided rather than fixed blind

| Dependent                                           | What it became                                                                                                                                                                                                                                                                                                                                                                                              |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sutura-exec-bigquery`'s `lib.rs` re-exports        | Deleted; `principal`'s four types are the crate's whole broker surface                                                                                                                                                                                                                                                                                                                                      |
| `sutura-exec-bigquery`'s `base64` and `parking_lot` | Deleted from the manifest - the claim decode and the cache lock were their only callers, and `unused-deps` is what caught them rather than a reading                                                                                                                                                                                                                                                        |
| `sutura-http`'s `identity_e2e`                      | **Kept.** Five of its seven cells never touched the broker; the two that drove it over a fake exchange are deleted, and the `sutura-exec-bigquery` DEV-dependency with them - so one adapter-to-adapter edge is gone                                                                                                                                                                                        |
| `sutura-cli`'s `serve::tests::agent_identity`       | **Kept.** Its two byte-join cells are deleted; the two spend-headroom cells stay and now run over `DeclaredPrincipalBroker`, the broker `serve` really attaches, which is stronger evidence than the fixture they used to run over                                                                                                                                                                          |
| `devco/claim-mutations/`                            | Three patches deleted with the cells they killed - `a_declared_subject_resolves_through_the_hop_to_the_declared_sa`, `the_shipped_exchanging_broker_exchanges_the_document_the_agent_route_verified`, `two_callers_over_the_agent_route_offer_two_distinct_subject_tokens_to_the_exchange`. A patch whose anchor no longer exists cannot apply, which is a gate that refuses rather than a gate that passes |
| `docs/adr/0031`, `docs/adr/0032`                    | Second amendments: each decision is spent, and each says what of its reasoning is still worth reading                                                                                                                                                                                                                                                                                                       |
| `security.credential_cache`                         | **Left in place, and it reads nothing.** It was already unread before this change; `docs/adr/0031`'s second amendment states the limit rather than this amendment implying a cache still exists                                                                                                                                                                                                             |

**What this does NOT change, and the sentence is the important one.**
`docs/where-identity-is-proven.md` reads exactly as it did: leg 2's venue stays `wired`, because
deleting evidence that was only ever a fake-port cell proves nothing. Leg 1's evidence is untouched -
the Keycloak cells are not in this diff.

## Ninth amendment, 2026-09-21: what is actually pinned, and how the Go module version was read

**This record says *the pinned `cloud.google.com/go/auth v0.23.2`* three times, and that phrasing
overstates what this repository pins.** Round 7's review went looking for the pin and found none:
there is no `go/` directory here and no Go module version anywhere in the tree. The only in-tree
occurrence of the version string is an illustrative `# e.g.` inside a shell comment in
`nix/bigquery-adbc.nix`, which is a sample of the `<path>@<version>` shape the install phase walks -
not a declaration.

**What is pinned is the driver SOURCE and its module closure's hash**, two values, both in-tree:

| Value                                             | Where                                                             |
| ------------------------------------------------- | ----------------------------------------------------------------- |
| flake input `bigquery-adbc-src`, ref `go/v1.13.0` | `flake.nix`, locked to one rev with its `narHash` in `flake.lock` |
| `vendorHash` over the whole resolved module set   | `nix/bigquery-adbc-drivers.nix`, shared by all four triples       |

The `cloud.google.com/go/auth` version is therefore *resolved*, not declared: that rev's own
`go.mod`/`go.sum` chooses it, and the `vendorHash` refuses a build in which anything about that
choice changed. The pair is as tight as a version literal would be - a different `go/auth` is a
different `vendorHash` - but it is a different claim, and *pinned* invited a reader to grep for
something that is not there.

**How the version was read, since a resolved value still has to be readable.** The install phase
writes every module in the build's own import closure, with its licence file name, to
`lib/DRIVER-MODULES.txt` beside the built `.so`. Reading that file out of a driver this tree built
gives `cloud.google.com/go/auth@v0.23.2` on its second line - so `v0.23.2` is correct as a
measurement of the current pin, and the three facts the sixth amendment took from those sources
stand. It is not correct as a description of what this repository declares. **The limit:** nothing
compares that file against a number written down anywhere, so a driver bump moves the resolved
version silently, and any prose naming `v0.23.2` - here, or in the two doc comments that cite
`credsfile::ExternalAccountFile` - is a measurement with a date on it rather than a checked fact.

## Tenth amendment, 2026-09-22: the driver IS the artefact, and route 1 was taken with a containment gate

**The seventh amendment's four-way owner decision is settled: route 1.** The owner's ruling was
*"we could have an option (maybe unsafe) that would use the right constructor on musl to handle SO
cant we do that?"*, and the narrowing beside it was equally explicit - the `forbid` is lifted for
one exception, not outright. So `-buildmode=c-archive` is built beside the `c-shared` `.so` by the
same derivation, `nix/shipped.nix` links it into every published artefact whose triple has one, and
`ManagedDriver::load_static` opens it. The consequences the seventh amendment predicted all hold:
the driver ships because it IS the binary, static musl works because no `dlopen` is reached, and no
release artefact reads `SUTURA_BIGQUERY_ADBC_DRIVER` at all.

**Routes 2, 3 and 4 are recorded as declined rather than deleted.** Route 2 - a crate outside the
workspace - was re-costed and is worse than it reads: `Cargo.toml`'s `exclude` takes the crate out
of `--workspace` clippy, out of the test sweep, out of `check-boundaries` and out of
`check-api-docs`' member census, so the one `unsafe` in the tree would sit in the one crate no gate
reads. Route 3 stays the right upstream ask and is not a blocker for a release; route 4 is the
status quo, and *BigQuery is unavailable on half the published triples* is what it costs.

**What route 1 costs, and what pays for it.** `[workspace.lints.rust] unsafe_code` is `deny` now,
not `forbid`. That change was forced rather than chosen: cargo refuses a member that both inherits
`[workspace.lints]` and overrides one entry - *"cannot override `workspace.lints` in `lints`"* -
and an `#[expect(unsafe_code)]` beneath an inherited `forbid` is `E0453`, both measured. What
replaces the lost strictness is a re-assertion at every crate root, which a crate cannot then
lower, and `cargo xtask check-unsafe` holding that every root carries it with exactly one declared
exception.

Measured in both directions, because *the exception did not widen* is the claim worth breaking:

| Probe                                                                                           | Verdict                                                                                                                               |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| an `#[expect(unsafe_code)]` plus an `unsafe extern` block planted in `sutura-tls`               | `error[E0453]: expect(unsafe_code) incompatible with previous forbid` and `error: usage of an unsafe extern block` - does not compile |
| the same probe with that crate root's `#![forbid(unsafe_code)]` deleted, so only `deny` applies | compiles clean, `Finished dev profile` - **so the workspace level alone is exactly the widening the gate refuses**                    |
| `cargo xtask check-unsafe` over that second shape                                               | `FAILED - 1 crate root(s) do not re-assert #![forbid(unsafe_code)] ... crates/sutura-tls/src/lib.rs`                                  |

**The limits, next to the claim.** The `unsafe extern` block lives behind
`cfg(adbc_driver_linked)`, which only a build that links the archive compiles - so `just lint` and
every `--all-features` cargo gate judge it not at all, and what reads it is the text gate plus the
`cross` release builds that link it. The gate reads the roots a conventional layout produces rather
than the target list cargo resolves, so a target declared with an explicit `path` is outside it.
And the driver a linked artefact carries is pinned by the flake lock and by nothing in the type
system: a second archive exporting `AdbcDriverInit` would be the wrong driver, silently.

**The fifth amendment's record is spent and this replaces it.** `just bigquery-driver-check` no
longer hands each binary a `.so` and asserts that the static musl one cannot load it. It runs
`sutura doctor` against both release artefacts with `SUTURA_BIGQUERY_ADBC_DRIVER` **cleared**, and
requires each to report a driver that both *initialised* and is *linked into this binary* - so a
derivation that stopped linking the archive is a red rather than a silent fall back to a mounted
path. The matcher asserts its own five known-answer cases before reading an artefact, because a
probe that passes by not measuring is the failure this repository has been bitten by.

**What linking the archive does to the artefact, measured on the x86_64-musl one.** The binary is
146 MB and its runtime closure gains exactly three paths - `tzdata`, `iana-etc` and `mailcap` -
which are Go's stdlib data references and not a toolchain, so `checks.one-binary`'s closure rule is
unaffected. The published image was already correct for the driver's TLS by accident of an earlier
decision: `nix/oci.nix` puts `cacert` and `tzdata` in `contents` and sets `SSL_CERT_FILE`, which Go's
`crypto/x509` reads on linux, so a `FROM scratch` image with a carried driver has a CA bundle. Worth
recording because the opposite would have been invisible until a question was asked.

**What no venue here establishes.** That a question can be answered. `probe` opens no connection
and reads no credential, and nothing in `just validate` links the static path at all - the four
`cross` jobs and `bigquery-driver-check` do. `x86_64-unknown-linux-musl` is the one triple whose
link was measured by hand for this amendment (`static-pie linked`, with a Go BuildID in the ELF
header); the other three are CI's to report.
