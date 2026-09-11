#!/usr/bin/env bash
# Markdown, YAML and TOML through dprint; Python through ruff, which formats AND lints.
#
# ONE IMPLEMENTATION, and that is the point of the file: the `format-text` pre-commit hook,
# `just lint-text`, `just fmt`, `just validate` and `.github/workflows/format.yml` all call this,
# so a finding a developer sees is one CI sees. It is the argument `nix/lint-workflows.sh` makes one file over -
# that sequence was written twice and the two copies are how they come to check different things.
#
# Usage: format-text.sh [check|fmt]
#
# THE CANDIDATE SET IS `git ls-files`, not a directory walk, and that is a correction rather than a
# preference. `.pixi/envs/` carries a vendored CPython standard library - hundreds of `.py` files
# that are not ours and that `ruff format` would happily rewrite - and `target/`, `.devenv/` and
# `result` carry more of the same. All of it is gitignored, and "every file we SHIP" is exactly
# what tracked means, so a walk needs a new exclusion the day somebody installs something into a
# new directory and a tracked-file list needs none. It is the same fix `nix/lint-workflows.sh`
# already made after a local run failed on six `.sh` files inside an installed pixi environment.
#
# `dprint.json`'s `excludes` still carries the TRACKED exclusions, which a tracked-file list
# cannot express: the generated API pages `cargo xtask check-api-docs` byte-compares against a
# fresh generation, and the vendored source whose whole value is a readable diff against upstream
# (`VENDOR.md`). dprint applies those excludes even to a path named explicitly on the command
# line, which is what makes the two mechanisms compose rather than fight.
#
# NIX OR A NOTICE. Both tools are pinned by the flake and by nothing else - the version decides
# what a formatter reports, so a second pin would be a second verdict, and `cargo xtask
# check-pins` fails if either name reappears in `pixi.toml`. A host without nix therefore cannot
# reach this verdict at all: it says which check it did not perform and continues, the contract
# the `shellcheck` and `zizmor` hooks use. CI runs it unconditionally.
set -euo pipefail

mode="${1:-check}"
case "$mode" in
check | fmt) ;;
*)
    echo "format-text: unknown mode '$mode' - expected check or fmt" >&2
    exit 2
    ;;
esac

root="$(git rev-parse --show-toplevel)"
cd "$root"

if ! command -v nix >/dev/null 2>&1; then
    echo "format-text: SKIPPED - no nix on this host, and both tools are pinned by the flake"
    echo "             alone. CI enforces it; this only delays the finding."
    exit 0
fi

mapfile -t text < <(git ls-files '*.md' '*.yml' '*.yaml' '*.toml' | sort)
# `.agents/skills/` is dropped rather than excluded in `dprint.json`, because it is the one
# exclusion that has to reach ruff as well: those trees are CONTENT-HASH-LOCKED imports
# (`.agents/skills.lock.json`), and `cargo xtask check-skills` reports a reformatted import as a
# local fork - measured, 23 of them at once. `.agents/skills-sync.py` is ours and does not match.
mapfile -t python < <(git ls-files '*.py' | grep -v '^\.agents/skills/' | sort)

# An empty list would pass by inspecting nothing, which is the failure mode a file-listing check
# is most prone to - `nix/lint-workflows.sh` carries the same two lines for the same reason. Both
# lists are non-empty in any real checkout.
printf 'format-text: %s over %d markdown/yaml/toml and %d python file(s)\n' \
    "$mode" "${#text[@]}" "${#python[@]}"
test "${#text[@]}" -gt 0
test "${#python[@]}" -gt 0

nix run .#dprint -- "$mode" "${text[@]}"

# ruff's formatter and its linter are two verdicts from one pin, and both run: a file can be
# formatted and still have an unused import. `--fix` only in `fmt` mode, for the reason `just fmt`
# and `just lint` are two recipes - a checking run must not change the tree it is judging.
if [ "$mode" = fmt ]; then
    nix run .#ruff -- format "${python[@]}"
    nix run .#ruff -- check --fix "${python[@]}"
else
    nix run .#ruff -- format --check "${python[@]}"
    nix run .#ruff -- check "${python[@]}"
fi
