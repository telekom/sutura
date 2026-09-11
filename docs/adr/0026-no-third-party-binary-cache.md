---
title: No third-party binary cache
description: Why the secret-gated Cachix wiring in ci.yml and cross-link.yml was deleted rather than provisioned, what the GitHub Actions cache does and does not carry in its place, what the deleted step was measured to be doing (nothing, silently, on every main push), and which gate now refuses its return.
---

# No third-party binary cache

Status: **accepted**, 2026-09-09. Supersedes the binary-cache half of `#472`'s Proposal 2 and the
wiring `#477` added for it. `docs/adr/0025` is the neighbouring case: a control this repository
publishes has to be held by a mechanism, or it is an overstated control.

## The decision

**The GitHub Actions cache is the only store this repository carries between runs.** No hosted or
third-party Nix binary cache is configured, and none is trusted as a substituter. `#472` asked for
one; the maintainer ruled against it, and this record is that ruling plus the measurements that
make it the cheap answer rather than the resigned one.

## What was there, and what it was measured to do

`ci.yml` and `cross-link.yml` each carried three pieces of wiring:

| piece | gated on |
| --- | --- |
| two `substituters` / `trusted-public-keys` lines in `install-nix-action`'s `extra_nix_config` | `secrets.NIX_CACHE_SUBSTITUTER`, `secrets.NIX_CACHE_PUBLIC_KEY` |
| a `hosted:` input to `.github/actions/nix-store-cache`, feeding one summary row | `vars.NIX_CACHE_NAME` |
| a `Populate the binary cache` step (`cachix/cachix-action`) | `github.event_name == 'push' && github.ref == 'refs/heads/main' && vars.NIX_CACHE_NAME != ''` |

**None of those four names has ever existed in this repository.** Read on 2026-09-08: the repository
declares no Actions variables at all (`total_count: 0`). So the two `extra_nix_config` lines
interpolated to the empty string on every run, the summary row said `UNPROVISIONED`, and the populate
step was `completed/skipped` on every `main` push it ever ran on - `run 34247863640`, step 6, is the
one that was read line by line.

That is the defect, and it is not the skip. **Staying inert while unprovisioned was deliberate and
safe.** The defect is that a reader saw a green `main` and a shipped feature while the mechanism did
nothing, which is the same class as `#476`'s witness that reddened when publication *worked*: a check
that is blind is worse than a check that is absent, because it looks like coverage.

## Why deleting it beats provisioning it

1. **The premise it was filed on is refuted.** `#472` argued the cache was cold and ref-scoped, so
   PRs recompiled the closure. Measured instead: a **~94-95% path hit rate**, and on a branch that
   moves no dependency input the first `ci` run restored **742** store paths against **37** built,
   the second **654** against **39**. `vendor-cargo-deps` and `vendor-registry` were rebuilt in
   neither. `main`'s Actions-cache entries DO serve pull requests. A second carrier would have been
   added on top of a carrier that already works.
2. **The two supporting hypotheses are refuted too.** The 18,688-entry drop was `cache-prune.yml`
   doing its job on `pull_request_target: closed`, not eviction; `#472` records the allocation as
   100 GB, so headroom is not the constraint. And crane's deps derivation is hash-stable while the
   lockfiles are: over 30 commits of `main`, `Cargo.lock` moved once and `flake.lock` not at all.
3. **It cannot be justified on wall clock.** Run-to-run duration here varies **64%** (1351 s against
   825 s, same branch, same dependency state), so no duration claim below that spread is
   measurable - and nothing measured here exceeds it. The deterministic oracle is counting
   `Compiling sutura-<crate>` lines in a build log, and `#486` is the issue that owns that number.
4. **It is a supply-chain surface for a benefit nobody has measured.** A substituter plus a trusted
   key means CI fetches store paths signed by a key held outside this repository. `ci.yml`
   deliberately withholds the flag that would let `flake.nix` add its own substituter, precisely so
   a pull request cannot point CI at a store it controls; adding a permanent third-party one back
   spends part of that guarantee. Against an unmeasured gain, on top of a working cache, that trade
   is refused.
5. **Code cannot fix a missing secret.** Provisioning was an owner task on `#472`'s ask list and is
   now retired. Leaving the wiring in place while it stays unprovisioned means keeping a mechanism
   whose only observable behaviour is a skip.

## What replaces it: nothing new

`.github/actions/nix-store-cache` restores `/nix/store` on **every** event and saves it **only** from
a push to `main` (`#490`, `#487`). That asymmetry is pinned token for token by
`xtask::workflows::cache_scope`, and the run reports what it carried rather than asserting it worked.
`cache-prune.yml` is unchanged and stays: with pull requests no longer writing, the entries it spares
are exactly what a pull request restores from, and there is no second carrier standing behind them.

## What this record does NOT say

* **Not that a binary cache would not help a cold run.** A genuinely cold run - a moved
  `flake.lock` or `Cargo.lock` - still compiles a dependency closure that is one of the largest in
  the ecosystem, and that is what `ci.yml`'s job timeout exists for. A hosted cache is a
  real answer to that case. It is refused here because the case is rare, the gain is unmeasured
  against a 64% spread, and it costs a trusted third-party key.
* **Not that the Actions cache is fast.** `#490` is explicit that it buys hygiene and legibility.
  The measured hit rate says the closure is reused; it says nothing about seconds.
* **Not that the entry always fits.** GitHub refuses a single cache entry over 10 GB and this store
  has never been weighed. If a save is refused, the summary line says so and the next run compiles.
  There is no fallback carrier any more, and that is the cost of this decision, stated here rather
  than left for a reader to discover.
* **Not a statement about the repository's cache allocation.** `orgs/{org}/actions/cache/usage`
  answers 403 without `admin:org`, so the allocation and its headroom are not re-readable by a
  contributor. The 100 GB figure is `#472`'s measurement.

## The mechanism that holds it

`cargo xtask check-workflows` refuses, over the whole `.github` tree:

* any step naming a hosted binary-cache publisher (`cachix/cachix-action`,
  `DeterminateSystems/flakehub-cache-action`) while this record stands;
* any step in ordinary CI whose `if:` or `save:` reads `secrets.` or `vars.` - repository state the
  workflow cannot observe, so the step's own skip is invisible. `secrets` is not even available to an
  `if:`, and a `vars` test in one is the exact shape that was silently green here;
* this record being missing or empty, for `sast.rs`'s reason: a refusal enforcing a decision nobody
  wrote down is a rule with no reason.

**Adding a binary cache later is a good change, and it makes this record wrong the moment it lands.**
So the gate refuses the combination rather than the tool, and its message names this file to edit.
