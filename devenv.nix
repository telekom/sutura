# The dev environment. CI enters this same shell, so a command that works locally works
# there — that is the whole reason to declare it rather than document it.
#
# direnv loads it automatically (.envrc); `direnv allow` once per clone.
{ pkgs, lib, ... }:

let
  # The compiler pin lives in rust-toolchain.toml so rustup, Nix and CI cannot disagree.
  rustChannel = (lib.importTOML ./rust-toolchain.toml).toolchain.channel;
in
{
  languages.rust = {
    enable = true;
    channel = "stable";
    version = rustChannel;
    components = [ "rustfmt" "clippy" "rust-std" "llvm-tools-preview" ];
  };

  packages = with pkgs; [
    # Linking dominates the inner loop; .cargo/config.toml points at these.
    clang
    lld

    # Gates.
    cargo-deny
    cargo-nextest

    # Hook runner: a single Rust binary, so hooks need no Python runtime.
    prek

    # Stacked branches — this plan is a chain of dependent changes by construction.
    stax

    # Python lives behind pixi only; this is just the launcher.
    pixi

    git
    ripgrep
    fd
  ];

  # A broken pin should take two seconds to diagnose, not a mid-CI failure.
  enterShell = ''
    echo "sutura devenv"
    echo "  rustc      $(rustc --version 2>/dev/null || echo MISSING)"
    echo "  cargo      $(cargo --version 2>/dev/null || echo MISSING)"
    echo "  clippy     $(cargo clippy --version 2>/dev/null || echo MISSING)"
    echo "  nextest    $(cargo nextest --version 2>/dev/null || echo MISSING)"
    echo "  cargo-deny $(cargo deny --version 2>/dev/null || echo MISSING)"
    echo "  prek       $(prek --version 2>/dev/null || echo MISSING)"
    echo "  pixi       $(pixi --version 2>/dev/null || echo MISSING)"
  '';

  # Task names are the stable interface; what they shell out to is an implementation
  # detail. `gates` is what CI runs and what a developer runs before pushing.
  scripts = {
    fmt.exec = "cargo fmt --all";
    lint.exec = "cargo clippy --workspace --all-targets -- -D warnings";
    test.exec = "cargo nextest run --workspace";
    boundaries.exec = "cargo run -q -p xtask -- check-boundaries";
    gates.exec = ''
      set -e
      cargo fmt --all -- --check
      cargo clippy --workspace --all-targets -- -D warnings
      cargo nextest run --workspace
      cargo deny check
      cargo run -q -p xtask -- check-boundaries
    '';
  };
}
