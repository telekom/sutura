# shellcheck shell=bash
# Run a pre-push gate through whatever this host actually has.
#
# The problem this solves is concrete: the pre-push hooks called `cargo nextest` and
# `cargo deny` directly, and on a host where neither is installed EVERY push was blocked with
# "no such command". A hook that cannot run must not be a wall.
#
# Three tiers, in order:
#   1. The tool itself, if it is on PATH. Inside `devenv shell` it always is.
#   2. nix, which pins the same version CI uses - so this is not a weaker check, it is the
#      same check reached differently.
#   3. Skip, with a notice naming what was skipped. CI runs all of these unconditionally, so
#      a skip delays a finding to the pull request; it never loses one.
#
# Tier 3 is the deliberate part. The alternative - fail the push - stops work on a host that
# is missing a tool for reasons the developer cannot fix, and the finding is coming from CI
# regardless.
#
# Usage: run-gate.sh <gate>
set -eu

gate="${1:?usage: run-gate.sh <gate>}"

# `nix build .#checks.<system>.<name>` needs the system pair, and hardcoding one would break
# on aarch64 macOS. Ask nix rather than guess.
nix_check() {
    local name="$1"
    local system
    system="$(nix eval --raw --impure --expr 'builtins.currentSystem')"
    nix build ".#checks.${system}.${name}" -L
}

case "$gate" in
tests)
    if cargo nextest --version >/dev/null 2>&1; then
        exec cargo nextest run --workspace --all-features
    elif command -v nix >/dev/null 2>&1; then
        echo "run-gate: cargo-nextest absent, using nix (same pin as CI)"
        nix_check nextest
    else
        echo "run-gate: SKIPPED tests - no cargo-nextest and no nix on this host."
        echo "          CI runs them on every push; this only delays the finding."
    fi
    ;;
supply-chain)
    if cargo deny --version >/dev/null 2>&1; then
        # Not `exec`: a failure here has two very different causes and they must not be
        # conflated. `cargo deny check` fetches the RustSec advisory database over the
        # network, and on a host with no direct egress it exits non-zero having checked
        # NOTHING - "failed to prepare fetch". Blocking a push on that reports a supply-chain
        # problem where there is only a firewall.
        #
        # So: a real finding still fails. An inability to fetch is reported as a skip. The
        # match is on the fetch error specifically, never on the exit code alone, because
        # swallowing a genuine advisory is the one outcome worse than a blocked push.
        output="$(cargo deny check 2>&1)" && status=0 || status=$?
        printf '%s
' "$output"
        if [ "$status" -ne 0 ] && printf '%s' "$output" | grep -q 'failed to fetch advisory database'; then
            echo
            echo "run-gate: SKIPPED supply chain - the advisory database is unreachable from"
            echo "          this host, so nothing was checked. CI fetches it on every push."
            exit 0
        fi
        exit "$status"
    elif command -v nix >/dev/null 2>&1; then
        echo "run-gate: cargo-deny absent, using nix (same pin as CI)"
        exec nix run .#deny -- check
    else
        echo "run-gate: SKIPPED supply chain - no cargo-deny and no nix on this host."
        echo "          CI runs it on every push; this only delays the finding."
    fi
    ;;
*)
    echo "run-gate: unknown gate '$gate'" >&2
    exit 2
    ;;
esac
