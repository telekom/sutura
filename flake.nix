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
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # The compiler pin is rust-toolchain.toml and nowhere else. Read as data so this
        # file cannot disagree with the dev shell or with a bare rustup fallback.
        rustToolchainFile = ./rust-toolchain.toml;

        # Targets we CROSS-build. Deliberately excludes the host architecture: on an
        # x86_64 builder `sutura` already IS the x86_64-linux binary, and building a
        # separate "cross" x86_64 derivation would compile the whole tree a second time
        # for a byte-identical result. `packages.sutura-x86_64-unknown-linux-gnu` is an
        # alias to the native build instead - see `crossPackages` below.
        crossTargets = [
          "aarch64-unknown-linux-gnu"
          # The statically linked pair, and `x86_64-unknown-linux-musl` counts as a CROSS target
          # even on an x86_64 builder: the CPU is the same but the libc is not, so it is a real
          # second compile rather than the byte-identical one the comment above rules out.
          #
          # Why ship them at all: a musl artifact has no dynamic loader and no libc version
          # floor, so it runs on a host older than the builder and in a `FROM scratch` image.
          # What it costs is the allocator - see `crates/sutura-cli/src/main.rs`, because musl's
          # own is why mimalloc is not optional here.
          "x86_64-unknown-linux-musl"
          "aarch64-unknown-linux-musl"
        ];

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
          # `checks.api-docs` each set `src = ./.` and read the whole tree, so this filter never
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
            # is `checks.nextest` that runs them, on `src = ./.`, so this arm is not what carries
            # them. It is here for `nix/*.nix` itself. `docs/` stays out by the file: it holds the
            # generated API pages and churns, and matching all of it would put every prose edit in
            # the derivation hash of each filtered-src check - see the blast radius above, which is
            # narrower than this comment used to claim, and which was zero until the arm below was
            # anchored.
            || (builtins.match "nix(/.*)?" rel != null)
            || (builtins.match "docs/crap\\.md" rel != null)
            || (craneLibFor system).filterCargoSources path type;
        };

        # The same pin as a package, for the tools that need `cargo` on PATH rather than a
        # crane derivation around it.
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile rustToolchainFile;

        # The NIGHTLY pin as a PACKAGE, never a crane toolchain: the only read of it here, and only
        # ever for the `cargo rustdoc` child that emits the JSON. See `checks.api-docs` below.
        nightlyToolchain = (import ./nix/toolchains.nix { rustPkgs = pkgs; }).nightly;

        # The pinned cargo, for the one workflow that has to touch Cargo.lock.
        cargoWrapper = pkgs.writeShellApplication {
          name = "sutura-cargo";
          text = ''
            export PATH="${rustToolchain}/bin:$PATH"
            exec cargo "$@"
          '';
        };

        # WRITES the committed API pages, and `checks.api-docs` below is the gate that fails when
        # they fall behind - the two must agree byte for byte, which is why one file defines the
        # writer and the check names it as the fix. In `nix/api-docs.nix` because this file was at
        # the 1000-line limit `cargo xtask max-lines` enforces; that module's header carries the
        # rest, including why the seam is here rather than at the checks.
        apiDocsWriter = import ./nix/api-docs.nix { inherit pkgs nightlyToolchain duckdb; };


        craneLibFor = sys:
          (crane.mkLib pkgs).overrideToolchain
            (p: p.rust-bin.fromRustupToolchainFile rustToolchainFile);

        # Native build: what `nix build` and `nix flake check` use.
        craneLib = craneLibFor system;

        # The data system the local Warehouse adapter links against, resolved by the SAME file
        # devenv.nix imports so the dev shell and CI cannot link two different libduckdbs. It also
        # explains why the crate is built without its `bundled` feature, and why the run-time path
        # is a third variable rather than an afterthought.
        duckdb = import ./nix/duckdb.nix { inherit pkgs; };
        postgresTier = import ./nix/postgres-tier.nix { inherit pkgs; };


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

        # What a BARE cargo needs before it can build this workspace, as shell lines: the linker
        # and the libraries an app inherits from nothing, plus the warm start that lets it reuse
        # the dependency closure the checks already built. In `nix/cargo-env.nix` because this
        # file was at the 1000-line limit; that module's header carries the reasoning, and it is
        # where to look when an app fails at the linker or recompiles the world.
        #
        # `cargoVendorDir` is crane's own vendor directory for `ciArgs`, so the app resolves out
        # of the SAME registry the artifacts were built against - which is the half of the warm
        # start that is easy to omit and silently useless without.
        inherit (import ./nix/cargo-env.nix {
          inherit pkgs duckdb;
          cargoArtifacts = ciArtifacts;
          cargoVendorDir = craneLib.vendorCargoDeps ciArgs;
        }) cargoLinkEnv cargoWarmStart;

        # The allocator's C as a derivation per target, and the opt level that HAS to match what
        # cc-rs computes for the cargo profile it is linked into. In `nix/mimalloc.nix` because
        # this file was at the 1000-line limit; of the three seams taken out of here that module
        # is the cleanest - it reads neither crane, nor the flake inputs, nor the source filter.
        inherit (import ./nix/mimalloc.nix { inherit pkgs; }) mimallocFor optLevelFor;

        # A native build for one profile. For `release` the deps derivation is identical to
        # `cargoArtifacts` above, so Nix dedupes it and the checks' work is reused. A
        # performance build necessarily compiles its own, since the profile is what changed.
        nativeFor = profile:
          let args = commonArgs // {
            CARGO_PROFILE = profile;
            # The prebuilt archive, so the build script links it instead of compiling the C.
            SUTURA_MIMALLOC_LIB_DIR = "${mimallocFor { targetPkgs = pkgs; optLevel = optLevelFor profile; isMusl = false; }}/lib";
          };
          in craneLib.buildPackage (args // {
            cargoArtifacts = craneLib.buildDepsOnly args;
            # ONE package. Without this, crane builds the whole workspace and the result held
            # three binaries - `sutura`, `sutura-dev` and `xtask` - which made two stated
            # invariants false: the image is supposed to hold one executable, and xtask's
            # compile-time `CARGO` reference pulled the whole cargo store path into the runtime
            # closure. It also broke reproducibility, because that path differs between builds.
            #
            # On the attrset and not on `args`: `buildDepsOnly` above must stay unscoped, or
            # the shared dependency build stops being shared with the checks.
            cargoExtraArgs = "--package sutura-cli";
            # Tests run as their own check below, sharing the same artifacts.
            doCheck = false;
          } // auditable.toolFor args // {
            cargoBuildCommand = auditable.buildCommand profile;
          });

        sutura = nativeFor "release";

        # One cross-compiled package per target. `cargoExtraArgs` pins the target and the
        # cross linker comes from pkgsCross, so no developer needs a local cross setup.
        crossFor = { target, profile }:
          let
            isMusl = pkgs.lib.hasSuffix "-linux-musl" target;
            crossPkgs = import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
              crossSystem = { config = target; };
            };
            crossLib = (crane.mkLib crossPkgs).overrideToolchain
              (p: p.rust-bin.fromRustupToolchainFile rustToolchainFile);
            args = commonArgs // {
              CARGO_BUILD_TARGET = target;
              CARGO_PROFILE = profile;
              # The prebuilt archive for THIS target. See `mimallocFor` above.
              SUTURA_MIMALLOC_LIB_DIR = "${mimallocFor { targetPkgs = crossPkgs; optLevel = optLevelFor profile; inherit isMusl; }}/lib";
              # Tests cannot run for a foreign architecture on this host; the native build
              # and CI's gates job cover correctness.
              doCheck = false;
              strictDeps = true;
            } // pkgs.lib.optionalAttrs isMusl {
              # THE C COMPILER. mimalloc is C, so a musl target needs a compiler that knows
              # MUSL's headers rather than the host's - compile against glibc headers and link
              # against musl and mimalloc takes its `__GLIBC__` code paths. Nothing is added to
              # `nativeBuildInputs` for it: `crossPkgs.stdenv` already carries the musl cross
              # cc, and crane's `mkCrossToolchainEnv` exports it to the `cc` crate as
              # `CC_<triple>` / `CXX_<triple>` / `AR_<triple>` plus the `TARGET_*` aliases, and
              # to cargo as `CARGO_TARGET_<TRIPLE>_LINKER`. The static-linking rustflags are in
              # `.cargo/config.toml`, which explains why they cannot live here.
              #
              # `-DMI_LIBC_MUSL=1` is upstream's musl switch, and it has to be a COMPILE DEFINE
              # rather than the environment variable of the same name: that name is a CMake
              # option, and `libmimalloc-sys` builds with the `cc` crate and never reads it.
              # nixpkgs' own mimalloc derivation sets it whenever the host libc is musl. In
              # mimalloc v2 it switches off `MI_USE_BUILTIN_THREAD_POINTER`
              # (`include/mimalloc/prim.h`), so the thread id comes from the TLS slot instead of
              # `__builtin_thread_pointer`. `CFLAGS_<triple>` is the `cc` crate's per-target
              # hook; it is appended to the flags cc already computed, not a replacement for
              # them, and it beats `TARGET_CFLAGS` / `CFLAGS` in cc's lookup order so nothing
              # in the sandbox can shadow it.
              "CFLAGS_${builtins.replaceStrings [ "-" ] [ "_" ] target}" = "-DMI_LIBC_MUSL=1";
            };
          in
          crossLib.buildPackage (args // {
            cargoArtifacts = crossLib.buildDepsOnly args;
            # One package, and the target. Same reasoning as `nativeFor`.
            cargoExtraArgs = "--package sutura-cli --target ${target}";
            # The embedded dependency list, per target. Same reasoning as `nativeFor`, and
            # `cargo-auditable` comes from `pkgs` rather than `crossPkgs` because it is a tool
            # that RUNS during the build - `strictDeps = true` above makes that distinction
            # load-bearing rather than stylistic.
          } // auditable.toolFor args // {
            cargoBuildCommand = auditable.buildCommand profile;
          });

        # Nix system -> Rust target triple. Needed because the alias below must be named
        # after the RUST target CI asks for, not after the Nix system.
        hostRustTarget = {
          "x86_64-linux" = "x86_64-unknown-linux-gnu";
          "aarch64-linux" = "aarch64-unknown-linux-gnu";
          "x86_64-darwin" = "x86_64-apple-darwin";
          "aarch64-darwin" = "aarch64-apple-darwin";
        }.${system} or null;

        # `sutura` and `sutura-<triple>` build the default profile; each has a
        # `-performance` sibling. Two NAMES rather than one name plus a flag, so a published
        # asset cannot be ambiguous about which profile produced it, and so a workflow can
        # select one without the build definition growing a mode.
        variants = [
          { suffix = ""; profile = "release"; }
          { suffix = "-performance"; profile = "release-performance"; }
          # The link-check sibling, for pull requests: a branch needs to know that every target
          # still COMPILES AND LINKS - the allocator C included, per target, which is where
          # cross breakage actually lives - and it does not need that answer at LTO prices.
          #
          # `ci` and not `dev`. `dev` optimises every dependency and keeps full debuginfo,
          # which is the right bargain in an incremental shell and the wrong one here: the
          # sandbox starts cold and nothing executes the result, so that was optimisation and
          # debuginfo bought and never used, cached at four targets' worth of size. See the
          # profile in Cargo.toml.
          #
          # Not a shipped artifact and never published. `releaseTargets`, `imageTargets` and the
          # `one-binary` check all key off the unsuffixed name, so nothing here can reach a
          # release asset by accident - `nix eval` shows no `oci-*-ci` attribute exists.
          { suffix = "-ci"; profile = "ci"; }
        ];

        crossPackages = builtins.listToAttrs (builtins.concatMap
          (v:
            (map
              (t: {
                name = "sutura-${t}${v.suffix}";
                value = crossFor { target = t; profile = v.profile; };
              })
              # Never cross-build the host triple: it would compile the whole tree a second
              # time for a byte-identical result.
              (builtins.filter (t: t != hostRustTarget) crossTargets))
            ++ (if hostRustTarget == null then [ ]
            else [{
              name = "sutura-${hostRustTarget}${v.suffix}";
              value = nativeFor v.profile;
            }]))
          variants);

        # Every target that becomes a published artifact: the cross list plus the host triple,
        # which is built natively rather than cross-built. One list, so the packages, the
        # images and the one-binary check cannot disagree about what "shipped" means.
        releaseTargets = pkgs.lib.unique (crossTargets
          ++ (if hostRustTarget == null then [ ] else [ hostRustTarget ]));

        # The subset that can become a container image. A darwin host triple lands in
        # `releaseTargets` and must not.
        imageTargets = builtins.filter (t: pkgs.lib.hasInfix "-linux-" t) releaseTargets;

        # A shipped binary becomes a container image. In `nix/oci.nix` for the line limit;
        # `ociImages` below and `packages.oci` stay HERE, so the `packages = ` block
        # `xtask/src/workflows.rs` scans out of this file is untouched by the split.
        inherit (import ./nix/oci.nix { inherit pkgs; inherit (commonArgs) version; }) ociArch ociFor;

        # One image per shipped target, named after the RUST triple like the binaries are, so a
        # published image and a published tarball can be traced back to the same build.
        ociImages = builtins.listToAttrs (map
          (t: {
            name = "oci-${t}";
            value = ociFor { bin = crossPackages."sutura-${t}"; architecture = ociArch t; };
          })
          imageTargets);
      in
      {
        packages = crossPackages // ociImages // {
          default = sutura;
          inherit sutura;

          sutura-performance = nativeFor "release-performance";

          # The gate binary on its own, so CI can run `nix run .#xtask -- classify` with
          # nothing but `nix` on the runner.
          #
          # `ciArtifacts`: this binary is what CI runs as `nix run .#xtask -- classify`, and
          # `classify` is the FIRST step, so on a cold cache whatever it waits for is on the
          # critical path before the pipeline can decide what to run. That step was 23.9 minutes
          # on the push that added the engine. At opt-level 0 it is a fraction of that, and it is
          # the same closure every gate uses rather than a second one.
          xtask = craneLib.buildPackage (ciArgs // {
            cargoArtifacts = ciArtifacts;
            pname = "xtask";
            cargoExtraArgs = "--package xtask";
            doCheck = false;
            meta.mainProgram = "xtask";
          });

          # `nix build .#oci` -> a loadable image tarball, from the native binary.
          #
          # streamLayeredImage, not buildLayeredImage: it avoids materialising a
          # multi-hundred-MB tarball in the store just to push it. See `ociFor`.
          #
          # `ociImages` above contributes the per-target set, `oci-<triple>` - four images for
          # four binaries. This keeps the unqualified name because it is what a developer and
          # the release workflow reach for, and on an x86_64 builder it is the same derivation
          # as `oci-x86_64-unknown-linux-gnu`.
          oci = ociFor {
            bin = sutura;
            architecture = if pkgs.stdenv.hostPlatform.isAarch64 then "arm64" else "amd64";
          };

          # The same image built from the performance binary, so a release shipping the
          # optimised profile ships a matching image rather than a mismatched pair.
          oci-performance = ociFor {
            bin = nativeFor "release-performance";
            architecture = if pkgs.stdenv.hostPlatform.isAarch64 then "arm64" else "amd64";
          };
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
          clippy = craneLib.cargoClippy (ciArgs // {
            cargoArtifacts = ciArtifacts;
            cargoClippyExtraArgs = "--workspace --all-targets --all-features -- -D warnings";
          });

          nextest = craneLib.cargoNextest ((ciArgs // {
            cargoArtifacts = ciArtifacts;
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
            src = ./.;
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
            nativeCheckInputs = [ postgresTier.tier ];
            preCheck = "${postgresTier.tier}/bin/sutura-postgres-tier start";
            postCheck = "${postgresTier.tier}/bin/sutura-postgres-tier stop";
            SUTURA_DEV_REQUIRE_TIER = "1";
          });

          # The image is supposed to hold one executable and no toolchain. It held three and
          # a full cargo, so this is a check rather than a sentence in a comment. Reads the
          # closure, so a compile-time store-path reference cannot sneak back in either.
          #
          # EVERY shipped target, not just the native one. There are four release artifacts now
          # and the invariant is a property of each: a cross build has its own dependency
          # derivation and its own `cargoExtraArgs`, so "the native package holds one binary"
          # says nothing about the musl one. The price is that this check pulls the cross builds
          # in, which is what it costs for the assertion to be true rather than assumed.
          one-binary =
            let
              shipped = map (target: { inherit target; drv = crossPackages."sutura-${target}"; }) imageTargets;
              checkOne = p: ''
                echo "one-binary: ${p.target}"
                count="$(ls ${p.drv}/bin | wc -l)"
                if [ "$count" != "1" ]; then
                  echo "${p.target}: the shipped package holds $count binaries, expected 1:" >&2
                  ls ${p.drv}/bin >&2
                  exit 1
                fi
                # A toolchain in the closure means something baked a build-time path into the
                # binary. That is how cargo got in: `env!("CARGO")` in a workspace member.
                if grep -qE '(cargo|rustc|rust-minimal)-[0-9]' ${pkgs.closureInfo { rootPaths = [ p.drv ]; }}/store-paths; then
                  echo "${p.target}: a Rust toolchain is in the runtime closure:" >&2
                  grep -E '(cargo|rustc|rust-minimal)-[0-9]' ${pkgs.closureInfo { rootPaths = [ p.drv ]; }}/store-paths >&2
                  exit 1
                fi
              '';
            in
            pkgs.runCommand "sutura-one-binary" { } ''
              set -eu
              ${pkgs.lib.concatMapStrings checkOne shipped}
              touch $out
            '';

          # pixi exists because nix does not run on every host we develop on - so a few tools
          # are pinned twice, and a second pin is a second source of truth unless something
          # checks it. nix is the authority; this fails if pixi.lock disagrees.

          # nextest deliberately does not run doctests. Zero exist today, so this is cheap
          # now and stays honest as `///` examples appear.
          doctest = craneLib.mkCargoDerivation (ciArgs // {
            cargoArtifacts = ciArtifacts;
            pnameSuffix = "-doctest";
            doCheck = false;
            buildPhaseCargoCommand = "cargo test --doc --workspace --all-features --profile \"$CARGO_PROFILE\"";
          });

          fmt = craneLib.cargoFmt {
            inherit src;
            inherit (commonArgs) pname version;
          };

          # The structural gates, as a flake check so CI needs only `nix` - devenv is a
          # DEV-SHELL tool, and installing it in CI just to reach these would add a
          # dependency the pipeline does not otherwise need. It runs the same xtask binary
          # a developer runs, so the two cannot drift.
          #
          # `src = ./.` and not the filtered source: these gates judge every file in the
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
          hygiene = craneLib.mkCargoDerivation (ciArgs // {
            cargoArtifacts = ciArtifacts;
            src = ./.;
            pnameSuffix = "-hygiene";
            doCheck = false;
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
          # worse half, as crane does not even export `CARGO_PROFILE`. `src = ./.` for `hygiene`'s
          # reason, and SUTURA_API_DOCS_PYTHON is `apiDocsWriter`'s interpreter. MEASURED: 10m01 of
          # PRIVATE phases became 2m10 cold, floored by 482 rustdoc units - 291 of them `rmeta`.
          # Those two were 484 and 293 and are now what `cargo rustdoc -p <lib> --all-features
          # --profile ci -Z unstable-options --unit-graph` reports, summed over the ten documented
          # libs and deduplicated on (package, target, mode): 482 units, of which 291 are `check`
          # and exactly 10 are the `doc` units themselves.
          api-docs = craneLib.mkCargoDerivation (ciArgs // {
            cargoArtifacts = ciArtifacts;
            src = ./.;
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
          # THE SHARED `cargoArtifacts`, and the reasoning is the opposite of what it looks like.
          # The coverage build cannot reuse them at all: `-C instrument-coverage` changes the rustc
          # invocation, so every dependency it needs is compiled fresh whatever is passed. What the
          # shared attribute buys is that no SECOND dependency derivation is created. `api-docs`
          # used to need one, being on a different channel, at the cost of a full extra workspace
          # build; it does not any more, so every check here names one closure. Here the artifacts
          # only make `cargo run -p xtask` cheap, and clippy and nextest already built them.
          #
          # The instrumented compile itself is the scope: `sutura-domain`, whose dependency set is
          # serde and thiserror. 11 s cold, measured. `SCOPE` in xtask/src/crap.rs carries the
          # cost of every wider option and docs/crap.md says why this one.
          #
          # `src = ./.` rather than the filtered source: the gate reads `.cargo-crap.toml` and
          # `docs/crap.md`, and crane's filter keeps only Cargo inputs.
          #
          # `HOME` because cargo-llvm-cov writes there and a build sandbox has no home directory -
          # without it the run fails on a path it cannot create.
          crap = craneLib.mkCargoDerivation (ciArgs // {
            cargoArtifacts = ciArtifacts;
            src = ./.;
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
            ${cargoWarmStart}
            exec cargo run -q --profile ci -p xtask -- test-causality "$@"
          '');
        };

        # `nix run .#bigquery-acceptance` - the one leg that talks to a real cloud service.
        #
        # **An app and NOT a check, and that is the whole design.** `checks.*` run in the nix
        # sandbox, which has no network at all, so this could not be a check even with a
        # credential. An app runs outside it and therefore can reach the endpoint - which also
        # means nothing about it is hermetic and it is not part of `just validate`.
        #
        # It supplies the pinned cargo and `cargo-nextest` for the reason `apps.deny` gives at
        # length: `nix run` puts only the named program on PATH, so a run that shelled out to an
        # unpinned host cargo would be a second toolchain.
        #
        # **What it needs from its environment, and it fails loudly without any of it:**
        # `GOOGLE_APPLICATION_CREDENTIALS` at a credential file, plus `SUTURA_BQ_DATASET` and
        # `SUTURA_BQ_TABLE`. The billing project comes from a service-account key's own
        # `project_id`, so CI configures no project variable. `--run-ignored only` is what reaches
        # the three `#[ignore]`d tests; every other task skips them.
        apps.bigquery-acceptance = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-bigquery-acceptance" ''
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:$PATH"

            ${cargoLinkEnv}
            ${cargoWarmStart}
            exec cargo nextest run -p sutura-exec-bigquery --all-features --run-ignored only "$@"
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

        # Tools CI runs, from the LOCKED nixpkgs.
        #
        # These were `nix run nixpkgs#<tool>`, which resolves through the flake registry to
        # whatever nixpkgs-unstable points at when the job runs - an unreviewed, mutable input
        # executing in jobs that hold a write token. It also contradicted this file's whole
        # premise. As apps they come from `flake.lock` like everything else.
        # The workflow and shell linters, from the LOCKED nixpkgs. CI reached these through
        # `nix run .#pixi -- run zizmor`, which took the VERSION from pixi.lock - so nix pinned
        # the compiler and pixi pinned the linters, and nothing checked that the two agreed.
        # One authority, and no second pin to keep in step: these three are deliberately
        # NOT in pixi.toml. Their version decides what they REPORT, so naming them twice
        # would mean two pins plus a synchroniser to keep them honest - which is what was
        # tried first. `cargo xtask check-pins` enforces the split instead.
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
        # `nix/auditable.nix`. `ci.yml`'s cross job runs it beside `syft` on every shipped
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

        formatter = pkgs.nixpkgs-fmt;
      });
}
