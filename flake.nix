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
          filter = path: type:
            (builtins.match ".*rust-toolchain\.toml$" path != null)
            || (builtins.match ".*/vendor(/.*)?$" path != null)
            || (craneLibFor system).filterCargoSources path type;
        };

        # The same pin as a package, for the tools that need `cargo` on PATH rather than a
        # crane derivation around it.
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile rustToolchainFile;

        # The pinned cargo, for the one workflow that has to touch Cargo.lock.
        cargoWrapper = pkgs.writeShellApplication {
          name = "sutura-cargo";
          text = ''
            export PATH="${rustToolchain}/bin:$PATH"
            exec cargo "$@"
          '';
        };


        craneLibFor = sys:
          (crane.mkLib pkgs).overrideToolchain
            (p: p.rust-bin.fromRustupToolchainFile rustToolchainFile);

        # Native build: what `nix build` and `nix flake check` use.
        craneLib = craneLibFor system;

        commonArgs = {
          inherit src;
          # Named explicitly: the root manifest is a virtual workspace with no [package],
          # so crane cannot infer these and would fall back to a placeholder - which shows
          # up as derivations called `cargo-package-*` and makes a build log say nothing
          # about what it built.
          pname = "sutura";
          version = "0.1.0";
          strictDeps = true;
          # .cargo/config.toml selects clang + lld for the linux targets. The Nix build
          # sandbox has neither unless we say so, and a flake that linked differently from
          # the dev shell would reintroduce exactly the drift this flake exists to remove.
          nativeBuildInputs = [ pkgs.clang pkgs.lld ];
        };

        # The two profiles we ship.
        #
        # `release` is the default and is cheap to build on purpose (thin LTO, 16 codegen
        # units). `release-performance` adds fat LTO and a single codegen unit: minutes
        # slower, for a binary worth shipping only once throughput has been measured. Both
        # are declared in Cargo.toml; this is where they become build targets.
        releaseArgs = commonArgs // { CARGO_PROFILE = "release"; };

        # Dependencies, compiled ONCE and reused by the build and by every check. This is
        # the reason to use crane rather than a plain buildRustPackage: a naive layout
        # recompiles the dependency tree for clippy, for the tests and for the build, and
        # on this dependency set that is most of the wall clock.
        cargoArtifacts = craneLib.buildDepsOnly releaseArgs;

        # A native build for one profile. For `release` the deps derivation is identical to
        # `cargoArtifacts` above, so Nix dedupes it and the checks' work is reused. A
        # performance build necessarily compiles its own, since the profile is what changed.
        # The allocator's C, compiled in its own derivation rather than by the build script.
        #
        # WHY A DERIVATION. `vendor/mimalloc_rust` is a PATH dependency, so crane's shared
        # dependency build does not shield it the way it shields a registry crate: without
        # this, every edit to our own Rust recompiled mimalloc's C, once per target. Here it
        # is hash-addressed by version, target and optimisation level, so it is built once per
        # combination and then reused from the store and from CI's cache. Our source changes
        # cannot invalidate it. The first build per target is still from source, because
        # nothing upstream caches a musl cross of mimalloc.
        #
        # WHY NOT CMAKE, which would have been the obvious way to build a C library. Upstream's
        # `CMakeLists.txt` decides three things behind our back. `MI_OVERRIDE` defaults ON, which
        # compiles `alloc-override.c` and exports `malloc`, `free` and `operator new` - a
        # semantic change, and the thing issue #5 turns on. `MI_OPT_ARCH` defaults ON for arm64
        # and raises the architecture floor implicitly, which is what Debian, Fedora and nixpkgs
        # all patch out; we DO raise that floor below, but as a stated decision rather than a
        # default nobody chose. And `MI_LIBC_MUSL=ON` appends `-ftls-model=local-dynamic`,
        # against the reasoning in `crates/sutura-cli/src/main.rs`. Compiling `src/static.c` -
        # the single translation unit upstream maintains for exactly this purpose, and the one
        # the build script itself compiles - means none of those defaults exist to override.
        #
        # THE FLAGS ARE A MEASUREMENT, not a design: they are what cc-rs passes today, captured
        # with `CC_ENABLE_DEBUG_OUTPUT=1`. The one addition is `-DMI_PADDING_CHECK_BYTES=1`,
        # because 3.5.0 redefined `MI_SECURE=4` to mean level 3 and moved byte-precise
        # buffer-overflow checking to level 5; without it a `secure level: 4` line would be
        # quietly weaker than the one it replaces. See issue #5.
        mimallocVersion = "3.5.0";
        mimallocFor = { targetPkgs, optLevel, isMusl }:
          let
            # ARMv8.3 FLOOR for the aarch64 targets, deliberately. mimalloc 3.5.0 gains from
            # `LDAPR` (FEAT_LRCPC, v8.3) for its C11 acquire loads, and the level also brings
            # `FEAT_LSE` (v8.1), so atomics become `cas`/`ldadd` rather than `ldxr`/`stxr`
            # retry loops. Measured on the real translation unit: 58 acquire loads move from
            # `ldar` to `ldapr`, with the object file the same size. The aarch64 artifacts
            # therefore REQUIRE ARMv8.3-A or later, and `.cargo/config.toml` sets matching
            # Rust features so the C and the Rust agree on that floor.
            isAarch64 = targetPkgs.stdenv.hostPlatform.isAarch64;
          in
          targetPkgs.stdenv.mkDerivation {
            pname = "mimalloc-static";
            version = mimallocVersion;
            # `fetchurl` on the release tarball, not `fetchFromGitHub`: this way the recorded
            # hash is the hash of the artifact upstream published, which anyone can check with
            # `curl` and `sha256sum`. `fetchFromGitHub` would record a NAR hash of the unpacked
            # tree instead, which is checkable only by nix.
            src = pkgs.fetchurl {
              name = "mimalloc-${mimallocVersion}.tar.gz";
              url = "https://codeload.github.com/microsoft/mimalloc/tar.gz/refs/tags/v${mimallocVersion}";
              sha256 = "1e432f0559a4ab512143b9bff7a700541a2c8d4712b26a72de3e0222790da305";
            };
            dontConfigure = true;
            # Matches cc-rs, which sets it for the same reason: a timestamp in the archive
            # would make the output differ between builds.
            env.ZERO_AR_DATE = "1";
            buildPhase = ''
              runHook preBuild
              $CC -O${optLevel} -ffunction-sections -fdata-sections -fPIC \
                -I include -I src \
                -Wall -Wextra -Wno-error=date-time \
                -ftls-model=initial-exec \
                -DMI_SECURE=4 -DMI_PADDING_CHECK_BYTES=1 \
                -DMI_DEBUG=0 -DMI_BUILD_RELEASE -DNDEBUG \
                ${pkgs.lib.optionalString isMusl "-DMI_LIBC_MUSL=1"} \
                ${pkgs.lib.optionalString isAarch64 "-march=armv8.3-a"} \
                -c src/static.c -o static.o
              $AR cqD libmimalloc.a static.o
              runHook postBuild
            '';
            installPhase = ''
              runHook preInstall
              mkdir -p $out/lib
              cp libmimalloc.a $out/lib/
              runHook postInstall
            '';
          };

        # The C tracks the cargo profile, so the derivation has to as well. Measured rather
        # than assumed: `release` compiles the allocator at `-O1` and `release-performance` at
        # `-O3`, because cc-rs reads cargo's `OPT_LEVEL`. Freezing one number here would
        # silently decouple the allocator from the profile, so this is a pure caching change
        # and not a performance one.
        #
        # `dev` lands on `-O3` and that is NOT an oversight: the allocator is a DEPENDENCY, and
        # `[profile.dev.package."*"] opt-level = 3` in Cargo.toml is what cc-rs sees for it - the
        # `opt-level = 0` on `[profile.dev]` applies to our own crates, not to this. Reading the
        # wrong one of those two keys is the easy mistake here. It also means `dev` reuses the
        # `release-performance` archive rather than adding a third C build to the cache.
        optLevelFor = profile: if profile == "release" then "1" else "3";

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
          # The dev-profile sibling. It exists for pull requests: a branch needs to know that
          # every target still COMPILES AND LINKS - the allocator C included, per target, which
          # is where cross breakage actually lives - and it does not need that answer at LTO
          # prices. `dev` and not a stripped-down release, so the answer comes from the profile
          # developers already build locally.
          #
          # Not a shipped artifact and never published. `releaseTargets`, `imageTargets` and the
          # `one-binary` check all key off the unsuffixed name, so nothing here can reach a
          # release asset by accident.
          { suffix = "-debug"; profile = "dev"; }
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

        # The OCI `architecture` field for a target triple. Not cosmetic: an image built from an
        # aarch64 binary that claims `amd64` gets scheduled onto a node that cannot run it, and
        # the failure surfaces as a crash loop rather than as a rejected placement.
        ociArch = target: if pkgs.lib.hasPrefix "aarch64-" target then "arm64" else "amd64";

        # Contents are the binary, CA certificates and tzdata. NO shell and NO package
        # manager: the attack surface of a governed service should be one executable, and it
        # is also the mechanical proof that no interpreter is in the query path.
        #
        # `cacert` and `tzdata` come from the NATIVE package set even in a cross image, and
        # deliberately: both outputs are data only - PEM text and endian-fixed TZif files - so
        # cross-building them would add a toolchain closure per architecture for a byte-for-byte
        # identical result. The binary is the only architecture-dependent thing in here.
        ociFor = { bin, architecture }: pkgs.dockerTools.streamLayeredImage {
          name = "sutura";
          tag = "latest";
          inherit architecture;
          # Pinned, not `now`: an image whose digest changes on every build cannot be the
          # thing a deployment pins.
          created = "1970-01-01T00:00:01Z";
          contents = [ bin pkgs.cacert pkgs.tzdata ];
          config = {
            Entrypoint = [ "/bin/sutura" ];
            Cmd = [ "--version" ];
            Env = [ "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt" ];
            # Non-root by default. The binary needs no privilege, and a cluster policy of
            # `runAsNonRoot` should be satisfied by the image rather than by a deployment
            # someone has to remember to write. 65532 is the conventional `nonroot` uid.
            User = "65532:65532";
            # `docker inspect` should answer "which commit is this" without a lookup table.
            Labels = {
              "org.opencontainers.image.title" = "sutura";
              "org.opencontainers.image.description" = "identity-aware semantic data runtime for AI agents";
              "org.opencontainers.image.licenses" = "Apache-2.0";
              "org.opencontainers.image.source" = "https://github.com/telekom/sutura";
              "org.opencontainers.image.version" = commonArgs.version;
            };
          };
        };

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
          # nothing but `nix` on the runner. It reuses `cargoArtifacts`, so exposing it
          # costs no extra dependency build.
          xtask = craneLib.buildPackage (releaseArgs // {
            inherit cargoArtifacts;
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
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--workspace --all-targets --all-features -- -D warnings";
          });

          nextest = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            cargoNextestExtraArgs = "--workspace --all-features";
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
          doctest = craneLib.mkCargoDerivation (releaseArgs // {
            inherit cargoArtifacts;
            pnameSuffix = "-doctest";
            doCheck = false;
            buildPhaseCargoCommand = "cargo test --doc --workspace --all-features";
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
          # `--release` reuses `cargoArtifacts` rather than compiling xtask's dependency
          # set a second time under the dev profile.
          hygiene = craneLib.mkCargoDerivation (commonArgs // {
            inherit cargoArtifacts;
            src = ./.;
            pnameSuffix = "-hygiene";
            doCheck = false;
            buildPhaseCargoCommand = ''
              cargo run --release -q -p xtask -- hygiene
            '';
          });

          # The committed API reference pages under `docs/api/` are GENERATED from the library
          # crates' doc comments. This is what FAILS when they fall behind the sources: it
          # regenerates them into a temporary directory and byte-compares against what is
          # committed. The gate is `cargo xtask check-api-docs` and the fix it asks for is
          # `just api`.
          #
          # NIGHTLY, and the only check here that is. `--output-format json` is an unstable
          # rustdoc option, so the stable pin every other check uses rejects `-Z` outright.
          # `nix/toolchains.nix` is the one place either pin becomes a compiler, so the nightly
          # comes from there rather than being resolved a second way in this file.
          #
          # Its own crane instance and its own dependency build. The shared `cargoArtifacts` is
          # compiled by stable, and alternating compilers in one target directory invalidates
          # every artifact in it.
          #
          # `src = ./.` and not the filtered source, for the same reason as `hygiene` above:
          # this check reads `docs/.tools/rustdoc_to_markdown.py` and the committed pages, and
          # crane's filter keeps only Cargo inputs.
          #
          # SUTURA_API_DOCS_PYTHON: the generator is a stdlib-only script, and `just api` runs
          # it through pixi because pixi owns every Python in this repo. A build sandbox has no
          # network and so cannot materialise a pixi environment, so the interpreter is named
          # here instead. The SCRIPT is the same either way, which is what stops this check
          # from disagreeing with what `just api` produces.
          api-docs =
            let
              nightlyCrane = (crane.mkLib pkgs).overrideToolchain
                (_: (import ./nix/toolchains.nix { rustPkgs = pkgs; }).nightly);
            in
            nightlyCrane.mkCargoDerivation (commonArgs // {
              # Unscoped, like `cargoArtifacts` above: scoping it to one package would stop the
              # dependency build being shared with the xtask compile in the build phase.
              cargoArtifacts = nightlyCrane.buildDepsOnly commonArgs;
              src = ./.;
              pnameSuffix = "-api-docs";
              doCheck = false;
              nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.python3 ];
              SUTURA_API_DOCS_PYTHON = "${pkgs.python3}/bin/python3";
              buildPhaseCargoCommand = ''
                cargo run --release -q -p xtask -- check-api-docs
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
        apps.causality = {
          type = "app";
          program = builtins.toString (pkgs.writeShellScript "sutura-causality" ''
            # cargo-nextest as well: the gate shells out to `cargo nextest`, and without it
            # the run fails with "no such command" rather than a verdict.
            export PATH="${rustToolchain}/bin:${pkgs.cargo-nextest}/bin:${pkgs.git}/bin:$PATH"
            exec cargo run --release -q -p xtask -- test-causality "$@"
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
