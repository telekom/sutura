---
title: An OSS binary cache for this repository's own paths
description: Why the decision in 0026 was reversed within a day, what changed to reverse it, which half of the store a substituter carries that the Actions cache cannot, how the write credential is bounded by an environment rather than by a workflow condition, and which control here is owner-held because no gate can reach it.
---

# An OSS binary cache for this repository's own paths

Status: **accepted**, 2026-09-09. **Supersedes `docs/adr/0026` (No third-party binary cache).** That
record is not wrong about what it decided or why; two of its inputs changed on the same day, and this
is the re-decision plus what changed.

## What changed, and it is two things rather than a preference

**1. The repository is public**, so an OSS tier exists that did not apply to the question 0026
answered.

**2. The effective Actions cache allocation is about a tenth of what the proposal it inherited
assumed.** `#472` was built on a recorded 100 GB; the repository's own storage-limit endpoint reads
**10 GB**. One generation of the six cache scopes measures roughly **17.2 GB compressed**, so the
working set exceeds the cap by ~70% and the largest entry - the one a pull request most needs - is
evicted every cycle. 0026's argument was *the Actions cache is enough*; that premise did not survive
measurement.

## The decision

**One store outside this repository is trusted: `https://sutura.cachix.org`, signed by
`sutura.cachix.org-1:ujnKDi7ITrNVSQofXXvhiLhxoVUaYcFOM+qjW/+yGz0=`.** Both halves are **committed in
the clear**. The Actions cache is **kept**, because the two carry different things.

### Why both, and not one

| Carrier | Holds | Unit |
| --- | --- | --- |
| the binary cache | nix **store paths** this repository builds | one path, content-addressed, deduplicated across every scope |
| the Actions cache | the writable cargo target directories the warm start seeds, which are **not** store paths | one tarball per scope, per key |

The Actions entry archives the whole store a job needs - the tier images, the duplicate-detector's
runtime, the database engine, the coverage tools, the linker - **all of which the upstream cache
already serves for free**. A substituter only has to hold what nobody else has. Measured: this
repository's shared dependency derivation is **~829 MB** of NAR against a **1.46 GB** closure, i.e.
its own path is about **57%** of that closure, and the vendored-source paths are **under 3 KB**. So
the unit of storage for the substituter is far smaller than the archive it replaces, and it
deduplicates across the scopes that currently store overlapping closures separately.

### Why the key is committed rather than secret

A public key is not a secret - it is published by the cache. Committing it buys two things a variable
or a secret cannot: **a reviewer sees which key is trusted in the diff**, and **`cargo xtask
check-workflows` can compare against it**. A value in a repository variable can be swapped with no
diff at all, and no gate can read it. The gate therefore permits **exactly this pair in exactly the
assignment spelling** and refuses an added store, a different key, that key on another host, and this
store passed as `--extra-substituters` or `--option substituters` on a command line - the flag
spelling stays refused even for the permitted store, because that is how a step reaches past the
configuration a reviewer read.

**What bounds the trust is nix's signature check**, not the gate: a path signed by a key that is not
named is not accepted. The gate's job is to keep the *set of named keys* honest.

## The write credential, and why an environment rather than an `if:`

Writes happen only from a push to the default branch. That is enforced **twice, deliberately**:

- the publishing workflow triggers on nothing else, and
- the credential is an **environment secret** whose deployment-branch policy admits only that branch.

The second is the load-bearing one. **A same-repository pull request can read repository secrets
here** - this project stacks branches in-repo rather than working through forks - so a pull request
that edited a workflow could have reached a repository-level token. The trigger lives in a file a
pull request can edit; the environment policy is held by the forge and a pull request cannot. The
repository-level copy of the token was **deleted**, which is what makes the environment more than a
label: an environment secret does not shadow a repository secret, so leaving both would have left the
protection cosmetic.

The publish is also a **separate workflow**, not a step in the main one, because an `environment:` is
declared per job: naming it on the main job would make every pull request's run fail the branch
policy.

## What is NOT held by a mechanism, and is therefore owner-held

**No gate in this repository can verify the environment's branch policy.** `check-workflows` runs
inside a derivation with no network, so it can pin the `environment:` key and refuse its removal, but
not the protection behind the name. Worse, **a job naming an environment that does not exist can
cause one to be created with no protection rules** - which reads as configured and protects nothing.

Verified by hand on the day this landed, and this is the evidence rather than an assurance: the
environment exists, its protection rules list a branch policy, that policy names the default branch
only, the token is present as an environment secret, and the repository-level secret list no longer
contains it.

## Deliberately excluded: tags

The release path is **not** given a publishing credential. Release-profile derivations are a
**disjoint** set of store paths - a different profile is a different derivation is a different path -
so publishing them would roughly double what a small tier must hold while competing with the
generation that pull requests actually need, and the payoff accrues only *between* releases, which
are rare. A tag-triggered job holding a write credential is also a second route to it under a weaker
protection instrument.

**The counter-fact, recorded so a revisit starts from it rather than from scratch:** the native
release derivations are built **only** by the tag-triggered release workflow, so they are cached
nowhere, and one measured attempt spent about an hour building them. Their size is **unmeasured** and
may exceed the ci-profile paths if the release profile enables link-time optimisation. If release
build time becomes the complaint, the answer is a **separate credential for the release environment**,
not widening this one to tags.

## What this record does not claim

**No speed improvement is measured.** There is no post-merge hit rate, no before-and-after wall clock,
and none is asserted: this repository has had three CI diagnoses refuted for exactly that substitution,
and run-to-run variance on identical inputs has been measured at 64%. What is measured is the storage
arithmetic above and the eviction it explains.

**Two further numbers are still open**, and they decide whether the tier is comfortable or marginal:
the real NAR-to-compressed ratio on these paths, and what fraction of a job's closure is this
repository's own versus substitutable from upstream. Both are cheap and neither is guessed here.
