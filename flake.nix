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
        crossTargets = [ "aarch64-unknown-linux-gnu" ];

        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          # Keep the toolchain file: crane's source filter drops non-Cargo files, and
          # without it the pin is invisible to the build.
          filter = path: type:
            (builtins.match ".*rust-toolchain\\.toml$" path != null)
            || (craneLibFor system).filterCargoSources path type;
        };

        # The same pin as a package, for the tools that need `cargo` on PATH rather than a
        # crane derivation around it.
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile rustToolchainFile;

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
        nativeFor = profile:
          let args = commonArgs // { CARGO_PROFILE = profile; };
          in craneLib.buildPackage (args // {
            cargoArtifacts = craneLib.buildDepsOnly args;
            # Tests run as their own check below, sharing the same artifacts.
            doCheck = false;
          });

        sutura = nativeFor "release";

        # One cross-compiled package per target. `cargoExtraArgs` pins the target and the
        # cross linker comes from pkgsCross, so no developer needs a local cross setup.
        crossFor = { target, profile }:
          let
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
              # Tests cannot run for a foreign architecture on this host; the native build
              # and CI's gates job cover correctness.
              doCheck = false;
              strictDeps = true;
            };
          in
          crossLib.buildPackage (args // { cargoArtifacts = crossLib.buildDepsOnly args; });

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
        # Contents are the binary, CA certificates and tzdata. NO shell and NO package
        # manager: the attack surface of a governed service should be one executable, and it
        # is also the mechanical proof that no interpreter is in the query path.
        ociFor = bin: pkgs.dockerTools.streamLayeredImage {
          name = "sutura";
          tag = "latest";
          # Pinned, not `now`: an image whose digest changes on every build cannot be the
          # thing a deployment pins.
          created = "1970-01-01T00:00:01Z";
          contents = [ bin pkgs.cacert pkgs.tzdata ];
          config = {
            Entrypoint = [ "/bin/sutura" ];
            Cmd = [ "--version" ];
            Env = [ "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt" ];
          };
        };
      in
      {
        packages = crossPackages // {
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

          # `nix build .#oci` -> a loadable image tarball.
          #
          # streamLayeredImage, not buildLayeredImage: it avoids materialising a
          # multi-hundred-MB tarball in the store just to push it. See `ociFor`.
          oci = ociFor sutura;

          # The same image built from the performance binary, so a release shipping the
          # optimised profile ships a matching image rather than a mismatched pair.
          oci-performance = ociFor (nativeFor "release-performance");
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
              cargo run --release -q -p xtask -- line-endings
              cargo run --release -q -p xtask -- text-hygiene
              cargo run --release -q -p xtask -- max-lines
              cargo run --release -q -p xtask -- unused-deps
              cargo run --release -q -p xtask -- check-boundaries
              cargo run --release -q -p xtask -- check-skills
              cargo run --release -q -p xtask -- check-guidance
              cargo run --release -q -p xtask -- check-secrets
              cargo run --release -q -p xtask -- check-docs
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
            export PATH="${rustToolchain}/bin:${pkgs.git}/bin:$PATH"
            exec cargo run --release -q -p xtask -- test-causality "$@"
          '');
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
