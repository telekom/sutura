# Verifying a release

Every artefact a tagged release publishes is signed, and every one carries SLSA provenance. This
page is what the signatures mean, the commands that check them, and - the part that matters more -
what they do **not** say.

## Read this part first

**A signature answers "did this pipeline produce these bytes". It does not answer "are these bytes
good".** Provenance is an *identity* claim, and the whole of what it establishes is that the file in
your hand is byte-for-byte what `github.com/telekom/sutura`'s release workflow emitted at a tag,
rather than something a mirror, a proxy or a compromised download page substituted. It says nothing
about whether the code is correct, whether a dependency has an advisory against it, or whether the
release is fit for what you want to do with it. The gates say the first, `cargo-deny` says the
second, and nothing says the third.

**And the SBOM published today inventories the image's files, not the crate graph.** That
distinction is spelled out under [What the SBOM covers](#what-the-sbom-covers) and it is the one
thing on this page most likely to be read as more than it is.

## What is signed

| Artefact | Sigstore bundle | SLSA provenance | Registry signature |
| --- | --- | --- | --- |
| the four binary tarballs | `<asset>.sigstore.json`, attached to the release | yes | n/a |
| the two musl image tarballs | `<asset>.sigstore.json`, attached to the release | yes | n/a |
| the eight SBOMs | `<asset>.sigstore.json`, attached to the release | yes | n/a |
| `image-digests.txt` | `<asset>.sigstore.json`, attached to the release | yes | n/a |
| the `.sha256` sidecars | no | yes | n/a |
| the four leaf images | n/a | no | `cosign sign`, by digest |
| the two manifest lists | n/a | yes | `cosign sign`, by digest |
| each leaf image's CycloneDX SBOM | n/a | n/a | `cosign attest --type cyclonedx` |

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
on purpose.

**The images get `cosign` and the assets get a bundle, and the reason is who can verify.**
`gh attestation verify` asks GitHub's API - it needs reach to the forge and an authenticated `gh`.
`cosign verify-blob` needs the file, its bundle and Sigstore's trust root, so an artefact you
mirrored into your own storage six months ago is still verifiable from the bytes you mirrored, with
no account anywhere. That is the case a release asset exists for.

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

**What it covers is the files in the image**, which is the binary, the CA certificate bundle and
tzdata - there is no shell and no package manager in there, so that list is short and complete.

**What it does not cover is the crate graph the binary was compiled from.** The scanner reads a Rust
dependency list out of a `Cargo.lock` on disk or out of a `cargo auditable` section in the
executable, and the shipped image has neither: nothing ships a lock file, and the release profile
does not embed one. So this document is an accurate answer to *"what files are in the image"* and it
is **not** an answer to *"which crates is this built from"*. Do not cite it as a dependency
inventory; a release that can answer the second question will say so on this page.

## What none of this establishes

Stated plainly, because a page full of green checkmarks invites the larger reading:

- **Not that the build is reproducible from source by you.** The provenance records which workflow
  ran, not a bit-for-bit rebuild recipe you can execute. The build *is* Nix-pinned, which is why
  that is worth attempting - but nothing here is a proof that you did.
- **Not that dependencies are free of known advisories.** That is `cargo-deny` against the RustSec
  database, run in CI on every dependency change and weekly regardless, and its verdict is a CI run
  rather than an artefact you can check offline.
- **Not that the licence obligations of the dependency set are discharged.** `deny.toml` holds an
  exact allowlist of SPDX identifiers, which is a policy check and not an attribution document.
- **Not that a transparency-log entry proves the log was honest.** Rekor's guarantees are Rekor's,
  and `cosign` checks an inclusion proof against it rather than auditing it.
- **Not that a verified image is one you should run.** It is the image this pipeline built. Whether
  that pipeline should be trusted is a question about this repository, and the answer to it is
  everything else in these docs.
