# How a toolchain file becomes a derivation.
#
# Imported by both flake.nix and devenv.nix so there is ONE code path from a pin to a
# compiler. They each resolved it themselves before, which is two places to get wrong - and
# going through `languages.rust.{channel,version}` in devenv instead was tried and silently
# produced a shell with no cargo on PATH.
#
# `stable` is the authority for everything shipped: CI, the release build and the OCI image
# all use it. `nightly` exists only for cranelift in the local inner loop.
{ rustPkgs }:
{
  stable = rustPkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
  nightly = rustPkgs.rust-bin.fromRustupToolchainFile ../devco/rust-toolchain-nightly.toml;
}
