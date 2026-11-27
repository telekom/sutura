# The fuzz crate's compile-only check: does `fuzz/` still build, with no fuzzing and no cache
# writes. Separate from `nix/fuzz.nix` on purpose - see #2095.
#
# WHY A SEPARATE APP AND NOT A FLAG ON `apps.fuzz`. `nix/fuzz.nix` resolves `cargo-fuzz` and runs
# with the sanitizer/coverage instrumentation a real fuzz run needs; this one never links any of
# that in, so it is the cheap leg a `pull_request`/`merge_group` CAN run on every push to the
# fuzzed tree, while the real fuzz run - `.github/workflows/fuzz.yml`'s own header explains why -
# stays tag-only. Folding this into `apps.fuzz` would make the compile-only question depend on
# `cargo-fuzz` resolving at all, which is a dependency this leg does not need.
#
# WHY NOT A `nix flake check`. Like `apps.fuzz`, `cargo check` wants a writable target directory
# and registry cache rather than a sandboxed build, so this stays an app.
#
# WHAT IT DOES NOT DO. No `cargo fuzz` anywhere in this file - this never links libFuzzer or the
# sanitizer runtime in, so a target that merely fails to COMPILE is the only thing this catches;
# it proves nothing about what a real fuzz run would find. A separate, dedicated `CARGO_TARGET_DIR`
# keeps this from sharing, or invalidating, either the dev shell's `target/` or the sanitizer
# build's `target/fuzz`.
{ pkgs, toolchain }:

pkgs.writeShellApplication {
  name = "sutura-fuzz-check";
  # `stdenv.cc`: `libfuzzer-sys`'s build script compiles a vendored copy of libFuzzer's own C++
  # sources through the `cc` crate even under `cargo check` - build scripts run regardless of
  # check-vs-build - so without a compiler on PATH the first build fails inside that build script.
  runtimeInputs = [ toolchain pkgs.stdenv.cc ];
  text = ''
    # Same root-finding reason as `nix/fuzz.nix`: `$0` is the nix store, not the repository.
    root="$PWD"
    while [ ! -f "$root/fuzz/Cargo.toml" ]; do
      parent="$(dirname "$root")"
      if [ "$parent" = "$root" ]; then
        echo "no fuzz/Cargo.toml at or above $PWD - run this from inside the repository" >&2
        exit 1
      fi
      root="$parent"
    done
    cd "$root"
    export CARGO_TARGET_DIR="$root/target/fuzz-check"
    # This shell's cranelift settings are for the local inner loop and are unscoped, so they
    # follow cargo in here; unset rather than risk an unrelated local build quirk here.
    unset CARGO_UNSTABLE_CODEGEN_BACKEND CARGO_PROFILE_DEV_CODEGEN_BACKEND
    exec cargo check --manifest-path fuzz/Cargo.toml --bins --locked "$@"
  '';
}
