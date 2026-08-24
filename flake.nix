# Why this file exists, since it is a fair question
# ------------------------------------------------
# It does one job that nothing else in the repo can: **it makes the build a function of
# pinned inputs instead of a function of the machine.**
#
# Concretely, three things depend on that:
#
#   1. Reproducible release artifacts. `nix build .#oci` yields the same image digest from
#      the same source, so "which build is running in production?" is answerable. A
#      Dockerfile gives you the same *recipe*, not the same *result* — `apt-get install`
#      resolves differently next Tuesday.
#   2. One toolchain definition, not two. The dev shell and the release build both read
#      `rust-toolchain.toml`. With a Dockerfile alongside a devenv you have two places to
#      bump a compiler and one of them will be forgotten.
#   3. Cross-compilation without a cross-toolchain per developer. `nix build
#      .#sutura-aarch64-unknown-linux-gnu` works from an x86_64 host with no local setup,
#      which is what makes shipping both Linux architectures cheap rather than a project.
#
# What it does NOT do: it is not the development environment (that is devenv.nix, which
# this flake also exposes), and it is not required to hack on the code — plain `cargo
# build` works fine. It is required to *ship*.
#
# Cranelift: deliberately absent here. It is a development-only codegen backend (see
# Cargo.toml); CI and every shipped artifact use the default backend, because a bug that
# reproduces under one backend and not the other is a genuinely bad afternoon.
{
  description = "sutura — an identity-aware semantic data runtime for AI agents";

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
        # alias to the native build instead — see `crossPackages` below.
        crossTargets = [ "aarch64-unknown-linux-gnu" ];

        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          # Keep the toolchain file: crane's source filter drops non-Cargo files, and
          # without it the pin is invisible to the build.
          filter = path: type:
            (builtins.match ".*rust-toolchain\\.toml$" path != null)
            || (craneLibFor system).filterCargoSources path type;
        };

        craneLibFor = sys:
          (crane.mkLib pkgs).overrideToolchain
            (p: p.rust-bin.fromRustupToolchainFile rustToolchainFile);

        # Native build: what `nix build` and `nix flake check` use.
        craneLib = craneLibFor system;

        commonArgs = {
          inherit src;
          strictDeps = true;
          # .cargo/config.toml selects clang + lld for the linux targets. The Nix build
          # sandbox has neither unless we say so, and a flake that linked differently from
          # the dev shell would reintroduce exactly the drift this flake exists to remove.
          nativeBuildInputs = [ pkgs.clang pkgs.lld ];
          # `release`, not `release-performance`: the default shipped profile is cheap to
          # build on purpose. Opt into the slow one when throughput has been measured.
          CARGO_PROFILE = "release";
        };

        # Dependencies, compiled ONCE and reused by the build and by every check. This is
        # the reason to use crane rather than a plain buildRustPackage: a naive layout
        # recompiles the dependency tree for clippy, for the tests and for the build, and
        # on this dependency set that is most of the wall clock.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        sutura = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          # Tests run as their own check below, sharing the same artifacts.
          doCheck = false;
        });

        # One cross-compiled package per target. `cargoExtraArgs` pins the target and the
        # cross linker comes from pkgsCross, so no developer needs a local cross setup.
        crossFor = target:
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

        crossPackages = builtins.listToAttrs
          (map (t: { name = "sutura-${t}"; value = crossFor t; })
            # Never cross-build the host triple: it would compile the whole tree a second
            # time for a byte-identical result.
            (builtins.filter (t: t != hostRustTarget) crossTargets))
          // (if hostRustTarget == null then { }
              else { "sutura-${hostRustTarget}" = sutura; });
      in
      {
        packages = crossPackages // {
          default = sutura;
          inherit sutura;

          # `nix build .#oci` -> a loadable image tarball.
          #
          # streamLayeredImage, not buildLayeredImage: it avoids materialising a
          # multi-hundred-MB tarball in the store just to push it.
          #
          # Contents are the binary, CA certificates and tzdata. NO shell and NO package
          # manager — the attack surface of a governed service should be one executable,
          # and it is also the mechanical proof that no interpreter is in the query path.
          oci = pkgs.dockerTools.streamLayeredImage {
            name = "sutura";
            tag = "latest";
            # Pinned, not `now`: an image whose digest changes on every build cannot be the
            # thing a deployment pins.
            created = "1970-01-01T00:00:01Z";
            contents = [ sutura pkgs.cacert pkgs.tzdata ];
            config = {
              Entrypoint = [ "/bin/sutura" ];
              Cmd = [ "--version" ];
              Env = [ "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt" ];
            };
          };
        };

        # `nix flake check` IS the gate. Every entry reuses `cargoArtifacts`, so the
        # dependency tree is built once for the whole set, not once per check.
        checks = {
          inherit sutura;

          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
          });

          nextest = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
          });

          fmt = craneLib.cargoFmt { inherit src; };
        };
        formatter = pkgs.nixpkgs-fmt;
      });
}
