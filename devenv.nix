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

  # The shared nix-native Postgres tier - the SAME derivation `checks.nextest` runs in the
  # sandbox. Exposed here so `just test` can start it and run the postgres cells rather than
  # skip them, which keeps one provisioner for both the sandbox and the developer's shell.
  postgresTier = import ./nix/postgres-tier.nix { inherit pkgs; };

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

  # A shell body ShellCheck has read, as a store path.
  #
  # `writeShellApplication`'s checkPhase is `bash -n` PLUS ShellCheck, so a body that does not
  # pass cannot be BUILT - and a devenv script that cannot be built cannot be run. Entering the
  # shell builds the whole profile, so one script's findings are every script's findings.
  #
  # Nothing else in this repository reads shell inside a Nix string. `nix/lint-workflows.sh`
  # globs tracked `*.sh` files, `nix/lint-action-shell.sh` extracts `run:` blocks from composite
  # actions, and the `shellcheck` hook is `files: \.sh$` - a Nix string is none of those.
  # Measured rather than assumed: the derivation devenv built for `ship-check` before this
  # carried `checkPhase = ""`, so those fifty lines had neither ShellCheck nor a syntax check,
  # and the first ShellCheck run over them found a live defect in the dirty-tree refusal.
  #
  # WHY THIS AND NOT AN EXTRACTOR OR A `*.sh` FILE. ShellCheck here reads the RENDERED text -
  # the exact string bash will see - so a Nix antiquotation is already resolved and there is no
  # substitution for a reader to get wrong, which is the hard half of extracting these. And a
  # reader following `just ship-check` still finds the sequence rather than a path to it.
  #
  # WHAT HOLDS A NEW BODY TO IT, because the wrapper alone does not and saying otherwise was the
  # defect. A body assigned as a plain literal beside the wrapped ones builds, runs, and is read
  # by nothing - measured, with ShellCheck findings in it and a green shell. The first version of
  # that rule was two forbidden LITERALS, `.exec = "` and `.exec = ''`, and it was walked past six
  # ways (`github.com/telekom/sutura#402`): no leading dot, two spaces, a newline after the `=`,
  # the `''` form, any attribute that is not `exec` - `enterTest`, and `enterShell` itself - and
  # the same literal in a module `imports` reaches. So what holds it now is
  # `cargo run -q -p xtask -- check-devenv-shell`, which reads this file and every module its
  # `imports` reach, keys on the ATTRIBUTE NAME rather than a spelling, and refuses a body whose
  # value does not begin with one of the wrappers below. `xtask/src/hook_coverage.rs` carries the
  # surface row that makes `just ship-check` reach the other half.
  #
  # AND THE ARGUMENT SET BELOW IS CLOSED, which is the half a textual rule cannot hold. Adding
  # `checkPhase = "true";` here removes `bash -n` AND ShellCheck from EVERY body, and every gate
  # in this repository stayed green over it - #402's seventh escape, and the same shape for
  # `doCheck`, `checkInputs` and `derivationArgs`. `check-devenv-shell` refuses any argument to
  # `writeShellApplication` beyond these three, and `just devenv-linter` reads the resulting
  # derivation's checkPhase out of the store, so the emission is measured rather than assumed.
  #
  # LIMIT, and both halves are stated because the earlier version overstated one. CI does not use
  # this file (see the header), so on a pull request the STRUCTURAL half runs - inside `hygiene`,
  # like every other gate - and the ShellCheck half does not: it needs a built dev shell, which
  # only a developer machine and `just ship-check` have. And these bodies get no `-x`, where the
  # tracked `*.sh` files do (`nix/lint-workflows.sh` passes it): adding it would mean overriding
  # the checkPhase, which is exactly the escape the closed argument set refuses, and the one
  # `source` here is a store path already marked `source=/dev/null`, so `-x` would follow nothing.
  # Neither half reaches beyond the wrappers below: `runCommand` and phase bodies in `nix/` and
  # the `writeShellScript` apps in `flake.nix` get `bash -n` at most.
  #
  # `bashOptions` is passed at each call rather than defaulted: nixpkgs would add `nounset` and
  # `pipefail` on top of the `errexit` these bodies already had, and turning those on changes
  # what they DO rather than what reads them.
  linted = name: bashOptions: text:
    pkgs.writeShellApplication { name = "sutura-${name}"; inherit bashOptions text; };

  # A devenv script: the linted body, invoked with whatever arguments devenv was given.
  runs = name: body: "${linted name [ "errexit" ] body}/bin/sutura-${name} \"$@\"";

  # The same for a body that has to be SOURCED rather than executed - `enterShell` runs in the
  # developer's interactive shell, so a subprocess would export nothing.
  #
  # A wrapper rather than the `''source ${linted ...}''` literal it replaces, and the reason is
  # the gate above: that literal's OUTER shell - the `source` line itself - went through no
  # wrapper, so it was a body nothing read, one line long. Written this way the value begins with
  # a wrapper and the rule needs no exception for it.
  sourced = name: body: "source ${linted name [ ] body}/bin/sutura-${name}";

  # The same, for a gate: on stable, in its own target directory. `nix/stable-env.sh` stays a real
  # shell file so the justfile can source the SAME one - "run this the way CI runs it" is
  # defined once - and the `source=/dev/null` directive is because it is reached here by store
  # path, which ShellCheck cannot follow and reports SC1091 for. That file is linted where it
  # lives, by the `*.sh` glob in `nix/lint-workflows.sh`.
  onStable = name: body: runs name ''
    # shellcheck source=/dev/null
    source ${./nix/stable-env.sh}
    ${body}
  '';
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

    # The nix-native Postgres tier script, shared with `checks.nextex`'s sandbox. `just test`
    # calls `sutura-postgres-tier start|stop` so the postgres corpus and differential cells run
    # in the developer's shell too. Listed here rather than in the `with pkgs` block for the same
    # reason as duckdb: `postgresTier` is a let-binding in this file.
    postgresTier.tier

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

    # The Pulumi CLI for the test-infra stack under `test-infra/pulumi/google`. It is the
    # nix-pinned CLI (nixpkgs pins the version, per the rule that a tool whose version changes
    # what it reports is pinned by nix); the Python SDK it drives lives in pixi's `infra` env.
    # Given here so `pulumi` is on PATH in the dev shell AND reachable by name from the
    # `infra` pixi tasks, and in CI via `nix run .#pulumi`. Note nixpkgs' pulumi may lag the
    # pypi SDK by a patch release; the CLI/SDK pair stays within the 3.x series.
    pulumi

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

  # The nix-pinned pulumi version, exported so the dev shell can see it. The pixi `infra` env's
  # SDK is pinned to this SAME version (see pixi.toml) - nix is the authority for the number,
  # and this env var is the readable witness that the two stay equal.
  env.PULUMI_VERSION = "${pkgs.pulumi.version}";

  # A broken pin should take two seconds to diagnose, not a mid-CI failure.
  #
  # SOURCED from a linted store file rather than left as an inline Nix string, for the reason
  # `linted` states: this is the longest shell body in the file and nothing had ever read it.
  # `sourced` is what carries the `bashOptions = [ ]` that goes with it - `set -e` in the
  # developer's interactive shell would end the session on the first non-zero command - and going
  # through the wrapper rather than gluing a `source` line around it is what makes every character
  # of shell here something ShellCheck read.
  enterShell = sourced "enter-shell" ''
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
    fmt.exec = onStable "fmt" ''
      set -e
      cargo run -q -p xtask -- fmt
      cargo run -q -p xtask -- text-hygiene --fix
    '';
    # `--all-features` is load-bearing rather than foresight: crates here declare features and make
    # dependencies optional, so an entry point missing the flag lints and tests nothing behind
    # them. `grep -rln '^\[features\]' --include=Cargo.toml .` is the current set.
    # `cargo run -q -p xtask -- check-guidance` holds the retired claim, and this file IS in scope.
    lint.exec = onStable "lint" "cargo clippy --workspace --all-targets --all-features -- -D warnings";
    test.exec = onStable "test" "cargo nextest run --workspace --all-features";
    boundaries.exec = onStable "boundaries" "cargo run -q -p xtask -- check-boundaries";
    max-lines.exec = onStable "max-lines" "cargo run -q -p xtask -- max-lines";
    line-endings.exec = onStable "line-endings" "cargo run -q -p xtask -- line-endings";
    check-skills.exec = onStable "check-skills" "cargo run -q -p xtask -- check-skills";
    check-guidance.exec = onStable "check-guidance" "cargo run -q -p xtask -- check-guidance";

    # THE EMISSION, and the one gate that cannot be a flake check: it reads a DERIVATION, so it
    # needs a store `linted` has been built into, and the hygiene sweep runs inside a nix
    # derivation with no nix. A script here rather than a justfile recipe because the store path
    # has to be interpolated by NIX - a recipe cannot name one, and naming one by hand would be a
    # gate over a path instead of over this tree's wrapper.
    #
    # The witness is a body of its own rather than one of the real ones, and that is sound because
    # `linted` is ONE function: `check-devenv-shell` refuses a second application of
    # `writeShellApplication`, so the checkPhase this body got is the checkPhase every body got.
    devenv-linter.exec = onStable "devenv-linter" ''
      cargo run -q -p xtask -- check-devenv-linter "${linted "linter-witness" [ ] "echo linter-witness\n"}"
    '';
    # The whole worktree, not just staged changes: `secrets` is for a sweep, the hook is
    # for a commit.
    secrets.exec = runs "secrets" "betterleaks dir . --config devco/gitleaks.toml --redact --verbose";
    check-docs.exec = onStable "check-docs" "cargo run -q -p xtask -- check-docs";
    unused-deps.exec = onStable "unused-deps" "cargo run -q -p xtask -- unused-deps";

    # The CRAP gate. `onStable` because coverage instrumentation is LLVM-specific and the shell's
    # bare cargo is a cranelift nightly where `-C instrument-coverage` does not exist - so this is
    # not the channel-consistency argument the lints have, it is that the instrumentation is
    # absent. The task hardens its own environment as well, since a gate whose failure mode is a
    # silently empty report must not depend on a `source` line somebody could forget.
    crap.exec = onStable "crap" "cargo run -q -p xtask -- crap";
    check-crap.exec = onStable "check-crap" "cargo run -q -p xtask -- check-crap";

    # The site. `docs` renders to site/ (gitignored); `docs-serve` watches and reloads.
    #
    # `--strict` so a broken link or an unrecognised config key fails rather than warning:
    # mkdocs is happy to publish a page nothing navigates to, and `check-docs` above is what
    # proves the nav and the files on disk agree in both directions.
    # Through pixi's isolated `docs` environment - see the note in flake.nix beside
    # `apps.pixi`. The toolchain is Python and pixi is the one resolver for Python.
    docs.exec = runs "docs" "pixi run --frozen -e docs docs";
    docs-serve.exec = runs "docs-serve" "pixi run --frozen -e docs docs-serve";

    # The cheap structural gates, grouped so CI can run them FIRST: a 1200-line file or a
    # dead dependency should fail in seconds, not after clippy and the test suite.
    hygiene.exec = onStable "hygiene" ''
      set -e
      cargo run -q -p xtask -- hygiene
    '';

    # The finishing sequence. One command, because a checklist in prose is a checklist
    # somebody half-remembers, and because the hooks already encode what has to hold.
    #
    # It judges the COMMITTED branch diff, not the working tree: that is what a reviewer
    # will see. Hence the clean-tree requirement - a dirty tree means the thing being
    # checked is not the thing being proposed.
    #
    # AND IT SAYS WHAT IT DID NOT RUN. `prek` filters every hook by the changed file set, which
    # is the whole reason this is fast enough to run before a push - and on a narrow diff it
    # means almost nothing ran. Measured on a branch whose diff was one workflow file and one
    # README: five of ten commit-stage hooks printed `(no files to check)Skipped` and the last
    # line still said `green`. So both prek runs are captured and handed to
    # `cargo xtask hook-coverage`, which derives the denominator from the hook config rather
    # than from the rows - a hook silenced with `PREK_SKIP` prints no row at all.
    #
    # `--surface-tasks` FIRST, because one surface has no hook: the shell inside a composite
    # action is invisible to `zizmor`, to `actionlint` and to a `*.sh` glob alike. The gate is
    # asked which extra tasks this diff needs, they run, and it is told they did - so the gap is
    # closed rather than reported.
    #
    # `-o pipefail` is not decoration: both prek runs go through `tee`, and without it the
    # pipeline's status is `tee`'s and a red hook run would read as green.
    ship-check.exec = onStable "ship-check" ''
      set -euo pipefail
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
        # ONE backslash, not two. `\\` inside double quotes is a literal backslash and leaves the
        # backtick live, so this line used to open a command substitution IN AN ERROR MESSAGE: bash
        # ran `gates\` and printed "run \ instead", deleting the remedy the sentence exists to name.
        # It survived because nothing had ever read this shell; ShellCheck reports it as SC1073 the
        # moment it does. The escaped form is what `release.yml` uses for the same reason.
        echo "  Commit or stash first. For uncommitted work run \`gates\` instead;" >&2
        echo "  ship-check validates the committed diff from the merge base." >&2
        exit 1
      fi

      merge_base="$(git merge-base "$base" HEAD)"
      echo "ship-check: $merge_base..HEAD"

      logs="$(mktemp -d)"
      trap 'rm -rf "$logs"' EXIT

      # Which surfaces this diff touches that NO hook reaches. One owner: the table is in
      # `xtask/src/hook_coverage.rs`, and this line executes what it is told rather than
      # repeating the globs in shell.
      cargo run -q -p xtask -- hook-coverage --since "$merge_base" --surface-tasks > "$logs/tasks"

      echo "== commit-stage hooks over the branch diff"
      # `--color never` so the captured log is the text the parser was measured against.
      pixi run --frozen prek run --color never --from-ref "$merge_base" --to-ref HEAD 2>&1 | tee "$logs/pre-commit.log"

      echo "== the gates' own unit tests"
      # A gate with no test is a gate nobody has seen fail, and these are the checks
      # everything else is trusted to.
      # NOT `-q`: `cargo-nextest` has no such flag, and the pin rejects it - which broke this
      # whole recipe rather than making it quieter. `--status-level fail` is the flag that
      # means what `-q` was reaching for, and `just test` is where the full output lives.
      cargo nextest run --status-level fail -p xtask --all-features

      echo "== red-before-green for changed tests"
      # EXIT 3 IS "I MEASURED NOTHING" and it is not a failure of this sequence, so `set -e` may
      # not end the run on it: the remaining hooks and surface tasks are what a push still needs.
      # It is not a pass either - both of the gate's inconclusive answers were exit 0 until
      # `github.com/telekom/sutura#307`, which is how a finished branch carried a green causality
      # step over no evidence. So the verdict is RETAINED as the last thing this section prints,
      # which is what the pull request has to state; the gate's own lines above it say which cause
      # and which remedy.
      causality_status=0
      cargo run -q -p xtask -- test-causality --since "$merge_base" || causality_status=$?
      if [ "$causality_status" -eq 3 ]; then
        echo "ship-check: causality was INCONCLUSIVE - this run proves NO red-before-green."
        echo "  State the substitute in the pull request: a mutation run, or this gate scoped"
        echo "  per commit - SHIP_CHECK_BASE_REF, or the just task with a commit as its base."
      elif [ "$causality_status" -ne 0 ]; then
        exit "$causality_status"
      fi

      echo "== pre-push hooks"
      pixi run --frozen prek run --color never --hook-stage pre-push --from-ref "$merge_base" --to-ref HEAD 2>&1 | tee "$logs/pre-push.log"

      # READ INTO AN ARRAY FIRST, never `while read ... done < file`: the loop body invokes a
      # `just` task, and a task that reads standard input would consume the rest of the list -
      # leaving a surface uncovered and, worse, unmentioned, since the gate below is told only
      # about the tasks this loop actually announced.
      mapfile -t extra < "$logs/tasks"
      ran=()
      for task in ''${extra[@]+"''${extra[@]}"}; do
        [ -n "$task" ] || continue
        echo "== $task (no prek hook reaches every surface this diff touches)"
        just "$task"
        ran+=(--ran "$task")
      done

      echo "== what the hooks covered, and what they did not"
      cargo run -q -p xtask -- hook-coverage --since "$merge_base" \
        --log "pre-commit:$logs/pre-commit.log" \
        --log "pre-push:$logs/pre-push.log" \
        ''${ran[@]+"''${ran[@]}"}

      echo "ship-check: green - the coverage lines above are what that covered"
    '';

    # Spelled out rather than calling `hygiene`, so this list does not depend on another
    # script being on PATH first. Cheapest first: fail before paying for clippy.
    gates.exec = onStable "gates" ''
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
