# shellcheck shell=bash
# Bring EVERY nix-native tier up for the command that follows, and tear down only what we brought up.
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

# # ALL OR NOTHING, and that is forced rather than chosen
#
# `SUTURA_DEV_REQUIRE_TIER` is one variable for every tier, so with two of them the requirement can
# no longer follow one tier: a shell that had Postgres up and no `sutura-keycloak-tier` on PATH would
# export the flag for Postgres's sake and turn the keycloak cells from an honest skip into a red run
# about a tool that is absent. So the flag is exported only when EVERY tier below is up, and a
# missing tier makes every tier's cells vacuous.
#
# It is the same direction the paragraphs above argue for and it costs more than it used to: a shell
# missing ONE tier script now loses the other's cells as well. `checks.nextest` provisions both
# unconditionally and is authoritative. A per-service requirement would remove the cost and is a
# change to `sutura_dev::requirement`'s shape, not to this file.
#
# # What every tier here costs, measured
#
# Postgres is about a second. Keycloak is a JVM and a schema migration: **19s wall on an
# aarch64-darwin dev machine** (`sutura-keycloak-tier start`, 2026-09-03), of which ~12s is the
# server and the rest is `kcadm.sh`. That is paid by every venue that sources this file, the commit
# hook included, and it is worth knowing before adding a third tier - the suite it wraps is minutes,
# so this is single digits of percent, and it would not be if a tier cost a minute.

# Every nix-native tier, in start order. One list, because `sutura_tier_up` and the teardown trap
# must not be able to disagree about what a tier is.
SUTURA_TIERS=(sutura-postgres-tier sutura-keycloak-tier)

# What THIS shell started, and therefore what it may stop. Written by `sutura_tier_up` and read by
# the trap, so a tier somebody else brought up is not torn down by our exit.
SUTURA_TIERS_STARTED=()

# Stop only the tiers this shell started. The trap's body, kept as a function so the trap can be one
# short single-quoted string that takes nothing from the calling scope that could go stale.
sutura_tier_down() {
    local tier
    # The length check is not decoration: `"${array[@]}"` on an EMPTY array is an unbound-variable
    # error under `set -u` in bash 3.2, which is what `/bin/bash` still is on macOS. The trap is
    # armed before anything is started, so this runs with an empty list on every failure path.
    if [ "${#SUTURA_TIERS_STARTED[@]}" -eq 0 ]; then
        return 0
    fi
    for tier in "${SUTURA_TIERS_STARTED[@]}"; do
        "$tier" stop
    done
}

# Bring every tier up if it is not already; stop on exit only what we started.
#
# Exports `SUTURA_DEV_REQUIRE_TIER=1` exactly when every tier is up afterwards.
sutura_tier_up() {
    local tier
    # EVERY tier is checked for BEFORE any of them is started, and the order is the fix rather than
    # tidiness: a loop that started Postgres and then discovered the keycloak script was missing
    # would return with no flag, no trap and a postmaster nobody owns.
    for tier in "${SUTURA_TIERS[@]}"; do
        if ! command -v "$tier" >/dev/null 2>&1; then
            echo "with-tier: no $tier on PATH - EVERY tier's cells will be vacuous, not just its own."
            echo "           They run in \`checks.nextest\` and in the dev shell; CI is authoritative."
            return 0
        fi
    done
    # ARMED BEFORE ANYTHING STARTS, so that one tier coming up and a later one refusing does not
    # leave a server nobody owns - which is what the `return 1` below would otherwise do. It reads
    # the started list, so an exit with nothing started stops nothing. Single quotes on purpose: the
    # body is evaluated at exit rather than now.
    trap 'sutura_tier_down' EXIT
    for tier in "${SUTURA_TIERS[@]}"; do
        # Output discarded on purpose, and not only to be quiet: a tier script from BEFORE `status`
        # existed answers `usage: ... start|stop` on standard error and exits 2, which this reads as
        # "nothing is up" and handles by starting one. That is the right degradation - the old
        # unconditional behaviour - and it must not print a usage message at somebody mid-commit.
        if "$tier" status >/dev/null 2>&1; then
            echo "with-tier: $tier is already up - leaving it to whoever started it."
            continue
        fi
        "$tier" start
        # A `start` that returned 0 is not the same claim as a tier that is UP, and the whole point
        # of exporting the requirement here is that the flag and the tier cannot come apart. So the
        # tier is asked, by the same `status` the loop above trusts, before it counts as started.
        if ! "$tier" status >/dev/null 2>&1; then
            echo "with-tier: $tier start succeeded and $tier status says it is not up." >&2
            echo "           Refusing to export the requirement for a tier that is not there." >&2
            return 1
        fi
        SUTURA_TIERS_STARTED+=("$tier")
    done
    export SUTURA_DEV_REQUIRE_TIER=1
}
