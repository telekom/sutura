# The CRAP gate's two tools, resolved in ONE place, at ONE version.
#
# Imported by both flake.nix and devenv.nix, for the same reason nix/duckdb.nix and
# nix/toolchains.nix are - and here the reason is sharper. Those two files have their OWN nixpkgs
# pins (flake.lock names nixos-unstable, devenv.lock names cachix/devenv-nixpkgs/rolling), so
# `pkgs.<tool>` is a DIFFERENT VERSION depending on which file did the importing. That was not
# hypothetical: `pkgs.cargo-llvm-cov` resolved to 0.9.0 through flake.nix and 0.8.7 through
# devenv.nix in the same checkout. For a library that is a latent bug. For a tool whose output is
# a VERDICT it means the dev shell reporting a score CI does not, and the disagreement reads as
# flakiness rather than as two pins.
#
# So neither tool comes from nixpkgs. Both are hash-pinned upstream release binaries, which is
# the only construction that yields the same version through both importers.
#
# TWO TOOLS, because CRAP needs two inputs and neither produces both:
#
#   cargo-llvm-cov   runs the tests under LLVM source-based coverage and writes LCOV
#   cargo-crap       parses that LCOV, computes cyclomatic complexity from the AST, scores
#
# WHY cargo-llvm-cov AND NOT tarpaulin: coverage instrumentation has to agree with the compiler,
# and cargo-llvm-cov drives the SAME nightly toolchain every gate and every build use - so the
# instrumentation it emits is the one the compiler this repo ships understands. The llvm tools it
# shells out to arrive as part of its hash-pinned prebuilt release (see `llvmCov` below) rather
# than from a second implementation of the same idea. tarpaulin uses ptrace and would be a second
# answer to "what is covered".
#
# WHY PREBUILT and not `buildRustPackage` from the crates: CI minutes. cargo-crap is not in
# nixpkgs at all, so from-source means compiling clap, syn, rayon, comfy-table and indicatif on
# the first runner that needs it. A `fetchurl` hash is the same provenance guarantee a
# `fetchCrate` hash gives - a fixed-output derivation, allowed network inside an otherwise
# offline sandbox precisely because the content is named in advance - and nothing is compiled.
{ pkgs }:
let
  # ONE version string per tool. `cargo xtask check-crap` reads `crapVersion` back out of this
  # file and fails if docs/crap.md states a different one, so a bump here cannot leave the
  # documentation describing a version nobody runs.
  crapVersion = "0.4.3";
  llvmCovVersion = "0.9.0";

  # One entry per system the flake evaluates for (flake-utils' default four). A system missing
  # from a table is a loud eval error below rather than a silently absent tool: a gate whose
  # binary is not there must not become a gate that passes.
  targets = {
    x86_64-linux = "x86_64-unknown-linux-gnu";
    aarch64-linux = "aarch64-unknown-linux-gnu";
    x86_64-darwin = "x86_64-apple-darwin";
    aarch64-darwin = "aarch64-apple-darwin";
  };

  crapHashes = {
    x86_64-unknown-linux-gnu = "sha256-bKzPSJNwTtx01wFNux4CLXxk73zSkwh1Gcp7wdjAIoU=";
    aarch64-unknown-linux-gnu = "sha256-9Jhm8/Ph8bpGk1SQktVHRswkJucSL89DswvKBLDSc20=";
    x86_64-apple-darwin = "sha256-HkrY/h9p/ev9MAM1kMtGyS5UK3aIw4/bvNcNtWoRvYw=";
    aarch64-apple-darwin = "sha256-spKaEpHkCg2KBIR2T0mrzb0W3DxUPp586AKKKnXZQqA=";
  };

  llvmCovHashes = {
    x86_64-unknown-linux-gnu = "sha256-sGj3yYhBqsucTzgrSgwYSugvSbVqMtRCtCmylhxzvhU=";
    aarch64-unknown-linux-gnu = "sha256-mvU7Jz5Q0B2L3oeF3oVB9nOMxDdSSM12g67ItXaLnSE=";
    x86_64-apple-darwin = "sha256-RZW8kxCwCZE1cFFOsP98Orp0kCViV4A48XANYReD/cI=";
    aarch64-apple-darwin = "sha256-G79dyK2C4Pb/DrkjqmppHHYK22D3l83LRU4gS5OZxPA=";
  };

  system = pkgs.stdenv.hostPlatform.system;

  target = targets.${system} or (throw ''
    nix/crap.nix has no release target for ${system}.
    Add it to `targets` and add a hash to both `crapHashes` and `llvmCovHashes`.
  '');

  hashFor = table: name:
    table.${target} or (throw ''
      nix/crap.nix has no ${name} hash for ${target}. Get it with:
        nix store prefetch-file <the release asset URL>
    '');

  # One derivation shape for both, because they differ only in the URL and the binary name.
  # Kept as a local function rather than its own file: two call sites in one file is not two
  # places to get wrong, and inlining it keeps the reason for every line beside the line.
  pinnedTool = { pname, version, url, hash }:
    pkgs.stdenvNoCC.mkDerivation {
      inherit pname version;

      src = pkgs.fetchurl { inherit url hash; };

      # Both tarballs hold the binary at the root, so there is nothing to descend into.
      sourceRoot = ".";

      # A foreign Linux binary looks for its interpreter at /lib64, which exists neither on NixOS
      # nor in the build sandbox; autoPatchelf rewrites it to the store. macOS binaries need none
      # of this, hence the guard rather than an unconditional list.
      nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.autoPatchelfHook ];
      buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.stdenv.cc.cc.lib ];

      installPhase = ''
        runHook preInstall
        install -D -m755 ${pname} "$out/bin/${pname}"
        runHook postInstall
      '';

      # A pinned binary that cannot run is a gate that fails at the moment it matters. Prove it
      # runs HERE, where the failure names the pin instead of the branch. Both tools are cargo
      # subcommands, so `<tool> <subcommand> --version` is the shape a caller will use.
      doInstallCheck = true;
      installCheckPhase = ''
        "$out/bin/${pname}" ${pkgs.lib.removePrefix "cargo-" pname} --version
      '';

      meta.mainProgram = pname;
    };

  cargoCrap = pinnedTool {
    pname = "cargo-crap";
    version = crapVersion;
    url = "https://github.com/minikin/cargo-crap/releases/download/v${crapVersion}/cargo-crap-${target}.tar.gz";
    hash = hashFor crapHashes "cargo-crap";
  };

  llvmCov = pinnedTool {
    pname = "cargo-llvm-cov";
    version = llvmCovVersion;
    url = "https://github.com/taiki-e/cargo-llvm-cov/releases/download/v${llvmCovVersion}/cargo-llvm-cov-${target}.tar.gz";
    hash = hashFor llvmCovHashes "cargo-llvm-cov";
  };
in
{
  # For `packages` in the dev shell and for the PATH of the gate's flake app and check.
  inherit cargoCrap llvmCov;

  # Echoed by the dev shell. `cargo xtask check-crap` asserts the cargo-crap one against
  # docs/crap.md.
  versions = {
    cargo-crap = crapVersion;
    cargo-llvm-cov = llvmCovVersion;
  };
}
