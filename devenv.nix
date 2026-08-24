# The dev environment: the shell a developer works in, and nothing else.
#
# direnv loads it automatically (.envrc); `direnv allow` once per clone.
#
# CI DOES NOT USE THIS FILE. It runs `nix build .#checks...`, so the pipeline depends on
# `nix` alone rather than on devenv as well. What keeps the two honest is not a shared
# shell but a shared implementation: the gate scripts below and the `hygiene` flake check
# invoke the SAME xtask binary, and fmt/clippy/tests run the same cargo subcommands against
# the same `rust-toolchain.toml` pin. A gate can therefore be added in one place only by
# forgetting the other, which is a visible diff, not a silent divergence.
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
# either way - see .env.example and devenv.local.nix.
{ pkgs, lib, config, inputs, ... }:

let
  # The compiler pin lives in rust-toolchain.toml and is resolved the SAME way here as in
  # flake.nix - rust-overlay reading the file directly. Going through
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

    # Task runner. `just` is the one name humans and agents both use, so documentation
    # cites a task rather than a command line that drifts from the one people run.
    just

    # Hook runner: a single Rust binary, so hooks need no Python runtime.
    prek

    # Stacked branches - this plan is a chain of dependent changes by construction.
    # `stax` rebases a stack; `gh-stack` describes one (PR bodies and cross-links) for a
    # stack that was built by hand. See the `stacked-branches` skill.
    stax
    gh-stack

    # For stax's `use_gh_cli` and for release commands that use `gh` rather than an action.
    gh

    # The documentation site. A Nix tool, not a Python one, so it belongs here rather than
    # in pixi.toml. CI builds the same book with `nix run nixpkgs#mdbook`.
    mdbook

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
  # breaks the entire shell - too high a price for a convenience tool.
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
    # Formatting includes line endings: rustfmt does not normalise CRLF, and a carriage
    # return kept inside a Nix ''...'' string becomes part of a shell argument - which
    # produces errors naming a lint or flag that looks byte-identical to the correct one.
    fmt.exec = ''
      set -e
      cargo fmt --all
      cargo run -q -p xtask -- text-hygiene --fix
    '';
    # `--all-features` because the adapters are feature-gated and default-off: without it
    # these commands lint and test almost nothing and still pass.
    lint.exec = "cargo clippy --workspace --all-targets --all-features -- -D warnings";
    test.exec = "cargo nextest run --workspace --all-features";
    boundaries.exec = "cargo run -q -p xtask -- check-boundaries";
    max-lines.exec = "cargo run -q -p xtask -- max-lines";
    line-endings.exec = "cargo run -q -p xtask -- line-endings";
    check-skills.exec = "cargo run -q -p xtask -- check-skills";
    check-guidance.exec = "cargo run -q -p xtask -- check-guidance";
    check-secrets.exec = "cargo run -q -p xtask -- check-secrets";
    check-docs.exec = "cargo run -q -p xtask -- check-docs";
    unused-deps.exec = "cargo run -q -p xtask -- unused-deps";

    # The book. `docs` renders to docs/book (gitignored); `docs-serve` watches and reloads.
    # The gate above is what proves it is complete - mdbook builds an unreachable page just
    # as happily as a linked one.
    docs.exec = "mdbook build docs";
    docs-serve.exec = "mdbook serve docs";

    # The cheap structural gates, grouped so CI can run them FIRST: a 1200-line file or a
    # dead dependency should fail in seconds, not after clippy and the test suite.
    hygiene.exec = ''
      set -e
      cargo run -q -p xtask -- max-lines
      cargo run -q -p xtask -- line-endings
      cargo run -q -p xtask -- text-hygiene
      cargo run -q -p xtask -- unused-deps
      cargo run -q -p xtask -- check-boundaries
      cargo run -q -p xtask -- check-skills
      cargo run -q -p xtask -- check-guidance
      cargo run -q -p xtask -- check-secrets
      cargo run -q -p xtask -- check-docs
    '';

    # The finishing sequence. One command, because a checklist in prose is a checklist
    # somebody half-remembers, and because the hooks already encode what has to hold.
    #
    # It judges the COMMITTED branch diff, not the working tree: that is what a reviewer
    # will see. Hence the clean-tree requirement - a dirty tree means the thing being
    # checked is not the thing being proposed.
    ship-check.exec = ''
      set -eu
      base="''${SHIP_CHECK_BASE_REF:-origin/main}"

      if ! git rev-parse --verify --quiet "$base" >/dev/null; then
        echo "ship-check: base ref '$base' does not exist locally." >&2
        echo "  Fetch it, or set SHIP_CHECK_BASE_REF to a local ref." >&2
        exit 1
      fi
      if [ -n "$(git status --porcelain)" ]; then
        echo "ship-check: the working tree is dirty." >&2
        git status --short >&2
        echo >&2
        echo "  Commit or stash first. For uncommitted work run \\`gates\\` instead;" >&2
        echo "  ship-check validates the committed diff from the merge base." >&2
        exit 1
      fi

      merge_base="$(git merge-base "$base" HEAD)"
      echo "ship-check: $merge_base..HEAD"

      echo "== commit-stage hooks over the branch diff"
      prek run --from-ref "$merge_base" --to-ref HEAD

      echo "== the gates' own unit tests"
      # A gate with no test is a gate nobody has seen fail, and these are the checks
      # everything else is trusted to.
      cargo test -q -p xtask --all-features

      echo "== red-before-green for changed tests"
      cargo run -q -p xtask -- test-causality --since "$merge_base"

      echo "== pre-push hooks"
      prek run --hook-stage pre-push --from-ref "$merge_base" --to-ref HEAD

      echo "ship-check: green"
    '';

    # Spelled out rather than calling `hygiene`, so this list does not depend on another
    # script being on PATH first. Cheapest first: fail before paying for clippy.
    gates.exec = ''
      set -e
      cargo run -q -p xtask -- max-lines
      cargo run -q -p xtask -- line-endings
      cargo run -q -p xtask -- text-hygiene
      cargo run -q -p xtask -- unused-deps
      cargo run -q -p xtask -- check-boundaries
      cargo run -q -p xtask -- check-skills
      cargo run -q -p xtask -- check-guidance
      cargo run -q -p xtask -- check-secrets
      cargo run -q -p xtask -- check-docs
      cargo fmt --all -- --check
      cargo clippy --workspace --all-targets --all-features -- -D warnings
      cargo nextest run --workspace --all-features
      cargo deny check
    '';
  };
}
