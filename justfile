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

# It exists because the alternative is a README section people skip, and the failure mode is
# silent - an uninstalled hook does not complain, it just never runs. `prek install` is the
# important line: without it every gate in this file is advisory.

# Everything a fresh clone needs. Idempotent: run it again any time.
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "== git hooks"
    # All three stages: the commit-msg hook is separate from pre-commit, and pre-push carries
    # the expensive gates. Missing one means that stage silently never runs.
    prek install --hook-type pre-commit --hook-type pre-push --hook-type commit-msg
    echo "== python tooling (zizmor, actionlint, shellcheck)"
    pixi install --frozen
    echo "== building the dev CLI and the gates"
    # Warms the target directory so the first hook run is not a cold compile, and fails here
    # rather than inside a git hook if something is wrong.
    cargo build -q -p xtask -p sutura-dev
    echo "== checking the environment"
    cargo run -q -p sutura-dev -- doctor
    echo
    echo 'Ready. just lists the tasks; just gates is what CI runs.'

# ---------------------------------------------------------------- inner loop ---

# Fast check of the domain crate only. Should stay sub-second.
check:
    cargo check -p sutura-domain --no-default-features

# Format Rust, and normalise line endings and whitespace.
fmt:
    cargo fmt --all
    cargo run -q -p xtask -- text-hygiene --fix

# `--all-features` is not optional here: adapters are default-off, so without it clippy
# inspects almost nothing and still reports success.

# Lint everything.
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# `--doc` is separate because nextest does not run doctests.

# Run the tests.
test:
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features

# ---------------------------------------------------------------- the gates ---

# One line, because xtask owns the list (`Kind::Hygiene` in its task table). It used to be
# transcribed here, twice in devenv.nix, in flake.nix and as eight hooks - and the order
# differed in three of them. `check-guidance` catches a renamed gate, never a forgotten one.

# The cheap structural gates. Seconds, not minutes.
hygiene:
    cargo run -q -p xtask -- hygiene

# Everything CI runs. What to run before pushing.
gates: hygiene
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features
    cargo deny check

# The finishing sequence, over the committed branch diff. Needs a clean tree.
ship-check:
    devenv shell ship-check

# What a diff requires. `just classify origin/main`
classify base="origin/main":
    cargo run -q -p xtask -- classify --since {{ base }}

# Red-before-green for changed tests. `just causality origin/main`
causality base="origin/main":
    cargo run -q -p xtask -- test-causality --since {{ base }}

# ---------------------------------------------------------------- artifacts ---

# The release binary.
build:
    nix build .#sutura

# The release image: one binary, no shell, no package manager.
image:
    nix build .#oci

# Cross-build both shipped architectures.
build-all:
    nix build .#sutura-x86_64-unknown-linux-gnu
    nix build .#sutura-aarch64-unknown-linux-gnu

# --------------------------------------------------------------------- docs ---

# Render the book to docs/book.
docs:
    mdbook build docs

# Serve the book with live reload.
docs-serve:
    mdbook serve docs

# ------------------------------------------------------------------ tooling ---

# Scan the whole worktree for secrets. The hook already covers each commit.
secrets:
    betterleaks dir . --redact --verbose

# Static analysis of the workflows.
zizmor:
    pixi run --frozen zizmor

# Lint the workflows and the shell scripts.
lint-ci:
    pixi run --frozen actionlint
    pixi run --frozen shellcheck-run

# Refresh every imported skill and rewrite the lock.
skills-refresh:
    pixi run --frozen skills-refresh

# Re-hash imported skills without fetching, for a network with no egress.
skills-relock:
    pixi run --frozen skills-relock

# Run the hooks over everything. Scope with `just hooks --files <path>` while iterating.
hooks *args:
    prek run {{ args }}

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

# What is present, what is missing, what would fail.
doctor:
    cargo run -q -p sutura-dev -- doctor
