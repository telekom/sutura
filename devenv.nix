# The dev environment. CI enters this same shell, so a command that works locally works
# there — that is the whole reason to declare it rather than document it.
#
# direnv loads it automatically (.envrc); `direnv allow` once per clone.
#
# WHERE THINGS ARE FETCHED FROM
#
# Nothing here hardcodes a source. Two knobs, both read from the environment so a network
# without direct egress is configured OUTSIDE this file:
#
#   NIX_SUBSTITUTER / NIX_TRUSTED_KEY   binary cache (set these to a mirror locally)
#   HTTP_PROXY / HTTPS_PROXY            routes flake-input fetches through a proxy
#
# Locally these point at an internal mirror; on public CI the defaults apply. Same file
# either way — see .env.example and devenv.local.nix.
{ pkgs, lib, config, inputs, ... }:

let
  # The compiler pin lives in rust-toolchain.toml and is resolved the SAME way here as in
  # flake.nix — rust-overlay reading the file directly. Going through
  # `languages.rust.{channel,version}` instead was tried and silently produced a shell
  # with no cargo on PATH, which is a worse failure than a loud one.
  rustPkgs = import inputs.nixpkgs {
    system = pkgs.stdenv.hostPlatform.system;
    overlays = [ (import inputs.rust-overlay) ];
  };
  rustToolchain = rustPkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
in
{

  # Every one of these was verified present in nixpkgs before being listed: a name that
  # does not resolve fails the WHOLE shell evaluation, not just that package.
  packages = [
    # The pinned toolchain: rustc, cargo, clippy, rustfmt and the components named in
    # rust-toolchain.toml. First in the list so it wins any PATH collision.
    rustToolchain
  ] ++ (with pkgs; [
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

    # For gh-axi (below) and any other npm-delivered tooling.
    nodejs_22

    git
    ripgrep
    fd
  ]);

  # gh-axi is an npm package, not a nixpkgs one. Installed into the devenv state dir on
  # first entry rather than listed in `packages`, because a missing nixpkgs attribute
  # breaks the entire shell — too high a price for a convenience tool.
  env.NPM_CONFIG_PREFIX = "${config.devenv.state}/npm";

  # A broken pin should take two seconds to diagnose, not a mid-CI failure.
  enterShell = ''
    export PATH="$NPM_CONFIG_PREFIX/bin:$PATH"

    # A GitHub token for gh-axi and anything else talking to the API. Taken from the
    # environment, else from a gitignored .env. Never committed, never echoed.
    if [ -z "''${GITHUB_TOKEN:-}" ] && [ -f .env ]; then
      GITHUB_TOKEN="$(grep -m1 '^GITHUB_TOKEN=' .env 2>/dev/null | cut -d= -f2- || true)"
      export GITHUB_TOKEN
    fi
    export GH_TOKEN="''${GITHUB_TOKEN:-}"

    # Non-fatal by design: a shell you cannot enter because a convenience tool is
    # unreachable is worse than a shell without that tool.
    if ! command -v gh-axi >/dev/null 2>&1; then
      npm install -g gh-axi >/dev/null 2>&1 || true
    fi

    echo "sutura devenv"
    echo "  rustc      $(rustc --version 2>/dev/null || echo MISSING)"
    echo "  cargo      $(cargo --version 2>/dev/null || echo MISSING)"
    echo "  clippy     $(cargo clippy --version 2>/dev/null || echo MISSING)"
    echo "  nextest    $(cargo nextest --version 2>/dev/null || echo MISSING)"
    echo "  cargo-deny $(cargo deny --version 2>/dev/null || echo MISSING)"
    echo "  prek       $(prek --version 2>/dev/null || echo MISSING)"
    echo "  pixi       $(pixi --version 2>/dev/null || echo MISSING)"
    echo "  gh-axi     $(gh-axi --version 2>/dev/null || echo 'not installed')"
    # Presence only. Printing a token into a CI log is how tokens leak.
    echo "  gh token   $( [ -n "''${GITHUB_TOKEN:-}" ] && echo present || echo 'absent (GITHUB_TOKEN or .env)' )"
  '';

  # Task names are the stable interface; what they shell out to is an implementation
  # detail. `gates` is what CI runs and what a developer runs before pushing.
  scripts = {
    fmt.exec = "cargo fmt --all";
    lint.exec = "cargo clippy --workspace --all-targets -- -D warnings";
    test.exec = "cargo nextest run --workspace";
    boundaries.exec = "cargo run -q -p xtask -- check-boundaries";
    max-lines.exec = "cargo run -q -p xtask -- max-lines";
    line-endings.exec = "cargo run -q -p xtask -- line-endings";
    unused-deps.exec = "cargo run -q -p xtask -- unused-deps";

    # The cheap structural gates, grouped so CI can run them FIRST: a 1200-line file or a
    # dead dependency should fail in seconds, not after clippy and the test suite.
    hygiene.exec = ''
      set -e
      cargo run -q -p xtask -- max-lines
      cargo run -q -p xtask -- line-endings
      cargo run -q -p xtask -- unused-deps
      cargo run -q -p xtask -- check-boundaries
    '';

    # Spelled out rather than calling `hygiene`, so this list does not depend on another
    # script being on PATH first. Cheapest first: fail before paying for clippy.
    gates.exec = ''
      set -e
      cargo run -q -p xtask -- max-lines
      cargo run -q -p xtask -- line-endings
      cargo run -q -p xtask -- unused-deps
      cargo run -q -p xtask -- check-boundaries
      cargo fmt --all -- --check
      cargo clippy --workspace --all-targets -- -D warnings
      cargo nextest run --workspace
      cargo deny check
    '';
  };
}
