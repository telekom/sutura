---
name: crate-map
description: The rules a crate is subject to by its prefix, why a data-system driver is a dev-dependency, why a networked adapter must hide behind a default-off feature, and what a release actually publishes. Open before adding or changing a crate, a feature, or the shipped set.
---

# What a crate may be

`ls crates/` gives the names and `cargo tree` gives the edges. **The prefix is the role, and the
convention is the contract rather than the count** - crates may be merged later, and a rule written
against the prefix survives that where a table of names would not.

Rules that are not visible from a manifest:

- **`sutura-domain`'s dependency list is an allowlist walked over the whole resolve graph**, so a
  macro crate arriving transitively is an architecture decision, not a convenience. `secrecy` is the
  only entry taken for a COMPILE ERROR rather than for a value the domain computes
  (`docs/adr/0020`), and it brought two names nothing compiles - the gate walks the resolve graph
  rather than feature resolution, so it names them anyway.
- **`sutura-semantic` may not reach the renderer OR the dialect layer.** Two forbidden edges, not
  one: the second is what stops the first returning transitively.
- **`sutura-app` owns the driving port**, because `Surface`'s operations *are* the tool set and the
  two transports cannot see each other. Its `Warehouses` registry is generic in one adapter type, so
  a heterogeneous set - or a catalog naming two KINDS of source - is an architecture decision.
- **A transport is transport-only.** It never reads a catalog directory and never opens a data
  system; a composition root does both. `sutura-mcp` depends on nothing in `sutura-http`.
- **`sutura-cli` reads the same `sources:` tree `sutura-serve` does**, and dispatches the declared
  `SourceKind` through an exhaustive match of its own - so a third kind is a compile error in both
  composition roots. The two differ in what an ABSENT entry means: a startup refusal there, and a
  fallback to that binary's own built-in `files` declaration - named `local`, over the directory on
  the command line - here. It still answers one question against one data system, so a catalog
  spanning two gets no engine where `sutura-serve` serves both.
- **A *declaring* catalog adapter is measured against its own `capabilities`, a *golden* one against
  the oracle** (`docs/adr/0016`). `agrees_with_the_oracle` is the golden contract and is not weakened
  for anything; the split is in the type system, so a golden-only cell cannot be expanded for a
  narrow adapter at all.

## Why a driver is a dev-dependency

`sutura-cli` links the engine only, and **that is what keeps the musl artifacts building** -
nixpkgs has no musl `libduckdb`. `nix/duckdb.nix` is the single path from nixpkgs to that library,
imported by `flake.nix` and `devenv.nix` alike so a pin cannot differ between the shell and CI.

Postgres and DuckDB adapters are dev-dependencies for the same reason, and their cells are
fail-closed: the Postgres tier is provisioned by `checks.nextest` **and** by `just test` from one
script, so the two cannot drift.

## Why a networked adapter hides behind a default-off feature

**Not to save build time - that reason was measured and retracted.** `crane.buildDepsOnly` is
deliberately unscoped so the checks can share one dependency derivation, which means each cross
build compiles the whole workspace's dependency *closure* for its target. It also builds
DEV-dependencies, and `sutura-catalog-datahub` has a non-optional `ureq` one - so the outbound TLS
closure is in all four `sutura-deps-<triple>` derivations at cargo's default set already, and
`cargo tree` on 2026-09-04 shows that dev-dependency as the only edge into `ureq` for a musl target.
Making a networked adapter non-optional would add nothing there.

**The reason is the ARTEFACT.** No published binary links an outbound TLS stack, and
`checks.shipped-features` asserts it out of each binary's own embedded dependency list rather than
out of a manifest. Cite it that way.

**The general rule:** an adapter with a native or outbound-TLS dependency arrives behind a
default-off feature on whichever composition root wants it, and the four `cross` CI jobs are the
gate that says whether that was necessary. Every other gate passes `--all-features` and `deny.toml`
sets `all-features = true`, so the registration is still compiled, linted and tested on every run -
and `just gates` adds a DEFAULT-feature lane, because a `#[cfg(feature = ..)]` compiled only with
the feature on is the shipped set's blind spot. **That lane's scope is the shipped set and no
wider** - its package list is derived from the `binaries` list below, so a feature on a crate that
does not ship is reached by the `--all-features` gates and by nothing at the default set - and it is
a developer lane: CI has the four `cross` builds for the compile half and nothing for the lint half.

**What makes that rule a gate rather than a wish** is `probeFeatures` in `nix/shipped.nix`: a
feature named there gets a `<bin>-<feature>-<triple>-ci` package per release triple, and the `cross`
jobs build them beside the shipped set, so the documented feature-on build is LINKED on every pull
request rather than argued about. Until it existed the only evidence was a native `cargo check`,
which stops at metadata and therefore says nothing about the musl link that is the whole risk.
**What it does not cover:** it links and never runs, and it probes only the features a binary
declares - `sutura-serve`'s `tls` and `bigquery` are the same shape and are deliberately unprobed,
because the closure is compiled per target and three probes would triple the job.

**Running it corrected the paragraph above, and default-off is a decision about the ARTEFACT and
not about build time** - cite it that way, held by `checks.shipped-features`. `docs/adr/0017` carries
the numbers; the transferable part is that `--features bigquery` adds exactly **12 compiled units**
- the adapter plus the outbound TLS closure, the same twelve on all four published triples - and
that they cost under 2% of a `cross` job (CI runs 33808343712 and 33838360913, 2026-09-04; a figure
with no run beside it cannot be re-taken). **Do not read the step's own elapsed seconds as that
number:** it prints a whole second crate derivation, 69-85s, and the step said *the price of the
feature ON* for one run before somebody read it. Two more traps in reading it:

| Trap | What is actually true |
| --- | --- |
| *The deps derivation already built `ring`, so the probe reuses it* | It does not. `buildDepsOnly` runs unscoped and the probe asks for one package, so the resolver gives a narrower feature set, a different `-C metadata` and a **recompile**. The probe's log says `sutura> Compiling ring`. |
| *So the feature is nearly free* | It is nearly free **in this graph**, because those units finish inside the slack ahead of `datafusion` on the critical path. A shorter critical path would expose them as time. |
| *The adapter is why the closure is in the deps derivation* | It is not - its `ureq` is `optional` behind `wire`. A **dev-dependency** of `sutura-catalog-datahub` is, and a build-dependency of `libduckdb-sys` puts a host-side copy there too. Drop that dev-dependency and the musl cost comes back. |
| *The probe links what the tutorial tells a reader to build* | The same package and features, at the **`ci` profile**. The page says `--release`, whose thin LTO and `panic = "abort"` are a different link, and nothing measures that one. |

Binary size remains unmeasured; no step prints it.

A feature-gated adapter's absence is a **startup refusal naming the feature**, never a silent
degradation - and `sutura_config` cannot see a link, so which adapters a BUILD contains is not in
the settings vocabulary.

## What a release publishes

`nix/shipped.nix`'s `binaries` list, built with cargo's **default** features - so no TLS and no
networked adapter in any published artefact.

Three gates hold it, answering three different questions. Worth reading as a set, because two were
added after a review found the first could be believed to cover more than it does:

- `check-shipped-binaries` compares that list against **every literal that spells it again** in the
  workflows and both build actions. The release path *cannot* derive the set - a `strategy.matrix`
  takes literals and a job cannot evaluate a flake before installing nix - and `release.yml`'s own
  arrival check reads like the mechanism and is not: both sides of its count come from the same
  literal, so a binary added to `nix/shipped.nix` and nowhere else leaves it green. **Its limit:** it
  reads the release path only. The `justfile`'s build and image recipes spell the same names and are
  held by review, on the argument that a subset there costs a developer a surprise rather than a
  release a binary.
- `checks.one-binary` reads each shipped package's `bin/` and runtime closure: one executable,
  **named what the image entrypoint expects**, no toolchain baked in. The name half is the cheap
  guard on a seam nothing else relates - `cargoExtraArgs` names a cargo package, the image names a
  path.
- `checks.shipped-features` reads the `cargo auditable` section **out of the binary itself**, so it
  is an assertion about the artefact rather than about a manifest. This is what keeps "the feature is
  off in everything published" from drifting into a sentence nobody checks.

## The inner loop's scope

`just check` compiles the domain crate alone, and **it prints what it covered** - cargo's
`Finished` line says nothing about scope, and a green run was once read as a green tree while a
branch whose settings crate did not compile got pushed on the strength of it. `check-scope` keeps the
printed scope equal to the `-p` flags above it, so the notice cannot drift into a lie the way a
comment would. What no gate can do is know what a developer *believed* a task covered; the honest
output is the fix for that half, and `just check-changed` is the cheap answer to "does what I
touched compile".
