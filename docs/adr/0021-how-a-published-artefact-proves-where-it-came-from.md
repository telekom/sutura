---
title: How a published artefact proves where it came from
description: Why a release is signed keylessly with Sigstore rather than with a key we hold, why both a cosign bundle and a GitHub attestation are published when either would verify, what the image SBOM covers and the exact question it cannot answer, and which of the four things a green verification does not establish.
---

# How a published artefact proves where it came from

Status: **accepted, and amended twice.** Built. A tagged release publishes a Sigstore bundle per
asset, a `cosign` signature on all six image references, a CycloneDX attestation on each of the four
leaf images, and SLSA provenance for every asset and for the two manifest lists.
`.github/actions/attest-and-sign` is the sequence; `docs/verifying-a-release.md` is what a consumer
reads.

**The second amendment closes the roadmap bullet's licence half** and is at the bottom under
*Second amendment: the attribution document, and the licence report as a signed asset*. **The first
amendment closes the crate-graph gap** and is at the bottom under *Amendment: the crate graph moved
inside the binary*. The paragraphs between them are kept as written rather than corrected in place -
they are the arguments the amendments act on, and rewriting them would leave a reader unable to tell
which half was decided when.

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
overstatement this section is about** - `sutura-cli` AS PUBLISHED links the engine only (its
`bigquery` feature is default-off, and `nix/shipped.nix` passes no `--features`), while
`libduckdb-sys` and the BigQuery wire put `ureq`, rustls and `ring` in the resolve graph for a binary
that links none of them. `deny.toml` has carried that argument for longer than this record has existed.

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
KEPT. `sutura-cli` as published links the engine only, while `libduckdb-sys` and both composition roots'
default-off `bigquery` features put `ureq`, rustls and `ring` into the resolve graph for a binary
that links none of them - an argument
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

### What was measured

Measured on 2026-08-31 against `nix build .#sutura`, the native release binary:

| Reader | Result |
| --- | --- |
| `rust-audit-info` | **263 packages**, `datafusion` among them |
| `syft`, CycloneDX | **241 components** |
| `syft`, SPDX | **242 ids** - the 241 packages plus the document itself |

So the section is embedded and the reader the release path uses turns it into an inventory. Both
halves are observed rather than argued.

### The measurement that was wrong, and what it cost

**This is the part worth reading, because the mistake was not in the code under test - it was in
the check, and it produced a confident false conclusion that reached three files before anything
caught it.**

The first version of both assertions counted with **`grep -c '"name":'`**. `grep -c` counts
matching **lines**, and syft and `rust-audit-info` both write JSON on **one line** - so it returns
`1` for a document naming 241 packages and `1` for a document naming none. It cannot distinguish
the two cases it exists to distinguish.

What that produced was not a useless number but a misleading one. Locally it read as *"syft named
one component where `rust-audit-info` read 263"*, and a whole explanation was built on it: that
`go-rustaudit` locates the data by asking for a section named `.dep-v0` and that `cargo-auditable`
spells it differently on Mach-O. That explanation is **false**. Counted properly, the same file has
2259 occurrences of `"name":` and 241 components. syft reads Mach-O fine. The claim was written
into a workflow comment, this record and a pull request body before the real count was taken.

Three things follow, and they are the reason this section exists rather than a silent correction:

- **Count occurrences, never lines.** `grep -o ... | wc -l`. Both assertions now do.
- **Count the thing that is the inventory, not a proxy for it.** `"name":` also appears in tool
  blocks, properties and metadata. CycloneDX gives every component a `"bom-ref"` and SPDX gives
  every package an `"SPDXID"`; a two-line case on the format buys an exact number, and the earlier
  comment claiming that avoiding the case was a virtue is what produced the fragile proxy.
- **A plausible mechanism is not evidence.** The Mach-O story was coherent, checked against
  `go-rustaudit`'s actual source, and wrong - because the number it was explaining was an artefact
  of the measurement. Reading upstream source confirmed that the explanation *could* be true; it
  could not confirm that it *was*.

### Why the check lives on the pull request as well

`.github/workflows/cross-link.yml`'s link matrix - `ci.yml`'s `cross` job is the caller that gates
it - runs both readers over each of the four shipped binaries on every pull request. **It earned its
place immediately: it is what caught the broken assertion**, on the pull request, before a release
depended on it - and `release.yml`'s own version of that check would have failed every release with
the counting bug in it.

Two readers rather than one, because they answer different questions: `rust-audit-info` says *is the
section there*, `syft` says *can the release path's reader turn it into an inventory*. The run where
the first passes and the second fails is the interesting one, and without both it is
indistinguishable from a build that stopped embedding. Both syft formats are checked, so a format
rendered differently cannot pass here and fail at the tag.

**The placement is the point.** Checking only in `release.yml` means the same failure arrives as a
red tag, after four cross builds and two manifest lists have been paid for, on a branch that has
already merged.

## Second amendment: the attribution document, and the licence report as a signed asset

**This record's *Not claimed* list said *"There is no attribution document, and `NOTICE` still says
nothing about third-party crates."* There is one now, and the licence report is no longer a CI
artifact that expires.** Two decisions, and the reason they are one amendment is that the first
decides the shape the second is signed in.

### What the attribution document is, and where it lives

`ATTRIBUTION.md`, **committed**, generated by `just attribution` from `cargo metadata`, and gated by
`cargo xtask check-attribution` against `Cargo.lock`. It carries one row per third-party crate: the
name, the resolved version, and the SPDX expression that crate's own manifest declares.

Committed rather than release-only, and the *Canonical Sources* rule in `AGENTS.md` is why: one
owner, regenerated rather than hand-edited, and the regeneration checked. A release-only document is
none of those - it has no owner a reviewer can open, and nothing can fail when it falls behind. The
release still publishes it, as a copy, which is a different thing from generating it there.

**Two gates, and the split is which input each one needs.** `cargo xtask check-attribution` reads
`Cargo.lock` and the document and invokes nothing, so it runs on every commit and inside the nix
sandbox: it holds the crate SET exactly. `cargo xtask check-attribution-current` regenerates and
byte-compares, so it holds the CONTENT - and it needs a resolvable registry, which is why it is a
`just gates` and `ci.yml` step rather than a hygiene gate. `check-api-docs` is the precedent, and the
reason has the same shape: an input the sandbox has not got.

**The second gate is a REVIEW'S CORRECTION, and why the first alone was not enough is the part worth
keeping.** The offline gate can only see that a licence cell is non-empty, so changing any row's SPDX
expression to arbitrary text passed it: the main content of a generated artefact was trusted rather
than compared, which is the one thing the *Canonical Sources* rule forbids. It also closes a case the
crate key cannot see at all - a git dependency moving to another revision, changing its declared
licence while keeping its name and version.

**Which packages belong in it is decided by workspace MEMBERSHIP and not by the presence of a
`source` line, and that is a second review correction of a real omission.** A `source` entry means
"from a registry or a git remote"; it does not mean third-party. `mimalloc` and `libmimalloc-sys` are
vendored under `vendor/` and declared as PATH dependencies, so they carry none - and `sutura-cli`
LINKS the allocator on Linux. Filtering on `source` therefore left two shipped third-party crates out
of the released asset, which is exactly the failure this record says the document exists to prevent,
and the code passed its own tests while doing it. `VENDOR.md` and `REUSE.toml` remain those trees'
provenance record; neither puts a row in the file a consumer downloads. The member list is read off
the root manifest and each member's own `name`, because a member is a PATH and `dev` holds
`sutura-dev`.

### Why a document generated from `Cargo.lock` is right here and wrong for the SBOM

**This amendment does the opposite of *Why not a document generated from `Cargo.lock`* above, and
that section is not superseded - the two artefacts fail in opposite directions.**

An SBOM's job is to say what is *in* an artefact. A workspace-wide list overstates it, and an
overstated inventory is a false statement about the thing in your hand; that is why the crate graph
went inside the binary, where it cannot drift.

An attribution document's job is to discharge an obligation. Naming a crate that did not ship
discharges an obligation nobody had, which costs a reader one line. **Omitting one that did ship is
the failure the document exists to prevent.** So the safe error points the other way, and the wide
list is the right list: every crate the workspace resolves at all features, which is more than any
single binary links.

Both facts are published, and a reader is told which answers which: the per-binary list is the
`cargo auditable` section, read back by `syft`; `ATTRIBUTION.md` is the obligation.

### The licence report joins the signed assets, and it had to move workflows to do it

`licence-review.yml` produced the report and kept it with `actions/upload-artifact` - **a CI
artifact that expires after ninety days, is not signed, and is not committed.** The one output an
auditor would ask for was the one output no consumer could obtain or verify.

**Uploading it to the release afterwards is not available, and that is a property of this forge
rather than a preference.** This repository has GitHub's immutable releases enabled: once a release
is published, its assets can never be added to, replaced or deleted - `release.yml`'s `Publish` step
carries the `HTTP 422` that taught us. A report produced by a second workflow racing the first
therefore cannot become an asset of it at all.

So the tag path moved into `release.yml` as a `licence` job, `needs: [gates]`, which packs the
reports into one deterministic tarball that lands in `dist/`. Everything after that is machinery
that already existed: `.github/actions/attest-and-sign` signs every file in `dist/` and gives every
one provenance, and its own header predicted this - *"Every artefact kind added later - a
crate-graph SBOM, an attribution document - is another thing to sign, and the loops below take it
without a new step."* It was a list entry rather than a new mechanism, exactly as the issue said.

`.github/actions/licence-review` is the ORT sequence, shared by both callers. `licence-review.yml`
keeps the schedule, which is what it was actually good at - catching a world that changed under a
lock nobody touched - and holds `contents: read` alone.

### What it cost

**A red licence review now fails a release.** That is the trade and it is deliberate: the point of
the change is that the licence statement ships *with* the artefact, so a release that skipped it on
a red run would be a release with no statement, which is the state being left. It is `needs:
[gates]` rather than `needs: [build]`, so it runs beside the four cross builds and costs the release
only whatever ORT exceeds the slowest of them by.

**Two steps left `release.yml` for composite actions**, because that file is under the 1000-line cap
`cargo xtask max-lines` enforces and the licence statement is what reached it. The SBOM step moved
intact into `.github/actions/build-artefacts`, which already carries the per-target binary and image
sequence for the same reason - **and the two extractions happened twice, in parallel, on `main` and
on `dev`.** This branch keeps the `build-artefacts` one, because it holds all three steps rather than
one and the release path calls it once per target instead of twice. `attest-and-sign`'s header had
said the cap would be reached by the next artefact kind and that the split would then be made under
pressure rather than for a reason, so the reason is recorded here instead.

**And that split had already opened a hole in a gate, which this closes.**
`cargo xtask check-workflows` reads `.github/workflows` and verifies that every `nix run .#name` is
a flake output that exists. It did not read `.github/actions`, so `attest-and-sign`'s
`nix run .#cosign` - the reference that publishes a release - was the one reference nothing checked.
The scan now walks both, and the test asserts *where* a reference came from rather than the verdict,
because a gate that walks one directory and a gate that walks two agree on a correct tree.

**And a second hole of the same shape, found while writing the first fix.** Nothing linted the SHELL
inside a composite action either. `actionlint` is the tool that would normally carry it - it shells
each workflow's `run:` block out to `shellcheck` itself - and it **cannot read a composite action at
all**: measured against the pinned 1.7.12, handed `.github/actions/attest-and-sign/action.yml` it
reports `"jobs" section is missing in workflow` and `unexpected key "runs" for "workflow" section`,
because it parses the file as a workflow. And the shellcheck list `nix/lint-workflows.sh` builds
from tracked files holds no `run:` block, because a `run:` block is not a file. So the release
path's `cosign` invocations were shell nothing had ever read.

`cargo xtask action-shell` extracts each block into a real file carrying a pointer back to its
source line, and `just lint-actions` plus `ci.yml`'s workflow-analysis step pair it with the pinned
`shellcheck`. **Extraction rather than moving the shell into `.sh` files**, which would have needed
no new mechanism at all: a step is read as configuration, and somebody following `release.yml` into
the action to see what `cosign` is invoked with should find the invocation rather than a path to it.
The cost of that choice is a parser that can be wrong, which is why it is Rust with tests and why it
**fails closed** - a composite action it read no shell out of is a non-zero exit, not a quiet
zero-script run that reads as coverage. It found ten blocks across four actions on the first run,
four of them `attest-and-sign`'s, and `shellcheck` is clean on all ten.

**What that does NOT recover** is everything `actionlint` would have said about the action itself -
that an input exists, that an expression is well formed, that a `shell:` is declared. Closing that
needs an `actionlint` that reads composite actions, and the measurement above is what makes it a
version to watch rather than a hope.

### What is now claimed, and what still is not

Claimed, and checkable:

- A consumer can obtain the attribution document **for a specific release** and verify it with the
  commands in `docs/verifying-a-release.md`, from the bytes and the bundle alone.
- `ATTRIBUTION.md` names every third-party package in `Cargo.lock` - registry, git and vendored path
  alike - at the resolved version, and names nothing else. That is `cargo xtask check-attribution`,
  in `just validate`.
- And it is what its generator produces, byte for byte. That is `cargo xtask
  check-attribution-current`, in `just gates` and in `ci.yml`.
- The release asserts it independently: at least 100 named rows, the floor the SBOM assertion
  already uses and for the same reason - pinning the count would fail a correct tree after a bump.

Not claimed, and each is real:

- **Not that the licence expressions are TRUE of each crate's source.** They are what the manifests
  declare, copied through - and now compared against a fresh generation rather than trusted.
  Verifying them against the licence FILES in each crate's tree is a source scan this repository
  does not perform.
- **Not the notice text of each dependency.** An Apache-2.0 crate's own `NOTICE` file is in its
  source tree, not in its metadata, so the document does not render one. **This is the largest remaining
  gap in the obligation** and closing it needs the scanner that is deliberately absent.
- **Not that the document is the list a given binary links.** Deliberately wider; see above.
- **Not that composite actions are fully linted.** Their SHELL is, now. The action metadata around
  it is not, and cannot be until `actionlint` reads these files. **And what the extraction models is
  the SUBSTITUTED text, which took two goes.** A GitHub expression is replaced by a variable
  expansion, so an unquoted interpolation reports SC2086 as the real thing would - a bare literal
  cannot word-split, so the first version hid the very class it exists to catch, and a review found
  it. The fix then had to learn quoting: `reclaim-disk` writes `minimum='${{ inputs.minimum-gb }}'`
  and single quotes are the SAFE form there, so a `$`-prefixed token inside them reports SC2016 on
  correct code and turned a clean run red. Both halves are measured against the pinned `shellcheck`
  0.11.0 and pinned by tests.
- **Not verified end to end.** `just validate` covers the generator, the gate and the document. The
  release half is verified the way the rest of this path is - a tag, then `cosign verify-blob` and
  `gh attestation verify` against the released asset.

### Amendment: the ORT report is removed

**On 2026-09-01 the ORT report and its scheduled workflow were removed.** Its Cargo analyzer
expands each workspace member's dependency tree independently and exhausted a 5 GiB heap on this
workspace. Raising the heap preserves a duplicate workspace-wide SBOM and advisory pass, not a
unique release requirement: `cargo deny check` owns licence policy and RustSec advisories,
`ATTRIBUTION.md` owns the checked workspace-wide declared-licence list, and Syft owns the two
per-binary SBOM formats. The source scanner was disabled, so ORT did not close the remaining
dependency-`NOTICE` gap. This amendment supersedes the earlier sections that describe the report as
still produced.
