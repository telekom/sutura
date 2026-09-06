# shellcheck shell=bash
# Bring the Postgres tier up for the command that follows, and tear down only what we brought up.
#
# ONE definition, sourced by every venue that runs the suite locally: `just test`, `just gates`,
# `just causality` and `nix/run-gate.sh tests` (which the commit and push hooks both reach). The nix sandbox does not
# source this - `checks.nextest` provisions the same script through `preCheck`/`postCheck`, which is
# the arrangement `nix/postgres-tier.nix` documents - so there are two provisioners sharing one start
# script, which is the rule this repository already applies to that pin.
#
# # Why it exists: four defects, each one a missing distinction
#
# The first three measured on 2026-09-02, in one session; the fourth on 2026-09-03.
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
# **4. And then "is a tier up" turned out to be two questions.** This file asked
# `sutura-postgres-tier status`, which answered from the postmaster; the suite asked
# `<root>/.sutura-dev/endpoints.json`, which is where an address actually is. Measured on
# 2026-09-03 (`github.com/telekom/sutura#298`): a postmaster outlived a teardown that had already
# withdrawn its entry, this file was told *already up*, it started nothing, and the two fail-closed
# cells panicked on a worktree that publishes nothing - one full `just test` discarded, and most of
# the cost was working out that a GREEN `status` was the reason.
#
# The fix is not here, and that is the point: `status` is DERIVED from the endpoint file now, so
# this file reads one record rather than a second opinion about it. What IS here is the third
# answer that derivation makes available - a server running with nothing publishing it, which is
# republished and deliberately not adopted. `checks.postgres-tier` drives all three arms through
# this exact file.
#
# So: read ONE record, start only if the suite would find nothing, arm the teardown only for a
# server this shell actually started, and let the REQUIREMENT follow the tier rather than being
# asserted beside it.
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
# `start` is idempotent and the second `stop` is a no-op on an already-stopped server.
#
# **That used to make the worst outcome a wasted teardown. It no longer does.** `stop` now removes
# the data directory - the tier is provisioned on demand and nothing it writes outlives a teardown,
# `github.com/telekom/sutura#377` - so a teardown that lands while another shell is inside `start`
# takes that shell's directory with it, leaving an endpoint entry published over nothing: the
# `#231`/`#298` state this file exists to prevent. The window is the duration of the endpoint
# withdrawal, tens of milliseconds, and reaching the end state in a test required widening it
# deliberately; the natural interleaving (A stops, B's `status` answers 1, B starts) puts B at its
# edge rather than inside it. So this is a stated limit and not a measured failure - but the
# sentence that used to bound the damage is gone, and a lockfile is the thing that would restore it.
#
# **And it cannot follow the tier across a WORKTREE, which is the half that bit.** The requirement
# follows the tier in TIME - exported only once a server is up - and an export follows a process
# tree, not a directory. The endpoint does not: it is published to `<root>/.sutura-dev/endpoints.json`
# and resolved by walking up from the harness, so a command that runs cargo in a DIFFERENT root
# carries the requirement there and leaves the endpoint behind. `just causality` does exactly that -
# its base run happens in a git-derived worktree under `target/`, where that file cannot exist
# because it is gitignored - and every tier-backed cell failed CLOSED in a tree nothing had
# provisioned. Measured: two such cells were the whole of one base run's red, 86 tests into 1810, and
# the gate reported it as proof about a change that touched neither.
#
# Nothing here can hold that; a shell export has no way to say *this root only*. So the caller that
# knows it is crossing holds it instead: `xtask::causality` removes this variable from the run it
# makes in a reconstructed worktree, and a unit test over the command it builds is the mechanism. A
# future gate that runs cargo in another root has to make the same removal, and nothing will remind
# it.

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
    #
    # `|| state=$?` rather than an `if`, because there are THREE answers and the caller's `set -e`
    # must not fire on the two that are not zero. `nix/postgres-tier.nix` documents them at
    # `status`; what they mean HERE is the whole of `github.com/telekom/sutura#298`.
    local state=0
    sutura-postgres-tier status >/dev/null 2>&1 || state=$?
    case "$state" in
        0)
            echo "with-tier: the Postgres tier is already up - leaving it to whoever started it."
            ;;
        3)
            # A postmaster with no entry in `endpoints.json`. THIS branch is the defect: reading
            # only `pg_ctl` put this state in the branch above, so nothing republished the entry,
            # nothing started a server, and every fail-closed cell then panicked on a worktree that
            # publishes nothing. `start` is idempotent about a live postmaster and republishes the
            # entry, so the state heals here instead of costing a suite run.
            #
            # And NO teardown is armed, for the reason defect 2 above was filed: this server is not
            # ours, whoever lost its entry. Republishing a claim is not adopting a process.
            echo "with-tier: a Postgres server is up with no endpoint entry - republishing it."
            echo "           It was started by something else, so it is left running afterwards."
            sutura-postgres-tier start
            ;;
        *)
            sutura-postgres-tier start
            # Single quotes on purpose: the trap body is evaluated at exit and takes nothing from
            # here that could go stale.
            #
            # **`|| true` is the load-bearing token here, and the sentence it replaced was false.**
            # That sentence said an EXIT trap cannot change the exit status of a shell that is
            # already exiting. It cannot in a plain shell; every venue that sources this file runs
            # bash with ERREXIT - `justfile`'s `test`, `gates` and `causality` are `set -euo
            # pipefail`, `nix/run-gate.sh` is `set -eu` - and under errexit a FAILING command in an
            # EXIT trap REPLACES the status the shell was leaving with. Measured, bash 5.3.15:
            #
            #   trap <exits 1> EXIT; exit 42                     -> 42
            #   set -euo pipefail; trap <exits 1> EXIT; exit 42   -> 1
            #   set -euo pipefail; trap <exits 1> EXIT; true      -> 1
            #
            # So without this token a GREEN suite plus a failed teardown exits 1, and nextest's
            # 100 - *some tests failed*, the one status the suite works to produce - is rewritten
            # to that same 1. A teardown is not a test result and does not get to answer for one.
            #
            # Nothing is swallowed by it, because the failure's signal was never the exit status:
            # `nix/postgres-tier.nix`'s `stop` keeps the endpoint entry over the live server and
            # says so on standard error, naming the retry. The next run reads that entry, is told
            # *already up*, and arms no teardown of its own. And the venue where a failed stop must
            # fail a BUILD is unaffected: `checks.nextest` tears the tier down in `postCheck`,
            # which calls `stop` directly rather than through this trap.
            #
            # `checks.postgres-tier` drives a failed stop THROUGH this trap and asserts the
            # subshell keeps the status its body chose, so this is held rather than remembered -
            # delete the `|| true` and that arm goes red.
            trap 'sutura-postgres-tier stop || true' EXIT
            ;;
    esac
    export SUTURA_DEV_REQUIRE_TIER=1
}
