---
title: What the BigQuery wire is built from
description: The dependency decision the BigQuery transport was gated on - four community and official clients priced against what each costs the release, the licence gate and Arrow, why every wrapper crate is refused on one transitive dependency, why the REST endpoint over a client already in the graph costs zero new packages, and the consequence that a wire now exists which nobody has run against a real project.
---

# What the BigQuery wire is built from

Status: **accepted.** The transport is built, feature-gated, linted, tested and audited. **Nothing in
this repository has sent a statement to a real `BigQuery` project**, and the section *What is still
not claimed* says so at the end rather than leaving it to be inferred.

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
4. **`wire::credential::AccessTokens`, a second port.** The wire needs one thing from a credential -
   a token usable now, and when it stops being usable - and everything else about how a deployment
   authenticates is somebody else's decision. **This is the seam per-subject execution arrives at**,
   which is why it is a port on the first day rather than a `String` field: the second `BigQuery`
   step mints a token per leg for the subject who asked, and under a port that is an implementor
   rather than a change to the transport.

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
`tokio-rustls` already carries in the root manifest for the inbound side. **Not proved by a build
here:** no musl cross build was run on this machine, and no shipped artifact links the feature, so
this paragraph is a prediction with its mechanism named rather than a measurement.

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
- **`wire::credential::ApplicationDefault`**, one `AccessTokens` implementor: it reads the file
  `gcloud auth application-default login` writes and exchanges its refresh token. That is exactly the
  fixture `0017` decided, and `just gcloud-login` is what produces it.
- **Four decisions the module header states and the suite pins.** One page or a refusal - a
  `pageToken`, an incomplete job or a total the delivered count does not equal is refused, because to
  `answer()` a first page would read as *under the cap, not truncated*. The service's own result
  cache **off** - an anchor that reproduces from a cache has reproduced the cache, and a cached
  answer under a shared identity is shared across every asker. `max_redirects(0)`, so the bearer has
  no second host to follow a redirect to. And every foreign string that reaches an error is bounded
  and character-filtered - the endpoint's `reason` is kept and its free-text `message` is not a field
  on the error type at all, because what is not read cannot be logged by accident.

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
- **A token cache.** `clippy.toml` disallows `std::sync::Mutex` here, so the cache would arrive with
  a dependency for the primitive to hold it - but that is the smallest of three reasons. *A credential
  is not reused past its expiry* is one of the assertions the per-subject step owes, and the shape
  that cannot get it wrong is the one with nothing to reuse. And it is **the credential-shaped version
  of the cache this crate already refuses**: a query-keyed result cache is a cross-user leak under
  row-level security, and a token cache keyed by nothing is the same defect one layer down, sitting in
  the code the per-subject step has to change. The cost is one extra round trip per job, against a
  query that costs seconds and money.

## What is still not claimed

**Acceptance.** Nobody has run this against a real project.

The machine this was written on has no `gcloud`, no application-default credential and no project
named anywhere, so `0017`'s *"the change that implements it is the change that can first run it"* did
not come true - and pretending otherwise is the failure that record spends three sections refusing.
What exists instead is the fixture it asked for, written and unexecuted:
`crates/sutura-exec-bigquery/tests/acceptance.rs`, three `#[ignore]`d tests, reached by
`just bigquery-acceptance`, needing three variables a developer names in their own environment.

**And an unconfigured run of it FAILS rather than skipping, which is a reversal worth recording
because the first version got it wrong.** That version printed `SKIPPED - ... is not set` and
returned, and all three tests then reported PASS with no project anywhere - a green nobody asked for,
over exactly the claim the file exists to make. The compose tier does skip, correctly, because its
cells run inside `just test` and failing would break the suite on every machine with no docker; these
tests are `#[ignore]`d, so the only way to reach one is to ask for it by name. **That asymmetry is the
general rule and not a special case: skip where the runner had no choice, fail where somebody typed
the command.**

So the honest summary of `BigQuery` support in this repository is `0017`'s sentence with one word
changed: **the statement is right as far as five mechanisms can tell, and nobody has run one.** The
fifth mechanism is the wire's own suite, and it is worth being exact about what it proves:

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
- A future `just update` that moves `ureq` to a version whose feature set no longer matches what
  `libduckdb-sys` resolves would turn the +0 into a real number. Nothing gates that, and it is the
  kind of thing worth a line in a `xtask` check the day it bites - the same shape `check-arrow`
  already has.
