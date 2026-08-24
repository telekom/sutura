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

# ---------------------------------------------------------------- inner loop ---

# Fast check of the domain crate only. Should stay sub-second.
check:
    cargo check -p sutura-domain --no-default-features

# Format Rust, and normalise line endings and whitespace.
fmt:
    cargo fmt --all
    cargo run -q -p xtask -- text-hygiene --fix

# Lint everything. `--all-features` is not optional: adapters are default-off.
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run the tests. nextest for the test suite; `--doc` separately because nextest does not
# run doctests.
test:
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features

# ---------------------------------------------------------------- the gates ---

# The cheap structural gates. Seconds, not minutes.
hygiene:
    cargo run -q -p xtask -- line-endings
    cargo run -q -p xtask -- text-hygiene
    cargo run -q -p xtask -- max-lines
    cargo run -q -p xtask -- unused-deps
    cargo run -q -p xtask -- check-boundaries
    cargo run -q -p xtask -- check-skills
    cargo run -q -p xtask -- check-docs
    cargo run -q -p xtask -- check-guidance

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
