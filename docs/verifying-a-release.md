# Verifying a release

The tagged-release workflow signs artefacts and records SLSA provenance as described below.
Signature and provenance bundles carry the evidence; they do not recursively attest themselves.
This page explains the checks and, just as importantly, what they do **not** say.

## Read this part first

**A signature answers "did this pipeline produce these bytes". It does not answer "are these bytes
good".** Provenance is an *identity* claim, and the whole of what it establishes is that the file in
your hand is byte-for-byte what `github.com/telekom/sutura`'s release workflow emitted at a tag,
rather than something a mirror, a proxy or a compromised download page substituted. It says nothing
about whether the code is correct, whether a dependency has an advisory against it, or whether the
release is fit for what you want to do with it. The gates say the first, `cargo-deny` says the
second, and nothing says the third.

**The SBOM does inventory the crate graph, and it is read out of the binary rather than out of a
lock file.** Why that distinction is worth the sentence is under
[What the SBOM covers](#what-the-sbom-covers).

## What a release publishes

**Two binaries, at four target triples each.** Which one you want is the first choice to make, and
nothing about the file names decides it for you:

| Binary         | What it is                                                                           | Asset                          | Image tag                                                                          |
| -------------- | ------------------------------------------------------------------------------------ | ------------------------------ | ---------------------------------------------------------------------------------- |
| `sutura`       | the command-line tool: `doctor`, `catalog`, `describe`, `prompt`, `compile`, `query` | `sutura-<triple>.tar.gz`       | `:<version>`, `:latest`, `:<version>-musl`, `:latest-musl`                         |
| `sutura-serve` | the service: answers certified questions over HTTP                                   | `sutura-serve-<triple>.tar.gz` | `:<version>-serve`, `:latest-serve`, `:<version>-serve-musl`, `:latest-serve-musl` |

**One registry repository**, `ghcr.io/telekom/sutura`, and the tag says which binary. The unsuffixed
tags have always been the command-line tool and they still are: pointing `:latest` at the server
because the server is the more deployable artefact would be a silent change of what an existing
`docker pull` returns. Which one you get is something you have to type.

**Both are built with cargo's default features**, which for `sutura-serve` means the published
server carries the HTTP surface, the caller-token verification, the rate limiter and the generated
interface description, and carries **neither in-process TLS nor the BigQuery adapter**. Both of those
cost an outbound rustls closure on two statically linked triples, and both are a startup refusal
naming the feature rather than a silent degradation - so a deployment that needs either builds from
source and knows it. `nix/shipped.nix` is where that decision is written, and
`nix build .#checks.x86_64-linux.shipped-features` is what asserts it, out of the shipped binary's
own embedded dependency list rather than out of a manifest.

**A release also carries a licence statement, and it is a different list on purpose.** What each of
the two documents answers is under [The licence statement](#the-licence-statement).

## What is signed

| Artefact                         | Sigstore bundle                                  | SLSA provenance          | Registry signature               |
| -------------------------------- | ------------------------------------------------ | ------------------------ | -------------------------------- |
| the eight binary tarballs        | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| the four musl image tarballs     | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| the sixteen SBOMs                | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| the two licence documents        | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| `image-digests.txt`              | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| the `.sha256` sidecars           | no                                               | yes                      | n/a                              |
| the eight leaf images            | n/a                                              | no                       | `cosign sign`, by digest         |
| the four manifest lists          | n/a                                              | yes                      | `cosign sign`, by digest         |
| each leaf image's CycloneDX SBOM | n/a                                              | n/a                      | `cosign attest --type cyclonedx` |
| `sutura-provenance.intoto.jsonl` | five existing signed attestation bundles         | not recursively attested | n/a                              |

Every count in that table is two binaries times four triples, or its consequence. A leaf and the SBOM
beside it are named by the same key - `<triple>` for `sutura`, `serve-<triple>` for `sutura-serve` -
which is also how `image-digests.txt` records them, so an entry there and an asset on the release page
are matched by reading rather than by working anything out.

Three asymmetries in that table are deliberate rather than gaps, and each has a reason worth
knowing before you conclude something is missing.

**The `.sha256` sidecars are not signed.** A Sigstore bundle over `sutura-<target>.tar.gz` already
commits to that file's digest, so signing a file whose entire content *is* that digest proves
nothing new and costs a certificate and a transparency-log entry. They are still published, because
a consumer's script may already read them, and they still get provenance, because provenance is one
call covering every subject and therefore free. **If you are choosing between the two, take the
bundle:** a `.sha256` file tells you a download was not corrupted, and a bundle tells you who
produced it.

**Provenance covers the manifest lists and not the leaves.** A list is what an unqualified
`docker pull` resolves and what the release notes tell you to pin. The leaves are what a list points
at, they are signed by `cosign` individually, and pinning one means asking for a single architecture
on purpose. There are four lists rather than two now, because there are two binaries.

**The images get `cosign` and the assets get a bundle.** By default, `gh attestation verify`
fetches provenance from GitHub. With `--bundle`, it reads the exported JSONL instead; retain that
asset alongside mirrored files. Verification still needs an independently trusted Sigstore root
and an explicit signer/source policy, not merely a bundle supplied by the same mirror.

## There is no key

Signing is **keyless**, and no signing key exists in this repository, in its secrets, or on any
machine. The release job mints a short-lived OpenID Connect token that states which repository,
which workflow file and which git ref is asking; Sigstore's certificate authority (Fulcio) returns a
certificate valid for minutes that records that identity; the signature is logged in the public
transparency log (Rekor).

So what you check is not *"somebody held the key"* - which is unfalsifiable once a key leaks, and
stays true for as long as nobody notices - but *"this workflow, at this ref, in this repository"*.
There is nothing here for an attacker to steal, and **that is why every command below passes
`--certificate-identity-regexp` and `--certificate-oidc-issuer`: those two flags are the check.**
Omitting them verifies that *somebody* signed the file, which is not a claim worth making.

## Verifying a release asset

Two ways, and they are not redundant - they differ in what you need to have.

Offline-ish, from the bytes and the bundle:

```bash
cosign verify-blob \
  --bundle sutura-x86_64-unknown-linux-gnu.tar.gz.sigstore.json \
  --certificate-identity-regexp '^https://github.com/telekom/sutura/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  sutura-x86_64-unknown-linux-gnu.tar.gz
```

Against this forge, which needs no bundle at all:

```bash
gh attestation verify sutura-x86_64-unknown-linux-gnu.tar.gz --repo telekom/sutura
```

The second prints the workflow, the ref and the commit the artefact was built from. Read them: an
attestation from a *different* ref of this repository verifies perfectly well and is still not the
release you meant to download.

### Provenance from mirrored bytes

For releases made by the exporting workflow, retain `sutura-provenance.intoto.jsonl` with the
original assets. It contains five Sigstore bundles: one multi-subject asset attestation and four
manifest-list attestations. It is not a new signing policy. The collector checks required
JSON/DSSE/SLSA fields and exact unique name/digest pairs against `subjects.sha256` and
`image-digests.txt`, then writes the asset only after all inputs pass. It does **not** verify
signatures, certificates or transparency proofs. A missing export blocks draft publication.

Choose the expected tag and full source commit independently of the downloaded bundle. Supply a
trusted-root file obtained through your trusted distribution channel; do not trust one merely
because a mirror placed it beside the artefact. For example, with those values already set:

```bash
gh attestation verify sutura-x86_64-unknown-linux-gnu.tar.gz \
  --bundle sutura-provenance.intoto.jsonl \
  --custom-trusted-root "$TRUSTED_ROOT" \
  --repo telekom/sutura \
  --source-ref "refs/tags/$TAG" --source-digest "$SOURCE_COMMIT" \
  --cert-identity "https://github.com/telekom/sutura/.github/workflows/release.yml@refs/tags/$TAG" \
  --cert-oidc-issuer https://token.actions.githubusercontent.com \
  --predicate-type https://slsa.dev/provenance/v1
```

`--cert-identity` and `--signer-workflow` are **mutually exclusive** - `gh` puts
`cert-identity`, `cert-identity-regex`, `signer-repo` and `signer-workflow` in one flag group and
refuses more than one of them before it verifies anything. This command used to print both and
therefore could not run; `--cert-identity` is the one kept, because it pins the workflow *and* the
tag in a single value. Measured against `gh` 2.98.0, and no gate in this tree holds it: the
`documented` suite spawns the `sutura` binary over the pages' own commands, so a `gh` invocation on
a published release is checked by running it and by nothing else.

The [verifier's bundle mode](https://cli.github.com/manual/gh_attestation_verify) accepts JSONL
without fetching attestations from the forge. Test the command with network access disabled to
establish the fully mirrored-byte claim for your verifier and trust-root distribution. An OCI
reference can still require registry access; this file example does not prove offline image
retrieval or verification.

**Venue limit:** PR CI can exercise fake bundle output and refusal paths, not the tagged job's
OIDC signing, real bundle export or release upload. Those were unproven until a tag ran, and
`v0.5.1` is that tag: the release carries `sutura-provenance.intoto.jsonl` as a five-record asset,
and over bytes downloaded from the release page the command above exits 0 against the tag's own
source commit, exits non-zero on a byte appended to the tarball (`verifying with issuer
"sigstore.dev"`), and exits non-zero against a wrong source ref (`expected SourceRepositoryRef to
be refs/tags/v0.4.1, got refs/tags/v0.5.1`). **What is still unproven is the OFFLINE half** - that
run had network reach, so it establishes verification from mirrored bytes and a locally supplied
trusted root, not that the verifier fetches nothing. Older releases need not contain the export. No
tag or release is created merely to make a PR check green.

## Verifying an image

Take the digest from the release notes, or from `image-digests.txt`. **Verify by digest, not by
tag** - `:latest` moves, and a verification against a name is a verification of whatever that name
meant when you ran it.

```bash
cosign verify \
  ghcr.io/telekom/sutura@sha256:... \
  --certificate-identity-regexp '^https://github.com/telekom/sutura/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

For the manifest lists, GitHub's provenance works on the same digest:

```bash
gh attestation verify oci://ghcr.io/telekom/sutura:0.1.0 --repo telekom/sutura
gh attestation verify oci://ghcr.io/telekom/sutura:0.1.0-serve --repo telekom/sutura
```

## What the SBOM covers

Each leaf image ships two inventories of the same scan - CycloneDX and SPDX, so the two cannot
disagree - attached to the release and, for CycloneDX, attached to the image itself:

```bash
cosign verify-attestation --type cyclonedx \
  ghcr.io/telekom/sutura@sha256:... \
  --certificate-identity-regexp '^https://github.com/telekom/sutura/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

It covers two things, and they come from two places.

**The files in the image** - the binary, the CA certificate bundle and tzdata. There is no shell and
no package manager in there, so that list is short and complete.

**The crates the binary was compiled from**, read out of a `cargo auditable` section inside the
executable. That is worth a sentence, because the obvious way to produce this list is the wrong one:

- A document generated from `Cargo.lock` beside the binary is a *checked* claim about it. The two
  can drift - a rebuild, a re-tag, a file swapped in a mirror - and neither the binary nor the
  document says so.
- It would also be the **wrong list**. `Cargo.lock` records what cargo *resolved*, not what the
  linker *kept*: neither shipped binary carries the DuckDB adapter or the BigQuery wire, while
  `libduckdb-sys` and that wire put `ureq`, rustls and `ring` into the resolve graph. A
  workspace-wide document names all three and is wrong in the direction that matters, which is
  overstating what ships. Measured on 2026-09-01, out of the artefacts themselves: 263 crates in
  `sutura` and 326 in `sutura-serve` - the difference is the transport - and `ring`, `ureq` and
  `rustls` in neither.

- **The two binaries therefore have two different lists, and there is one SBOM per image rather than
  one per release.** A single document would be true of neither.

So the list is put **inside the artefact at build time** and read back out of the bytes being
scanned. It cannot drift from the binary, because it is the binary. The release job checks that the
section is actually there rather than trusting it: if it ever stops being produced, the scan still
succeeds and still writes valid CycloneDX naming three files, and nothing downstream could tell that
apart from a correct run.

**What the list is not is a statement about those crates.** It says which versions were compiled in.
Whether any of them has an advisory against it is `cargo-deny` against the RustSec database, which
runs in CI on every dependency change and weekly regardless - a run, not a property of the artefact.
`cargo audit --bin` will read this section and answer that question against the file you have.

## The licence statement

The release carries the workspace-wide attribution document beside the per-binary SBOMs.

| Asset                   | What it is                                                                                      | Generated from                                                                                                         |
| ----------------------- | ----------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `sutura-attribution.md` | every third-party crate this workspace resolves, with the SPDX expression its manifest declares | `cargo metadata`, generated in the release run from the tagged tree's own `Cargo.lock`. **There is no committed copy** |

It is signed and carries provenance, so it verifies exactly like a binary tarball:

```bash
cosign verify-blob \
  --bundle sutura-attribution.md.sigstore.json \
  --certificate-identity-regexp '^https://github.com/telekom/sutura/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  sutura-attribution.md
```

**The attribution document is deliberately WIDER than the SBOM, and that is not a contradiction.**
The SBOM says what is *in* one binary, so overstating it would be a false statement about the file
in your hand - which is why that list lives inside the executable. The attribution document
discharges a licence obligation, and there the errors are not symmetric: naming a crate that did not
ship costs you one line to read, while omitting one that did ship is the failure the document exists
to prevent. So it names every crate the workspace resolves at all features, which is more than
`sutura-cli` links.

**Why it is generated at release time rather than committed.** It was committed once, with two
gates over it, and the arrangement had a cost that was not theoretical: a derived file falls behind
`Cargo.lock` the moment a dependency moves, Dependabot runs with a read-only token and cannot
regenerate it, so **every bot dependency bump was red on arrival** and none could go green without
a human pushing to the bot's branch. The artefact was never the problem; its committed form was.

So the generator is the only owner, and the chain is shorter than it was: the release generates the
document from the `Cargo.lock` of the tag it is publishing, rather than copying a file from `main`
that a gate had to keep honest. Three mechanisms hold it:

| Mechanism                             | What it holds                                                                                                                                                                                               |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo xtask check-attribution`       | a generation names every third-party package in `Cargo.lock` and carries a **declared licence for each one** - a crate declaring none is a refusal, where the old generator wrote `NOT DECLARED` and passed |
| `cargo xtask check-attribution-owner` | no committed `ATTRIBUTION.md` comes back, and `release.yml` still generates the asset rather than copying one                                                                                               |
| the release's own count floor         | the generated bytes name at least a hundred crates, asserted against the file that will be signed - which catches a generator that writes a well formed document naming nothing                             |

**What this arrangement lost, stated plainly.** A new dependency's declared licence no longer shows
up in a pull-request diff. Nothing here restores that. `cargo deny check` still *refuses* a licence
that is not on the allowlist, and the first gate above refuses one that is absent entirely, so the
policy floor did not move - but a human reading a diff is no longer among the things that would
notice a licence *changing* from one permitted value to another.

**It names vendored code too.** The two crates under `vendor/mimalloc_rust` are declared as path
dependencies rather than pulled from a registry, and `sutura-cli` links the allocator on Linux - so
they are rows like anything else somebody else wrote. What selects a row is not being a workspace
member, never where the code came from.

**What neither document carries is the notice text of each dependency.** An Apache-2.0 crate's own
`NOTICE` file lives in its source tree rather than in its metadata, so nothing that reads metadata
can render one. That is the largest remaining gap in the obligation, and closing it needs a
source-code licence scan of the whole closure, which is deliberately not run.

## What none of this establishes

Stated plainly, because a page full of green checkmarks invites the larger reading:

- **Not that the build is reproducible from source by you.** The provenance records which workflow
  ran, not a bit-for-bit rebuild recipe you can execute. The build *is* Nix-pinned, which is why
  that is worth attempting - but nothing here is a proof that you did.
- **Not that dependencies are free of known advisories.** That is `cargo-deny` against the RustSec
  database, run in CI on every dependency change and weekly regardless, and its verdict is a CI run
  rather than an artefact you can check offline.
- **Not that the licence obligations of the dependency set are fully discharged.** There is an
  attribution document now, and it is signed - see above. What is still missing is the notice text
  each Apache-2.0 dependency's own `NOTICE` file carries, which is in its source and not in its
  metadata. `deny.toml`'s allowlist remains a policy check either way.
- **Not that the crate list covers everything in the binary.** It is what `cargo` compiled in. C
  that a build script compiled - the allocator, for one - is linked into the executable and is not
  a crate, so it appears in the image inventory as a file and not in the crate list as a package.
- **Not that a transparency-log entry proves the log was honest.** Rekor's guarantees are Rekor's,
  and `cosign` checks an inclusion proof against it rather than auditing it.
- **Not that a verified image is one you should run.** It is the image this pipeline built. Whether
  that pipeline should be trusted is a question about this repository, and the answer to it is
  everything else in these docs.
