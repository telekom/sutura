# shellcheck shell=bash
# Bring the Postgres tier up for the command that follows, and tear down only what we brought up.
#
# ONE definition, sourced by every venue that runs the suite locally: `just test`, `just gates`,
# `just causality` and `nix/run-gate.sh tests` (which the commit and push hooks both reach). The nix sandbox does not
# source this - `checks.nextest` provisions the same script through `preCheck`/`postCheck`, which is
# the arrangement `nix/postgres-tier.nix` documents - so there are two provisioners sharing one start
# script, which is the rule this repository already applies to that pin.
#
# # Why it exists: three defects, one missing distinction
#
# All three measured on 2026-09-02, in one session.
#
# **1. The hook and `just gates` provisioned nothing.** `just test` started the tier and
# `checks.nextest` started the tier; the pre-commit `cargo nextest` hook and `just gates` ran the
# same suite bare. The two postgres cells are fail-closed, so a `git commit` on a machine where
# nothing else had started a server was BLOCKED - and the workaround was a person remembering two
# commands.
#
# **2. An unconditional `stop` on EXIT tore down somebody else's server.** Every wrapper started the
# tier and stopped it when it exited, so a developer who had started one by hand - or a nested run -
# lost it to another command's trap. That happened twice: the tier was started by hand, `just test`
# ran and its trap stopped it, and the commit that followed was blocked again.
#
# **3. And `stop` left the endpoint file behind**, so discovery kept claiming a server was there.
# `nix/postgres-tier.nix` carries that half of the fix and the measurement with it.
#
# So: start only if nothing is up, arm the teardown only in that case, and let the REQUIREMENT follow
# the tier rather than being asserted beside it.
#
# # The requirement follows the tier, and that is the part worth reading
#
# `SUTURA_DEV_REQUIRE_TIER` is the skip-or-fail direction for those cells, and every caller used to
# set it on its own line - which is two statements that can disagree. It is exported HERE, and only
# when a tier is actually up, so "the server is there" and "the cells are required" cannot come
# apart. The consequence matters for a hook: on a host with `cargo nextest` but no
# `sutura-postgres-tier`, nothing is exported, the cells are vacuous rather than failing, and the
# commit is not blocked - which is `nix/run-gate.sh`'s own posture, *a hook that cannot run must not
# be a wall*, applied one level down. CI provisions the tier unconditionally and is authoritative.
#
# It is a small weakening of `just test` in exactly one case - a dev shell missing its own tier
# binary, where that recipe used to fail - and it is the honest direction: a missing tool is not a
# test failure.
#
# # What it does NOT do
#
# It is not a lock. Two commands starting at once could both see nothing up and both arm a teardown;
# `start` is idempotent and the second `stop` is a no-op on an already-stopped server, so the outcome
# is a wasted teardown rather than a corrupted one. A lockfile is not worth that.

# Bring the tier up if it is not already; stop it on exit only if we started it.
#
# Exports `SUTURA_DEV_REQUIRE_TIER=1` exactly when a tier is up afterwards.
sutura_tier_up() {
    if ! command -v sutura-postgres-tier >/dev/null 2>&1; then
        echo "with-tier: no sutura-postgres-tier on PATH - the postgres cells will be vacuous."
        echo "           They run in \`checks.nextest\` and in the dev shell; CI is authoritative."
        return 0
    fi
    # Output discarded on purpose, and not only to be quiet: a tier script from BEFORE `status`
    # existed answers `usage: ... start|stop` on standard error and exits 2, which this reads as
    # "nothing is up" and handles by starting one. That is the right degradation - the old
    # unconditional behaviour - and it must not print a usage message at somebody mid-commit.
    if sutura-postgres-tier status >/dev/null 2>&1; then
        echo "with-tier: the Postgres tier is already up - leaving it to whoever started it."
        export SUTURA_DEV_REQUIRE_TIER=1
        return 0
    fi
    sutura-postgres-tier start
    # Single quotes on purpose: the trap body is evaluated at exit and takes nothing from here that
    # could go stale.
    trap 'sutura-postgres-tier stop' EXIT
    export SUTURA_DEV_REQUIRE_TIER=1
}
