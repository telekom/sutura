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
  # Both compiler pins are resolved by nix/toolchains.nix, the SAME file flake.nix imports -
  # one code path from a pin to a compiler. Going through `languages.rust.{channel,version}`
  # instead was tried and silently produced a shell with no cargo on PATH, which is a worse
  # failure than a loud one.
  rustPkgs = import inputs.nixpkgs {
    system = pkgs.stdenv.hostPlatform.system;
    overlays = [ (import inputs.rust-overlay) ];
  };
  toolchains = import ./nix/toolchains.nix { inherit rustPkgs; };

  # The data system the local Warehouse adapter links against, resolved by the SAME file
  # flake.nix imports so the dev shell and CI cannot link two different libduckdbs.
  duckdb = import ./nix/duckdb.nix { inherit pkgs; };

  # The CRAP gate's two tools, resolved by the SAME file flake.nix imports. For a tool whose
  # output is a VERDICT this matters more than it does for a library: resolving cargo-crap
  # independently here and there would mean the dev shell reporting a score CI does not, and the
  # disagreement would look like flakiness rather than like two pins.
  crap = import ./nix/crap.nix { inherit pkgs; };

  # The stacked-branches tool, pinned to one upstream release rather than taken from nixpkgs -
  # which carries 0.102.2 on both locks AND on master, so no input bump reaches a newer one. Its
  # own file for the same reason the three above have one: the pin, the reason for it and the
  # per-system hashes belong beside each other, not spread across a package list. Unlike those
  # three, flake.nix does not import it: CI neither runs stax nor has an opinion about it.
  stax = import ./nix/stax.nix { inherit pkgs; };

  # NIGHTLY is what the interactive shell gets, because cranelift is nightly-only and it is
  # the reason the inner loop is fast. STABLE is what the gates get - see `stableBin` below.
  rustToolchain = toolchains.nightly;

  # The stable toolchain's bin directory, prepended by every gate script.
  #
  # This split is deliberate and the alternative was worse. Clippy's lint set differs between
  # channels, and this repo enables the whole `restriction` category with `-D warnings`: a
  # nightly clippy reports lints stable has never heard of, so running the gates on nightly
  # means local failures CI does not have and local passes CI rejects. Build fast on nightly,
  # gate on exactly what CI gates on.
  stableBin = "${toolchains.stable}/bin";

  # Wrap a gate so it runs on stable, in its own target directory. The snippet is a
  # real shell file so shellcheck lints it and the justfile can source the SAME one -
  # "run this the way CI runs it" is defined once.
  onStable = body: ""
    + "set -e
"
    + "# shellcheck source=nix/stable-env.sh
"
    + "source ${./nix/stable-env.sh}
"
    + body;
in
{

  # Cranelift, enabled rather than described. Cargo.toml told developers to set
  # `profile.dev.codegen-backend` themselves, which could not work: the profile key needs the
  # `-Zcodegen-backend` unstable flag, and the toolchain it documented was stable.
  #
  # Environment rather than .cargo/config.toml because that file is committed and read by CI
  # too - an `[unstable]` table there would fail every stable build. These variables exist
  # only inside this shell.
  # Read by nix/stable-env.sh, which every gate sources. Unset outside this shell,
  # where the snippet is then a no-op - which is exactly right for CI.
  env.SUTURA_STABLE_BIN = stableBin;

  # Build-time and run-time paths to libduckdb, from nix/duckdb.nix. Spelled out there, once,
  # including why the run-time one is separate and what breaks without it.
  env.DUCKDB_LIB_DIR = duckdb.env.DUCKDB_LIB_DIR;
  env.DUCKDB_INCLUDE_DIR = duckdb.env.DUCKDB_INCLUDE_DIR;
  env.LD_LIBRARY_PATH = duckdb.env.LD_LIBRARY_PATH;

  env.CARGO_UNSTABLE_CODEGEN_BACKEND = "true";
  env.CARGO_PROFILE_DEV_CODEGEN_BACKEND = "cranelift";

  # The standard library source, from the shell's own toolchain (nix/toolchains.nix). Both
  # are pinned together: rust-analyzer is in the same nightly's bin and rust-src in the same
  # sysroot, so the language server navigates the std of the compiler it is paired with. Set
  # explicitly rather than left to discovery, which is the documented devenv behaviour.
  env.RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

  # Every one of these was verified present in nixpkgs before being listed: a name that
  # does not resolve fails the WHOLE shell evaluation, not just that package.
  packages = [
    # The pinned NIGHTLY toolchain: rustc, cargo, clippy, rustfmt and the components named
    # in devco/rust-toolchain-nightly.toml, cranelift among them. First in the list so it wins any
    # PATH collision - the gates override it back to stable per-command.
    rustToolchain

    # The data system the local Warehouse adapter links against, and its CLI, which is handy for
    # looking at a fixture by hand. Listed here rather than inside the `with pkgs` block below
    # because `duckdb` is a let-binding in this file and reading it as `pkgs.duckdb` in one place
    # and the binding in another is exactly the drift nix/duckdb.nix exists to remove.
    duckdb.package

    # The CRAP gate. Two tools because the metric needs two inputs and neither produces both:
    # cargo-llvm-cov runs the tests under LLVM coverage and writes LCOV, cargo-crap reads that
    # LCOV, computes complexity from the AST and scores. Listed here rather than inside the
    # `with pkgs` block below because `crap` is a let-binding in this file, and reading one of
    # them as `pkgs.cargo-llvm-cov` here and through the binding there is exactly the drift
    # nix/crap.nix exists to remove.
    crap.cargoCrap
    crap.llvmCov

    # Stacked branches - this plan is a chain of dependent changes by construction. `stax`
    # rebases a stack (`gh-stack`, which describes one, is a nixpkgs package and stays below).
    # Listed here rather than inside the `with pkgs` block for the same reason as the two above,
    # and here it is not only tidiness: `stax` is a let-binding in this file AND a nixpkgs
    # attribute, and a `with` binding loses to a `let` one - so the bare name inside that block
    # would silently resolve to this derivation while reading as `pkgs.stax`. Naming the
    # attribute is what makes which one is meant visible. See the `stacked-branches` skill.
    stax.package
  ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
    # What `-liconv` resolves to on a mac. rustc emits it for every darwin link, the SDK does not
    # carry it under nix, and .cargo/config.toml routes the link through the clang wrapper so that
    # this package being present is what puts it on the search path. Absent on linux, where glibc
    # provides iconv and adding a second one is how a build finds the wrong symbols.
    pkgs.libiconv
  ] ++ (with pkgs; [
    # Linking dominates the inner loop; .cargo/config.toml points at these.
    clang
    lld

    # Secret detection, by the people who wrote gitleaks. A maintained rule set beats a
    # hand-written pattern list, which is what this replaced.
    betterleaks

    # Gates.
    cargo-deny
    cargo-nextest

    # Task runner. `just` is the one name humans and agents both use, so documentation
    # cites a task rather than a command line that drifts from the one people run.
    just

    # `gh-stack` describes a stack (PR bodies and cross-links) for one that was built by hand.
    # It is not a replacement for `st refresh`, which restacks. nixpkgs' version is the one we
    # want, so unlike `stax` above it needs no pin of its own.
    gh-stack

    # For stax's `use_gh_cli` and for release commands that use `gh` rather than an action.
    gh

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
    # Only when there IS one. An unconditional export set `GH_TOKEN=` on every shell without a
    # token, and an EMPTY variable is not the same as an absent one: zizmor takes `--gh-token`
    # from the environment and refuses an empty one outright ("GitHub token cannot be empty"), so
    # the workflow-analysis hook failed on every machine without a token - a gate reporting a
    # configuration problem as a finding about the workflows.
    if [ -n "''${GITHUB_TOKEN:-}" ]; then
      export GH_TOKEN="$GITHUB_TOKEN"
    fi

    # gh-axi, installed on first entry. Two things that used to be wrong about this, both
    # fixed rather than the tool removed:
    #
    #   - the install ran with GITHUB_TOKEN and GH_TOKEN in its environment, so a compromised
    #     release of it or of any transitive dependency could read the highest-value credential
    #     on the machine. `env -u` strips both for the duration of the install; nothing in an
    #     npm postinstall needs a GitHub token.
    #   - failures were discarded with `|| true` and the output sent to /dev/null, so a broken
    #     install looked identical to a working one. It now says so.
    #
    # Residual risk, stated rather than hidden: this is npm, resolved at install time, so the
    # dependency tree is not hash-pinned the way everything else here is.
    if ! command -v gh-axi >/dev/null 2>&1; then
      if env -u GITHUB_TOKEN -u GH_TOKEN npm install -g --no-fund --no-audit gh-axi; then
        echo "  gh-axi     installed"
      else
        echo "  gh-axi     install FAILED (the shell is otherwise fine)" >&2
      fi
    fi

    echo "sutura devenv"
    echo "  rustc      $(rustc --version 2>/dev/null || echo MISSING)"
    echo "  cargo      $(cargo --version 2>/dev/null || echo MISSING)"
    echo "  clippy     $(cargo clippy --version 2>/dev/null || echo MISSING)"
    echo "  nextest    $(cargo nextest --version 2>/dev/null || echo MISSING)"
    echo "  cargo-deny $(cargo deny --version 2>/dev/null || echo MISSING)"
    # The CRAP gate's two tools. Echoed for the same reason as the rest: a broken pin should
    # take two seconds to diagnose, and for a tool whose output is a verdict the version is
    # part of the answer, so seeing it is not a nicety.
    echo "  llvm-cov   $(cargo llvm-cov --version 2>/dev/null || echo MISSING)"
    echo "  cargo-crap $(cargo crap --version 2>/dev/null || echo MISSING)"
    # stax, because this is the one tool in the shell whose version the DOCUMENTATION quotes -
    # the `stacked-branches` skill cites its `--help` for what `sync` does and does not do, and
    # `check-guidance` fails if that citation and nix/stax.nix disagree. Seeing the number on
    # entry is what makes the third party to that agreement observable rather than assumed.
    echo "  stax       $(stax --version 2>/dev/null || echo MISSING)"
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
    # THROUGH xtask, never the `--all` form. `--all` formats every package `cargo metadata`
    # reports, INCLUDING the path dependencies `[workspace] exclude` keeps out of the member
    # list - so it wanted to rewrite the VENDORED mimalloc source, which is the one thing
    # vendoring must not do. xtask derives the member list instead and says so at length in
    # xtask/src/fmt.rs. The justfile and the commit hook were fixed for this; these two scripts
    # were missed, so `xtask fmt --check` passing did not prove `devenv shell gates` passed.
    # `cargo xtask check-guidance` now fails on the `--all` form so neither can come back.
    fmt.exec = onStable ''
      set -e
      cargo run -q -p xtask -- fmt
      cargo run -q -p xtask -- text-hygiene --fix
    '';
    # `--all-features` is a no-op today, since no crate declares a feature. It stays so that
    # coverage cannot silently drop the day an adapter goes behind one.
    lint.exec = onStable "cargo clippy --workspace --all-targets --all-features -- -D warnings";
    test.exec = onStable "cargo nextest run --workspace --all-features";
    boundaries.exec = onStable "cargo run -q -p xtask -- check-boundaries";
    max-lines.exec = onStable "cargo run -q -p xtask -- max-lines";
    line-endings.exec = onStable "cargo run -q -p xtask -- line-endings";
    check-skills.exec = onStable "cargo run -q -p xtask -- check-skills";
    check-guidance.exec = onStable "cargo run -q -p xtask -- check-guidance";
    # The whole worktree, not just staged changes: `secrets` is for a sweep, the hook is
    # for a commit.
    secrets.exec = "betterleaks dir . --config devco/gitleaks.toml --redact --verbose";
    check-docs.exec = onStable "cargo run -q -p xtask -- check-docs";
    unused-deps.exec = onStable "cargo run -q -p xtask -- unused-deps";

    # The CRAP gate. `onStable` because coverage instrumentation is LLVM-specific and the shell's
    # bare cargo is a cranelift nightly where `-C instrument-coverage` does not exist - so this is
    # not the channel-consistency argument the lints have, it is that the instrumentation is
    # absent. The task hardens its own environment as well, since a gate whose failure mode is a
    # silently empty report must not depend on a `source` line somebody could forget.
    crap.exec = onStable "cargo run -q -p xtask -- crap";
    check-crap.exec = onStable "cargo run -q -p xtask -- check-crap";

    # The site. `docs` renders to site/ (gitignored); `docs-serve` watches and reloads.
    #
    # `--strict` so a broken link or an unrecognised config key fails rather than warning:
    # mkdocs is happy to publish a page nothing navigates to, and `check-docs` above is what
    # proves the nav and the files on disk agree in both directions.
    # Through pixi's isolated `docs` environment - see the note in flake.nix beside
    # `apps.pixi`. The toolchain is Python and pixi is the one resolver for Python.
    docs.exec = "pixi run --frozen -e docs docs";
    docs-serve.exec = "pixi run --frozen -e docs docs-serve";

    # The cheap structural gates, grouped so CI can run them FIRST: a 1200-line file or a
    # dead dependency should fail in seconds, not after clippy and the test suite.
    hygiene.exec = onStable ''
      set -e
      cargo run -q -p xtask -- hygiene
    '';

    # The finishing sequence. One command, because a checklist in prose is a checklist
    # somebody half-remembers, and because the hooks already encode what has to hold.
    #
    # It judges the COMMITTED branch diff, not the working tree: that is what a reviewer
    # will see. Hence the clean-tree requirement - a dirty tree means the thing being
    # checked is not the thing being proposed.
    ship-check.exec = onStable ''
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
      pixi run --frozen prek run --from-ref "$merge_base" --to-ref HEAD

      echo "== the gates' own unit tests"
      # A gate with no test is a gate nobody has seen fail, and these are the checks
      # everything else is trusted to.
      # NOT `-q`: `cargo-nextest` has no such flag, and the pin rejects it - which broke this
      # whole recipe rather than making it quieter. `--status-level fail` is the flag that
      # means what `-q` was reaching for, and `just test` is where the full output lives.
      cargo nextest run --status-level fail -p xtask --all-features

      echo "== red-before-green for changed tests"
      cargo run -q -p xtask -- test-causality --since "$merge_base"

      echo "== pre-push hooks"
      pixi run --frozen prek run --hook-stage pre-push --from-ref "$merge_base" --to-ref HEAD

      echo "ship-check: green"
    '';

    # Spelled out rather than calling `hygiene`, so this list does not depend on another
    # script being on PATH first. Cheapest first: fail before paying for clippy.
    gates.exec = onStable ''
      set -e
      cargo run -q -p xtask -- hygiene
      cargo run -q -p xtask -- fmt --check
      cargo clippy --workspace --all-targets --all-features -- -D warnings
      cargo nextest run --workspace --all-features
      cargo test --doc --workspace --all-features
      cargo deny check
    '';
  };
}
