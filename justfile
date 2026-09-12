# The task list, for humans and agents. One name per job, so docs cite `just <task>` rather than a
# command line that drifts; recipes stay thin - the work lives in xtask, devenv or the flake.
#
# CI does NOT use these: a runner has nix and nothing else. That is the one place a command is
# written twice, and `cargo xtask check-guidance` stops the two disagreeing.

# Show the tasks. `just` with no argument lands here.
default:
    @just --list --unsorted

# The Pulumi stack the `infra-preview` / `infra-up` tasks operate on. Set SUTURA_PULUMI_STACK in
# the machine env (or pass to the shell) to target a specific stack; defaults to `dev`.
stack := env_var_or_default("SUTURA_PULUMI_STACK", "dev")

# The GitHub environment whose secrets/vars `just infra-set` re-populates from the stack's outputs.
# Defaults to `bq-test` (the acceptance environment); override with BQ_TEST_ENV.
bq_test_env := env_var_or_default("BQ_TEST_ENV", "bq-test")

# `prek install` is the important line: an uninstalled hook does not complain, it never runs, so
# without it every gate in this file is advisory.

# Everything a fresh clone needs. Idempotent: run it again any time.
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "== git hooks"
    # `core.hooksPath` first: this repo had it pointing at a deleted `.githooks/` while `prek install`
    # wrote to `.git/hooks`, so every "runs in the hooks" claim was silently false for a whole session.
    hooks_path="$(git config --local --get core.hooksPath || true)"
    if [ -n "$hooks_path" ] && [ ! -d "$hooks_path" ]; then
      echo "   core.hooksPath points at missing '$hooks_path' - unsetting it"
      git config --local --unset core.hooksPath
    fi
    # All three stages: commit-msg is separate from pre-commit, and pre-push carries whole-tree gates.
    pixi run --frozen hooks-install
    echo "== pixi env (the hook runner and the maintenance interpreter)"
    pixi install --frozen
    echo "== building the dev CLI and the gates"
    # Warms the target dir so the first hook run is not a cold compile, and fails here, not in a hook.
    cargo build -q -p xtask -p sutura-dev
    echo "== checking the environment"
    cargo run -q -p sutura-dev -- doctor
    echo
    echo 'Ready. just lists the tasks; just gates is what CI runs.'

# Both locks in one task: no tool is pinned in both places, and `check-pins` keeps that true.
# Bump the pinned inputs.
update:
    #!/usr/bin/env bash
    set -euo pipefail
    nix flake update
    # Needs network. pixi holds only the hook runner and interpreter - neither changes a verdict.
    pixi update
    cargo update --workspace
    echo
    echo 'Bumped flake.lock, pixi.lock and Cargo.lock. Review the diff before committing.'

# ---------------------------------------------------------------- inner loop ---

# ECHOES, NOT A GATE CALL. Cargo's `Finished dev profile` says nothing about scope, so a green run
# here once read as a green tree and a branch with an unclosed delimiter was pushed on it. Measured:
# this loop is 0.17s warm and `cargo run -p xtask` adds 0.49s. `check-scope` reads this recipe.
# Fast check of the domain crate only. Should stay sub-second.
check:
    cargo check -p sutura-domain --no-default-features
    @echo 'check: compiled sutura-domain only, default features off - NOT a workspace check.'
    @echo '  `just check-changed` compiles the packages your working tree actually changes.'
    @echo '  `just lint` is the workspace gate, and `just test` runs the suite.'

# On the shell's nightly toolchain like every gate and like CI, so a finding here is CI's finding.
# Format Rust, and normalise line endings and whitespace.
fmt:
    #!/usr/bin/env bash
    set -euo pipefail
    # `xtask fmt` and NOT `cargo fmt --all`: `--all` reaches path dependencies that are not workspace
    # members, so it would rewrite the vendored allocator - the one thing vendoring must never do.
    cargo run -q -p xtask -- fmt
    cargo run -q -p xtask -- text-hygiene --fix
    # The non-Rust text - dprint for markdown/YAML/TOML, ruff for Python. `just lint-text` checks it.
    bash nix/format-text.sh fmt

# No tests here - `just test` owns the suite. The three commands are the exact scalars of the
# `rust-fmt`, `rust-clippy` and `structural gates` hooks, so a green run is what a commit sees.
# Lint everything: the three checks the commit hooks gate on, run directly.
lint:
    #!/usr/bin/env bash
    set -euo pipefail
    # The `--check` half of `just fmt`'s argument above: never `cargo fmt --all`.
    cargo run -q -p xtask -- fmt --check
    # `--all-features` is load-bearing: crates declare features and make deps optional, so an
    # entry point missing the flag lints nothing behind them.
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    # The cheap structural gates, seconds not minutes.
    cargo run -q -p xtask -- hygiene

# `--doc` is separate because nextest does not run doctests.

# Run the tests.
test:
    #!/usr/bin/env bash
    set -euo pipefail
    # The SAME nixpkgs Postgres `checks.nextest` uses, so the postgres cells RUN here rather than skip.
    # Through `nix/with-tier.sh`, which tears down only what it started.
    source nix/with-tier.sh
    # `SUTURA_DEV_REQUIRE_TIER` is exported by `sutura_tier_up` when a tier is up, rather than
    # asserted on this line - see that file: two statements about one fact can disagree.
    sutura_tier_up
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features

# THE FOUR E2E TARGETS BELOW are all gates: `checks.nextest` runs each, because files, a loopback
# port and a pipe need no network and no credential. Each exists to run its one target while working
# on it, not as a second tier, and unlike `just bigquery-acceptance` nothing in them is `#[ignore]`d.

# Run the end-to-end suite against the composed serve binary.
serve-e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "serve-e2e: scope sutura-serve - the composed HTTP surface, over a loopback listener."
    echo "serve-e2e: run \`just test\` for the whole workspace's suite; this target is part of it."
    cargo nextest run -p sutura-serve --all-features

# The agent surface: `crates/sutura-cli/tests/mcp.rs` speaks MCP over the spawned process's pipes.

# Run the end-to-end suite against the composed agent surface.
mcp-e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "mcp-e2e: scope sutura-cli - the composed agent surface, over the spawned binary's pipes."
    echo "mcp-e2e: run \`just test\` for the whole workspace's suite; this target is part of it."
    cargo nextest run -p sutura-cli --all-features

# The PAGES, run: `documented.rs` runs every invocation `docs/getting-started.md` and the example
# README print and holds their refusal and provenance output. It exists because both pages drifted.

# Run the suite that runs every command the documentation prints.
documented:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "documented: scope sutura-cli - the pages' own commands, over the spawned binary."
    echo "documented: run \`just test\` for the whole workspace's suite; this target is part of it."
    cargo nextest run -p sutura-cli --all-features

# The DECLARED source: `declared_source.rs` spawns `sutura query` with `SUTURA_CONFIG_DIR` at a
# catalog using `source: warehouse`. The one venue reaching `environment_from_process`,
# `config_dir_from_process` and `<dir>/base.yaml` - `set_var` is `unsafe`, so a child is needed.

# Run the end-to-end suite against a source the deployment declared.
declared-source:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "declared-source: scope sutura-cli - a declared source, over the spawned binary."
    echo "declared-source: run \`just test\` for the whole workspace's suite; this target is part of it."
    cargo nextest run -p sutura-cli --all-features

# `*paths`, not `+paths`: with nothing to go on the gate reads the working tree, so the no-argument
# form answers "does what I touched compile". The commit hook keeps passing filenames.

# The same invocation the `rust-check-changed` hook and CI use - hooks.rs executes this body.
# cargo check, narrowed to the packages that changed. No paths reads the working tree.
check-changed *paths:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -q -p xtask -- check-changed {{ paths }}

# It is `ci` plus the two app-backed checks, and deliberately the nix path: a nix check builds its
# own GIT-DERIVED copy of the tree, so it is the only thing that catches a file the build needs and
# that copy lacks. `include_str!("defaults.yaml")` passed every dev-shell check and failed CI.
# THE gate. Run this before saying a change is done; nothing else counts as verified.
validate:
    #!/usr/bin/env bash
    set -euo pipefail
    # THE SITE BUILD, first and in TWO steps, because two things can go wrong and only one is a finding
    # about this tree. It cannot be a nix check: the docs toolchain is Python, pixi is the one resolver
    # for it, and a nix sandbox has no network to materialise the env. A pixi step is allowed in the
    # recipe that counts as verified because `nix run .#deny` below ALREADY fails offline (measured), so
    # it adds no network requirement. Cost: ~2s warm, 23-51s to materialise. Separated so a machine that
    # cannot materialise it still gets the nix checks and the sweep, and fails at the END. What it alone
    # catches: a page can be correctly generated and still not render - #352 shipped one, gates green.
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
    # Here rather than inside `just ci`: `ci` is the nix checks and this is not one - the candidate set
    # is `git ls-files` and the sandbox's copy carries no `.git`. `tasks.rs` also EXECUTES the `ci` body
    # against a fake nix with an empty PATH, so a line there that shells out breaks another fixture.
    printf '\n=== format-text ===\n'
    bash nix/format-text.sh check
    just secrets
    nix run .#deny
    if [ "$site" != ok ]; then
        printf '\nvalidate: FAILED - every nix check above passed and the site build never ran.\n'
        printf '  Nothing here has rendered a page. See the SKIPPED line above.\n'
        exit 1
    fi
    printf '\nvalidate: ok - the site build, the nix checks, the secret sweep and the supply chain\n'

# A failed check ends this invocation. An automatic offline retry would also retry failed tests, so
# a later pass could hide the failure - diagnose before rerunning.
# What CI runs, through nix, without entering the dev shell. Prefer `just validate`.
ci:
    #!/usr/bin/env bash
    set -euo pipefail
    # The system is READ, not written down: `.#checks.x86_64-linux.*` on darwin fails before running
    # anything, and a gate that cannot run on the machine of the person who runs it gets skipped.
    system="${SUTURA_NIX_SYSTEM:-$(nix eval --raw --impure --expr builtins.currentSystem)}"
    printf 'checks for %s\n' "$system"
    # api-docs IS in this list: the committed pages are byte-compared and no test covers them, so four
    # stale-page incidents were invisible locally while this task was called THE gate.
    for check in hygiene reuse fmt clippy nextest doctest crap api-docs keycloak-tier postgres-tier; do
        printf '\n=== %s ===\n' "$check"
        nix build ".#checks.$system.$check" -L
    done

# The fat-LTO build. Opt-in, never automatic: minutes of build time for throughput nobody
# has measured yet.
perf:
    nix build .#sutura-performance

# ---------------------------------------------------------------- the gates ---

# One line, because xtask owns the list (`Kind::Hygiene`). `check-guidance` catches a renamed gate.

# The cheap structural gates. Seconds, not minutes.
hygiene:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -q -p xtask -- hygiene

# Everything CI runs. What to run before pushing.
gates: hygiene
    #!/usr/bin/env bash
    set -euo pipefail
    # The tier, for the reason `just test` gives - without it this failed the two postgres cells on any
    # machine where nothing else had started a server, while claiming to be what CI runs.
    source nix/with-tier.sh
    sutura_tier_up
    cargo run -q -p xtask -- fmt --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features
    cargo deny check
    # The DERIVING half of the attribution gate, here because it runs `cargo metadata`, which needs a
    # registry the nix sandbox has not got. The document is generated and NOT committed, so there is
    # nothing to byte-compare; `check-attribution-owner` holds the ABSENCE of a committed copy.
    cargo run -q -p xtask -- check-attribution
    # The DEFAULT-feature lane, here for the line above's reason. Every other compiling gate passes
    # `--all-features` while `nix/shipped.nix` publishes cargo's default set. That shipped once.
    cargo run -q -p xtask -- check-default-features
    # And the lane's TESTS, which the line above only compiles: every `#[cfg(not(feature))]` test was
    # compiled here and executed by nothing. Two were in that state when this landed.
    cargo run -q -p xtask -- check-default-feature-tests
    bash nix/run-gate.sh crap

# The finishing sequence, over the committed branch diff. Needs a clean tree.
ship-check:
    devenv shell ship-check

# Through `nix/run-gate.sh`: local tools when the dev shell is active, the pinned Nix route when not.
# Every run leaves `target/crap/baseline.json`, which `just crap-delta` compares. Scope, cost and
# both halves of the ratchet are in docs/crap.md.

# The CRAP score: complexity weighted by the tests that cover it. Scoped to sutura-domain.
crap:
    #!/usr/bin/env bash
    set -euo pipefail
    bash nix/run-gate.sh crap

# TWO FILES, NO COVERAGE RUN - both sides are baselines an earlier `just crap` wrote, so this is file
# reading. CI resolves the base's baseline from the `crap-baseline` artifact for the pull request's
# exact merge base, and warns and skips when none exists. No `gh run download` here deliberately:
# that would need a token and a network, and could then disagree with CI about the commit.

# `just crap-delta <base-baseline.json> [head-baseline.json]`
crap-delta base head="target/crap/baseline.json":
    cargo run -q -p xtask -- crap-delta --baseline {{ base }} --head {{ head }}

# What a diff requires. `just classify origin/main`
classify base="origin/main":
    cargo run -q -p xtask -- classify --since {{ base }}

# RUNS THE SUITE twice, on the shell's nightly toolchain (LLVM by default), which is what CI gates
# on. AGENTS.md's rule for clippy - never conclude a branch is red from a bare `cargo` line -
# applies to any gate that runs the compiler, and this recipe is what makes it hold here.
# Red-before-green for changed tests. `just causality origin/main`
causality base="origin/main":
    #!/usr/bin/env bash
    set -euo pipefail
    # The SAME tier `test` provisions, and it was missing: this gate's first step is *are the tests green
    # on HEAD*, and the postgres cells are fail-closed, so it failed its own precondition.
    source nix/with-tier.sh
    sutura_tier_up
    cargo run -q -p xtask -- test-causality --since {{ base }}

# ---------------------------------------------------------------- artifacts ---

# BOTH, because both are published: before #111 this built one binary and so did the release, which
# is why nothing a release published could serve a question. `nix/shipped.nix` is the list.
# The release binaries: the command-line tool and the server.
build:
    nix build .#sutura
    nix build .#sutura-serve

# The release images: one binary each, no shell, no package manager.
image:
    nix build .#oci
    nix build .#oci-serve

# All four triples, not the two glibc ones: the musl targets are statically linked and swap in
# mimalloc, so they are a genuinely different build with their own deps derivation and C compile.
# Cross-build every shipped artifact: two binaries at four triples.
build-all:
    nix build .#sutura-x86_64-unknown-linux-gnu
    nix build .#sutura-aarch64-unknown-linux-gnu
    nix build .#sutura-x86_64-unknown-linux-musl
    nix build .#sutura-aarch64-unknown-linux-musl
    nix build .#sutura-serve-x86_64-unknown-linux-gnu
    nix build .#sutura-serve-aarch64-unknown-linux-gnu
    nix build .#sutura-serve-x86_64-unknown-linux-musl
    nix build .#sutura-serve-aarch64-unknown-linux-musl

# Two checks, different questions. `one-binary` reads each shipped package's `bin/` and its runtime
# closure: one executable, named what the entrypoint expects, no toolchain baked in.
# `shipped-features` reads `cargo auditable` out of each native binary. NOT in `just ci`: these
# build release-profile artifacts, so minutes rather than seconds.
# What a release asserts about the artifacts it publishes, without publishing anything.
shipped:
    #!/usr/bin/env bash
    set -euo pipefail
    system="${SUTURA_NIX_SYSTEM:-$(nix eval --raw --impure --expr builtins.currentSystem)}"
    for check in one-binary shipped-features; do
        printf '\n=== %s ===\n' "$check"
        nix build ".#checks.$system.$check" -L
    done

# Through nix: git-cliff's version is part of what it produces, so nix is its only pin. No argument
# rewrites CHANGELOG.md with the unreleased section current; with a tag it renders that section AS
# the release. Neither form tags anything or pushes anything.

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

# Through pixi's ISOLATED `docs` env: the toolchain is Python and pixi is the one resolver for it.
# It lived in flake.nix as `python3.withPackages` first, where mike could not import pymdownx.
# Render the site to site/.
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

# Nightly on purpose: `--output-format json` is an unstable rustdoc option, and the dev shell's bare
# cargo IS the nightly pin. The renderer runs in the DEFAULT pixi env, not `docs` - it is a plain
# stdlib script - and defaults to `docs/api`.

# Regenerate the committed API reference pages from rustdoc JSON. Through nix, so it needs no
# dev shell: it called a bare `cargo` and a bare `pixi` and only worked where one was active.
api:
    nix run .#api-docs

# `actionlint` CANNOT READ a composite action - measured against 1.7.12 - and CI's shellcheck pass
# globs `*.sh`, which a `run:` block is not, so the release path's signing sequence was never linted.
# Shellcheck the shell inside every local composite action.
lint-actions:
    bash nix/lint-action-shell.sh

# What `ci.yml` runs, reached the cheap way.
# Every static check that reads a workflow, an action or a shell script.
lint-workflows:
    bash nix/lint-workflows.sh

# The same script the `format-text` hook, `just fmt`, `just validate` and `format.yml` call - a
# sequence with more than one caller, written more than once, is how the callers come to differ.
# Markdown, YAML and TOML formatted; Python formatted AND linted.
lint-text:
    bash nix/format-text.sh check

# The shell `just lint-workflows` cannot reach: the bodies inside `devenv.nix` are Nix strings, so
# the `*.sh` glob, zizmor and the action reader filter them out - `hook_coverage.rs` carries that as
# a surface with an EMPTY hook set. It asserts the EMISSION: `checkPhase = "true";` in that wrapper
# removed bash -n and shellcheck from every body and left every other gate green (#402).
# Lint the shell script bodies inside devenv.nix.
devenv-linter:
    devenv shell devenv-linter

# **NOT committed, and that is the point.** A committed copy fell behind `Cargo.lock` on every bump,
# and Dependabot cannot regenerate it, so every bot bump was red on arrival. Writes under `/target`;
# `check-attribution-owner` refuses a committed copy and release.yml generates the released asset.
# Write the attribution document: every third-party crate and the licence it declares.
attribution:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -q -p xtask -- attribution

# `checks.reuse` is the authority; this hits the same pin against the REAL tree, so it also sees a
# file you have not staged yet, which the check cannot.
# Is every file's licence answerable by a tool?
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

# Through nix, the only pin: these tools report findings, so their version is part of the verdict.
# Static analysis of the workflows.
zizmor:
    nix run .#zizmor -- .github/workflows

# Everything zizmor's default persona leaves out. Stylistic, so not a gate.
zizmor-pedantic:
    nix run .#zizmor -- --persona pedantic .github/workflows

# A GLOB, not a list: it was two files here and one in ci.yml under a comment claiming "the glob is
# the list", true of neither - and `nix/run-gate.sh`, which decides whether four gates run, was in
# neither of them.

# Lint the workflows and every shell script we ship.
lint-ci:
    #!/usr/bin/env bash
    set -euo pipefail
    nix run .#actionlint
    # `-x` follows `source` directives, so a sourced-only file is judged too. `find`, not a `**` glob:
    # this recipe runs under `sh` on some hosts, where globstar is off.
    mapfile -t scripts < <(find . -name '*.sh' -not -path './.git/*' -not -path './target/*' \
      -not -path './.devenv/*' -not -path './.direnv/*' -not -path './.pixi/*' \
      -not -path './site/*' -not -path './vendor/*' | sort)
    printf 'shellcheck: %d script(s)\n' "${#scripts[@]}"
    # An empty list would pass by checking nothing, which is the failure mode a glob-driven
    # check is most prone to.
    test "${#scripts[@]}" -gt 0
    nix run .#shellcheck -- -x "${scripts[@]}"

# It exists because `docs.yml`'s verify job skips the 15m45s `hygiene` build for a docs-only pull
# request, and a citation of a task that does not exist is the one property that skip cannot defer.
# Same script CI runs, and the task list comes from `just --summary` rather than from a copy.
# The cheap, text-only half of the citation gate. Seconds, and no compiler.
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

# Through pixi's ISOLATED `gcloud` env, which holds a task and no packages: the CLI is reached as a
# pinned container because conda-forge has no `win-64` build and this workspace declares it.
# INTERACTIVE (`docker run -it`), so no gate can use it. Nothing is written into this repository.

# Authenticate against Google Cloud, for the BigQuery work. Both logins, in a container.
gcloud-login:
    pixi run --frozen -e gcloud gl

# ------------------------------------------------------------------ test-infra ---
# The Pulumi test infrastructure under `test-infra/`. The `infra-` prefix keeps these out of the way
# of the Rust surface's tasks; everything runs through pixi, so there is no venv to keep in step.

# Maps to pixi's `gl` task, which runs BOTH gcloud logins into ~/.config/gcloud: the CLI's own
# credentials, and ADC, which client libraries read. `-e gcloud` is required because pixi declares
# `gl` in that FEATURE's tasks. The same task as `gcloud-login` above.
# Authenticate against Google Cloud for the Pulumi provider.
infra-gl:
    pixi run --frozen -e gcloud gl

# The pixi task carries its own `cwd`, so no `cd` is needed and it is correct from any directory.
# The state backend is a PROJECT-LOCAL file and the passphrase comes from PULUMI_CONFIG_PASSPHRASE.
# The stack is configured FROM THE ENVIRONMENT FIRST (config-from-env.sh, refusing on anything
# missing) and the SAME stack name is passed to pulumi - no reliance on an "active" selection.
# `pulumi preview` over the `test-infra/pulumi/google` stack, through the `infra` pixi env.
infra-preview *flags:
    test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || (echo "infra: set PULUMI_CONFIG_PASSPHRASE (machine env or secret)" >&2 && exit 1)
    # The developer's gcloud ADC (their own elevated account), NOT the limited BigQuery SA key the
    # acceptance legs use. just runs each line in a fresh shell, so the identity is set per command.
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" bash {{ justfile_directory() }}/test-infra/pulumi/google/config-from-env.sh --stack "{{stack}}"
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" pixi run -e infra preview --stack "{{stack}}" {{flags}}

# `pulumi up` - apply the stack. Sample/verify before applying: `just infra-preview`.
infra-up *flags:
    test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || (echo "infra: set PULUMI_CONFIG_PASSPHRASE (machine env or secret)" >&2 && exit 1)
    # See infra-preview: run as the developer's gcloud ADC, not the limited BigQuery SA key.
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" bash {{ justfile_directory() }}/test-infra/pulumi/google/config-from-env.sh --stack "{{stack}}"
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" pixi run -e infra up --stack "{{stack}}" {{flags}}

# Same local-file backend and developer-ADC identity as infra-up, but NO config-from-env.sh:
# destroying reads the existing state. GCP APIs stay ENABLED deliberately (disable_on_destroy=False)
# - GCP refuses to disable some that still hold resources, and re-enabling is slower.
# `pulumi destroy` - tear down the Google test infra this stack created.
infra-down:
    test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || (echo "infra: set PULUMI_CONFIG_PASSPHRASE (machine env or secret)" >&2 && exit 1)
    PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" GOOGLE_APPLICATION_CREDENTIALS="${GOOGLE_ADC:-$HOME/.config/gcloud/application_default_credentials.json}" pixi run -e infra destroy --stack "{{stack}}" --yes

# Run after an `infra-up` (especially one following `infra-down`, which rotates every key). Reads
# the local `.pulumi/` state and pushes via `gh`, which must be authenticated.
# Re-export the fresh `up` outputs into the {{bq_test_env}} GitHub environment.
infra-set:
    test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || (echo "infra: set PULUMI_CONFIG_PASSPHRASE (machine env or secret)" >&2 && exit 1)
    STACK="{{stack}}" BQ_TEST_ENV="{{bq_test_env}}" PULUMI_BACKEND_URL="file://{{ justfile_directory() }}/test-infra/pulumi/google" bash {{ justfile_directory() }}/test-infra/pulumi/google/sync-bq-test-env.sh

# Live BigQuery acceptance is outside `just validate`: nix checks have no network. CI runs it in
# `bq-test` for pushes and same-repository PRs, not forks without secrets. The dataset is shared
# with CI; per-run table suffixes isolate reads and drops, and a 24-hour expiration bounds leftovers.

# Run the live BigQuery acceptance suite against the configured project.
bigquery-acceptance:
    #!/usr/bin/env bash
    set -euo pipefail
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

# **Its own task rather than a third leg above, for a developer's reason:** it needs five values and
# two key documents the other legs do not, so one task demanding all of them would make the legs
# somebody CAN run unreachable. **What a green run does NOT mean** is the first thing
# `tests/two_principals.rs` says: both principals are service accounts whose keys this leg holds, so
# it is leg 2's source half and not leg 2. `docs/where-identity-is-proven.md` is the map.
# Run the two-principal BigQuery cell against the configured row-access-policied dataset.

# Run the two-principal BigQuery cell against the configured row-access-policied dataset.
bigquery-two-principals:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "bigquery-two-principals: scope sutura-exec-bigquery - two principals, one statement, one row access policy."
    echo "bigquery-two-principals: this is NOT a gate. Run \`just test\` for the whole workspace's suite."
    echo "bigquery-two-principals: CI runs it through \`nix run .#bigquery-two-principals\`, in the bq-test job."
    cargo nextest run -p sutura-exec-bigquery --all-features --run-ignored only -E 'binary(two_principals)'

# The only BigQuery leg that holds no principal's key - which is what separates impersonation from
# credential selection. NO WORKFLOW INVOKES IT: two of the five values it needs are not in the
# `bq-test` environment. **It has never run.**
# Run the exchanged-identity cell: one workload identity, exchanged per subject.
bigquery-exchanged-identity:
    #!/usr/bin/env bash
    set -euo pipefail
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

# Dry run is the default, because a cleanup task that deletes on a bare invocation is one nobody runs
# twice. `git branch --merged` cannot see a squash-merged branch at all, so what decides instead is
# patch-id equivalence, a merged pull request whose recorded head is this tip, and an age bound.
# `--delete` applies the plan; there is no flag that overrides a refusal.
# What has landed and could go: local branches, and the worktrees holding them. DELETES NOTHING.
clean-branches *args:
    cargo run -q -p xtask -- clean-branches {{ args }}

# What is present, what is missing, what would fail.
doctor:
    cargo run -q -p sutura-dev -- doctor

# NOT a step anybody has to remember before committing - no gate needs it. `checks.keycloak-tier`
# provisions and tears down its own instance in the sandbox, which is where that property is held.
# Through `nix run` rather than a dev-shell package, so a contributor who never touches identity
# does not fetch a JVM and a 186 MB server on `nix develop`.
# The identity provider's nix-native tier, by hand: `just keycloak-tier start|stop|status`.
keycloak-tier *args:
    nix run .#keycloak-tier -- {{ args }}

# `credentials` prints the three `export` lines the adapter needs and refuses if nothing is
# provisioned (#455): `FixtureCredential::from_env` has no fallback. `just test` evaluates them
# through `nix/with-tier.sh`. NOT a step before committing, and a tier started here survives a suite
# run. Straight to the script rather than `nix run`: it is in the dev shell already, which is where
# `nix/with-tier.sh` looks, and `checks.nextest` runs the same one from the same file.
# The Postgres tier, by hand: `just postgres-tier start|stop|status|credentials`.
postgres-tier *args:
    sutura-postgres-tier {{ args }}

# ------------------------------------------------------- the compose tier ---
#
# One service instance per worktree, through xtask rather than the shipped binary: docker
# orchestration inside a release artifact is test scaffolding delivered to users, and xtask is never
# packaged. NOT a nix check and cannot be one - the sandbox has no network and no docker socket.
# A missing docker SKIPS here and FAILS in CI; SUTURA_DEV_REQUIRE_TIER picks the direction.

# This worktree's services, on ports docker allocates, with a discovery file a harness reads.
dev-up:
    cargo run -q -p xtask -- dev-up

# Off by default because nothing here can use one yet: `CredentialBroker` does not exist. The
# reasoning lives beside the service in compose.services.yaml.
# The same, plus the identity provider.
dev-up-identity:
    cargo run -q -p xtask -- dev-up --with identity

# Five containers, of which one - `datahub`, its GMS - is the endpoint the discovery file carries.
# Off by default because it COSTS: three JVMs and a migration job that creates the topics, the
# schema and the indices before GMS will start. The reasoning lives in compose.services.yaml.
# The same, plus the DataHub metadata platform.
dev-up-datahub:
    cargo run -q -p xtask -- dev-up --with datahub

# The sibling of `dev-up-identity` and `dev-up-datahub`, and it exists for the reason they do: a
# service behind a profile is brought up by the task named after that profile, and the tier's own
# remedy for a missing service cites that task. Unlike the other two it must also BUILD the derived
# image, which lives in `demo/start.sh` so one owner shapes the build and the validation.
# The demo profile, built and started but not supervised. `just demo` is the walkthrough.
dev-up-demo:
    bash demo/start.sh --up-only

# A named task rather than a cell in the default suite, and the venue is the whole reason: `just
# test` sets `SUTURA_DEV_REQUIRE_TIER=1`, the DataHub profile costs three JVMs and a migration job,
# and the nix sandbox has no docker socket at all. `.sutura-dev/endpoints.json` has two writers and
# both halves of the clobbering are gone (#317) - each merges per ENTRY and leaves keys it did not
# write. THE LIMIT ON THAT REPAIR: nothing compares the two writers' shapes, so they agree by review
# and a THIRD writer would be held by neither. It brings the profile up first, because asking for
# the fail-closed direction against a tier nobody started is a confusing way to spell an error.
# The provisioned DataHub, asked whether it can carry the deployment-defined metric document.
datahub-acceptance:
    #!/usr/bin/env bash
    set -euo pipefail
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

# One service's host:port on stdout and nothing else, so a shell can substitute it: `PORT="${$(just
# dev-endpoint clickhouse)##*:}"`. `just dev-endpoints` is the readable table.
@dev-endpoint service:
    cargo run -q -p xtask -- dev-endpoint {{ service }}

# Remove this worktree's services, its network and its named volumes. Nothing else, ever.
dev-down:
    cargo run -q -p xtask -- dev-down


# Remove only the demo service and its named volume, leaving every other worktree service alone.
dev-down-demo:
    cargo run -q -p xtask -- dev-down --only demo

# What `just dev-down` would remove, and what it would deliberately spare. Removes nothing.
dev-down-dry:
    cargo run -q -p xtask -- dev-down --dry-run

# NOT a gate, deliberately - the plan's own rule and #595's: a demo that fails a gate gets disabled,
# and a disabled demo holds nothing. It needs a language model, which no gate has. The configuration,
# image build and supervision are in `demo/start.sh`; `docs/demo.md` is the walkthrough.
# The local chat demo: the sutura server and a chat client over `examples/single-player`.
demo:
    bash demo/start.sh
