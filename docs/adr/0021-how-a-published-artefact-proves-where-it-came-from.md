---
title: How a published artefact proves where it came from
description: Why a release is signed keylessly with Sigstore rather than with a key we hold, why both a cosign bundle and a GitHub attestation are published when either would verify, what the image SBOM covers and the exact question it cannot answer, and which of the four things a green verification does not establish.
---

# How a published artefact proves where it came from

Status: **accepted, and amended once.** Built. A tagged release publishes a Sigstore bundle per
asset, a `cosign` signature on all six image references, a CycloneDX attestation on each of the four
leaf images, and SLSA provenance for every asset and for the two manifest lists.
`.github/actions/attest-and-sign` is the sequence; `docs/verifying-a-release.md` is what a consumer
reads.

**The amendment closes the gap this record left open**, and it is at the bottom under *Amendment: the
crate graph moved inside the binary*. The paragraph below it, and the section *The SBOM, and the
question it does not answer*, are kept as written rather than corrected in place - they are the
argument the amendment acts on, and rewriting them would leave a reader unable to tell which half
was decided when.

**Nothing here signs source, gates a merge, or says an artefact is good.** The scope is one
question - *are these bytes the ones this pipeline emitted* - and the rest of this record is mostly
about what that does not cover.

## What was wrong, and there was no incident

The release path was already careful about *what* it built. Four cross-compiled targets from a
Nix-pinned toolchain, static linking asserted rather than described, two manifest lists whose
platform entries are checked before anything records them as good, and every image digest read back
out of what the registry said rather than out of local bookkeeping.

What a consumer got was a tarball, a `.sha256` file **served by the same page as the tarball**, and
a table of digests in the release notes. That is a corruption check, not a provenance check: an
attacker who can rewrite the asset can rewrite the sidecar, and a reader comparing the two learns
that the download completed. The digests in the notes are better - they came from the registry - and
they are still text on a page that the same actor would be rewriting.

So the defect is not a leak and not a bad artefact. It is that **the strongest claim a consumer
could check was one the publisher also controlled**, and there was no way to distinguish "this is
what the pipeline built" from "this is what the page currently serves".

## Decision

**A release is signed keylessly through Sigstore, and every asset carries provenance.**

Concretely, in `publish` - the one job that can write anything and the one job that compiles
nothing:

| What | Mechanism |
| --- | --- |
| every release asset except the `.sha256` sidecars | `cosign sign-blob --bundle`, one `.sigstore.json` per asset, attached |
| all six image references, four leaves and two lists | `cosign sign`, **by digest** |
| each leaf image's CycloneDX SBOM | `cosign attest --type cyclonedx`, attached to the image |
| every asset including the sidecars | `actions/attest-build-provenance`, one call via `subject-checksums` |
| the two manifest lists | `actions/attest-build-provenance`, one call each via `subject-digest` |

The tools come from the locked nixpkgs as `apps.cosign` and `apps.syft`, for the reason `apps.deny`
states at length: `nix run nixpkgs#cosign` resolves through the flake registry to whatever
nixpkgs-unstable points at when the job runs, and this is the one job in the repository holding a
write token. Both are apps rather than checks because a Nix build sandbox has no network, and
`cosign` exists to reach Fulcio and Rekor.

### Keyless, and this is the part that is a decision rather than a default

There is **no signing key** in this repository, in its secrets, or on any machine. The job mints an
OIDC token stating which repository, which workflow file and which ref is asking; Fulcio returns a
certificate valid for minutes recording that identity; the signature goes into Rekor.

A long-lived key was the alternative and it is worse in the way that matters. A key is a thing to
store, rotate, and eventually lose, and while it exists it can sign **anything** - so what a
verifier learns from a key signature is *"somebody held the key"*, which stays true for as long as
nobody notices it leaked. What a verifier learns here is *"this workflow, at this ref, in this
repository"*, and there is nothing to steal.

The cost is real and is stated where it bites: **the two identity flags are the check.**
`cosign verify-blob --bundle x.sigstore.json x` with no `--certificate-identity-regexp` and no
`--certificate-oidc-issuer` verifies that *somebody* signed the file. Every command in
`docs/verifying-a-release.md` carries both, and the release notes render them into the table rather
than leaving a reader to know.

### Both a bundle and a GitHub attestation, though either would verify

This looks like redundancy and is not. They differ in **what a verifier has to have**.

`gh attestation verify` asks GitHub's API: it needs network reach to this forge and an
authenticated `gh`, and it needs the attestation store to still hold the record. `cosign
verify-blob` needs the file, its bundle, and Sigstore's trust root - so an artefact somebody
mirrored into their own storage six months ago is verifiable from the bytes they mirrored, with no
account anywhere.

**A release asset exists for exactly that consumer**, so it gets the signature that survives being
copied off this page. The forge-side attestation is the cheaper one to check and covers everything
including the sidecars, because `subject-checksums` takes a whole `sha256sum` file in one call.

### Signed by digest, never by name

Every image signature is against `${IMAGE}@sha256:...`. Signing `${IMAGE}:${tag}` resolves the tag
at signing time, and the same job moves `:latest` two steps earlier - so a signature made against a
name is a signature against whatever that name meant at that instant. The digests come out of
`image-digests.txt`, which the push step wrote from what the registry *said*.

The two lists are signed **without** `--recursive`, because their four children are signed here by
their own digests; `--recursive` would sign each leaf a second time under the list's certificate and
leave two signatures per leaf differing in nothing a verifier reads.

### `push-to-registry` is off, deliberately

`actions/attest-build-provenance` can write its attestation into the registry as an OCI referrer,
for a verifier that reads the registry and not the forge. That verifier is already served: `cosign
sign` covers all six references and `cosign attest` covers all four leaves, and
`gh attestation verify oci://...` resolves the digest from the registry and fetches the attestation
from GitHub - so nothing is unreachable without it. It would also need registry credentials held
across a step the workflow does not control, which is the shape the login/logout bracketing in that
job exists to avoid.

### The `.sha256` sidecars are not signed

A bundle over `sutura-<target>.tar.gz` already commits to that file's digest, so a signature over a
file whose entire content **is** that digest proves nothing new and costs a certificate and a
transparency-log entry per release. They keep provenance, which is free, and they stay published
because a consumer's script may read them. `docs/verifying-a-release.md` says which of the two to
take and why.

### The sequence is a local composite action

`.github/actions/attest-and-sign`, and it is a split with two reasons rather than a preference.
`release.yml` is hand-written configuration under the 1000-line cap `cargo xtask max-lines`
enforces, and inlining this reached 1084 - so the split was forced, and the question was only where
to cut. The signing sequence is the right cut because it is **the part of the release path that
grows**: every artefact kind added later is another thing to sign, and the loops take it without a
new step. `.github/actions/reclaim-disk` is the precedent, and its argument for being local rather
than third-party applies unchanged.

**What that action cannot do is know what should have been published.** It signs what it is handed,
so an artefact that never reaches `dist/` is not signed and nothing in it notices. The step that
keeps that from becoming a quietly unsigned release is `Check that all four builds arrived`, which
already existed for the analogous failure and which this change extends with the SBOMs.

## The SBOM, and the question it does not answer

Each leaf image gets a `syft` scan rendered as CycloneDX and SPDX from one pass, so the two cannot
disagree. It reads the image **tarball** rather than the daemon or the registry, so what it
inventories is exactly the bytes `publish` pushes rather than an image resolved through a tag that
could have moved between two steps. It runs in `build`, because that job is where compute belongs
and the token-holding job inspects nothing.

**What it covers:** the files in the image. That is the binary, the CA certificate bundle and
tzdata, and nothing else - there is no shell and no package manager in there.

**What it does not cover: the crate graph.** `syft` reads a Rust dependency list out of a
`Cargo.lock` on disk or out of a `cargo auditable` section in the executable, and the shipped image
has neither - nothing ships a lock file and the release profile embeds nothing. So this document is
an accurate answer to *"what files are in the image"* and it is **not** an answer to *"which crates
is this built from"*.

That gap is stated in four places on purpose - the workflow step, the release notes table, the
verification page and here - because an SBOM naming no crates is a well-formed document that
publishes, attaches and verifies exactly like a real one, and a reader who assumes otherwise has
been misled by a green check. The step therefore also **asserts** that the document names at least
two things, on the same reasoning as the static-linking and manifest-list assertions either side of
it: a scan that inventoried nothing must fail rather than ship.

Closing the gap is a separate change, and it is a build change rather than a reporting one: the
crate graph belongs in the artefact rather than beside it, which means `cargo auditable` on the four
cross builds and a CycloneDX document scoped to the shipped package rather than to the workspace.
**Scoped, because the workspace graph is not the binary's graph and saying so would be the exact
overstatement this section is about** - `sutura-cli` links the engine only, while `libduckdb-sys`
and the BigQuery wire put `ureq`, rustls and `ring` in the resolve graph for a binary that links
none of them. `deny.toml` has carried that argument for longer than this record has existed.

## What this costs, measured where it can be

**No new packages, anywhere.** `cosign` and `syft` are nixpkgs derivations reached through `nix
run`; nothing enters `Cargo.lock`, nothing enters the shipped binary, and no image layer changes. The
artefact bytes this change publishes are byte-identical to what the previous release path would have
published.

**One third-party action, GitHub's own, pinned by commit** - `actions/attest-build-provenance` at
`v4.2.2`, resolved to its commit like every other action in these workflows. Checked 2026-08-31;
re-resolve rather than trusting this line, per this repository's own rule that a version in a record
is stale before anybody builds from it.

**Two more write grants on `publish`**, and the paragraph in `release.yml`'s header was rewritten
rather than left to be inferred: `id-token: write` mints the OIDC token Fulcio signs against and
`attestations: write` stores the provenance. Neither can read secrets and neither can push code, and
the property that made the three-job split worth having is unchanged - **this job still never
executes anything the build jobs produced.** `cosign` and the attestation action hash bytes and sign
digests; `docker load` and `docker push` move layers without running an entrypoint; the smoke tests
stayed in `build`.

**Wall clock.** One `cosign sign-blob` per asset means one Fulcio certificate and one Rekor entry
each, and `nix run` is invoked per call - so the signing steps are dominated by round trips rather
than by compute. Not measured yet: this record is written before the first tagged release runs it,
and the honest thing to say is that the number will be in a release log rather than here. Excluding
the `.sha256` sidecars roughly halves it, which is a second reason for that exclusion and not the
first.

## What was considered instead

**`slsa-framework/slsa-github-generator`.** The canonical SLSA generator, and it produces a level-3
provenance where `actions/attest-build-provenance` produces level 2 - the difference being whether
the provenance is generated in an isolated reusable workflow the calling repository cannot influence.
Declined for now on cost against threat: it is a third-party reusable workflow with its own release
cadence in the one job that can write, it wants the build restructured to hand it subjects, and the
attacker it defends against - one who can modify this workflow file - can also modify the source. The
same argument the cache-poisoning suppression in that file already makes. **Revisit if this repository
ever publishes to a registry consumers cannot re-derive from source.**

**A KMS-held key.** Rejected above, in *Keyless*. A key would also make the signature verifiable
without Sigstore's trust root, which is the one genuine advantage and does not outweigh having a
thing that can sign anything, forever.

**Deep licence and dependency scanning as part of this change.** A curated licence review and an
attribution document are a real obligation and a real gap - and they are a JVM container that needs
network, curated upstream data, and minutes rather than seconds. Putting them here would have made
one change out of two and made neither reviewable. Separate.

**Signing at build time rather than at publish time.** Would put `id-token: write` on the four
matrix jobs that compile, which is exactly the token boundary the three-job split bought. The
signature is over bytes and does not care which job produced them.

## What is claimed, and what is not

Claimed, and checkable:

- Every published asset can be traced to this repository's release workflow at a named ref, from the
  bytes alone plus Sigstore's trust root.
- Every image reference carries a signature over its **digest**.
- Each leaf image carries an inventory of its own filesystem, in two formats, generated from one
  scan.
- The counts are asserted rather than assumed - six signatures, four attestations, one bundle per
  asset. **What they catch is a PARTIAL set**, three leaves where four were built, which is silent:
  the loop signs what it finds and reports success. An empty set is caught a layer earlier by
  something else - `grep` with no match exits non-zero under `set -eu` - so the counts are for the
  case where the file is present, well formed, and short. An earlier version of this line credited
  them with the empty case, which `set -e` already had.

Not claimed:

- **Not that the build is reproducible by a third party.** The provenance says which workflow ran.
  The build is Nix-pinned, which is what makes a rebuild worth attempting; nothing here is evidence
  that anybody has.
- **Not that the dependency set is free of advisories.** That is `cargo-deny` in CI, and its verdict
  is a run rather than an artefact.
- **Not that licence obligations are discharged.** `deny.toml` is an allowlist of SPDX identifiers,
  which is a policy check. There is no attribution document, and `NOTICE` still says nothing about
  third-party crates.
- **Not that the SBOM is a dependency inventory.** See above; it is a filesystem inventory, and the
  distinction is the most load-bearing sentence on this page.
- **Not that a transparency log is honest.** `cosign` checks an inclusion proof against Rekor. Rekor's
  guarantees are Rekor's.

## Amendment: the crate graph moved inside the binary

**The four shipped binaries are built with `cargo auditable`, so the crate list the SBOM reports is
read out of the artefact rather than assembled beside it.** `nix/auditable.nix` is the mechanism;
`syft`'s `cargo-auditable-binary-cataloger` is what reads it back. The section *The SBOM, and the
question it does not answer* above is superseded on its central claim and kept for its argument.

### Why not a document generated from `Cargo.lock`

That was the obvious option and it is wrong twice.

**It is a checked claim rather than a construction.** A CycloneDX file generated beside a binary can
drift from it - a rebuild, a re-tag, a file swapped in a mirror - and neither the binary nor the
document says so. A section *inside* the executable cannot drift from the executable. This
repository's own rule is *prefer unrepresentable to checked*, and `Secret` and `TimeRange` are that
rule applied to values; this is the same move applied to provenance.

**And it would be the wrong list.** `Cargo.lock` records what cargo RESOLVED, not what the linker
KEPT. `sutura-cli` links the engine only, while `libduckdb-sys` and the BigQuery `wire` feature put
`ureq`, rustls and `ring` into the resolve graph for a binary that links none of them - an argument
`deny.toml` has carried at length for longer than this record has existed. A workspace-wide document
names all three, and errs in the direction that matters: it overstates what ships. Scoping it to
`--package sutura-cli` narrows that and does not fix it, because a feature-gated dependency is still
in the resolve.

### What it cost

**Nothing in `Cargo.lock` and nothing at run time.** `cargo-auditable` is a build-time tool from the
locked nixpkgs, on `nativeBuildInputs` of the final build only. The section it adds is a compressed
crate list - kilobytes.

**The dependency closure is still shared.** The tool is deliberately NOT added to the args that feed
`crane.buildDepsOnly`, because that derivation has to stay byte-identical to the one every check
reuses. It does not need to be: `cargo auditable` works by setting `RUSTC_WORKSPACE_WRAPPER`, which
by construction applies to workspace members and not to registry dependencies.

**One correctness trap, found by reading crane rather than by a failed build.** crane's default build
command is `cargoWithProfile build`, a shell helper that inserts the profile flag after the FIRST word
of the command. `cargoWithProfile auditable build` emits `cargo auditable --release build`, which is
not a command. So the profile flag is computed in Nix, where the profile is already a parameter.

**Every shipped binary's digest changes**, and so does every image digest. That is a new release
either way, and the builds stay reproducible: the embedded data is a function of the lock file.

### What is now claimed, and what still is not

Claimed: the SBOM attached to each leaf image, and to the release, names the crates the compiler
actually put into that binary, and the release job **asserts** it rather than trusting it - at least
100 named entries and `datafusion` present by name. That assertion exists because the failure is
silent in a specific way: if the section stops being produced, the scan still succeeds and still
writes valid CycloneDX inventorying three files, and nothing downstream can tell that apart from a
correct run. The floor is a floor and not a fixed count, because pinning the count would make a
dependency bump fail on a correct tree.

Not claimed, and each is a real edge:

- **Not complete for non-Rust code.** The list is what cargo compiled. C that a build script
  compiled - the vendored allocator, for one - is linked into the executable and is not a crate.
- **Not a statement about the crates.** It says which versions were compiled in, not whether any has
  an advisory. That is `cargo-deny`, and its verdict is a CI run rather than a property of the
  artefact. `cargo audit --bin` reads this section and answers that question against a file in hand.
- **Not complete for non-Rust code.** Repeated because it is the one most likely to be forgotten
  when this list is quoted: the vendored allocator's C is in the executable and in no crate list.

### What was measured, and the one thing that had to move to CI

**The section is there.** `nix build .#sutura` on 2026-08-31, then `rust-audit-info` over the
result: **263 crates**, `datafusion` among them. That is the reference reader, and it is what says
`nix/auditable.nix` does anything at all.

**`syft` could not be shown to read it locally, and the reason is worth writing down rather than
hedging about.** On the machine this was written on the native binary is Mach-O, and `syft` named
**one** component where `rust-audit-info` had just read 263 - including with the cataloger selected
explicitly and with a directory scan rather than a file scan. The cause is in `go-rustaudit`, which
`syft` uses: it locates the data by asking for a section named `.dep-v0`, and on Mach-O
`cargo-auditable` does not put it under that name. **Nothing we ship is Mach-O**, so this is not a
defect in the artefact - but it does mean the single thing `release.yml` depends on is observable
only on a Linux target, and this machine cannot build one.

**So the proof moved into CI, and specifically into the job that already builds those targets.**
`ci.yml`'s `cross` job now runs both readers over each of the four shipped binaries on every pull
request: `rust-audit-info` for *is the section there*, `syft` for *can the release path's reader
parse it*, each with the same floor of 100 and the same named crate. Two tools because the
interesting run is the one where the first passes and the second fails - and without both, that is
indistinguishable from a build that stopped embedding.

**That placement is the point rather than a detail.** Checking it only in `release.yml` means the
same failure arrives as a red tag, after four cross builds and two manifest lists have been paid
for, on a branch that has already merged.
