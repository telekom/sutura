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
  nightly = rustPkgs.rust-bin.fromRustupToolchainFile ../devco/rust-toolchain-nightly.toml;
}
