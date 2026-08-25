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
    # the expensive gates. Missing one means that stage silently never runs.
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
#
# Both locks in one task because they are bumped for the same reason and reviewed together.
# Nothing needs generating: no tool is pinned in both places - `cargo xtask check-pins` is
# what keeps that true - so there is no table to rewrite and nothing to fall out of step.
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
check:
    cargo check -p sutura-domain --no-default-features

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
    cargo fmt --all
    cargo run -q -p xtask -- text-hygiene --fix

# `--all-features` is not optional here: adapters are default-off, so without it clippy
# inspects almost nothing and still reports success.

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
    cargo nextest run --workspace --all-features
    cargo test --doc --workspace --all-features

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

# Cross-build every shipped artifact.
#
# All four, not the two glibc ones: the musl targets are statically linked and swap in mimalloc,
# so they are a genuinely different build - a cross target has its own deps derivation and its
# own C compile. A recipe that skipped them would let a developer pass `build-all` locally and
# still break the release.
build-all:
    nix build .#sutura-x86_64-unknown-linux-gnu
    nix build .#sutura-aarch64-unknown-linux-gnu
    nix build .#sutura-x86_64-unknown-linux-musl
    nix build .#sutura-aarch64-unknown-linux-musl

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

# Regenerate the committed API reference pages from rustdoc JSON.
api:
    cargo rustdoc -q -p sutura-domain --all-features -- -Z unstable-options --output-format json
    pixi run --frozen python docs/.tools/rustdoc_to_markdown.py target/doc/sutura_domain.json

# ------------------------------------------------------------------ tooling ---

# Scan the whole worktree for secrets. The hook already covers each commit.
secrets:
    betterleaks dir . --config .gitleaks.toml --redact --verbose

# Static analysis of the workflows.
#
# Through nix, the only pin for it. These three tools report findings, so their version is
# part of the verdict - `cargo xtask check-pins` fails if any of them reappears in pixi.toml.
zizmor:
    nix run .#zizmor -- .github/workflows

# Everything zizmor's default persona leaves out. Stylistic, so not a gate.
zizmor-pedantic:
    nix run .#zizmor -- --persona pedantic .github/workflows

# Lint the workflows and the shell scripts. Same list CI runs.
lint-ci:
    nix run .#actionlint
    nix run .#shellcheck -- .claude/hooks/ponytail-session-start.sh nix/stable-env.sh

# Refresh every imported skill and rewrite the lock.
skills-refresh:
    pixi run --frozen skills-refresh

# Re-hash imported skills without fetching, for a network with no egress.
skills-relock:
    pixi run --frozen skills-relock

# Run the hooks over everything. Scope with `just hooks --files <path>` while iterating.
hooks *args:
    pixi run --frozen prek run {{ args }}

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
