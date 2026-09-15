---
name: crate-map
description: The rules a crate is subject to by its prefix, why a data-system driver is a dev-dependency, why a networked adapter must hide behind a default-off feature, and what a release actually publishes. Open before adding or changing a crate, a feature, or the shipped set.
---

# What a crate may be

`ls crates/` gives the names and `cargo tree` gives the edges. **The prefix is the role, and the
convention is the contract rather than the count** - crates may be merged later, and a rule written
against the prefix survives that where a table of names would not.

**`sutura-tls` carries no `-domain`/`-exec-`/`-catalog-`/`-http`/`-config`/`-runtime`/`-cli`/`-serve`
prefix, and that is the point rather than an omission.** It holds exactly the bundle-or-system-store
read and the client-identity read a TLS source channel needs (`load_anchors`, `load_identity`), with
no dependency on a crypto provider, a network client, or `sutura-config` - not an adapter (it opens no
data system and speaks no wire protocol), not a transport, not settings. It exists so two adapters in
the SAME forbidden-edge class (`sutura-exec-postgres`, and a `ureq`-based outbound adapter under
`github.com/telekom/sutura#125`) can share one read without one becoming the other's library, which
`xtask/src/boundaries.rs`'s "data systems" class already forbids directly. A crate in this shape - a
small read or computation two same-class adapters both need, with no crypto provider, no network
client and no `sutura-config` - joins no existing prefix's rules and starts in no forbidden class by
construction; `sutura-sql` is the precedent for a shared, adapter-facing library with its own prefix,
and this is the same shape one size smaller.

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
  **Since issue #202's `datahub` composition, this is measured rather than only argued: a
  heterogeneous CATALOG set - one deployment serving both a markdown and a datahub catalog at once -
  is an architecture decision NOT taken there.** `Surface::start_composed` takes one
  `C: SemanticCatalog` per call and `SemanticCatalog::KIND`/`capabilities()` are per-TYPE associated
  items with no instance to dispatch on, which is why no enum can wrap both the way `OpenedSources`
  wraps two `Warehouse` adapters (reached through the SAME trait's *instance* methods instead).
  `sutura-serve`'s `catalog::OpenedCatalogs` is wave one's answer: one catalog kind per deployment,
  a mixed declaration refused by name. `docs/adr/0016`'s same-day addendum carries the identical
  sentence.
- **A transport is transport-only.** It never reads a catalog directory and never opens a data
  system; a composition root does both. `sutura-mcp` carries no NORMAL dependency on `sutura-http` -
  a dev-dependency exists, for one differential test, and is exempt from that rule the same way
  `sutura-exec-bigquery` dev-depending on `sutura-exec-datafusion` is. **That cuts both ways:**
  `sutura-http`'s default-off `agent` feature (which enables the `/mcp` mount, code and no
  dependency) carries the MCP transport OPAQUELY in a `ServiceState::AgentMount` boxed service, so
  `sutura-http` never names a `sutura-mcp` type - two transports pair at the composition root
  (`sutura-serve`'s `agent` feature links both), never at a transport crate.
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

**A SERVED Postgres source is the other half of that, and it does not contradict it.** Since
`telekom/sutura#124`/`#125` landed as one change, `sutura-exec-postgres` is also an optional,
default-off `postgres` dependency of both composition roots: the corpus path reaches it as a
dev-dependency, and a deployment that writes `kind: postgres` pays the link only when it asks for
the feature.

**Nothing a release publishes links it, and `checks.shipped-features` holds that only by PROXY.**
That gate's `forbidden` list is `ring` and `ureq`; it never names `sutura-exec-postgres`. What makes
the ban reach this adapter is that `rustls` is a **non-optional** dependency of it and the workspace
pins rustls to the `ring` provider - `sutura-exec-postgres` -> `rustls` -> `ring`, readable in
`Cargo.lock` and in neither edge optional - so a published binary linking the adapter would carry
`ring` in its `cargo auditable` section and the ban would fire. **The limit, and it is the whole
reason to cite the mechanism rather than the sentence:** the day that rustls dependency goes behind
a feature, or the provider pin moves off `ring`, the gate stays green over a published binary that
links the adapter, and nothing says so. A direct assertion would have to name the adapter in that
list.

## Why a networked adapter hides behind a default-off feature

**Not to save build time - that reason was measured and retracted.** `crane.buildDepsOnly` is
deliberately unscoped so the checks can share one dependency derivation, which means each cross
build compiles the whole workspace's dependency *closure* for its target. It also builds
DEV-dependencies, and `sutura-catalog-datahub` has a non-optional `ureq` one - so the outbound TLS
closure is in all four `sutura-deps-<triple>` derivations at cargo's default set already, and
`cargo tree` on 2026-09-04 shows that dev-dependency as the only edge into `ureq` for a musl target.
Making a networked adapter non-optional would add nothing there. **Since #202's HTTP reader,
`sutura-catalog-datahub` also has a SECOND `ureq` entry**, behind its own default-off `http` feature -
the real `AspectReader`'s. It is the `sutura-exec-bigquery`/`wire` shape exactly (optional,
feature-gated, off at the default set), so it does not change this paragraph's conclusion: the
dev-dependency was already pulling the same closure in unconditionally, and a feature nobody
requested still adds nothing to the default `sutura-deps-<triple>` derivations.

**The reason is the ARTEFACT.** No published binary links an outbound TLS stack, and
`checks.shipped-features` asserts it out of each binary's own embedded dependency list rather than
out of a manifest. Cite it that way.

**`sutura-mcp`'s `http` feature is the same shape but for the opposite reason.** Its
`transport-streamable-http-server` closure pulls no HTTP client (`server-side-http` names neither
`reqwest` nor `oauth2`) - it is NOT the native/outbound-TLS rule above. It is default-off because an
in-process server LISTENER is a bigger surface than a background TLS client: the only new package it
resolves against the lock is `sse-stream`, and it is off so a plain `cargo build`/`just check` never
resolves even that. The rule for the reader is the same - a default-off `http` feature on a transport
crate is registered here, not just in the manifest - and `docs/adr/0023` and the `#378` decision carry
the decision. It is not in `nix/shipped.nix`'s probes, because no published binary's feature list
names it; the `--all-features` gates compile, lint and test it on every run.
**A fake for a networked adapter's own tests lives in `src/`, `pub`, behind the SAME feature as the
reader it fakes - not in `tests/`.** `sutura-catalog-datahub::test_support` (PR2 of #202) is the
precedent: `sutura-serve`'s served-binary suite needed the identical loopback `DataHub` fake
`tests/http_reader.rs` already built, and an integration test binary cannot see another crate's
`tests/` directory at all - Rust does not expose one, only the library does. `#[cfg(feature =
"http")]` rather than `#[cfg(test)]`, because a downstream crate's OWN test compilation is what has
to see it, and `#[cfg(test)]` never crosses a dependency edge. **The cost is stated, not hidden:**
this ships the fake's object code (never called) inside any NON-test `--features http` build too,
including a shipped `sutura-serve --features datahub` binary - `std::net::TcpListener` adds no new
DEPENDENCY edge, so this does not reopen the paragraph above; it trades a few kilobytes of dead code
against a second hand-maintained fake. A dedicated `test-support`-only feature is the follow-up if
that trade stops being worth it.

**The general rule:** an adapter with a native or outbound-TLS dependency arrives behind a
default-off feature on whichever composition root wants it, and the four `cross` CI jobs are the
gate that says whether that was necessary. Every other gate passes `--all-features` and `deny.toml`
sets `all-features = true`, so the registration is still compiled, linted and tested on every run -
and `just gates` adds a DEFAULT-feature lane, because a `#[cfg(feature = ..)]` compiled only with
the feature on is the shipped set's blind spot. **That lane's scope is the shipped set and no
wider** - its package list is derived from the `binaries` list below, so a feature on a crate that
does not ship is reached by the `--all-features` gates and by nothing at the default set - and its
CI half is whole now, as two steps of the required `ci` job: `nix run .#default-features` for the
compile and lint halves, `nix run .#default-feature-tests` for the tests. Neither LINKS, so the four
`cross` builds are still the only thing that links the default set, on a release's own triples.

**What makes that rule a gate rather than a wish** is `probeFeatures` in `nix/shipped.nix`: a
feature named there gets a `<bin>-<feature>-<triple>-ci` package per release triple, and the `cross`
jobs build them beside the shipped set, so the documented feature-on build is LINKED on every pull
request rather than argued about. Until it existed the only evidence was a native `cargo check`,
which stops at metadata and therefore says nothing about the musl link that is the whole risk.
**What it does not cover:** it links and never runs, and it probes only the features a binary
declares - `sutura-serve`'s `tls`, `bigquery` and `postgres` are the same shape and are deliberately
unprobed, because the closure is compiled per target and a probe for each would multiply the job.

**Running it corrected the paragraph above, and default-off is a decision about the ARTEFACT and
not about build time** - cite it that way, held by `checks.shipped-features`. `docs/adr/0017` carries
the numbers; the transferable part is that `--features bigquery` adds exactly **12 compiled units**
- the adapter plus the outbound TLS closure, the same twelve on all four published triples - and
that they cost under 2% of a `cross` job (CI runs 33808343712 and 33838360913, 2026-09-04; a figure
with no run beside it cannot be re-taken). **Do not read the step's own elapsed seconds as that
number:** it prints a whole second crate derivation, 62-85s over three runs, and the step said
*the price of the feature ON* for one run before somebody read it. Two more traps in reading it:

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
