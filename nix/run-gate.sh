# shellcheck shell=bash
# Prefer configured local stable tools, then the existing pinned Nix routes.
# Missing stable configuration is not a new skip: Rust cases require either that configuration
# or Nix. Once configured, the existing optional-tool abstentions remain below, as do the
# non-Rust gates' tool-availability skips. Every skip names the check it did not perform.
#
# Usage: run-gate.sh <gate>
set -eu

gate="${1:?usage: run-gate.sh <gate>}"

# Nix's pinned cargo must not inherit these either, including when local stable is unconfigured.
unset CARGO_UNSTABLE_CODEGEN_BACKEND CARGO_PROFILE_DEV_CODEGEN_BACKEND

# `nix build .#checks.<system>.<name>` needs the system pair, and hardcoding one would break
# on aarch64 macOS. Ask nix rather than guess.
nix_check() {
    local name="$1"
    local system
    system="$(nix eval --raw --impure --expr 'builtins.currentSystem')"
    nix build ".#checks.${system}.${name}" -L
}

# Never probe host cargo without the configured stable toolchain. An unconfigured host can still
# use the existing pinned Nix route; with neither route, the helper refuses rather than skips.
# Non-Rust gates need no local Rust toolchain. Callers run from the repository root.
case "$gate" in
tests | supply-chain | crap)
    if [ -n "${SUTURA_STABLE_BIN:-}" ] || ! command -v nix >/dev/null 2>&1; then
        # shellcheck source=nix/stable-env.sh
        . nix/stable-env.sh
    fi
    ;;
esac

case "$gate" in
tests)
    if [ -n "${SUTURA_STABLE_BIN:-}" ] && cargo nextest --version >/dev/null 2>&1; then
        # The Postgres tier, which this arm did NOT bring up - so the two fail-closed postgres
        # cells failed and BLOCKED EVERY COMMIT on a machine where nothing else had started a
        # server. Measured 2026-09-02. Tier 2 below has always provisioned it, through
        # `checks.nextest`'s own `preCheck`, so this was the one venue running the suite bare.
        #
        # NOT `exec`, and that is load-bearing rather than tidy: `exec` replaces this shell, so the
        # EXIT trap `sutura_tier_up` arms would never fire and the server would outlive the hook.
        # shellcheck source=nix/with-tier.sh
        . nix/with-tier.sh
        sutura_tier_up
        cargo nextest run --workspace --all-features
    elif command -v nix >/dev/null 2>&1; then
        echo "run-gate: local stable nextest unavailable, using nix (same pin as CI)"
        nix_check nextest
    else
        echo "run-gate: SKIPPED tests - no cargo-nextest and no nix on this host."
        echo "          CI runs them on every push; this only delays the finding."
    fi
    ;;
supply-chain)
    if [ -n "${SUTURA_STABLE_BIN:-}" ] && cargo deny --version >/dev/null 2>&1; then
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
        echo "run-gate: local stable cargo-deny unavailable, using nix (same pin as CI)"
        exec nix run .#deny
    else
        echo "run-gate: SKIPPED supply chain - no cargo-deny and no nix on this host."
        echo "          CI runs it on every push; this only delays the finding."
    fi
    ;;
secrets)
    # The WHOLE tree, with the same config CI uses. The commit-stage hook scans only staged
    # changes, which is right for its tier and is why it could not have caught the finding that
    # turned CI red: an allowlist-worthy literal in a file committed long ago never appears in a
    # staged diff. Push is the last local chance to see what CI will see.
    if command -v betterleaks >/dev/null 2>&1; then
        exec betterleaks dir . --config devco/gitleaks.toml --redact
    elif command -v nix >/dev/null 2>&1; then
        echo "run-gate: betterleaks absent, using nix (same pin as CI)"
        exec nix run .#betterleaks -- dir . --config devco/gitleaks.toml --redact
    else
        echo "run-gate: SKIPPED the secret sweep - no betterleaks and no nix on this host."
        echo "          CI runs it on every push; this only delays the finding."
    fi
    ;;
crap)
    # The CRAP score. Both tools or nothing: `cargo xtask crap` fails rather than skips when one
    # is missing, which is right for the gate and wrong for a hook, so the tiering happens here.
    if [ -n "${SUTURA_STABLE_BIN:-}" ] && cargo llvm-cov --version >/dev/null 2>&1 && cargo crap --version >/dev/null 2>&1; then
        exec cargo run -q -p xtask -- crap
    elif command -v nix >/dev/null 2>&1; then
        echo "run-gate: local stable coverage tools unavailable, using nix (same pin as CI)"
        exec nix run .#crap
    else
        echo "run-gate: SKIPPED the CRAP score - no cargo-crap/cargo-llvm-cov and no nix here."
        echo "          CI runs it on every push; this only delays the finding."
    fi
    ;;
fmt-parity)
    # The push-tier format check uses the flake's stable pin. Local Rust gates also require
    # configured stable tools; no equality between nightly and stable formatter output is assumed.
    #
    # Through the FLAKE CHECK rather than a local stable rustfmt, which is what makes it cheap:
    # `checks.fmt` is crane's `cargoFmt` and compiles nothing, so with nix present this is the
    # exact verdict CI reaches for the price of a cache lookup. No second toolchain is installed
    # and no second target directory is filled.
    if command -v nix >/dev/null 2>&1; then
        system="$(nix eval --raw --impure --expr 'builtins.currentSystem')"
        # `--offline` is retried rather than passed always, matching `just ci`. This tree's
        # substituter list includes a private cache, and an expired credential there answers
        # 401 - which nix treats as a hard failure of the build, not of a lookup it could skip.
        # A push blocked because a *cache* would not talk to us reports a formatting problem
        # where there is none. Offline still reaches the same verdict from local store paths;
        # only a genuinely uncached check degrades, and it degrades to a local build.
        nix build ".#checks.${system}.fmt" -L \
            || nix build ".#checks.${system}.fmt" -L --offline
        exit "$?"
    else
        echo "run-gate: SKIPPED the stable-channel format check - no nix on this host."
        echo "          Local Rust formatting requires configured stable tools; CI runs its pinned check."
    fi
    ;;
*)
    echo "run-gate: unknown gate '$gate'" >&2
    exit 2
    ;;
esac
