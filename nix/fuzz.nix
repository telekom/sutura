# The fuzzing runner: the pinned nightly compiler, `cargo-fuzz`, and a C++ toolchain, from the
# LOCKED nixpkgs.
#
# WHY A DERIVATION AND NOT A CI ACTION OR AN `npm`/`cargo install` LINE. Everything else in this
# repository resolves its tools through `flake.lock`, and a fuzzer is the tool whose version most
# changes what it reports: libFuzzer's mutators, the sanitizer runtime and the coverage
# instrumentation all move between releases, so "the fuzzer found nothing" is only attributable if
# the fuzzer is pinned. `cargo xtask check-pins` already refuses a tool named in both nix and pixi;
# this is the nix side of that split.
#
# WHY ITS OWN FILE. `flake.nix` sits within a few lines of the 1000-line cap `cargo xtask
# max-lines` enforces, and the cap exempts nothing that would help here. `flake.nix` keeps the
# `apps.fuzz` declaration - it has to, two gates read that file textually for the app names - and
# everything the app POINTS AT is here.
#
# WHY NIGHTLY. `cargo-fuzz` compiles with `-Zsanitizer=address` and the sancov passes, which are
# nightly-only. This is the SECOND narrow nightly use in the repository, beside the rustdoc JSON
# one `devco/rust-toolchain-nightly.toml` documents, and it has the same shape: nightly compiles
# something no gate reads and nothing shipped: every fuzz binary is executed and discarded.
#
# WHAT IT DOES NOT DO. It builds nothing in a nix sandbox. `cargo fuzz` wants a writable target
# directory and a registry, exactly like `apps.deny`, `apps.causality` and `apps.crap`, so this is
# an app rather than a check - and it is also why fuzzing cannot become a `nix flake check`.
{ pkgs, toolchain }:

pkgs.writeShellApplication {
  name = "sutura-fuzz";
  # `stdenv.cc` for the C++ compiler: `libfuzzer-sys` builds a vendored copy of libFuzzer's own
  # C++ sources through the `cc` crate, so without a compiler on PATH the first build fails inside
  # a build script rather than in anything recognisable.
  runtimeInputs = [ toolchain pkgs.cargo-fuzz pkgs.stdenv.cc ];
  text = ''
    # `cargo fuzz` resolves `fuzz/` relative to the current directory, so find the root by walking
    # up for the fuzz crate's own manifest. `$0` cannot be used for this: a `writeShellApplication`
    # runs from the nix store, and the store is not the repository.
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
    # A target directory of its own: it is the one tree compiled by the fuzz toolchain and its
    # sanitizer, so it must not share artifacts with, or invalidate, the dev shell's `target/`.
    export CARGO_TARGET_DIR="$root/target/fuzz"
    # This shell's cranelift settings are for the local inner loop and are unscoped, so they follow
    # cargo in here. Cranelift cannot emit the sancov instrumentation a fuzz build needs.
    unset CARGO_UNSTABLE_CODEGEN_BACKEND CARGO_PROFILE_DEV_CODEGEN_BACKEND
    exec cargo fuzz "$@"
  '';
}
