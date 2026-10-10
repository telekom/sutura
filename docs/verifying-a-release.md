# Verifying a release

The tagged-release workflow signs artefacts and records SLSA provenance as described below.
Signature and provenance bundles carry the evidence; they do not attest themselves.
This page explains the checks.

## Read this part first

**A signature answers "did this pipeline produce these bytes".** Provenance is an *identity* claim:
the file in your hand is byte-for-byte what the release workflow of `github.com/telekom/sutura`
emitted at a tag, and not something a mirror, a proxy or a compromised download page
substituted.

**The SBOM inventories the crate graph. It is read out of the binary, not out of a lock file.**
The reason is under [What the SBOM covers](#what-the-sbom-covers).

## What a release publishes

**One binary, `sutura`, at four target triples.** It also answers certified questions over HTTP, as
`sutura serve`:

| Binary   | What it is                                                                                                                                                           | Asset                    | Image tag                                                  |
| -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ | ---------------------------------------------------------- |
| `sutura` | the command-line tool (`doctor`, `catalog`, `describe`, `prompt`, `compile`, `query`) and, as `sutura serve`, the service that answers certified questions over HTTP | `sutura-<triple>.tar.gz` | `:<version>`, `:latest`, `:<version>-musl`, `:latest-musl` |

**One registry repository**, `ghcr.io/telekom/sutura`. The unsuffixed tags resolve to this binary.
You choose the libc with the tag: `:latest-musl` for the static pair, `:latest` for glibc.

**Built with the optional features that `nix/shipped.nix` lists.** The published binary always
links the HTTP surface, the caller-token verification, the rate limiter and the generated interface
description. It also carries every entry of the `features` list in that file. A deployment uses an
adapter through a settings-tree entry, not through a build. A binary with fewer adapters is a source
build, and a feature that is not in that list is available in a source build only.

**The musl pair also carries OpenSSL 3**, statically, inside the PostgreSQL ADBC driver that it
links. libpq has no other TLS backend. Its fixes arrive with a `nixpkgs` bump, not with rustls. That
libpq signs in with Kerberos through a static MIT krb5, and has no OAuth flow. `sutura doctor`
prints a `pg driver` line that says whether the binary links that driver and whether it
initialises. Every `kind: postgres` source is answered through it.

**The optimised build is a separate prerelease, `v<version>-performance`**, never `latest`,
published only when a maintainer dispatches `release-performance.yml` on a release tag whose tree
contains the publishing steps of that workflow. A dispatch runs the workflow as the tag holds it.
It builds the same commit at the `release-performance` profile: assets
`sutura-<triple>-performance.tar.gz`, leaf images `:<version>-performance-<triple>`, lists
`:<version>-performance` and `:<version>-performance-musl`. The same signing, SBOM and provenance
sequence runs over it, so the regexp-based `cosign` commands below apply unchanged. The bundle-mode
`gh attestation verify` below does not. For an optimised asset its identity is
`https://github.com/telekom/sutura/.github/workflows/release-performance.yml@refs/tags/$TAG`, and
`$TAG` is the dispatched `v<version>`, not `v<version>-performance`. Its change list, licence
statement and chart are the ones on `v<version>`.

**A release also carries a licence statement. It is a different list on purpose.** The section
[The licence statement](#the-licence-statement) says what each of the two documents answers.

## What is signed

| Artefact                         | Sigstore bundle                                  | SLSA provenance          | Registry signature               |
| -------------------------------- | ------------------------------------------------ | ------------------------ | -------------------------------- |
| the four binary tarballs         | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| the two musl image tarballs      | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| the eight SBOMs                  | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| the two licence documents        | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| `image-digests.txt`              | `<asset>.sigstore.json`, attached to the release | yes                      | n/a                              |
| the `.sha256` sidecars           | no                                               | yes                      | n/a                              |
| the four leaf images             | n/a                                              | no                       | `cosign sign`, by digest         |
| the two manifest lists           | n/a                                              | yes                      | `cosign sign`, by digest         |
| the chart                        | n/a                                              | no                       | `cosign sign`, by digest         |
| each leaf image's CycloneDX SBOM | n/a                                              | n/a                      | `cosign attest --type cyclonedx` |
| `sutura-provenance.intoto.jsonl` | three existing signed attestation bundles        | not recursively attested | n/a                              |

Every count in that table is one binary times four triples, or its consequence. A leaf and the SBOM
beside it have the same key, `<triple>` for `sutura`. `image-digests.txt` records them under that
key, so you match an entry there to an asset on the release page by reading.

Three asymmetries in that table are deliberate. Each has a reason.

**The `.sha256` sidecars are not signed.** A Sigstore bundle over `sutura-<target>.tar.gz` already
commits to that file's digest, so signing a file whose whole content *is* that digest proves
nothing new and costs a certificate and a transparency-log entry. They are still published, because
a script of yours may read them. They get provenance, because one provenance call covers every
subject. **If you must choose, take the bundle:** a `.sha256` file tells you a download was not
corrupted, and a bundle tells you who produced it.

**Provenance covers the manifest lists, not the leaves.** A list is what an unqualified
`docker pull` resolves and what the release notes tell you to pin. The leaves are what a list points
at. `cosign` signs each leaf individually, and to pin one you ask for a single architecture. There
are two lists, one per libc. Provenance does not cover the chart: `cosign sign` signs the chart by
digest, and the table shows nothing more.

**The images get `cosign` and the assets get a bundle.** By default, `gh attestation verify`
fetches provenance from GitHub. With `--bundle`, it reads the exported JSONL instead. Keep that
asset with mirrored files. Verification needs an independently trusted Sigstore root and an explicit
signer and source policy, not only a bundle that the same mirror supplied.

## There is no key

Signing is **keyless**, and no signing key exists in this repository, in its secrets, or on any
machine. The release job mints a short-lived OpenID Connect token that states which repository,
which workflow file and which git ref is asking; Sigstore's certificate authority (Fulcio) returns a
certificate valid for minutes that records that identity; the signature is logged in the public
transparency log (Rekor).

So you do not check *"somebody held the key"*. That claim stays true after a key leaks, until
somebody notices. You check *"this workflow, at this ref, in this repository"*. An attacker has
nothing to steal here. **Every command below passes `--certificate-identity-regexp` and
`--certificate-oidc-issuer`, because those two flags are the check.** Without them, the command only
verifies that *somebody* signed the file.

## Verifying a release asset

There are two ways. They differ in what you need.

From the bytes and the bundle:

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

The second prints the workflow, the ref and the commit that built the artefact. Read them. An
attestation from a *different* ref of this repository verifies, and it is not the release you meant
to download.

### Provenance from mirrored bytes

For releases made by the exporting workflow, retain `sutura-provenance.intoto.jsonl` with the
original assets. It contains five Sigstore bundles: one multi-subject asset attestation and four
manifest-list attestations. It is not a new signing policy. The collector checks the required
JSON, DSSE and SLSA fields and the exact, unique name and digest pairs against `subjects.sha256` and
`image-digests.txt`. It writes the asset only after all inputs pass. It does **not** verify
signatures, certificates or transparency proofs.

Choose the expected tag and the full source commit independently of the downloaded bundle. Supply a
trusted-root file from your trusted distribution channel. Do not trust a file only because a mirror
put it beside the artefact. This example assumes that you already set those values:

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

`--cert-identity` and `--signer-workflow` are **mutually exclusive**. `gh` puts `cert-identity`,
`cert-identity-regex`, `signer-repo` and `signer-workflow` in one flag group, and refuses more than
one of them before it verifies anything. The command uses `--cert-identity`, because it pins the
workflow *and* the tag in one value.

The [verifier's bundle mode](https://cli.github.com/manual/gh_attestation_verify) accepts JSONL
without fetching attestations from the forge. To check mirrored-byte verification in your own setup,
run the command with network access disabled. An OCI reference can still need registry access.
Older releases can lack the export.

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
gh attestation verify oci://ghcr.io/telekom/sutura:0.1.0-musl --repo telekom/sutura
```

## Verifying the chart

The Helm chart is pushed as an OCI artefact beside the images, to `ghcr.io/telekom/charts/sutura`
(`.github/actions/publish-chart`), and signed by digest in the same release run, keyless like the
images. The release notes do not list it: take the digest from the `chart` line of
`image-digests.txt`, the signed asset, which reads
`chart sutura ghcr.io/telekom/charts/sutura@sha256:...`. **Verify by digest, not by tag**, for the
reason the images give.

```bash
cosign verify \
  ghcr.io/telekom/charts/sutura@sha256:... \
  --certificate-identity-regexp '^https://github.com/telekom/sutura/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

The regexp pins the repository, not the tag. When you know the tag, use `--certificate-identity`
in place of the regexp flag, with
`https://github.com/telekom/sutura/.github/workflows/release.yml@refs/tags/<tag>`, to pin the exact
identity. You can run `cosign` from the flake pin with `nix run .#cosign`.

The chart carries a signature and nothing else: no SBOM attestation, no SLSA provenance and no
Sigstore bundle asset. No Helm `.prov` file exists, so `helm install --verify` does not apply.
Whether the registry package is readable without a login is a registry setting.

## What the SBOM covers

Each leaf image carries two inventories of the same scan - CycloneDX and SPDX, so the two cannot
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
executable. The obvious way to produce this list is the wrong one:

- A document generated from `Cargo.lock` beside the binary is a claim that you must check against
  the binary. The two can drift (a rebuild, a re-tag, a file swapped in a mirror), and neither the
  binary nor the document says so.
- It is also the **wrong list**. `Cargo.lock` records what cargo *resolved*, not what the linker
  *kept*. `xtask`, the repository's own gate tool, is a workspace member, so it is in the resolve
  graph and never in the binary. A workspace-wide document names it anyway, and so lists a crate
  that is not in the binary.
- There is one SBOM per image, not one per release.

So the list is put **inside the artefact at build time** and read back out of the bytes that the
scan reads. It cannot drift from the binary, because it is the binary.

The list says which crate versions were compiled in. `cargo audit --bin` reads this section and
checks them against the RustSec database for the file you have.

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

**The attribution document is deliberately WIDER than the SBOM.** The SBOM says what is *in* one
binary, so an overstated SBOM is a false statement about the file in your hand. That is why the list
lives inside the executable. The attribution document discharges a licence obligation, and there the
two errors differ. A crate that is not in the binary costs you one line to read. A crate that is in
the binary and is missing is the failure that the document exists to prevent. So it names every
crate that the workspace resolves at all features, which is more than `sutura-cli` links.

**The release generates the document, and nobody commits it.** A committed derived file falls behind
`Cargo.lock` as soon as a dependency moves, and Dependabot runs with a read-only token and cannot
regenerate it. The release generates the document from the `Cargo.lock` of the tag that it
publishes.

**It names vendored code too.** The two crates under `vendor/mimalloc_rust` are path dependencies,
not registry crates, and `sutura-cli` links the allocator on Linux. They are rows like any other
third-party code. A row depends on the crate not being a workspace member, never on where the code
came from.

**Neither document carries the notice text of each dependency.** The `NOTICE` file of an Apache-2.0
crate lives in its source tree, not in its metadata, so nothing that reads metadata can render it.
