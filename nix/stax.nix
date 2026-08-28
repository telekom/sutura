# The stacked-branches tool, pinned to ONE upstream release.
#
# Imported by devenv.nix and by nothing else, which is the honest difference from
# nix/crap.nix and nix/duckdb.nix: those two exist because flake.nix ALSO needs the thing and
# two importers resolving `pkgs.<x>` independently is two versions. `stax` is a developer's
# tool, not a gate - CI neither runs it nor has an opinion about it - so there is no second
# importer to keep in step and no verdict that could disagree.
#
# WHY NOT pkgs.stax, which needs none of this. Because nixpkgs does not carry the release we
# want, anywhere: `pkgs.stax` is 0.102.2 on devenv.lock's nixpkgs, 0.102.2 on flake.lock's, and
# 0.102.2 on nixpkgs master - checked, not assumed. So `just update` moves every other tool in
# the shell and still leaves stax where it was. Bumping an input is not an upgrade of a package
# the input does not have.
#
# WHY PREBUILT and not a from-source override. Overriding nixpkgs' derivation means owning its
# `cargoHash` - a vendor hash for a dependency tree we would then recompute on every bump, and a
# full Rust build on the first machine that enters the shell after one. nix/crap.nix already
# settled that trade for cargo-crap, which nixpkgs does not package at all, and the argument
# transfers unchanged: a `fetchurl` hash is the same provenance guarantee a `fetchCrate` hash
# gives - a fixed-output derivation, allowed network inside an otherwise offline sandbox
# precisely because the content is named in advance - and nothing is compiled.
#
# The residual difference from nixpkgs' package, stated rather than hidden: upstream's binary is
# built on upstream's runners, so we are trusting their build rather than reproducing it. That is
# what a release-asset pin is, cargo-crap's included. What the hash buys is that the bytes cannot
# change under us without this file changing.
#
# TWO BINARIES, and missing the second one is the easy mistake: the tarball ships `stax` and the
# short `st`, and every command in CONTRIBUTING.md and in the `stacked-branches` skill is spelled
# `st`. Installing only `mainProgram` would leave a shell where the documentation does not run.
{ pkgs }:
let
  # ONE version string. The dev shell echoes it on entry and `check-guidance` reads it back out
  # of this file, so prose that names a different one fails the gate rather than rotting.
  staxVersion = "0.108.0";

  # One entry per system the dev shell is entered on. Upstream also publishes a Windows zip,
  # which has no system here to map to. A system missing from the table is a loud eval error
  # below rather than a silently absent tool.
  targets = {
    x86_64-linux = "x86_64-unknown-linux-gnu";
    aarch64-linux = "aarch64-unknown-linux-gnu";
    x86_64-darwin = "x86_64-apple-darwin";
    aarch64-darwin = "aarch64-apple-darwin";
  };

  hashes = {
    x86_64-unknown-linux-gnu = "sha256-DfH+0D/lvIFma2dTMHjuOs7kfBHO1EggSLAXMHxezb8=";
    aarch64-unknown-linux-gnu = "sha256-NwLtf2AUvrbBwWVSI9TYX+/zc1D6X5md/x0WirRnlfc=";
    x86_64-apple-darwin = "sha256-1PGJs/Q5K+BzgtTiGuVDxGpbOaxqDAGOPaRxhKUsO58=";
    aarch64-apple-darwin = "sha256-tt1xQJiKCuhr4eS5mbm2wcroFNxQ1smInPqVe3kYu/Q=";
  };

  system = pkgs.stdenv.hostPlatform.system;

  target = targets.${system} or (throw ''
    nix/stax.nix has no release target for ${system}.
    Add it to `targets` and add a hash to `hashes`.
  '');

  hash = hashes.${target} or (throw ''
    nix/stax.nix has no hash for ${target}. Get it with:
      nix store prefetch-file https://github.com/cesarferreira/stax/releases/download/v${staxVersion}/stax-${target}.tar.gz
  '');

  package = pkgs.stdenvNoCC.mkDerivation {
    pname = "stax";
    version = staxVersion;

    src = pkgs.fetchurl {
      url = "https://github.com/cesarferreira/stax/releases/download/v${staxVersion}/stax-${target}.tar.gz";
      inherit hash;
    };

    # Both binaries sit at the root of the tarball, so there is nothing to descend into.
    sourceRoot = ".";

    # A foreign Linux binary looks for its interpreter at /lib64, which exists neither on NixOS
    # nor in the build sandbox; autoPatchelfHook rewrites it to the store. The needed libraries
    # were read off the ELF rather than guessed - `libz.so.1`, `libgcc_s.so.1`, `libm.so.6`,
    # `libc.so.6` - and zlib is the one a copy of nix/crap.nix would have omitted, because that
    # file's two tools need only libgcc. There is no libssl in the list: stax's TLS is Rust's,
    # not OpenSSL's, which is why nixpkgs' openssl buildInput has no counterpart here.
    # macOS binaries need none of this, hence the guard rather than an unconditional list.
    nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.autoPatchelfHook ];
    buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
      pkgs.stdenv.cc.cc.lib
      pkgs.zlib
    ];

    installPhase = ''
      runHook preInstall
      install -D -m755 stax "$out/bin/stax"
      install -D -m755 st "$out/bin/st"
      runHook postInstall
    '';

    # A pinned binary that cannot run is a broken shell at the moment somebody needs the tool.
    # Prove it runs HERE, where the failure names the pin instead of the branch - and prove the
    # version it reports is the version this file claims, which is what makes the echo in
    # devenv.nix and the number in the skill file the same fact rather than three assertions.
    doInstallCheck = true;
    installCheckPhase = ''
      "$out/bin/stax" --version | grep -Fw "${staxVersion}"
      "$out/bin/st" --version | grep -Fw "${staxVersion}"
    '';

    meta = {
      description = "Stacked-branch workflow for Git with an interactive TUI, smart PRs, and safe undo";
      homepage = "https://github.com/cesarferreira/stax";
      license = pkgs.lib.licenses.mit;
      mainProgram = "stax";
      platforms = builtins.attrNames targets;
    };
  };
in
{
  # For `packages` in the dev shell.
  inherit package;

  # Echoed by the dev shell on entry.
  version = staxVersion;
}
