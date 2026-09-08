# The task list, for humans and agents.
#
# One name per job, so documentation cites `just <task>` instead of a command line that drifts
# from the one people run. Recipes are thin on purpose: the work lives in xtask, devenv or the
# flake, and this file only names it.
#
# CI does NOT use these. It runs the flake outputs directly, because a runner has nix and
# nothing else - no just, no devenv. That is the one place a command is written out twice, and
# `cargo xtask check-guidance` is what stops the two from disagreeing.

# Show the tasks. `just` with no argument lands here.
default:
    @just --list --unsorted

# The Pulumi stack the `infra-preview` / `infra-up` tasks operate on. Set SUTURA_PULUMI_STACK in
# the machine env (or pass to the shell) to target a specific stack; defaults to `dev`.
stack := env_var_or_default("SUTURA_PULUMI_STACK", "dev")

# The GitHub environment whose secrets/vars `just infra-set` re-populates from the stack's outputs.
# Defaults to `bq-test` (the acceptance environment); override with BQ_TEST_ENV.
bq_test_env := env_var_or_default("BQ_TEST_ENV", "bq-test")

# It exists because the alternative is a README section people skip, and the failure mode is
# silent - an uninstalled hook does not complain, it just never runs. `prek install` is the
# important line: without it every gate in this file is advisory.

# Everything a fresh clone needs. Idempotent: run it again any time.
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "== git hooks"
    # `core.hooksPath` first, and this is not hypothetical: this repo had it pointing at a
    # `.githooks/` directory that was later deleted, so git looked for hooks in a directory
    # that did not exist AND `prek install` wrote to `.git/hooks` where git was not looking.
    # Every "runs in the hooks" claim in AGENTS.md was false, silently, for the whole session.
    hooks_path="$(git config --local --get core.hooksPath || true)"
    if [ -n "$hooks_path" ] && [ ! -d "$hooks_path" ]; then
      echo "   core.hooksPath points at missing '$hooks_path' - unsetting it"
      git config --local --unset core.hooksPath
    fi
    # All three stages: the commit-msg hook is separate from pre-commit, and pre-push carries
    # the whole-tree gates. Missing one means that stage silently never runs.
    pixi run --frozen hooks-install
    echo "== pixi env (the hook runner and the maintenance interpreter)"
    pixi install --frozen
    echo "== building the dev CLI and the gates"
    # Warms the target directory so the first hook run is not a cold compile, and fails here
    # rather than inside a git hook if something is wrong.
    cargo build -q -p xtask -p sutura-dev
    echo "== checking the environment"
    cargo run -q -p sutura-dev -- doctor
    echo
    echo 'Ready. just lists the tasks; just gates is what CI runs.'

# Bump the pinned inputs.
# Both locks in one task and nothing needs generating: no tool is pinned in both places -
# `cargo xtask check-pins` is what keeps that true - so there is no table to fall out of step.
update:
    #!/usr/bin/env bash
    set -euo pipefail
    nix flake update
    # Needs network. pixi holds only the hook runner and the interpreter, neither of which
    # can change what a gate concludes, so this is a routine bump rather than a gate change.
    pixi update
    cargo update --workspace
    echo
    echo 'Bumped flake.lock, pixi.lock and Cargo.lock. Review the diff before committing.'

# ---------------------------------------------------------------- inner loop ---

# Fast check of the domain crate only. Should stay sub-second.
#
# IT SAYS SO ON THE WAY OUT, and that is the whole point of the three echo lines. Cargo's own
# `Finished dev profile ... in 0.10s` says nothing about scope, so a green run here read as a green
# tree - and a branch whose `sutura-config` had an unclosed delimiter was pushed on the strength of
# it, with `just lint` finding it one step later. The narrowness is deliberate and AGENTS.md pins
# it; what was missing was the sentence, not the coverage.
#
# ECHO RATHER THAN A GATE CALL, on a measurement: this loop is 0.17s warm here and
# `cargo run -q -p xtask -- <anything>` is another 0.49s even fully built, which would triple the
# thing whose whole value is being instant. `cargo xtask check-scope` is what keeps the sentence
# honest instead - it reads this recipe, fails if a package named after `-p` is missing from the
# output, and fails if the task cited below stops existing or stops covering the workspace. So the
# claim cannot drift from the command above it without failing `just hygiene`.
check:
    cargo check -p sutura-domain --no-default-features
    @echo 'check: compiled sutura-domain only, default features off - NOT a workspace check.'
    @echo '  `just check-changed` compiles the packages your working tree actually changes.'
    @echo '  `just lint` is the workspace gate, and `just test` runs the suite.'

# Format Rust, and normalise line endings and whitespace.
#
# On stable, like every gate below. The dev shell's bare `cargo` is nightly so cranelift can
# accelerate the inner loop; rustfmt and clippy differ between channels, and this repo gates
# on the whole clippy `restriction` category, so gating on nightly would produce local
# failures CI cannot reproduce. nix/stable-env.sh also gives stable its own target directory:
# alternating compilers in one directory invalidates every artifact in it.
fmt:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    # `xtask fmt` and NOT `cargo fmt --all`. `--all` reaches path dependencies that are not
    # workspace members, which means it rewrites the vendored allocator - the one thing vendoring
    # must never do. xtask/src/fmt.rs derives the member list and explains it at length.
    cargo run -q -p xtask -- fmt
    cargo run -q -p xtask -- text-hygiene --fix

# `--all-features` is load-bearing: crates declare features and make deps optional, so an entry
# point missing the flag lints and tests nothing behind them. `grep -rln '^\[features\]' --include=Cargo.toml .`
# is the current set, held by review (this file has no extension, outside `check-guidance`'s scope).

# Lint everything.
lint:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# `--doc` is separate because nextest does not run doctests.

# Run the tests.
test:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    # Bring up the SAME nixpkgs Postgres `checks.nextest` runs in the sandbox, so the postgres
    # corpus and differential cells RUN here rather than skip. Through `nix/with-tier.sh` rather
    # than a `start` plus an unconditional `stop` trap, which is what this recipe had and what tore
    # down a tier a developer had started by hand - see that file.
    # shellcheck source=nix/with-tier.sh
    source nix/with-tier.sh
    # `SUTURA_DEV_REQUIRE_TIER` is exported by `sutura_tier_up` when a tier is up, rather than
    # asserted on this line - see that file: two statements about one fact can disagree.
    sutura_tier_up
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features

# The served deployment, asked a question: `crates/sutura-serve/tests/served.rs` writes a settings
# file over `examples/single-player`, starts the composed binary on a kernel-chosen port and asks it.
#
# It IS a gate - `checks.nextest` runs it, because a files-backed source needs no network and no
# credential - so this recipe is a way to run that one target while working on it rather than a
# second tier. Unlike `just bigquery-acceptance`, nothing here is `#[ignore]`d.

# Run the end-to-end suite against the composed serve binary.
serve-e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "serve-e2e: scope sutura-serve - the composed HTTP surface, over a loopback listener."
    echo "serve-e2e: run \`just test\` for the whole workspace's suite; this target is part of it."
    cargo nextest run -p sutura-serve --all-features

# The agent surface, spawned: `crates/sutura-cli/tests/mcp.rs` starts `sutura mcp` over
# `examples/single-player` and speaks the Model Context Protocol on the process's own pipes.
#
# The sibling of `just serve-e2e` one transport over: that one drives the HTTP composition through a
# loopback listener, this one drives the agent composition through the process's own pipes. Also a
# gate - `checks.nextest` runs it, because a pipe needs no network, no port and no credential - so
# this recipe runs that one target while working on it rather than being a second tier. Unlike
# `just bigquery-acceptance`, nothing here is `#[ignore]`d.

# Run the end-to-end suite against the composed agent surface.
mcp-e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "mcp-e2e: scope sutura-cli - the composed agent surface, over the spawned binary's pipes."
    echo "mcp-e2e: run \`just test\` for the whole workspace's suite; this target is part of it."
    cargo nextest run -p sutura-cli --all-features

# The PAGES, run: `crates/sutura-cli/tests/documented.rs` reads every invocation of this binary
# `docs/getting-started.md` and `examples/single-player/README.md` print, runs it from a clone's
# working directory, and holds every refusal block and provenance line they print as output against
# the command in the fence above it.
#
# It exists because the pages drifted: both printed a `Debug` dump `render_refusal` had replaced, so
# the first page a reader is sent to showed output no build had produced. Also a gate -
# `checks.nextest` runs it, because the example needs no network and no credential. A published page
# has to be able to cite the task rather than a raw `cargo` line, which is what this recipe is for.

# Run the suite that runs every command the documentation prints.
documented:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "documented: scope sutura-cli - the pages' own commands, over the spawned binary."
    echo "documented: run \`just test\` for the whole workspace's suite; this target is part of it."
    cargo nextest run -p sutura-cli --all-features

# The DECLARED source, asked a question: `crates/sutura-cli/tests/declared_source.rs` copies the
# example catalog with `source: warehouse` in place of `source: local`, writes a `sources.warehouse`
# entry over the example's own data, and spawns `sutura query` with `SUTURA_CONFIG_DIR` pointing at
# it.
#
# **The one venue that exercises the door `github.com/telekom/sutura#121` is about.** Every case in
# `crates/sutura-cli/src/sources.rs` builds its registry through an overlay, so
# `environment_from_process`, `config_dir_from_process` and the `<dir>/base.yaml` layering are
# reached by nothing else - and `std::env::set_var` is `unsafe` in this edition, so a spawned child is
# what a test can decide the environment of. The same argument `just serve-e2e` makes for its own
# binary. Also a gate: `checks.nextest` runs it, because files need no network and no credential.

# Run the end-to-end suite against a source the deployment declared.
declared-source:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "declared-source: scope sutura-cli - a declared source, over the spawned binary."
    echo "declared-source: run \`just test\` for the whole workspace's suite; this target is part of it."
    cargo nextest run -p sutura-cli --all-features

# `*paths`, not `+paths`, and the no-argument form is the one a PERSON uses: with nothing to go on
# the gate reads the working tree itself, so `just check-changed` answers "does what I have touched
# compile" without anybody having to type a path list. The commit hook keeps passing filenames.
#
# It used to be `+paths`, which made the useful form unreachable from here and left
# `cargo xtask check-changed` printing `no Rust files changed` on a dirty tree - a green line about
# a diff nobody had read. See the doc comment on `run_check_changed`.

# cargo check, narrowed to the packages that changed. No paths reads the working tree.
#
# It sources the helper because it did NOT while the `rust-check-changed` hook entry did: the hook
# ran on stable, and the recipe a person types ran the cranelift nightly in the nightly target
# directory - what that helper exists to refuse. hooks.rs executes this body, so the line is held.
check-changed *paths:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    cargo run -q -p xtask -- check-changed {{ paths }}

# THE gate. Run this before saying a change is done; nothing else counts as verified.
#
# It is `ci` plus the two app-backed checks, and it is deliberately the nix path rather than the
# dev shell. The difference is not speed: a nix check builds its own GIT-DERIVED copy of the tree,
# so it is the only thing that catches a file the build needs and that copy does not have - an
# untracked one, most often. `gates` reads the real tree and cannot see that class of bug at all:
# `include_str!("defaults.yaml")` passed every dev-shell check and failed CI, and a new module left
# untracked does the same. See AGENTS.md on why the source FILTER is not what catches it.
validate:
    #!/usr/bin/env bash
    set -euo pipefail
    # THE SITE BUILD, first, and in TWO steps because two different things can go wrong here and
    # only one of them is a finding about this tree.
    #
    # It is not a nix check, and cannot be: the docs toolchain is Python, pixi is the one resolver
    # for Python here, and a nix sandbox has no network to materialise a pixi environment. Solving
    # mkdocs-material a second time under `python3.withPackages` is what this tree already tried,
    # in flake.nix - mike could not import pymdownx from the mkdocs it subprocessed, two resolvers
    # and one interpreter - so this invokes the ISOLATED docs env rather than widening the linters'.
    #
    # WHY A PIXI STEP IS ALLOWED IN THE ONE RECIPE THAT COUNTS AS VERIFIED. Not because it cannot
    # fail for an environment reason - it can, measured: `.pixi/` absent, an empty package cache
    # and no network gives `failed to fetch ncurses-...conda ... Connection refused`. It is
    # allowed because `nix run .#deny` four lines down ALREADY fails offline - measured on this
    # branch with every proxy pointed at a closed port, `failed to fetch advisory database
    # https://github.com/RustSec/advisory-db ... Failed to connect to github.com:443`, exit 1. So
    # this step adds no network requirement the recipe did not already have.
    #
    # COST, as a range rather than one machine's number: ~2s wall once the env exists (measured
    # 4.0s and 2.0s on two runs, of which mkdocs is ~2.7s), and tens of seconds to materialise it
    # (23s and 51s measured, both with a warm package cache; a cold cache downloads the env and
    # needs a network). So the steady-state tax is seconds and the first run is the expensive one.
    #
    # SEPARATED, because ordering it first would otherwise mean a machine that cannot materialise
    # the env gets NO signal from this recipe at all, where before it got the nix checks and the
    # secret sweep. So: materialise, and if that fails say so in
    # one line, run everything else, and fail at the END. A page that cannot RENDER still aborts
    # immediately, which is the whole point of running it first.
    #
    # What it catches that nothing else here does: `mkdocs build --strict` renders every page, and
    # a page can be CORRECTLY GENERATED and still not render. `check-api-docs` byte-compares the
    # committed pages against a fresh generation, and a generator that emits an unrenderable link
    # consistently passes that byte-compare - which is how github.com/telekom/sutura#352 shipped a
    # page that aborted the site build with every local gate green.
    site=ok
    if pixi install --frozen -e docs; then
        just docs
    else
        site=skipped
        printf '\nvalidate: SKIPPED the site build - the docs env could not be materialised.\n'
        printf '  It needs a network on a cold package cache. Run `just docs` once online.\n'
        printf '  Continuing, and this run will NOT be green.\n\n'
    fi
    just ci
    just secrets
    nix run .#deny
    if [ "$site" != ok ]; then
        printf '\nvalidate: FAILED - every nix check above passed and the site build never ran.\n'
        printf '  Nothing here has rendered a page. See the SKIPPED line above.\n'
        exit 1
    fi
    printf '\nvalidate: ok - the site build, the nix checks, the secret sweep and the supply chain\n'

# What CI runs, through nix, without entering the dev shell. Prefer `just validate`, which adds
# the two checks that need network and therefore cannot be nix checks.
#
# A failed check ends this invocation. An automatic offline retry also retries failed tests,
# so a later pass can hide the failure. Network failures also leave this invocation red;
# diagnose the failure before explicitly rerunning the task.
ci:
    #!/usr/bin/env bash
    set -euo pipefail
    # The system is READ rather than written down. `.#checks.x86_64-linux.*` on an aarch64-darwin
    # host fails "platform mismatch" before it runs anything, and a gate that cannot run on the
    # machine of the person who has to run it is a gate that gets skipped. Set SUTURA_NIX_SYSTEM to
    # force one - a linux builder, or reproducing what a CI log shows.
    system="${SUTURA_NIX_SYSTEM:-$(nix eval --raw --impure --expr builtins.currentSystem)}"
    printf 'checks for %s\n' "$system"
    # api-docs IS in this list, and the omission was not harmless: the committed API pages are
    # byte-compared and no test covers them, so four stale-page incidents were invisible locally
    # while this task was called THE gate. It is a flake check - `nix flake check` ran it all
    # along - but this loop names its checks, so a name left out is a check nobody ran.
    for check in hygiene reuse fmt clippy nextest doctest crap api-docs keycloak-tier postgres-tier; do
        printf '\n=== %s ===\n' "$check"
        nix build ".#checks.$system.$check" -L
    done

# The fat-LTO build. Opt-in, never automatic: minutes of build time for throughput nobody
# has measured yet.
perf:
    nix build .#sutura-performance

# ---------------------------------------------------------------- the gates ---

# One line, because xtask owns the list (`Kind::Hygiene` in its task table). It used to be
# transcribed here, twice in devenv.nix, in flake.nix and as eight hooks - and the order
# differed in three of them. `check-guidance` catches a renamed gate, never a forgotten one.

# The cheap structural gates. Seconds, not minutes.
hygiene:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    cargo run -q -p xtask -- hygiene

# Everything CI runs. What to run before pushing.
gates: hygiene
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    # The tier, for the reason `just test` gives - and this recipe did NOT have it, which is why
    # `just gates` failed the two postgres cells on any machine where nothing else had started a
    # server. It claims to be what CI runs, and CI's `checks.nextest` provisions one.
    # shellcheck source=nix/with-tier.sh
    source nix/with-tier.sh
    sutura_tier_up
    cargo run -q -p xtask -- fmt --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features
    cargo deny check
    # The BYTE-COMPARE half of the attribution gate. Here rather than in `hygiene` because it runs
    # `cargo metadata`, which needs a resolvable registry the nix sandbox has not got - the same
    # reason `check-api-docs` is not a hygiene gate. `check-attribution` in the sweep only sees that
    # a licence cell is non-empty, so without this the main content of a generated file is trusted.
    cargo run -q -p xtask -- check-attribution-current
    # The DEFAULT-feature lane, and it is here for the line above's reason: it shells out to cargo.
    # Every other compiling gate in this repo passes `--all-features`, and `nix/shipped.nix`
    # publishes cargo's default set - so a `#[cfg(feature = ...)]` compiled only with the feature on
    # can be a hard error in exactly the configuration a release builds. That shipped once.
    cargo run -q -p xtask -- check-default-features
    # And the lane's tests, which the line above only COMPILES: `cargo check` and `cargo clippy`
    # both stop at metadata, so every `#[cfg(not(feature = ...))]` test in the tree was compiled
    # here and executed by nothing - each venue that runs a test passes --all-features, where that
    # cfg is false. Two such tests were in that state when this landed.
    cargo run -q -p xtask -- check-default-feature-tests
    bash nix/run-gate.sh crap

# The finishing sequence, over the committed branch diff. Needs a clean tree.
ship-check:
    devenv shell ship-check

# Exactly what `nix build .#checks.x86_64-linux.crap` runs, reached the cheap way. Through
# `nix/run-gate.sh`: configured local stable tools, then the pinned Nix route. Missing stable
# configuration requires Nix; existing optional-tool abstentions remain after configuration.
#
# `source nix/stable-env.sh` because coverage instrumentation is LLVM-specific and the dev
# shell's bare cargo is a cranelift nightly, where `-C instrument-coverage` does not exist.
# `cargo xtask crap` re-establishes it anyway rather than trusting this line.
#
# Every run leaves `target/crap/baseline.json` behind, which is what `just crap-delta` below
# compares. Scope, cost and the two halves of the ratchet are all in docs/crap.md.

# The CRAP score: complexity weighted by the tests that cover it. Scoped to sutura-domain.
crap:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    bash nix/run-gate.sh crap

# Did the CHANGE make anything worse? The other half of the CRAP gate.
#
# TWO FILES, NO COVERAGE RUN. Both sides are baselines an earlier `just crap` wrote, so this is
# file reading and costs nothing. Locally that means: run `just crap` on the base commit, copy
# `target/crap/baseline.json` somewhere, come back to the branch, run `just crap` again, then
# point this at the two files.
#
# CI does not do it that way and does not need to. There the base's baseline was computed when
# the base commit was built and has been sitting in the `crap-baseline` artifact since; the
# workflow resolves the artifact for the pull request's exact merge base, and when no such
# artifact exists it warns and skips rather than comparing against a baseline from a different
# commit. See docs/crap.md and .github/workflows/ci.yml.
#
# NO `gh run download` HERE, deliberately. A recipe that fetched a baseline would need the
# GitHub CLI, an authenticated token and network, and would then silently disagree with CI about
# which commit the baseline came from. Two paths in, no guessing.

# `just crap-delta <base-baseline.json> [head-baseline.json]`
crap-delta base head="target/crap/baseline.json":
    cargo run -q -p xtask -- crap-delta --baseline {{ base }} --head {{ head }}

# What a diff requires. `just classify origin/main`
classify base="origin/main":
    cargo run -q -p xtask -- classify --since {{ base }}

# Red-before-green for changed tests. `just causality origin/main`
#
# ON STABLE, and that is a bug fix rather than consistency. This gate RUNS THE SUITE - twice - so it
# inherits every difference between the channels, and the dev shell's bare `cargo` is nightly for the
# cranelift backend. One test in `sutura-runtime` behaves differently under the two: it installs a
# panic hook and asserts on what the hook logged, and under nightly it fails while `just test` on
# stable passes it. So this gate was red on every branch, for a reason that had nothing to do with any
# of them, and the failure looked exactly like the one it exists to report.
#
# The rule `AGENTS.md` states for `clippy` - never conclude a branch is red from a bare `cargo` line -
# applies to any gate that runs the compiler, and this recipe is now what makes it hold here.
causality base="origin/main":
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    # The SAME tier `test` provisions, for the same reason and it was missing here: this gate's
    # first step is *are the tests green on HEAD*, and the postgres corpus and differential cells
    # are fail-closed - so without the tier the gate fails its own precondition and reports nothing
    # about the change. Measured on a real run before this pair was added. Through
    # `nix/with-tier.sh`, so a gate that takes minutes does not stop a server it did not start.
    # shellcheck source=nix/with-tier.sh
    source nix/with-tier.sh
    sutura_tier_up
    cargo run -q -p xtask -- test-causality --since {{ base }}

# ---------------------------------------------------------------- artifacts ---

# The release binaries: the command-line tool and the server.
#
# BOTH, because both are published. Before `github.com/telekom/sutura#111` this recipe built one
# binary and so did the release, which is why nothing a release published could serve a question.
# `nix/shipped.nix` is the list; a recipe that builds a subset of it is a recipe that says a green
# local run means the release will build.
build:
    nix build .#sutura
    nix build .#sutura-serve

# The release images: one binary each, no shell, no package manager.
image:
    nix build .#oci
    nix build .#oci-serve

# Cross-build every shipped artifact: two binaries at four triples.
#
# All four triples, not the two glibc ones: the musl targets are statically linked and swap in
# mimalloc, so they are a genuinely different build - a cross target has its own deps derivation
# and its own C compile. A recipe that skipped them would let a developer pass `build-all` locally
# and still break the release.
build-all:
    nix build .#sutura-x86_64-unknown-linux-gnu
    nix build .#sutura-aarch64-unknown-linux-gnu
    nix build .#sutura-x86_64-unknown-linux-musl
    nix build .#sutura-aarch64-unknown-linux-musl
    nix build .#sutura-serve-x86_64-unknown-linux-gnu
    nix build .#sutura-serve-aarch64-unknown-linux-gnu
    nix build .#sutura-serve-x86_64-unknown-linux-musl
    nix build .#sutura-serve-aarch64-unknown-linux-musl

# What a release asserts about the artifacts it publishes, without publishing anything.
#
# Two checks and they answer different questions. `one-binary` reads each shipped package's `bin/`
# and its runtime closure: one executable, named what the image entrypoint expects, and no
# toolchain baked in. `shipped-features` reads the `cargo auditable` section out of each native
# binary and asserts the feature set `nix/shipped.nix` declares - `axum` present in the server,
# `ring` and `ureq` absent from both.
#
# NOT in `just ci`, deliberately, and the same reason `one-binary` never was: these build the
# release-profile artifacts, so they are minutes rather than seconds. The tag-triggered release
# workflow runs them before publishing.
shipped:
    #!/usr/bin/env bash
    set -euo pipefail
    system="${SUTURA_NIX_SYSTEM:-$(nix eval --raw --impure --expr builtins.currentSystem)}"
    for check in one-binary shipped-features; do
        printf '\n=== %s ===\n' "$check"
        nix build ".#checks.$system.$check" -L
    done

# Through nix: git-cliff's version is part of what it produces, so nix is its only pin and
# `cargo xtask check-pins` fails if it reappears in pixi.toml. Docs go through pixi because
# that toolchain is Python; a tool whose output is the verdict goes through nix.
#
# No argument rewrites CHANGELOG.md with the unreleased section current - the same render
# version-bump.yml commits on every push to main. With a tag it renders that section AS the
# release instead, which is what the release commit carries and what release.yml puts in the
# GitHub Release body. Neither form tags anything or pushes anything.

# Render CHANGELOG.md as CI does. `just changelog v0.2.0` renders it as that release.
changelog tag="":
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -n "{{ tag }}" ]; then
      nix run .#git-cliff -- --tag "{{ tag }}" -o CHANGELOG.md
    else
      nix run .#git-cliff -- -o CHANGELOG.md
    fi
    git --no-pager diff --stat -- CHANGELOG.md

# --------------------------------------------------------------------- docs ---

# Render the site to site/.
#
# Through pixi's ISOLATED `docs` environment: the docs toolchain is Python, and pixi is the one
# resolver for Python here. It lived in flake.nix as a `python3.withPackages` first, where mike
# could not import pymdownx from the mkdocs it subprocessed - two resolvers, one interpreter.
# The env is isolated so mkdocs-material's dependency tree cannot perturb the linters' solve.
docs:
    pixi run --frozen -e docs docs

# Serve the site with live reload.
docs-serve:
    pixi run --frozen -e docs docs-serve

# Publish one version to gh-pages. CI does this on push and on a tag; this reproduces it
# locally, and `--push` is deliberately absent so a local run cannot publish by accident.
docs-deploy version="local":
    pixi run --frozen -e docs docs-deploy {{ version }}

# What is published, per mike.
docs-list:
    pixi run --frozen -e docs docs-list

# Nightly on purpose: `--output-format json` is an unstable rustdoc option, and the dev shell's
# bare `cargo` is the nightly pin. Every other gate sources nix/stable-env.sh; this one must NOT,
# because stable rejects `-Z` outright - so wrapping it the way the others are wrapped is the one
# thing that breaks it.
#
# `pixi run --frozen python` is the DEFAULT pixi environment, not the `docs` one: the renderer is
# a plain stdlib script, and the docs environment exists to keep mkdocs-material's dependency
# tree away from everything else. It takes an output directory as an optional last argument and
# defaults to `docs/api`.

# Regenerate the committed API reference pages from rustdoc JSON. Through nix, so it needs no
# dev shell: it called a bare `cargo` and a bare `pixi` and only worked where one was active.
api:
    nix run .#api-docs

# Shellcheck the shell inside every local composite action. `actionlint` CANNOT READ a composite
# action - measured against 1.7.12, it parses `action.yml` as a workflow and rejects it - and the
# shellcheck pass in CI globs `*.sh`, which a `run:` block is not. So the release path's own signing
# sequence was shell nothing had ever linted. `nix/lint-action-shell.sh` is the sequence, shared with
# `nix/lint-workflows.sh` so a local run and CI cannot check different things.
lint-actions:
    bash nix/lint-action-shell.sh

# Every static check that reads a workflow, an action or a shell script: zizmor, actionlint,
# shellcheck over the tree, and the composite-action pass above. What `ci.yml` runs, reached the
# cheap way - it used to be inline there and had no local caller at all.
lint-workflows:
    bash nix/lint-workflows.sh

# The shell the line above cannot reach: the script bodies inside `devenv.nix`. They are Nix
# strings, so the `*.sh` glob, zizmor and the composite-action reader all filter them out - and
# `xtask/src/hook_coverage.rs` carries that as a surface with an EMPTY hook set, which is what
# makes `just ship-check` run this recipe when a diff touches the file.
#
# What it asserts is the EMISSION rather than a spelling: `devenv.nix`'s `linted` wraps each body
# in `writeShellApplication`, and this reads the resulting derivation's `checkPhase` out of the
# store and requires `bash -n` plus a store-path shellcheck in it. `checkPhase = "true";` in that
# wrapper removes both from every body, and it left every other gate in this repository green -
# `github.com/telekom/sutura#402`. `cargo xtask check-devenv-shell`, inside `hygiene`, is the
# structural half and the one CI runs.
#
# THROUGH `devenv shell`, because the store path is interpolated by nix at the call site: a
# justfile cannot name one, and a hand-written path would be a gate over a path rather than over
# this tree's wrapper.
devenv-linter:
    devenv shell devenv-linter

# Regenerate the committed attribution document from `cargo metadata`. `ATTRIBUTION.md` is the
# statement a distributor hands on - every third-party crate this workspace resolves and the licence
# it declares - and `cargo xtask check-attribution` is the gate that fails when it falls behind the
# lock. Through a bare `cargo` rather than nix, because `cargo metadata` is the tool and it needs the
# workspace's own resolver, not a pinned binary.
attribution:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    cargo run -q -p xtask -- attribution

# Is every file's licence answerable by a tool? `checks.reuse` is what CI runs and is the
# authority; this reaches the same pin the cheap way, against the REAL tree rather than the
# git-derived copy - so it also sees a file you have not staged yet, which the check cannot.
licences:
    nix run .#reuse -- lint

# Fuzz every target, or one, with a time budget. NOT A GATE: a run that fails a merge on a fresh
# random path gets turned off, and then nothing generates input. `nix/fuzz.nix` argues the
# provisioning; each `fuzz/fuzz_targets/*.rs` header says what it covers and what it does not.
fuzz seconds="300" target="":
    bash nix/run-fuzz.sh run "{{ seconds }}" "{{ target }}"

# Replay every committed corpus seed, mutating nothing - the regression half of fuzzing.
fuzz-smoke:
    bash nix/run-fuzz.sh smoke

# ------------------------------------------------------------------ tooling ---

# Scan the whole worktree for secrets. The hook already covers each commit.
secrets:
    betterleaks dir . --config devco/gitleaks.toml --redact --verbose

# Static analysis of the workflows.
#
# Through nix, the only pin for it. These three tools report findings, so their version is
# part of the verdict - `cargo xtask check-pins` fails if any of them reappears in pixi.toml.
zizmor:
    nix run .#zizmor -- .github/workflows

# Everything zizmor's default persona leaves out. Stylistic, so not a gate.
zizmor-pedantic:
    nix run .#zizmor -- --persona pedantic .github/workflows

# A GLOB, not a list. It was two files here and one in ci.yml, under a comment in that file
# claiming "every shell script we ship... the glob is the list" - true of neither.
# `nix/run-gate.sh` was in neither, and that file decides whether the tests, the secret sweep,
# the supply-chain gate and the CRAP score run at all: a `set -eu` slip there turns four gates
# into silent no-ops. The pre-commit `shellcheck` hook covers the same set from the staged side.

# Lint the workflows and every shell script we ship.
lint-ci:
    #!/usr/bin/env bash
    set -euo pipefail
    nix run .#actionlint
    # `-x` follows `source` directives, which is how a sourced-only file gets judged too - and
    # without it a script that sources another fails SC1091 even with a `# shellcheck source=`
    # directive, which is what the flag exists to honour.
    # `find`, not a `**` glob: this recipe runs under `sh` on some hosts, where globstar is off.
    mapfile -t scripts < <(find . -name '*.sh' -not -path './.git/*' -not -path './target/*' \
      -not -path './.devenv/*' -not -path './.direnv/*' -not -path './.pixi/*' \
      -not -path './site/*' -not -path './vendor/*' | sort)
    printf 'shellcheck: %d script(s)\n' "${#scripts[@]}"
    # An empty list would pass by checking nothing, which is the failure mode a glob-driven
    # check is most prone to.
    test "${#scripts[@]}" -gt 0
    nix run .#shellcheck -- -x "${scripts[@]}"

# The cheap, text-only half of the citation gate. Seconds, and no compiler.
#
# It exists because `docs.yml`'s verify job skips the 15m45s `hygiene` build for a pull request
# that changes only markdown under `docs/`, and a citation of a task that does not exist was the
# one property that skip could not defer: deferring it to the `main` push blocks the publish
# instead of the merge. Same script CI runs, same two authorities - this recipe's own name
# included, since the list comes from `just --summary` rather than from a copy.
citations:
    #!/usr/bin/env bash
    set -euo pipefail
    just --summary | sh .github/scripts/check-task-citations.sh

# Refresh every imported skill and rewrite the lock.
skills-refresh:
    pixi run --frozen skills-refresh

# Re-hash imported skills without fetching, for a network with no egress.
skills-relock:
    pixi run --frozen skills-relock

# Run the hooks over everything. Scope with `just hooks --files <path>` while iterating.
hooks *args:
    pixi run --frozen prek run {{ args }}

# Through pixi's ISOLATED `gcloud` environment, which holds a task and no packages: the Google
# Cloud CLI is reached as a pinned container rather than as a conda dependency, because
# conda-forge has no `win-64` build of it and this workspace declares that platform. pixi.toml
# carries the argument and the two variables a developer can set.
#
# It is INTERACTIVE - `docker run -it` - so it needs a real terminal and cannot be part of any
# gate. Nothing is written into this repository: both logins land in the developer's own gcloud
# configuration directory, where a native `gcloud`, `bq` or a client library already looks.

# Authenticate against Google Cloud, for the BigQuery work. Both logins, in a container.
gcloud-login:
    pixi run --frozen -e gcloud gl

# ------------------------------------------------------------------ test-infra ---
# The Pulumi test infrastructure under `test-infra/`. `infra-` prefix keeps these out of
# the way of the Rust surface's tasks; everything runs through pixi (the `infra` env
# owns Python + pulumi + pulumi-gcp, the `gcloud` env owns the login), so there is no
# venv and no requirements.txt to keep in step - pixi owns the interpreter and everything
# in it, the same reasoning the `docs` env uses.

# Authenticate against Google Cloud for the Pulumi provider. Maps to pixi's `gl` task,
# which runs BOTH gcloud logins in one container into ~/.config/gcloud:
#   * `gcloud auth login` - the CLI's OWN credentials (credentials.db);
#   * `gcloud auth application-default login` - ADC, what client libraries (the Pulumi
#     provider, Google SDKs) read from ~/.config/gcloud/application_default_credentials.json.
# `< -e gcloud` is required because pixi declares `gl` in the `gcloud` FEATURE's tasks, not
# in the default environment. Interactive (`-it`); not in any gate; nothing is written into
# this repository. This is the same task as `gcloud-login` above.
infra-gl:
    pixi run --frozen -e gcloud gl

# `pulumi preview` over the `test-infra/pulumi/google` stack, through the `infra` pixi env.
# The pixi task carries its own `cwd` (see pixi.toml), so no `cd` is needed here and the task
# is correct from any directory. Extra flags (e.g. `--stack dev`) flow through as appended args.
#
# The state backend is a PROJECT-LOCAL file (`PULUMI_BACKEND_URL=file://.../test-infra/pulumi/google`,
# into which pulumi writes a gitignored `.pulumi/`)
# and the stack-secrets passphrase comes from `PULUMI_CONFIG_PASSPHRASE` (set in the machine's
# `~/.config/sutura/env.sh` locally; a secret in CI).
# The stack is configured FROM THE ENVIRONMENT FIRST (config-from-env.sh maps SUTURA_GOOGLE_* onto
# the SUTURA_PULUMI_STACK stack, refusing on anything missing), and the SAME stack name is passed to
# pulumi - no reliance on an "active" selection, which a fresh shell does not have. The stack name
# defaults to `dev` and is overridden by SUTURA_PULUMI_STACK (the machine env sets it to the
# developer's own). Nothing here reaches pulumi cloud, and nothing is committed.
infra-preview *flags:
    test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || (echo "infra: set PULUMI_CONFIG_PASSPHRASE (machine env or secret)" >&2 && exit 1)
    # The infra run identity is the developer's gcloud ADC (their own elevated account), NOT the
    # limited BigQuery SA key that env.sh points the acceptance legs at. GOOGLE_ADC overrides the
    # default ADC path. just runs each line in a fresh shell, so the identity is set per command.
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" bash {{ justfile_directory() }}/test-infra/pulumi/google/config-from-env.sh --stack "{{stack}}"
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" pixi run -e infra preview --stack "{{stack}}" {{flags}}

# `pulumi up` - apply the stack. Sample/verify before applying: `just infra-preview`.
infra-up *flags:
    test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || (echo "infra: set PULUMI_CONFIG_PASSPHRASE (machine env or secret)" >&2 && exit 1)
    # See infra-preview: run as the developer's gcloud ADC, not the limited BigQuery SA key.
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" bash {{ justfile_directory() }}/test-infra/pulumi/google/config-from-env.sh --stack "{{stack}}"
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" pixi run -e infra up --stack "{{stack}}" {{flags}}

# `pulumi destroy` - tear down the Google test infra this stack created, so a redeployment starts
# clean. Same local-file backend and developer-ADC identity as infra-up, but NO config-from-env.sh:
# destroying reads the existing state and needs none of the SUTURA_* values. GCP APIs stay ENABLED
# (the Service resources set disable_on_destroy=False), deliberately - GCP refuses to disable some
# APIs that still hold resources, and re-enabling is slower than leaving it. Run `just infra-preview`
# and then `just infra-up` to re-create afterwards.
infra-down:
    test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || (echo "infra: set PULUMI_CONFIG_PASSPHRASE (machine env or secret)" >&2 && exit 1)
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" pixi run -e infra destroy --stack "{{stack}}" --yes

# Re-export the fresh `up` outputs into the {{bq_test_env}} GitHub environment's secrets/vars, so
# CI's acceptance key + resource names follow the stack without hand-editing. Run after an
# `infra-up` (especially one following an `infra-down`, which rotates every key). Reads the local
# `.pulumi/` state (no GCP credential needed) and pushes via `gh`, which must be authenticated.
infra-set:
    test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || (echo "infra: set PULUMI_CONFIG_PASSPHRASE (machine env or secret)" >&2 && exit 1)
    STACK="{{stack}}" BQ_TEST_ENV="{{bq_test_env}}" PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" bash {{ justfile_directory() }}/test-infra/pulumi/google/sync-bq-test-env.sh

# Live BigQuery acceptance is outside `just validate`: nix checks have no network. CI runs it in `bq-test` for pushes and same-repository PRs, not forks without secrets.
# The ignored acceptance/corpus cells run by binary selection; missing inputs fail. Separate principal, exchange and cross-resource venues are excluded by binary.
# The dataset is shared with CI. Per-run table suffixes isolate fixture reads and drops; 24-hour expiration bounds cancelled-run leftovers. Printed names identify the run.
# Run `just gcloud-login` first and supply the test headers' environment inputs. Keep resource values in the invoking environment, never in this repository.

# Run the live BigQuery acceptance suite against the configured project.
bigquery-acceptance:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "bigquery-acceptance: scope sutura-exec-bigquery - the acceptance leg only, against a real project."
    echo "bigquery-acceptance: this is NOT a gate. Run \`just test\` for the whole workspace's suite."
    echo "bigquery-acceptance: CI runs the same leg through \`nix run .#bigquery-acceptance\`, in its own job."
    # Keep this filter aligned with apps.bigquery-acceptance; neither derives the other (#430).
    # Identity and cross-resource venues must not run under the ordinary acceptance name.
    cargo nextest run -p sutura-exec-bigquery --all-features --run-ignored only \
      -E 'not binary(two_principals) and not binary(exchanged_identity) and not binary(cross_resource)'

# One shared credential, two disposable datasets in its billing project. Not run by ordinary acceptance.
bigquery-cross-dataset:
    #!/usr/bin/env bash
    set -euo pipefail
    bash nix/mask-bigquery-resources.sh
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "bigquery-cross-dataset: explicit writable fixture venue; not a local gate or identity proof."
    echo 'scope: sutura-exec-bigquery; run `just test` for the whole workspace.'
    cargo nextest run -p sutura-exec-bigquery --all-features --run-ignored only \
      -E 'binary(cross_resource) and test(join_across_datasets_)'

# Read-only preprovisioned mirrors, with no fixture creation or changed billing semantics.
bigquery-cross-project:
    #!/usr/bin/env bash
    set -euo pipefail
    bash nix/mask-bigquery-resources.sh
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "bigquery-cross-project: explicit read-only mirror venue; not provisioning or identity proof."
    echo 'scope: sutura-exec-bigquery; run `just test` for the whole workspace.'
    cargo nextest run -p sutura-exec-bigquery --all-features --run-ignored only \
      -E 'binary(cross_resource) and test(join_across_projects_)'

# The two-principal cell: one statement, two principals, two row sets. `docs/adr/0017`'s eighth
# amendment and issue #123.
#
# **Its own task rather than a third leg above, and the reason is a developer's.** It needs five
# values and two key documents the other two legs do not, so a single task demanding all of them
# would make the legs somebody CAN run unreachable. The filter is on the BINARY and not on a test
# list, so both tasks still reach every `#[ignore]`d test in their own target without a count here.
#
# **What a green run here does NOT mean** is the first thing `tests/two_principals.rs` says: the two
# principals are service accounts whose keys this leg holds, so it is leg 2's source half and not
# leg 2. `docs/where-identity-is-proven.md` is the map.

# Run the two-principal BigQuery cell against the configured row-access-policied dataset.
bigquery-two-principals:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "bigquery-two-principals: scope sutura-exec-bigquery - two principals, one statement, one row access policy."
    echo "bigquery-two-principals: this is NOT a gate. Run \`just test\` for the whole workspace's suite."
    echo "bigquery-two-principals: CI runs it through \`nix run .#bigquery-two-principals\`, in the bq-test job."
    cargo nextest run -p sutura-exec-bigquery --all-features --run-ignored only -E 'binary(two_principals)'

# Run the exchanged-identity cell: one workload identity, exchanged per subject, against SESSION_USER().
#
# The only BigQuery leg that holds no principal's key - which is what separates impersonation from
# credential selection, and the whole reason it is a cell of its own. NO WORKFLOW INVOKES IT: two of
# the five values it is pointed at are not in the `bq-test` environment, and
# `crates/sutura-exec-bigquery/tests/exchanged_identity.rs` carries why they cannot be derived from
# the CI workload identity with the exchange this adapter ships. **It has never run.**
bigquery-exchanged-identity:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "bigquery-exchanged-identity: scope sutura-exec-bigquery - one workload identity, exchanged per subject."
    echo "bigquery-exchanged-identity: this is NOT a gate. Run \`just test\` for the whole workspace's suite."
    echo "bigquery-exchanged-identity: no workflow runs it - see the test file's header for what is missing."
    cargo nextest run -p sutura-exec-bigquery --all-features --run-ignored only -E 'binary(exchanged_identity)'

# ------------------------------------------------------------------ dev flow ---

# This worktree's service ports and compose project.
ports:
    cargo run -q -p sutura-dev -- ports

# An isolated worktree for a stacked change. `just worktree feat/thing`
worktree branch:
    cargo run -q -p sutura-dev -- worktree create {{ branch }}

# Every worktree and its scope.
worktrees:
    cargo run -q -p sutura-dev -- worktree list

# What has landed and could go: local branches, and the worktrees holding them. DELETES NOTHING.
#
# Dry run is the default, because a cleanup task that deletes on a bare invocation is one nobody
# runs twice. `git branch --merged` is deliberately not the mechanism: it cannot see a
# squash-merged branch at all, because the branch's commits are not ancestors of the commit
# carrying their content - so where everything is squash-merged its answer is useless. What decides
# instead is patch-id equivalence against the default branch, a merged pull request whose recorded
# head is still this branch's tip, and an optional age bound. A reason is printed for every branch,
# kept ones included, and anything undetermined keeps the branch.
#
# `just clean-branches --delete` applies the plan. The other flags are `--unused-days <n>`,
# `--fetch` and `--repo <path>`; there is none that overrides a refusal.
clean-branches *args:
    cargo run -q -p xtask -- clean-branches {{ args }}

# What is present, what is missing, what would fail.
doctor:
    cargo run -q -p sutura-dev -- doctor

# The identity provider's nix-native tier, by hand: `just keycloak-tier start|stop|status`.
#
# NOT a step anybody has to remember before committing, and that is deliberate - no gate needs it.
# `checks.keycloak-tier` provisions and tears down its own instance in the sandbox, which is where
# the property that no human is needed is actually held. This task is for looking at a real issuer:
# `start` writes the port into `.sutura-dev/endpoints.json` and the realm, client and two subjects
# into `.sutura-dev/keycloak-realm.json`, both generated per start and removed by `stop`.
#
# Through `nix run` rather than a dev-shell package, so a contributor who never touches identity
# does not fetch a JVM and a 186 MB server on `nix develop`. See `apps.keycloak-tier` in flake.nix.
keycloak-tier *args:
    nix run .#keycloak-tier -- {{ args }}

# The Postgres tier, by hand: `just postgres-tier start|stop|status|credentials`.
#
# `credentials` prints the three `export` lines the adapter needs and refuses if nothing here is
# provisioned - `github.com/telekom/sutura#455`: `FixtureCredential::from_env` has no fallback, so
# the provisioner is what says how to log in. `just test` evaluates them for you through
# `nix/with-tier.sh`; type it yourself only to point a bare `cargo nextest` at a tier you started
# here.
#
# The same shape as `just keycloak-tier` and it exists for the same reason: a remedy that names the
# venue for a missing service has to name a task a reader can type, and `nix/postgres-tier.nix` had
# none - so `sutura_dev::provisioned` cited `just test` for every nix-native tier, which provisions
# THIS one and no other. See that module; the correspondence is held by its recipe scan.
#
# NOT a step before committing. `just test` brings the tier up through `nix/with-tier.sh` and tears
# down only what it started, so a tier started here survives a suite run - which is what makes the
# remedy's second line (`just dev-endpoint postgres`) true afterwards.
#
# Straight to the script rather than through `nix run`: it is in the dev shell already, which is
# where `nix/with-tier.sh` looks for it. `checks.nextest` runs the same one from the same file.
postgres-tier *args:
    sutura-postgres-tier {{ args }}

# ------------------------------------------------------- the compose tier ---
#
# One independent service instance per worktree, provisioned through xtask rather than through the
# shipped binary: docker orchestration inside a release artifact is test scaffolding delivered to
# users, and xtask is never packaged.
#
# NOT a nix check, and it cannot be one - the sandbox has no network and no docker socket. So these
# are just tasks and a CI job over nix-built artifacts.
#
# A missing docker SKIPS here and FAILS in CI. Both directions come from one flag: export
# SUTURA_DEV_REQUIRE_TIER=1 to get the CI direction on this machine, or =0 to get this one there.
# (`just test` provisions Postgres from nix itself, so Postgres is not a dev-up service - see
# nix/postgres-tier.nix.)

# This worktree's services, on ports docker allocates, with a discovery file a harness reads.
dev-up:
    cargo run -q -p xtask -- dev-up

# The same, plus the identity provider. Off by default because nothing here can use one yet:
# `CredentialBroker` does not exist. It is no longer the slowest thing in this tier - the DataHub
# stack below is - but that was never the argument for the profile. The reasoning lives beside the
# service in compose.services.yaml.
dev-up-identity:
    cargo run -q -p xtask -- dev-up --with identity

# The same, plus the DataHub metadata platform: five containers, of which one - `datahub`, its GMS -
# is the endpoint the discovery file carries. Off by default because it COSTS: three JVMs and a
# migration job that creates the topics, the schema and the indices before GMS will start. This is
# what `crates/sutura-catalog-datahub` is read against when a test wants a real instance rather than
# the recorded fixture; the reasoning lives beside the stack in compose.services.yaml.
dev-up-datahub:
    cargo run -q -p xtask -- dev-up --with datahub

# The provisioned DataHub, asked whether it can carry the deployment-defined metric document.
#
# A named task rather than a cell in the default suite, and NOT because a network is missing - the
# `bigquery-acceptance` shape for a different reason. `.sutura-dev/endpoints.json` has two writers,
# and ONE HALF OF THE CLOBBERING IS NOW GONE: every nix-native tier writes through
# `nix/tier-endpoints.nix`, which MERGES its own service into the file, so `sutura-postgres-tier
# start` no longer leaves `postgres` as the only entry. The other half stands - `xtask dev-up` goes
# through `sutura_dev::discovery::publish`, which serialises the whole document from the docker
# services it just read, so a `dev-up` after a nix tier still drops the nix entry and the server it
# named goes on running unnamed. For POSTGRES that no longer blocks a suite run: `nix/with-tier.sh`
# reads a running-but-unpublished tier as its own state and republishes the entry (#298), which
# `checks.postgres-tier` holds. For any other nix tier it stands whole, because nothing sources a
# wrapper for one - and the wholesale write itself is gated by nothing either way. The remaining
# half is recorded in `crates/sutura-catalog-datahub/tests/provisioned.rs` rather than papered over
# here.
#
# It brings the profile up first, because a task that asked for the fail-closed direction against a
# tier nobody started would just be a confusing way to spell an error.
datahub-acceptance:
    #!/usr/bin/env bash
    set -euo pipefail
    # shellcheck source=nix/stable-env.sh
    source nix/stable-env.sh
    echo "datahub-acceptance: scope sutura-catalog-datahub - one target, two cells: the instance is"
    echo "datahub-acceptance: reachable, and a document written under a property THE DEPLOYMENT names"
    echo "datahub-acceptance: comes back and decodes into a certified metric. There is no HTTP"
    echo "datahub-acceptance: AspectReader, so this is NOT a read path - the requests and the mapping"
    echo "datahub-acceptance: onto the adapter's shape are in the test, not in src/."
    echo "datahub-acceptance: run \`just test\` for the whole workspace's suite; this target is NOT part of it."
    cargo run -q -p xtask -- dev-up --with datahub
    SUTURA_DEV_REQUIRE_TIER=1 cargo test -p sutura-catalog-datahub --test provisioned -- --ignored --nocapture

# Where this worktree's services are listening. The only way to learn it - there is no constant.
dev-endpoints:
    cargo run -q -p xtask -- dev-endpoints

# One service's host:port, on stdout and nothing else, so a shell can substitute it:
# `PORT="${$(just dev-endpoint clickhouse)##*:}"`. Anyone following `examples/` uses this instead
# of learning what a scope or an ephemeral port is. `just dev-endpoints` is the readable table.
@dev-endpoint service:
    cargo run -q -p xtask -- dev-endpoint {{ service }}

# Remove this worktree's services, its network and its named volumes. Nothing else, ever.
dev-down:
    cargo run -q -p xtask -- dev-down

# What `just dev-down` would remove, and what it would deliberately spare. Removes nothing.
dev-down-dry:
    cargo run -q -p xtask -- dev-down --dry-run
