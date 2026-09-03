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

**A release derivation's `--package` is not enough on its own.** `crane.buildDepsOnly` is
deliberately unscoped so the checks can share one dependency derivation - which means each cross
build compiles the whole workspace's dependency *closure* for its target even though it builds one
binary out of it. With a networked adapter non-optional, its TLS stack cross-compiles for four
targets, two of them musl, for a binary that links none of it.

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
jobs build them beside the shipped set - so the price of the feature ON is a job time on every pull
request rather than an argument. Until it existed the only evidence was a native `cargo check`,
which stops at metadata and therefore says nothing about the musl link that is the whole risk.
**What it does not cover:** it links and never runs, and it probes only the features a binary
declares - `sutura-serve`'s `tls` and `bigquery` are the same shape and are deliberately unprobed,
because the closure is compiled per target and three probes would triple the job.

**And the first run of it corrected the paragraph above.** Measured 2026-09-03 over the four `cross`
jobs of run 33781193001, `sutura` alone with deps already in the store: 70.5→71.0s
(x86_64-gnu), 69.2→69.1s (aarch64-gnu), 83.0→84.2s (x86_64-musl), 77.3→78.6s (aarch64-musl) - a
delta of **at most +1.7%**, and all four linked. Repeated on run 33792642655, because one timing
sample is not a measurement: -0.4% to +1.6%. The two runs agree that the delta is inside the noise,
which is the claim - not the individual seconds. The reason it is that small is the reason the
build-cost argument for default-off does not hold: `buildDepsOnly` is called on the UNSCOPED
argument set so the checks can share one dependency derivation, so the deps build resolves the whole
workspace at cargo's default features, and `sutura-exec-bigquery` takes `ureq` non-optionally -
`ring`, `rustls` and `ureq` are therefore compiled inside `sutura-deps-<triple>` for all four
triples **with the feature off**. A feature gate on a composition root cannot subtract a closure the
deps derivation already pulled. **So default-off is a decision about what the ARTEFACT links, held
by `checks.shipped-features`, and not a decision about build time** - cite it that way. What is
still unmeasured is binary size, which no step prints.

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
