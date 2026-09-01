#!/usr/bin/env bash
# Every static check that reads a workflow, an action, or a shell script.
#
# ITS OWN FILE for the reason `nix/reuse.nix` and `nix/lint-action-shell.sh` give: the file that used
# to hold it is under the same 1000-line cap `cargo xtask max-lines` enforces on everything else, and
# it reached 1020 with this inline. What moved is the SEQUENCE; the step in `ci.yml` stays, because
# that is where the classification gate on it lives.
#
# A second caller falls out of the move: `just lint-workflows` reaches the same sequence locally,
# which the inline version could not offer.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

nix run .#zizmor -- .github/workflows
nix run .#actionlint

# Every shell script we ship, and the list IS the list. It used to name one file under this comment,
# so `nix/run-gate.sh` - which decides whether the tests, the secret sweep, the supply-chain gate and
# the CRAP score run at all - was linted by nothing. An empty list would pass by checking nothing, so
# it is a failure.
#
# `git ls-files` RATHER THAN `find`, and it is a correction rather than a preference. The find form
# excluded `./.git` and `./target` by path, and a local run then failed on six `.sh` files inside
# `.pixi/envs/docs` - tcl and h2 config scripts belonging to an installed pixi environment, which is
# gitignored and is not ours to lint. CI never saw it because it does not install that environment
# in this job, so the failure was reachable only where a developer works, which is the worst place
# for it. An exclusion list needs a new entry per directory somebody installs something into; the
# tracked-file list needs none, and "every shell script we SHIP" is exactly what tracked means.
#
# What it gives up: an untracked new script is not linted here. The commit hook covers a staged
# `.sh`, and in CI everything is tracked by construction - so the gap is a file nobody has added yet.
mapfile -t scripts < <(git ls-files '*.sh' | sort)
printf 'shellcheck: %d script(s)\n' "${#scripts[@]}"
test "${#scripts[@]}" -gt 0
nix run .#shellcheck -- -x "${scripts[@]}"

# And the shell inside a composite action, which none of the three above reaches. That script's
# header carries the measurement.
bash nix/lint-action-shell.sh
