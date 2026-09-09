# Why this file exists, since it is a fair question
# ------------------------------------------------
# It does one job that nothing else in the repo can: **it makes the build a function of
# pinned inputs instead of a function of the machine.**
#
# Concretely, three things depend on that:
#
#   1. Reproducible release artifacts. `nix build .#oci` yields the same image digest from
#      the same source, so "which build is running in production?" is answerable. A
#      Dockerfile gives you the same *recipe*, not the same *result* - `apt-get install`
#      resolves differently next Tuesday.
#   2. One toolchain definition, not two. The dev shell and the release build both read
#      `rust-toolchain.toml`. With a Dockerfile alongside a devenv you have two places to
#      bump a compiler and one of them will be forgotten.
#   3. Cross-compilation without a cross-toolchain per developer. `nix build
#      .#sutura-aarch64-unknown-linux-gnu` works from an x86_64 host with no local setup,
#      which is what makes shipping both Linux architectures cheap rather than a project.
#
# What it does NOT do: it is not the development environment (that is devenv.nix, which
# this flake also exposes), and it is not required to hack on the code - plain `cargo
# build` works fine. It is required to *ship*.
#
# Cranelift: deliberately absent here. It is a development-only codegen backend (see
# Cargo.toml); CI and every shipped artifact use the default backend, because a bug that
# reproduces under one backend and not the other is a genuinely bad afternoon.
{
  description = "sutura - an identity-aware semantic data runtime for AI agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    jscpd-src.url = "github:kucherenko/jscpd/v5.2.0";
    jscpd-src.flake = false;
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay, jscpd-src, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # The compiler pin is rust-toolchain.toml and nowhere else. Read as data so this
        # file cannot disagree with the dev shell or with a bare rustup fallback.
        rustToolchainFile = ./rust-toolchain.toml;

        # The source root as a string, so the filter below can match a REPO-RELATIVE path.
        # `./.` is the flake source, and in every build that is a store path - which is the
        # whole reason the filter cannot match on the absolute one. See the filter's header.
        srcRoot = toString ./.;

        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          # Keep the toolchain file: crane's source filter drops non-Cargo files, and
          # without it the pin is invisible to the build.
          #
          # Keep `vendor/` WHOLESALE, and this is load-bearing rather than tidy: the vendored
          # allocator is a path dependency, so cargo has to read its manifests to resolve the
          # workspace at all, and crane's filter is written for first-party Rust and drops
          # both them and the C. Without this the build fails in `cargo check` with
          # "failed to read vendor/mimalloc_rust/Cargo.toml".
          #
          # Keep `crates/*/tests/**` WHOLESALE, and this one is the trap. crane keeps Cargo
          # inputs, which means `.rs`, `Cargo.toml` and `Cargo.lock` - so the golden suites'
          # committed `.snap` snapshots are dropped. insta reads those at RUN TIME, and
          # `checks.nextest` sets `INSTA_UPDATE = "no"` precisely so a missing snapshot FAILS
          # rather than being written and passed - so the symptom is every snapshot in the suite
          # reported absent at once, which reads as a catastrophe and is one deleted clause.
          # `just test` in the dev shell reads the real tree and would not notice, so `just ci`
          # is the only thing that catches it.
          #
          # **The corpus moved out from under this clause and the clause still carries the
          # snapshots.** The golden suite used to hold its own e-commerce catalog under
          # `crates/sutura-app/tests/fixtures`; it now reads `examples/single-player`, the same
          # directory the CLI example test reads. So the `examples/` clause below is load-bearing
          # for TWO test targets rather than one, and this clause is load-bearing for the
          # snapshots of both. Deleting either because "the fixtures moved" is exactly the
          # CI-only failure this block exists to warn about.
          #
          # THE RULE, because this filter has now bitten three times and each clause below is one
          # of them: **any directory a build or a test READS has to be named here.** crane keeps
          # Cargo inputs only, so everything else is absent from the sandbox while being present
          # in the dev shell - which makes this the one bug class local gates cannot see. The
          # three: the golden suites' snapshots under `crates/*/tests`, `defaults.yaml` under
          # `crates/*/src`, and `examples/` - the corpus BOTH `sutura-cli`'s example test and
          # `sutura-app`'s golden suite read. Adding a data directory means adding a clause, and
          # `just validate` is what proves it.
          #
          # Keep non-Rust files under `crates/*/src/**` for the same reason, one layer in, and
          # this one bit for real: `sutura-config` holds its defaults as `defaults.yaml` beside
          # the code and reads them with `include_str!`. crane dropped the file, so CI failed
          # with `couldn't read crates/sutura-config/src/defaults.yaml` while every local build
          # passed - `include_str!` resolves against the real tree in the dev shell and against
          # the FILTERED copy in a nix build. A data file next to the code that reads it is a
          # normal thing to write, so the filter has to expect it rather than the author having
          # to remember this. `.rs` still goes through crane's own filter below.
          #
          # EVERY ARM MATCHES THE REPO-RELATIVE PATH, and that is not style. Matching the
          # ABSOLUTE path is what made this filter inert for its whole life: a nix source root
          # IS `/nix/store/<hash>-source`, so `.*/nix(/.*)?$` - written for our own `nix/`
          # directory - matched EVERY path in the tree, `.*` absorbing the store prefix and
          # `/nix` landing on the store's own segment. The `||` chain then short-circuited to
          # true for everything, so the filter dropped NOTHING and every check depended on the
          # entire tree. Verified both ways, and the second half is why nobody caught it:
          # `builtins.match ".*/nix(/.*)?$" "/nix/store/deadbeef-source/justfile"` matches,
          # while the same regex against `/home/x/sutura/justfile` does not - from a working-tree
          # root it behaves exactly as intended, so no local experiment can show the bug.
          #
          # `builtins.match` is a WHOLE-STRING match. `.*/` was glue for "somewhere in the
          # path", and deleting the glue is the fix - not adding `^`, which was never missing.
          # `$` goes for the same reason: it read as an anchor and was never doing anything.
          #
          # Only the `nix` arm actually collided, checked one arm at a time against
          # `/nix/store/<hash>-source/justfile`, because `nix` is the only arm whose name is
          # also a segment of the store prefix - `store` would be the next. Anchoring all seven
          # is free (the kept file set is byte-identical to fixing that one arm alone, measured
          # over this tree) and puts the collision out of reach of the next arm somebody adds.
          #
          # `\\.` and not `\.`: inside a Nix `"…"` string `\.` is just `.`, so those two regexes
          # were spelling a wildcard while reading as a literal dot. Inert here - no sibling
          # file collides - and wrong the moment one does.
          #
          # WHAT THIS FILTER ACTUALLY REACHES, because the comment below used to overstate it in
          # both directions. What reads the filtered copy: `ciArtifacts`, the native and cross
          # release packages, `packages.xtask`, `checks.clippy`, `checks.doctest` and
          # `checks.fmt`. What does not: `checks.nextest`, `checks.hygiene`, `checks.crap` and
          # `checks.api-docs` each take `wholeTree` and read the whole tree, so this filter never
          # protected them and a prose edit re-runs all four by design.
          #
          # It does NOT reach the dependency closure, which is the expensive half. crane builds
          # `sutura-deps` from a DUMMIFIED source it synthesises out of the manifests, so that
          # derivation is byte-identical either side of this fix - checked with
          # `nix-store -q --references` on each consumer, one `sutura-deps` before and the same
          # one after. What a prose edit used to cost was every filtered-src check's own compile
          # of our crates (clippy's is ~40 s warm here) plus the four unfiltered ones.
          filter = path: type:
            let rel = pkgs.lib.removePrefix (srcRoot + "/") (toString path); in
            (builtins.match "rust-toolchain\\.toml" rel != null)
            || (builtins.match "vendor(/.*)?" rel != null)
            || (builtins.match "crates/[^/]+/tests(/.*)?" rel != null)
            || (builtins.match "crates/[^/]+/src(/.*)?" rel != null)
            || (builtins.match "examples(/.*)?" rel != null)
            # `xtask` is a repo-inspection tool, so its tests read repo files by design - and it
            # is `checks.nextest` that runs them, on `wholeTree`, so this arm is not what carries
            # them. It is here for `nix/*.nix` itself. `docs/` stays out by the file: it holds the
            # generated API pages and churns, and matching all of it would put every prose edit in
            # the derivation hash of each filtered-src check - see the blast radius above, which is
            # narrower than this comment used to claim, and which was zero until the arm below was
            # anchored.
            || (builtins.match "nix(/.*)?" rel != null)
            || (builtins.match "docs/crap\\.md" rel != null)
            || (craneLibFor system).filterCargoSources path type;
        };

        # The WHOLE tree, for the four checks that judge every file rather than the Rust ones -
        # `nextest`, `hygiene`, `crap` and `api-docs`. Each used to write `src = ./.` for itself.
        #
        # **It is a binding rather than four literals because of the NAME, and the name is
        # load-bearing.** `src` above is `lib.cleanSourceWith` with no `name`, which defaults to
        # `source`, so every filtered-src derivation unpacks into `/build/source`. A bare `./.` is
        # the flake source, whose store path already carries its own hash in the name, so those
        # derivations unpacked into `/build/<hash>-source` instead - a DIFFERENT absolute path for
        # the same tree.
        #
        # That mattered because a build script may bake an absolute path into generated code, and
        # one here does - `utoipa-swagger-ui`, whose generated `rust-embed` `#[folder]` names its
        # own `$OUT_DIR`, which arrives in these checks by decompressing `sutura-deps`. Under one
        # source-root name a linux check reuses that literal successfully; under two it did not,
        # and `checks.nextest` failed in a THIRD-PARTY crate while `checks.clippy` passed on the
        # same tree. `nix/purge-baked-out-dirs.sh` carries the whole failure and both build roots.
        #
        # `filter` is the trivial one, so nothing is dropped - the whole point of these four is
        # that nothing is. But **agreement is no longer the mechanism**: see the two limits.
        #
        # **TWO LIMITS, and the first is the one to read before believing this bought anything
        # else.** It is NOT what makes these four checks reuse `sutura-deps`: they decompress that
        # artifact and then compile the closure anyway - `tokio`, `ring`, `rustls`, `arrow`,
        # `parquet` - and the green run after this change still compiles 80 crates, now under
        # `/build/source`. Whatever discards the artifact is something else and is untouched here,
        # so the `nextest spent 57 minutes compiling` note further down is NOT explained by this.
        # What changed is only that a recompile of a crate with a baked path now succeeds.
        # **Second, and it is why the name stopped being load-bearing:** a darwin build directory
        # is `/nix/var/nix/builds/nix-<pid>-<random>/`, unique per derivation, so the roots cannot
        # be made to agree there AT ALL - and while agreement WAS the mechanism, `just validate`
        # exited 1 at `checks.aarch64-darwin.nextest` on an unmodified tree, before a single test
        # ran. That is #325's F9, and `inheritedArtifacts` below is what fixes it: whatever baked a
        # build root is regenerated rather than required. The `name` stays because it costs nothing
        # and changing it would rehash every check for no gain.
        wholeTree = pkgs.lib.cleanSourceWith {
          src = ./.;
          name = "source";
          filter = _path: _type: true;
        };

        # The same pin as a package, for the tools that need `cargo` on PATH rather than a
        # crane derivation around it.
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile rustToolchainFile;

        # The NIGHTLY pin as a PACKAGE, never a crane toolchain: the only read of it here, and only
        # ever for the `cargo rustdoc` child that emits the JSON. See `checks.api-docs` below.
        nightlyToolchain = (import ./nix/toolchains.nix { rustPkgs = pkgs; }).nightly;

        # The pinned cargo, for the one workflow that has to touch Cargo.lock.
        cargoWrapper = import ./nix/cargo-wrapper.nix { inherit pkgs rustToolchain; };

        # WRITES the committed API pages, and `checks.api-docs` below is the gate that fails when
        # they fall behind - the two must agree byte for byte, which is why one file defines the
        # writer and the check names it as the fix. In `nix/api-docs.nix` because this file was at
        # the 1000-line limit `cargo xtask max-lines` enforces; that module's header carries the
        # rest, including why the seam is here rather than at the checks.
        apiDocsWriter = import ./nix/api-docs.nix { inherit pkgs nightlyToolchain duckdb; };
        fuzzRunner = import ./nix/fuzz.nix { inherit pkgs nightlyToolchain; };


        craneLibFor = sys:
          (crane.mkLib pkgs).overrideToolchain
            (p: p.rust-bin.fromRustupToolchainFile rustToolchainFile);

        # Native build: what `nix build` and `nix flake check` use.
        craneLib = craneLibFor system;

        # The copy/paste detector (issue #474); body in nix/jscpd.nix.
        jscpd = import ./nix/jscpd.nix { inherit pkgs craneLib inheritedArtifacts; src = jscpd-src; };

        # The data system the local Warehouse adapter links against, resolved by the SAME file
        # devenv.nix imports so the dev shell and CI cannot link two different libduckdbs. It also
        # explains why the crate is built without its `bundled` feature, and why the run-time path
        # is a third variable rather than an afterthought.
        duckdb = import ./nix/duckdb.nix { inherit pkgs; };
        postgresTier = import ./nix/postgres-tier.nix { inherit pkgs; };

        # The identity provider's CI venue, on the same pattern and from the same one file the
        # `keycloak-tier` app runs, so the sandbox and a developer's shell cannot drift.
        keycloakTier = import ./nix/keycloak-tier.nix { inherit pkgs; };


        # The CRAP gate's two tools, from the SAME file devenv.nix imports so the dev shell and
        # CI cannot score with two different versions. See nix/crap.nix for which one comes from
        # nixpkgs, which is a hash-pinned prebuilt, and why.
        crap = import ./nix/crap.nix { inherit pkgs; };

        commonArgs = {
          inherit src;
          # Named explicitly: the root manifest is a virtual workspace with no [package],
          # so crane cannot infer these and would fall back to a placeholder - which shows
          # up as derivations called `cargo-package-*` and makes a build log say nothing
          # about what it built.
          pname = "sutura";
          version = "0.1.0";
          strictDeps = true;
          # .cargo/config.toml routes EVERY target through clang + lld, the two apple ones included, and the
          # Nix sandbox has neither unless we say so: linking differently here than in the dev shell is the drift.
          nativeBuildInputs = [ pkgs.clang pkgs.lld ];
          # `buildInputs` and not `nativeBuildInputs`: a library the built artifact links against,
          # not a tool that runs during the build, and `strictDeps = true` above makes the
          # distinction load-bearing rather than stylistic.
          #
          # Only the NATIVE args carry either. The cross builds below deliberately do not: nixpkgs
          # has no musl libduckdb, and `sutura-cli` keeps the adapter behind a default-off feature
          # so the musl artifacts never ask for one. `libiconv` is what `-liconv` resolves to on a
          # mac, where rustc emits it for every link and nix keeps it out of the SDK.
          buildInputs = [ duckdb.package ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.libiconv ];
        } // duckdb.env;

        # The shipped binary carries its own dependency list: `cargo auditable` adds one ELF
        # section naming the crates it was really built from, and `syft` reads it back out - so the
        # SBOM cannot drift from the artifact, because it is inside it. `nix/auditable.nix` carries
        # the argument for embedding rather than generating a sidecar, and the two correctness
        # notes: why the profile flag is computed rather than taken from crane's helper, and why
        # the tool goes on the final build and never on the args `buildDepsOnly` reads.
        auditable = import ./nix/auditable.nix { inherit pkgs; };

        # The licensing gate and the tool it runs, from one expression so `checks.reuse` and
        # `apps.reuse` cannot resolve to two versions. `src = ./.` and NOT the filtered source:
        # this gate judges every file, and crane's filter keeps only Cargo inputs - against it the
        # check would pass having read a fraction of the tree. `nix/reuse.nix` carries the rest,
        # including what the check cannot catch.
        licensing = import ./nix/reuse.nix { inherit pkgs; src = ./.; };

        # The two profiles we ship.
        #
        # `release` is the default and is cheap to build on purpose (thin LTO, 16 codegen
        # units). `release-performance` adds fat LTO and a single codegen unit: minutes
        # slower, for a binary worth shipping only once throughput has been measured. Both
        # are declared in Cargo.toml; this is where they become build targets.
        releaseArgs = commonArgs // { CARGO_PROFILE = "release"; };

        # Dependencies, compiled ONCE and reused by the build and by every check. This is
        # ONE dependency closure for everything CI does except ship a binary, at opt-level 0.
        #
        # Measured, on the run that first got there: `nextest` spent 57 minutes compiling to run
        # **1.567 seconds** of tests, because every test binary links DataFusion, Arrow and
        # DuckDB. At this profile the same check builds and runs in 1 min 5 s. Our own crates are
        # small; the minutes were all dependencies, which is exactly what opt-level 0 on the
        # closure addresses.
        #
        # Named in BOTH places, and that is half the fix. `clippy`, `nextest`, `doctest` and
        # `crap` were built from `commonArgs`, which sets no `CARGO_PROFILE`, while the artifacts
        # they inherited were built as `release`. Cargo stores artifacts per profile and the two
        # derivations are provably different - `pkb4gj15` against `v7whyzp6f` - so a check could
        # not reuse deps built under the other profile. `nix eval` now shows one drv hash across
        # every consumer.
        #
        # There is deliberately no second, smaller closure. An `xtask`-only one was tried: it
        # made `checks.hygiene` standalone-cheap but gave CI two dependency builds and two cache
        # entries for one dependency set, which is the opposite of what a shared cache is for.
        # At opt-level 0 the full closure is cheap enough that scoping it buys less than the
        # duplication costs.
        #
        # `release` stays for the shipped binary and the cross artifacts - the only place an
        # optimised build is worth paying for.
        ciArgs = commonArgs // { CARGO_PROFILE = "ci"; };
        ciArtifacts = craneLib.buildDepsOnly ciArgs;

        # ARTIFACTS BUILT IN ANOTHER DERIVATION, AND THE ONE THING THAT MAKES THEM SAFE TO INHERIT,
        # as a single attrset - so a consumer cannot take the artifacts without the regeneration.
        # A build script in this closure bakes the absolute `$OUT_DIR` it ran in into the code it
        # generates, and a darwin build directory is per-derivation, so the inherited literal names
        # nothing. `nix/purge-baked-out-dirs.sh` carries the measured failure - `just validate` red
        # at `checks.nextest` on an unmodified tree - the detector, its cost and what it misses; it
        # is a tracked `.sh` rather than a string here so `just lint-workflows` shellchecks it.
        # NOT folded into `commonArgs`: `buildDepsOnly` inherits nothing, so the sweep would be a
        # no-op in the producer while moving `sutura-deps`' hash and rebuilding ~80 crates for it.
        #
        # HELD BY `cargo xtask check-warm-start` RATHER THAN BY THE SHAPE OF THIS BINDING, which is
        # #336: one attrset makes the pairing hard to separate by accident and holds nothing at all
        # against `//`, which updates one level deep - a consumer binding `preBuild` after this one
        # loses the sweep with no error and nothing red. That gate reads every binding of
        # `cargoArtifacts` in every nix file, attributes each to whatever receives it, and refuses a
        # second `preBuild` anywhere: a phase this file does not own is exactly the shape that
        # displaces this one. A `preBuild` needed for another reason therefore composes HERE, where
        # it runs beside the sweep instead of in place of it.
        inheritedArtifacts = artifacts: {
          cargoArtifacts = artifacts;
          preBuild = builtins.readFile ./nix/purge-baked-out-dirs.sh;
        };

        # What a BARE cargo needs to build this workspace (the linker, the libraries an app
        # inherits from nothing, and the warm start that reuses the dependency closure). In
        # `nix/cargo-env.nix` because this file was at the 1000-line limit; that module's header
        # carries the reasoning. `cargoVendorDir` is crane's vendor dir for `ciArgs`, so an app
        # resolves out of the SAME registry the artifacts were built against.
        inherit (import ./nix/cargo-env.nix {
          inherit pkgs duckdb;
          cargoArtifacts = ciArtifacts;
          cargoVendorDir = craneLib.vendorCargoDeps ciArgs;
        }) cargoLinkEnv cargoWarmStart;

        # The allocator's C as a derivation per target, and the opt level that MUST match what
        # cc-rs computes for the cargo profile it is linked into. In `nix/mimalloc.nix` (the
        # cleanest seam - reads neither crane, the flake inputs, nor the source filter).
        inherit (import ./nix/mimalloc.nix { inherit pkgs; }) mimallocFor optLevelFor;

        # WHAT A RELEASE PUBLISHES: the shipped binaries, the cross matrix over them, and the
        # images. In `nix/shipped.nix` - it was fourteen lines under this cap when taken out,
        # and #111 adds a SECOND shipped executable; that module's header carries the reasoning,
        # incl. why `binaries` is a LIST rather than a `--package` per derivation.
        #
        # `apps.<name>`, the `packages = ` block below and the `checks = {` block after it all
        # stay HERE, because `xtask/src/pins.rs` and `xtask/src/workflows.rs` scan this file for
        # them textually and both fail closed on finding none. A module holds what a package or a
        # check POINTS AT, never the declaration.
        shipped = import ./nix/shipped.nix {
          inherit pkgs nixpkgs system crane rust-overlay rustToolchainFile craneLib commonArgs
            inheritedArtifacts auditable mimallocFor optLevelFor;
          inherit (commonArgs) version;
        };

        inherit (shipped) binaries crossPackages imageTargets;

      in
      rec {
        # WHAT `nix build .#<name>` OFFERS, and every name in it but `xtask` comes from
        # `nix/shipped.nix`'s `binaries` list rather than from a line here:
        #
        #   * `sutura`, `sutura-serve`                        - the native release binaries
        #   * `sutura-performance`, `sutura-serve-performance` - the same, fat LTO
        #   * `<binary>-<triple>`, plus `-performance` and `-ci` siblings - the cross matrix
        #   * `oci`, `oci-serve`, and `-performance` siblings  - local Linux images
        #   * `oci-<triple>`, `oci-serve-<triple>`            - one image per shipped artifact
        #   * `<binary>-<feature>-<triple>-ci`                - the feature-on link probes
        #   * `feature-probes-<triple>`                       - which of those exist, as a file
        #
        # `sutura-serve` and its images are what closed #111: before them every published
        # artefact was the command-line tool, so nothing a release published could answer a
        # question over HTTP. `docs/serving.md` is where the deployment shape lives. The probes
        # and their manifests are the entries here that are NOT shipped artifacts - see
        # `probeFeatures`.
        packages = crossPackages // shipped.ociImages // shipped.nativeBinaries
          // shipped.localImages // shipped.featurePackages // shipped.probeManifests // {
          default = shipped.nativeBinaries.sutura;

          # The Pulumi CLI, as a package as well as an app, so `nix build .#pulumi` works from CI.
          pulumi = pkgs.pulumi;

          # The gate binary on its own, so CI can run `nix run .#xtask -- classify` with
          # nothing but `nix` on the runner.
          #
          # `ciArtifacts`: this binary is what CI runs as `nix run .#xtask -- classify`, and
          # `classify` is the FIRST step, so on a cold cache whatever it waits for is on the
          # critical path before the pipeline can decide what to run. That step was 23.9 minutes
          # on the push that added the engine. At opt-level 0 it is a fraction of that, and it is
          # the same closure every gate uses rather than a second one.
          xtask = craneLib.buildPackage (ciArgs // inheritedArtifacts ciArtifacts // {
            pname = "xtask";
            cargoExtraArgs = "--package xtask";
            doCheck = false;
            meta.mainProgram = "xtask";
          });

        };

        # `nix flake check` IS the gate. Every entry reuses `cargoArtifacts`, so the
        # dependency tree is built once for the whole set, not once per check.
        # Deliberately does NOT include the package: `nix flake check` runs its entries in
        # arbitrary order, and the release build must come AFTER lints and tests, not
        # alongside them. CI builds the package as an explicit later step.
        checks = {
          # `--all-features` is load-bearing, not thoroughness for its own sake: the
          # adapters are feature-gated and default-off, so the default feature set is
          # nearly empty. Without it, clippy and the tests would cover none of them and
          # would still report success.
          clippy = craneLib.cargoClippy (ciArgs // inheritedArtifacts ciArtifacts // {
            cargoClippyExtraArgs = "--workspace --all-targets --all-features -- -D warnings";
          });

          nextest = craneLib.cargoNextest ((ciArgs // inheritedArtifacts ciArtifacts // {
            # THE UNFILTERED TREE, and this is what ends a bug class rather than patching its
            # fourth instance. `xtask` is a repo-inspection tool, so its tests read repo files
            # BY DESIGN - `nix/crap.nix` against `docs/crap.md`, `devco/max-lines-ignore`, the
            # workflows. Both golden suites read the one corpus under `examples/` and their own
            # committed snapshots under `crates/*/tests`. Every one of those is
            # invisible under crane's filter, so each new one was a green local run and a red
            # CI step: three found that way already, and the fourth was found here.
            #
            # It costs nothing where the cost would matter. `ciArtifacts` above still builds
            # from the FILTERED source, and that is the expensive derivation - the dependency
            # closure. This only widens what the cheap half sees: our own crates, and the tests.
            # `checks.hygiene` has been doing exactly this since it was written, for the same
            # reason, and the filter clauses stay because clippy and the release build read them.
            src = wholeTree;
            cargoNextestExtraArgs = "--workspace --all-features";
            # `insta` writes a `.snap.new` beside a snapshot that did not match and then fails. In
            # a sandbox that file goes nowhere anybody will read, so this turns the failure into a
            # diff in the log and nothing else. It is also the setting that makes a MISSING
            # snapshot a failure rather than something quietly created and passed.
            INSTA_UPDATE = "no";
          }) // {
            # A real Postgres, provisioned from nixpkgs inside this sandbox over a unix socket, so
            # the postgres corpus and differential cells run HERE (in the single stable test pass)
            # rather than in a separate `nix develop` job. `ciArtifacts` - the expensive dependent
            # closure - is untouched, so its cache key does not move; only this cheap derivation
            # gains the server. The same `nix/postgres-tier.nix` script `just test` runs starts
            # and stops it, so the two places cannot drift. `SUTURA_DEV_REQUIRE_TIER` makes a tier
            # that quietly failed to provision a RED run rather than a loud skip.
            nativeCheckInputs = [ postgresTier.tier pkgs.git ];
            # `start`, then the credential it published: the adapter refuses rather than
            # defaulting one (`github.com/telekom/sutura#455`), so this sandbox has to carry the
            # three `SUTURA_POSTGRES_TIER_*` exports into `checkPhase` the way `nix/with-tier.sh`
            # carries them into `just test`. `runHook preCheck` evaluates this in the phase's own
            # shell, so an `export` here reaches the tests.
            preCheck = "${postgresTier.tier}/bin/sutura-postgres-tier start && eval \"$(${postgresTier.tier}/bin/sutura-postgres-tier credentials)\"";
            postCheck = "${postgresTier.tier}/bin/sutura-postgres-tier stop";
            SUTURA_DEV_REQUIRE_TIER = "1";
          });

          # The identity tier, brought up and provisioned INSIDE the sandbox: a realm, a client
          # and two subjects, and a token fetched AS each of them before it passes. Body and
          # reasoning in `nix/keycloak-tier.nix`, beside the script it drives.
          keycloak-tier = keycloakTier.check;

          # The Postgres tier's state machine: `status` derived from the one document the harness
          # reads, a stop that cannot stop keeping its claim, and `nix/with-tier.sh`'s three-way
          # decision sourced from the file `just test` sources. `checks.nextest` starts and stops
          # this tier, which says a server came up and nothing about any of those. Body in
          # `nix/postgres-tier.nix` beside the script it drives - the declaration stays here,
          # because two xtask gates read this block textually.
          postgres-tier = postgresTier.check;

          # The two release-only assertions about a SHIPPED ARTEFACT - one executable per
          # package, and which features it links - are in `nix/shipped.nix`, beside the list
          # and the features paragraph they are assertions about. Declared here, because
          # `checks = {` is the block `nix flake check` and two xtask gates read, and named
          # one per line rather than `inherit`ed, because that is what those gates parse.
          one-binary = shipped.artifactChecks.one-binary;
          shipped-features = shipped.artifactChecks.shipped-features;

          # A few tools are pinned twice because nix does not run everywhere. `check-pins` fails
          # if pixi.lock disagrees; nix is the authority.

          # nextest deliberately does not run doctests. Zero exist today, so this is cheap
          # now and stays honest as `///` examples appear.
          doctest = craneLib.mkCargoDerivation (ciArgs // inheritedArtifacts checks.nextest // {
            pnameSuffix = "-doctest";
            src = wholeTree;
            doCheck = false;
            buildPhaseCargoCommand = "cargo test --doc --workspace --all-features --profile \"$CARGO_PROFILE\"";
          });

          fmt = craneLib.cargoFmt {
            inherit src;
            inherit (commonArgs) pname version;
          };

          # The structural gates, as a flake check so CI needs only `nix` (devenv is a dev-shell
          # tool); it runs the same xtask binary a developer runs, so the two cannot drift.
          #
          # `wholeTree` and not the filtered source: these gates judge every file in the
          # repo - workflows, Nix files, docs - and crane's filter keeps only Cargo inputs.
          # There is no `.git` in the sandbox, which is why `repo::all_files()` falls back
          # to walking the tree instead of failing.
          #
          # THE PROFILE IS NAMED IN THE COMMAND, and it has to be. `CARGO_PROFILE` in `ciArgs`
          # is a crane convention: crane's own helpers (`cargoClippy`, `cargoNextest`,
          # `buildPackage`) read it and append `--profile`. A hand-written
          # `buildPhaseCargoCommand` is run verbatim, so there the variable is inert and cargo
          # falls back to its default - a different profile, a different `target/` subdirectory,
          # and `cargoArtifacts` that cannot be reused however correctly they were declared.
          #
          # That is not hypothetical. `doctest` had no `--profile` and compiled into
          # `target/debug` while its artifacts sat in `target/ci`, so it rebuilt DataFusion from
          # scratch inside its own derivation and died on `No space left on device` after
          # exhausting the runner's 14 GB. `crap` said `--release` against `ci` artifacts and
          # paid the same tax more quietly. The derivation graph showed one shared closure the
          # whole time - `nix eval` agreed - because sharing an input is not the same as
          # compiling into it.
          hygiene = craneLib.mkCargoDerivation (ciArgs // inheritedArtifacts ciArtifacts // {
            src = wholeTree;
            pnameSuffix = "-hygiene";
            doCheck = false;
            # `check-jscpd` shells to `jscpd`; same expression as `apps.jscpd` (issue #474).
            nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ jscpd ];
            buildPhaseCargoCommand = ''
              cargo run -q --profile "$CARGO_PROFILE" -p xtask -- hygiene
            '';
          });

          # Every file's licence, answerable by a tool. `nix/reuse.nix` carries the derivation and
          # the argument - including, at length, the one thing this check CANNOT catch, which is a
          # newly vendored file inheriting our licence from `REUSE.toml`'s catch-all.
          #
          # The NAME stays here rather than moving with the derivation: `xtask/src/workflows.rs`
          # scans this file textually for the entries of this block, so a check whose name only
          # exists in a module is a workflow reference that gate cannot verify.
          #
          # NOTE FOR WHOEVER EDITS A COMMENT IN THIS BLOCK, because it cost a debugging round:
          # that scan counts braces PER LINE and does not skip comments when it does so. A comment
          # containing an opening brace with no closing one - quoting this block's own header, for
          # instance - pushes its depth accounting to 2, and every later entry then looks nested
          # and is not collected. It fails loudly rather than silently, which is why this is a note
          # and not a bug report, but the shape is worth knowing before writing an example here.
          reuse = licensing.check;

          # The committed API reference pages under `docs/api/` are GENERATED from the library
          # crates' doc comments, and this FAILS when they fall behind: it regenerates them into a
          # temporary directory and byte-compares. The fix it names is `just api`.
          #
          # THE SHARED STABLE `ci` CLOSURE, like every other check here, because nightly is needed
          # only to EMIT THE JSON and `xtask/src/api_docs.rs` reaches it by SHELLING OUT - that
          # module's header carries the argument. It replaced a nightly `crane.mkLib` with a second
          # full DataFusion/Arrow/DuckDB `buildDepsOnly` at `release`, and is also why CI read
          # `devco/rust-toolchain-nightly.toml` on every push while AGENTS.md said it never did.
          # Three things keep the channels apart: `ciArtifacts`, as `hygiene` and `crap` use it, so
          # `nix-store -q --references` names ONE `sutura-deps` across seven consumers; nightly as
          # a command PREFIX with its own `CARGO_TARGET_DIR`, because alternating compilers in one
          # target directory invalidates every artifact in it; and `SUTURA_API_DOCS_PROFILE`, since
          # cargo's default `dev` optimises every dependency and build script at `opt-level = 3`.
          # NAMED IN THE COMMAND both times - see above `hygiene`; a spawned child is the
          # worse half, as crane does not even export `CARGO_PROFILE`. `wholeTree` for `hygiene`'s
          # reason, and SUTURA_API_DOCS_PYTHON is `apiDocsWriter`'s interpreter. MEASURED: 10m01 of
          # PRIVATE phases became 2m10 cold, floored by 482 rustdoc units - 291 of them `rmeta`.
          # Those two were 484 and 293 and are now what `cargo rustdoc -p <lib> --all-features
          # --profile ci -Z unstable-options --unit-graph` reports, summed over the ten documented
          # libs and deduplicated on (package, target, mode): 482 units, of which 291 are `check`
          # and exactly 10 are the `doc` units themselves.
          api-docs = craneLib.mkCargoDerivation (ciArgs // inheritedArtifacts ciArtifacts // {
            src = wholeTree;
            pnameSuffix = "-api-docs";
            doCheck = false;
            nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.python3 ];
            SUTURA_API_DOCS_PYTHON = "${pkgs.python3}/bin/python3";
            buildPhaseCargoCommand = ''
              cargo build -q --profile "$CARGO_PROFILE" -p xtask
              # Read, not assumed, and resolved BEFORE the prefix below overrides it for the child.
              xtask="''${CARGO_TARGET_DIR:-target}/$CARGO_PROFILE/xtask"
              # The binary directly: `cargo run` would have to BE the nightly cargo for the child
              # to inherit nightly, and then nightly would compile `xtask`.
              CARGO="${nightlyToolchain}/bin/cargo" \
              PATH="${nightlyToolchain}/bin:$PATH" \
              CARGO_TARGET_DIR="$TMPDIR/api-docs-rustdoc" \
              SUTURA_API_DOCS_PROFILE="$CARGO_PROFILE" \
                "$xtask" check-api-docs
            '';
          });

          # The CRAP gate: cyclomatic complexity weighted by the tests that cover it.
          #
          # A CHECK and not an app, which is the opposite of `deny` below, and the difference is
          # the network. cargo-deny fetches the RustSec database; this needs nothing but the
          # vendored dependency set, a compiler and two tools already in the store. So it can be
          # sandboxed, and being sandboxed is what makes it reproducible.
          #
          # THE SHARED `cargoArtifacts`, and the reasoning is the opposite of what it looks like:
          # the coverage build cannot reuse them at all (`-C instrument-coverage` changes the
          # rustc invocation), so what the shared attribute buys is no SECOND dependency
          # derivation; here they only make `cargo run -p xtask` cheap, and clippy/nextest built them.
          #
          # The instrumented compile itself is the scope: `sutura-domain`, whose dependency set is
          # serde and thiserror. 11 s cold, measured. `SCOPE` in xtask/src/crap.rs carries the
          # cost of every wider option and docs/crap.md says why this one.
          #
          # `wholeTree` rather than the filtered source: the gate reads `.cargo-crap.toml` and
          # `docs/crap.md`, and crane's filter keeps only Cargo inputs.
          #
          # `HOME` because cargo-llvm-cov writes there and a build sandbox has no home directory -
          # without it the run fails on a path it cannot create.
          crap = craneLib.mkCargoDerivation (ciArgs // inheritedArtifacts ciArtifacts // {
            src = wholeTree;
            pnameSuffix = "-crap";
            doCheck = false;
            nativeBuildInputs = commonArgs.nativeBuildInputs ++ [
              crap.cargoCrap
              crap.llvmCov
              pkgs.cargo-nextest
            ];
            buildPhaseCargoCommand = ''
              export HOME="$TMPDIR/home"
              mkdir -p "$HOME"
              cargo run -q --profile "$CARGO_PROFILE" -p xtask -- crap
            '';
          });

          # NOTE: cargo-deny is deliberately NOT a check here. It fetches the RustSec
          # advisory database, and a Nix build sandbox has no network - as a check it could
          # only ever fail, or pass while silently auditing nothing. CI runs it as
          # `nix run nixpkgs#cargo-deny -- check`, which still needs nothing but `nix`.
        };
        # `nix run .#deny` - the supply-chain gate.
        #
        # An app and not a check because it fetches the RustSec advisory database, and a Nix
        # build sandbox has no network: as a check it could only fail, or pass while auditing
        # nothing.
        #
        # It wraps cargo-deny with the PINNED toolchain on PATH rather than relying on
        # `nix run nixpkgs#cargo-deny`, which was tried and does not work: cargo-deny shells
        # out to `cargo metadata`, and `nix run` puts only cargo-deny on PATH. On a runner
        # that happens to ship Rust it would have silently audited using *that* cargo - a
        # second, unpinned toolchain, which is the drift this flake exists to remove.
        apps.deny = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-deny" ''
            export PATH="${rustToolchain}/bin:${pkgs.cargo-deny}/bin:$PATH"
            exec cargo deny check "$@"
          '');
        };

        # `nix run .#keycloak-tier -- start|stop|status` - the identity tier by hand.
        #
        # An app rather than a package on the dev shell's PATH, and the difference is who pays.
        # `devenv.nix` carries the Postgres tier because `just test` provisions it on every run;
        # nothing here reads Keycloak yet, so putting it in the shell would make every contributor
        # fetch a JVM and a 186 MB server on `nix develop` for a tier they will not use. As an app
        # it arrives when somebody asks for it - `just keycloak-tier start` - and `checks.keycloak-
        # tier` is the venue that runs it unattended.
        #
        # The SAME derivation the check runs, from the same file, which is the property that keeps
        # a demo and CI from drifting.
        apps.keycloak-tier = {
          type = "app";
          program = "${keycloakTier.tier}/bin/sutura-keycloak-tier";
        };

        # `nix run .#causality -- --since <ref>` - the red-before-green gate.
        #
        # An app and not a check for three reasons: it needs git history (a build sandbox has
        # no `.git`), it creates a worktree (a sandbox source is read-only), and it compiles
        # the tree twice to compare behaviours.
        #
        # It wraps the gate with the PINNED cargo and git on PATH. The xtask binary alone
        # cannot do the job - it runs `cargo test` to compare the two behaviours, so handing
        # it whatever cargo the runner ships would compare using a different compiler than
        # the one everything else is pinned to.
        #
        # `cargoWarmStart` is what stops it compiling the closure a third time. It shells out to
        # `cargo nextest` TWICE, and a bare cargo reads neither /nix/store nor the artifacts
        # `checks.nextest` built minutes earlier in the same CI job - 9m48s of the last run's
        # 12m16s was exactly that. See `nix/cargo-env.nix`, including the directory name it has
        # to agree with `xtask/src/causality.rs` about.
        apps.causality = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-causality" ''
            # cargo-nextest as well: the gate shells out to `cargo nextest`, and without it
            # the run fails with "no such command" rather than a verdict.
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:${pkgs.git}/bin:$PATH"

            ${cargoLinkEnv}
            # The warm start carries the baked-`OUT_DIR` sweep itself, for the whole of #346:
            # this app used to inline the script and three of the five consumers relied on it
            # having done so first. `nix/cargo-env.nix` owns it now, one owner for all five.
            ${cargoWarmStart}
            exec cargo run -q --profile ci -p xtask -- test-causality "$@"
          '');
        };

        # `nix run .#bigquery-acceptance` - the one leg that talks to a real cloud service, and an app rather than a check
        # because `checks.*` run in the nix sandbox, which has no network: nothing here is hermetic and none of it is in
        # `just validate`. It carries the pinned cargo and `cargo-nextest` for `apps.deny`'s reason - `nix run` puts only the
        # named program on PATH, so shelling out to a host cargo would be a second toolchain. It fails loudly without
        # `GOOGLE_APPLICATION_CREDENTIALS`, `SUTURA_BQ_DATASET` or `SUTURA_BQ_TABLE`; the billing project comes from the key's
        # own `project_id`, so CI configures none. `--run-ignored only` reaches every `#[ignore]`d test in the targets it runs
        # rather than a listed set - which is why the identity and cross-resource legs are excluded by BINARY, not by a list.
        apps.bigquery-acceptance = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-bigquery-acceptance" ''
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:$PATH"

            ${cargoLinkEnv}
            ${cargoWarmStart}
            exec cargo nextest run --cargo-profile ci -p sutura-exec-bigquery --all-features \
              --run-ignored only -E 'not binary(two_principals) and not binary(exchanged_identity) and not binary(cross_resource)' "$@"
          '');
        };
        # `nix run .#bigquery-two-principals` - the two-principal cell, `docs/adr/0017`'s eighth amendment and issue #123. Its
        # own app for the filter reason above, and everything that app says about being an app rather than a check holds here.
        # Beyond that app's environment it needs `SUTURA_BQ_RLS_DATASET`, `SUTURA_BQ_RLS_TABLE`, `SUTURA_BQ_GROUP_COLUMN`,
        # `SUTURA_BQ_PRINCIPAL_A_ROWS`, `SUTURA_BQ_PRINCIPAL_B_ROWS` and a key document per principal at
        # `SUTURA_BQ_PRINCIPAL_A_KEY` / `SUTURA_BQ_PRINCIPAL_B_KEY`, failing loudly without any; `tests/two_principals.rs` names each.
        apps.bigquery-two-principals = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-bigquery-two-principals" ''
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:$PATH"

            ${cargoLinkEnv}
            ${cargoWarmStart}
            exec cargo nextest run --cargo-profile ci -p sutura-exec-bigquery --all-features \
              --run-ignored only -E 'binary(two_principals)' "$@"
          '');
        };

        # `nix run .#bigquery-exchanged-identity` - the exchanged-identity cell, issue #376: the only
        # leg holding no principal's key. **No workflow invokes it**, and its own header says why.
        apps.bigquery-exchanged-identity = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-bigquery-exchanged-identity" ''
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:$PATH"

            ${cargoLinkEnv}
            ${cargoWarmStart}
            exec cargo nextest run --cargo-profile ci -p sutura-exec-bigquery --all-features \
              --run-ignored only -E 'binary(exchanged_identity)' "$@"
          '');
        };
        # `nix run .#bigquery-cross-dataset` / `.#bigquery-cross-project` - issue #118's two cross-resource venues: writable
        # per-run fixtures across two datasets, read-only preprovisioned mirrors across two projects. Two apps because each
        # needs inputs the other does not, and one demanding both would strand the runnable leg. **No workflow invokes either.**
        apps.bigquery-cross-dataset = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-bigquery-cross-dataset" ''
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:$PATH"
            ${cargoLinkEnv}
            ${cargoWarmStart}
            exec cargo nextest run --cargo-profile ci -p sutura-exec-bigquery --all-features \
              --run-ignored only -E 'binary(cross_resource) and test(join_across_datasets_)' "$@"
          '');
        };
        apps.bigquery-cross-project = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-bigquery-cross-project" ''
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:$PATH"
            ${cargoLinkEnv}
            ${cargoWarmStart}
            exec cargo nextest run --cargo-profile ci -p sutura-exec-bigquery --all-features \
              --run-ignored only -E 'binary(cross_resource) and test(join_across_projects_)' "$@"
          '');
        };

        # `nix run .#default-features` - the shipped feature set COMPILES and LINTS.
        #
        # The other half of the app below, and they are two apps because they are two verdicts:
        # `cargo check` plus `cargo clippy` at the default set here, `cargo nextest` at it there. A
        # single app would collapse a compile failure and a test failure into one exit code on the
        # one lane no other gate in this repository sees at all.
        #
        # An app for `apps.causality`'s reason (it shells out to cargo, which needs a registry and a
        # writable target directory), with the pinned toolchain for `apps.deny`'s - and here that
        # second reason is load-bearing rather than tidy: clippy's lint set depends on its channel,
        # so a host cargo would report a set nothing else here agrees with, on the half of this lane
        # that exists BECAUSE `cargo check` cannot see a lint.
        #
        # NO profile argument, for the reason the app below gives: the gate derives cargo's profile
        # from the stamp `cargoWarmStart` leaves behind, so there is no flag to put on the wrong side
        # of a `--`. The `--profile ci` on this line is cargo's own, building the xtask binary into
        # the warmed directory, and `check-warm-start` is what reads it.
        #
        # The sweep this app most needs is the warm start's own now (#346). It asks for one shipped
        # package with NO feature flags, which the v2 resolver gives a narrower feature set and
        # therefore a different `-C metadata` than the workspace-wide union the closure was built
        # at - so `utoipa-swagger-ui` is recompiled rather than reused, against the `OUT_DIR` its
        # build script baked in another derivation. It used to inline the script here and the other
        # four consumers inherited a purge from whichever of them ran first, which is an ordering
        # rather than a mechanism; `nix/cargo-env.nix` carries it for all five.
        apps.default-features = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-default-features" ''
            export PATH="${rustToolchain}/bin:$PATH"

            ${cargoLinkEnv}
            ${cargoWarmStart}
            exec cargo run -q --profile ci -p xtask -- check-default-features "$@"
          '');
        };

        # `nix run .#default-feature-tests` - the shipped feature set's tests actually RUN.
        #
        # An app for `apps.causality`'s reason (it shells out to cargo, which needs a registry and
        # a writable target directory), with the pinned cargo and nextest for `apps.deny`'s. It
        # takes NO profile argument: the gate derives cargo's profile from the stamp
        # `cargoWarmStart` leaves behind, so there is no flag to put on the wrong side of a `--`.
        # `xtask/src/default_feature_tests.rs` carries the whole argument.
        apps.default-feature-tests = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-default-feature-tests" ''
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:$PATH"

            ${cargoLinkEnv}
            ${cargoWarmStart}
            exec cargo run -q --profile ci -p xtask -- check-default-feature-tests "$@"
          '');
        };

        # `nix run .#crap` - the CRAP gate, outside the sandbox.
        #
        # `checks.crap` above is what CI runs and is the authority. This app exists for the
        # host that has nix and no dev shell: `nix/run-gate.sh` falls back to it, so a commit
        # hook on such a machine reaches the SAME pin rather than skipping.
        #
        # It supplies the pinned cargo as well as the two tools, for the reason `apps.deny`
        # gives at length: `nix run` puts only the named program on PATH, and a tool that
        # shells out to cargo would otherwise use whatever cargo the host happens to ship -
        # a second, unpinned toolchain, which is the drift this file exists to remove.
        apps.crap = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-crap" ''
            export PATH="${rustToolchain}/bin:${crap.cargoCrap}/bin:${crap.llvmCov}/bin:${pkgs.cargo-nextest}/bin:$PATH"

            ${cargoLinkEnv}
            exec cargo run -q --profile ci -p xtask -- crap "$@"
          '');
        };

        # Tools CI runs, from the LOCKED nixpkgs, and the three linters below made the rule: CI
        # reached them as `nix run nixpkgs#<tool>` and as `nix run .#pixi -- run zizmor`, so an
        # unreviewed mutable input, or a second pin nothing synchronised, decided what jobs holding
        # a write token reported. One authority per tool whose version decides its verdict - none
        # is in pixi.toml, and `cargo xtask check-pins` fails a tool named in both.
        apps.zizmor = {
          type = "app";
          program = "${pkgs.zizmor}/bin/zizmor";
        };
        apps.actionlint = {
          type = "app";
          program = "${pkgs.actionlint}/bin/actionlint";
        };
        apps.shellcheck = {
          type = "app";
          program = "${pkgs.shellcheck}/bin/shellcheck";
        };

        apps.betterleaks = {
          type = "app";
          program = "${pkgs.betterleaks}/bin/betterleaks";
        };

        # The copy/paste detector (issue #474); not in pixi.toml (`cargo xtask check-pins`).
        apps.jscpd = {
          type = "app";
          program = "${jscpd}/bin/jscpd";
        };

        # The two supply-chain tools the release path runs, from the LOCKED nixpkgs for the
        # reason the block above states at length: `nix run nixpkgs#cosign` resolves through the
        # flake registry to whatever nixpkgs-unstable points at when the job runs, and one of the
        # jobs running them is the only job in this repository that holds a write token.
        #
        # NEITHER is in pixi.toml - `cargo xtask check-pins` fails a tool named in both - and for
        # both of them the version decides what they PRODUCE rather than only what they report.
        # That is a stronger reason to pin than the linters have: a signature bundle and an SBOM
        # are read by somebody else's verifier, months later, and a format change is silent on the
        # producing side.
        #
        # Apps rather than checks, and it is the `apps.deny` argument twice over. `cosign` reaches
        # Fulcio for a certificate and Rekor for a transparency entry, so in a nix build sandbox it
        # could only fail. `syft` needs no network to read a local image tarball - but the tarball
        # it reads is produced by a workflow step rather than by a derivation, so there is nothing
        # in a sandbox to point it at.
        apps.cosign = {
          type = "app";
          program = "${pkgs.cosign}/bin/cosign";
        };
        apps.syft = {
          type = "app";
          program = "${pkgs.syft}/bin/syft";
        };

        # The REFERENCE reader for the dependency list `cargo auditable` embeds - see
        # `nix/auditable.nix`. The pull-request link check runs it beside `syft` on every shipped
        # target, and it is a second tool rather than a redundant one: this one answers "is the
        # section there", `syft` answers "can the release path's reader parse it". A run where
        # the first passes and the second fails is the interesting one, and without both there
        # is no way to tell it from a build that stopped embedding.
        apps.rust-audit-info = {
          type = "app";
          program = "${pkgs.rust-audit-info}/bin/rust-audit-info";
        };

        # `reuse` on its own, so `just licences` and a host with nix but no dev shell reach the
        # SAME pin `checks.reuse` uses - which is why the program comes off `licensing.tool` rather
        # than off `pkgs` a second time: one expression, so the app and the check cannot resolve to
        # two different versions. Not in pixi.toml - `cargo xtask check-pins` fails a tool named in
        # both - and its version decides what it reports, which is this repo's stated reason for
        # nix being the only pin for such a tool.
        apps.reuse = {
          type = "app";
          program = "${licensing.tool}/bin/reuse";
        };

        # `just` itself, for the one workflow that needs the TASK LIST rather than a task.
        #
        # `docs.yml` runs `.github/scripts/check-task-citations.sh`, which checks every
        # `just <task>` a page cites against the names `just --summary` reports. The point of
        # that check is that it costs seconds instead of the 15m45s a Rust dependency closure
        # costs, so it cannot reach the list through `xtask` - and the list must come from
        # `just` rather than from a copy, because a second list of task names is the drift this
        # repository already has gates about.
        #
        # An app, from the locked nixpkgs, for the reason the block above states: the registry
        # form resolves to whatever nixpkgs-unstable points at when the job runs. Not in
        # pixi.toml either - `cargo xtask check-pins` fails a tool named in both - and no
        # verdict depends on its version here: it prints its own recipe names, so a bump can
        # change the FORMAT but cannot change what the authority is.
        apps.just = {
          type = "app";
          program = "${pkgs.just}/bin/just";
        };
        # The pinned cargo, for the one workflow that has to touch Cargo.lock. `nix develop`
        # was used here and could never have worked: this flake exposes no devShells.
        # writeShellApplication, not `toString (writeShellScript ...)`: the latter yields a
        # store path that nothing in the closure realises, so `nix run` fails with "No such
        # file or directory" naming the wrapper itself. An app whose program lives inside a
        # package gets that package built.
        apps.cargo = {
          type = "app";
          program = "${cargoWrapper}/bin/sutura-cargo";
        };
        # `just api`. The writer for what `checks.api-docs` byte-compares.
        apps.api-docs = {
          type = "app";
          program = "${apiDocsWriter}/bin/sutura-api-docs";
        };
        apps.git-cliff = {
          type = "app";
          program = "${pkgs.git-cliff}/bin/git-cliff";
        };
        # No mkdocs or mike app, deliberately. The docs toolchain is Python, and it lives in
        # pixi's isolated `docs` environment - `pixi run --frozen -e docs docs`.
        #
        # It WAS here, as a `python3.withPackages`, and it did not work: mike shells out to
        # `mkdocs`, and the composed environment produced an mkdocs that ran but could not
        # import `pymdownx`, so `mike deploy` failed after naming the version. Two resolvers
        # for one interpreter is what that failure looks like. One resolver per language:
        # pixi owns Python, nix owns the rest.
        apps.pixi = {
          type = "app";
          program = "${pkgs.pixi}/bin/pixi";
        };

        # `just fuzz` and `just fuzz-smoke`. `nix/fuzz.nix` carries why it is an app rather than a
        # check; `fuzz/` carries what each target covers and what it does not.
        apps.fuzz = {
          type = "app";
          program = "${fuzzRunner}/bin/sutura-fuzz";
        };

        # The Pulumi CLI, nix-pinned. CI adds `nix build .#pulumi`'s bin to PATH so the test-infra
        # stack under `test-infra/pulumi/google` runs the same CLI everywhere; the pypi `pulumi`
        # package is the Python SDK and is not a CLI, which is why the CLI is a nix package.
        apps.pulumi = {
          type = "app";
          program = "${pkgs.pulumi}/bin/pulumi";
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
