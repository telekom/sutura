#!/usr/bin/env bash
# Every static check that reads a workflow, an action, or a shell script.
#
# ITS OWN FILE for two reasons, and the line cap is only the second one.
#
# ONE, AND IT IS THE REASON THAT SURVIVES ANYWHERE: this sequence had two callers and was written
# TWICE - the composite-action pass inline in `ci.yml` and again in the justfile - and two copies of
# a gate are how the two come to check different things. A second caller also falls out of the move:
# `just lint-workflows` reaches the whole sequence locally, which the inline version could not offer.
#
# TWO, measured against the `dev` trunk rather than against this branch's base: `ci.yml` there is 995
# lines against the 1000-line cap `cargo xtask max-lines` enforces, and adding the composite-action
# pass inline took it to 1020. On `main` the same addition lands at 949 and is not forced by the cap
# - so the cap is why the split happened WHEN it did, and the duplication is why it is right.
#
# What moved is the SEQUENCE; the step in `ci.yml` stays, because that is where the classification
# gate on it lives.
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
