# How a toolchain file becomes a derivation.
#
# Imported by both flake.nix and devenv.nix so there is ONE code path from a pin to a
# compiler. They each resolved it themselves before, which is two places to get wrong - and
# going through `languages.rust.{channel,version}` in devenv instead was tried and silently
# produced a shell with no cargo on PATH.
#
# The single pin is devco/rust-toolchain-nightly.toml - the one toolchain the dev shell, CI,
# every gate and every shipped artifact are built from.
{ rustPkgs }:
{
  # The aggregate propagates a C compiler for build scripts and crate linking. Match the
  # selected LLVM tools, or that propagated compiler wins PATH before the explicit package.
  nightly = (rustPkgs.rust-bin.fromRustupToolchainFile ../devco/rust-toolchain-nightly.toml).overrideAttrs {
    depsHostHostPropagated = [ rustPkgs.llvmPackages_latest.clang ];
    propagatedBuildInputs = [ rustPkgs.llvmPackages_latest.clang ];
  };
}
