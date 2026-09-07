---
title: What the BigQuery wire is built from
description: The dependency decision the BigQuery transport was gated on - four community and official clients priced against what each costs the release, the licence gate and Arrow, why every wrapper crate is refused on one transitive dependency, why the REST endpoint over a client already in the graph costs zero new packages, and the consequence that a wire now exists which nobody has run against a real project.
---

# What the BigQuery wire is built from

Status: **accepted.** The transport is built, feature-gated, linted, tested and audited - and since
2026-08-30 it has been **run against a real project**, which is the first time anything in this
repository has had a statement accepted by `BigQuery`. *What is claimed, and what is not* is the
section at the end, and it is the one to read before taking a green run for more than it is:
**one hand-built `SUM` was accepted, not the corpus.**

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

| Cost | The mechanism that would fail |
| --- | --- |
| **The release** | `sutura-cli` is the only crate any release package builds - `flake.nix` sets `cargoExtraArgs = "--package sutura-cli"` on both the native and the cross paths - across four target triples, two of which are musl. nixpkgs has no musl `libduckdb`, which is why a data system's driver is a dev-dependency here. A client with a native library repeats that problem in a source that is not optional |
| **The licence gate** | `deny.toml` runs an exact allowlist with `unused-allowed-license = "deny"`, so an allowed licence nothing uses is itself a failure. Every addition is a deliberate entry |
| **Arrow** | `cargo xtask check-arrow` fails when the `arrow-*` family spans more than one major without a dated, reasoned entry in `devco/arrow-majors-allow`. The engine sets the type vocabulary and adapters conform |
| **`anyhow`** | Not on the issue's list and it is the one that decides this. `AGENTS.md` states that *`anyhow` appears nowhere in this workspace - `Cargo.lock` included, so not even transitively*, and `cargo xtask check-boundaries` fails a dynamic-error crate in a library. A dependency that pulls it transitively spends a claim this repository makes in writing |

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

| Option | Packages added | `anyhow` | `openssl` | Arrow | Native build | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| `gcp-bigquery-client` 0.28.0 | +71 | **yes** | no | no | `zstd-sys` (`links = "zstd"`) | **refused** |
| `google-cloud-bigquery` 0.15.0 (what the name resolves to) | not counted | **yes** | **yes** | **53.4.1** | `openssl-sys` | **refused** |
| `gcloud-bigquery` 1.7.0 (the same family, renamed) | not counted | **yes** (twice over) | no | **58.4.0** | `aws-lc-sys`, cmake | **refused** |
| `google-cloud-bigquery` 0.16.1-preview (official) | +92 | **yes** | no | no | `aws-lc-sys`, cmake | **refused** |
| `reqwest` + `gcp_auth`, calling `jobs.query` | +68 | no | no | no | `ring` only | viable |
| **`ureq`, calling `jobs.query`** | **+0** | no | no | no | `ring` only | **chosen** |

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

### Which shipped artifact links what, per target

**The answer today is: none of them link either half of this crate**, and that is checkable rather
than asserted.

| Artifact | Built from | Links `sutura-exec-bigquery`? | Links `ureq`? |
| --- | --- | --- | --- |
| `sutura` (`x86_64-unknown-linux-gnu`, native) | `--package sutura-cli` | no | no |
| `sutura` (`aarch64-unknown-linux-gnu`, cross) | `--package sutura-cli --target …` | no | no |
| `sutura` (`x86_64-unknown-linux-musl`, cross) | `--package sutura-cli --target …` | no | no |
| `sutura` (`aarch64-unknown-linux-musl`, cross) | `--package sutura-cli --target …` | no | no |
| `oci-<triple>` for each of those four | the unsuffixed cross package | no | no |
| `sutura-serve` | nothing - **it is built by no release package at all**, because the flake's release derivations name `sutura-cli` only | no | no |

`crates/sutura-cli/Cargo.toml` declares no edge to `sutura-exec-bigquery`, and the root manifest
keeps the crate out of `[workspace.dependencies]` on purpose - `cargo xtask unused-deps` is what
keeps it out, because no member inherits it. So the four cross builds are byte-identical in their
dependency requirements before and after this change, and the `wire` feature is off in every
resolution any of them performs.

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
  no second host to follow a redirect to. And every foreign string that reaches an error is bounded
  and character-filtered through **one** shared function - the endpoint's `reason` and the credential
  file's `type` are kept, the free-text `message` is not a field on the error type at all, and there
  used to be two copies of the bounding that had drifted by one character in their allowed set.
- **Failure is derived from the RESULT SHAPE, never from `errors` being non-empty**, and the first
  version got this wrong in the direction that matters. The endpoint documents that array as *"the
  first errors or warnings encountered"* and says entries *"do not necessarily mean that the job has
  completed or was unsuccessful"* - so refusing on it **declined successful queries that merely
  warned**, and answered a caller a `503` for a result the service had produced. What refuses is
  `jobComplete`, a `pageToken`, an absent `totalRows` and a delivered count that is not the reported
  total; the reported reason is folded into whichever of those fires, which is also where a genuinely
  failed job lands, because the endpoint reports one as complete with no total.
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
a live run is worth most for. The corpus-wide leg `0017` specifies - load the fixtures, run the 21
questions, compare rows with the engine - is #78's importer shape and is not built. Every sentence in
this repository that promised the corpus has been narrowed to what the leg does.

**It says nothing about identity.** A service-account key is `SharedServiceUser`: one identity for
everybody who asks. So what is established is *accepted, and correct for that identity*.
`BigQueryWarehouse::IMPERSONATION` still reads `NoPlaceForASubject`, and nothing here is a step towards
per-subject execution.

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
`crates/sutura-exec-bigquery/tests/acceptance.rs`, five `#[ignore]`d tests, reached by
`just bigquery-acceptance`, needing three variables a developer names in their own environment.

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
at a dataset** - load the example fixtures, run the 21 questions, compare rows with the engine - and it
is not built. Every sentence in this repository that promised the corpus has been narrowed to that:
0017's amendment, `AGENTS.md`, `docs/architecture.md`, both plan pages, the justfile recipe and the
leg's own header.

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
- The `data_systems:` axis of `crates/sutura-app/tests/adapters/mod.rs` still gains **no** entry.
  That registry's rule is that an entry is something somebody could deploy, and a cell that has never
  executed reads as coverage. It gains one in the change that pastes a green acceptance run.
- `sutura-serve` still refuses `kind: bigquery` by name, and correctly: it links no `BigQuery`
  adapter. Un-refusing it is a composition change that also has to answer where the deployment's
  credential comes from, and the only source built today reads a developer's own login.
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
  here** - no dataset is named in this environment, so `just bigquery-acceptance` fails on its own
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
  most N of these M* for that reason.

  **What the decision does NOT reach, stated where the claim is.** Only `Short` is read. A service
  that re-spells the count as well leaves `Unreadable`, which by this crate's own words is *itself a
  shape change*, and it still answers *absent* - that is `telekom/sutura#443`, and it is out of reach
  here rather than overlooked: `Unaccounted` carries a `NonZeroU64` shortfall and `Unreadable` has no
  number to put in it, so covering it changes the answer's shape. `Unreported` beside no ids is an
  empty dataset and is deliberately not in that issue; a dataset every id of which `usable_table_id`
  drops is `Accounted` beside no ids and is an ordinary dataset. A gap is not a diagnosis, so a
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
  now, in `just bigquery-acceptance`, one rung BELOW the domain port because `TablesPresent` carries
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
