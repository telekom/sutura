#!/usr/bin/env bash
# Shellcheck the shell inside every local composite action.
#
# WHY THIS EXISTS, and it is a gap rather than a nicety. `ci.yml`'s workflow analysis runs three
# tools and between them they cover every line of shell this repository ships EXCEPT the shell
# inside `.github/actions/*/action.yml`:
#
#   * `zizmor` is pointed at `.github/workflows`.
#   * `actionlint` is the one that would normally carry it - it shells each workflow's `run:` block
#     out to shellcheck itself - and it CANNOT READ a composite action. MEASURED against the pinned
#     1.7.12: handed `.github/actions/attest-and-sign/action.yml` it reports `"jobs" section is
#     missing in workflow` and `unexpected key "runs" for "workflow" section`, because it parses the
#     file as a workflow.
#   * the shellcheck pass globs `*.sh`, and a `run:` block is not a file.
#
# So the release path's own signing sequence - `nix run .#cosign` over every published asset - was
# shell that nothing had ever linted. The gap widens every time a step moves out of a workflow to
# stay under the 1000-line cap, which is exactly what this repository does.
#
# ONE OWNER, because it has two callers: `just lint-actions` and `nix/lint-workflows.sh`. It was
# written twice - once in the justfile and once inline in `ci.yml` - and two copies of a gate are how
# the two come to check different things.
#
# Through nix for both tools, so a developer reaches the same versions CI does. `cargo xtask
# action-shell` is what parses the YAML: Rust with tests rather than `awk` here, because a parser
# that silently reads nothing out of an action would look exactly like a clean run. It fails closed.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

nix run .#xtask -- action-shell "$out"

# Globbed into an array and asserted non-empty for the reason `nix/lint-workflows.sh` gives about
# its own find: an empty list passes by checking nothing, which is the failure that looks exactly
# like success.
mapfile -t scripts < <(find "$out" -name '*.sh' | sort)
printf 'shellcheck: %d composite-action script(s)\n' "${#scripts[@]}"
test "${#scripts[@]}" -gt 0
nix run .#shellcheck -- -x "${scripts[@]}"
printf 'lint-action-shell: ok - %d extracted script(s)\n' "${#scripts[@]}"
