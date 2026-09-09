# shellcheck shell=bash
# Prefer the dev shell's local tools, then the existing pinned Nix routes. The dev shell's bare
# cargo is the same nightly toolchain CI gates on, so a local run cannot disagree with a Nix run
# about the toolchain. The non-Rust gates keep their tool-availability skips. Every skip names the
# check it did not perform.
#
# Usage: run-gate.sh <gate>
set -eu

gate="${1:?usage: run-gate.sh <gate>}"

# Nix's pinned cargo must not inherit an opt-in cranelift from the shell (it is not the default,
# but a developer can turn it on): cranelift's unwind tables abort on C++/Rust-boundary exceptions.
unset CARGO_UNSTABLE_CODEGEN_BACKEND CARGO_PROFILE_DEV_CODEGEN_BACKEND

# `nix build .#checks.<system>.<name>` needs the system pair, and hardcoding one would break
# on aarch64 macOS. Ask nix rather than guess.
nix_check() {
    local name="$1"
    local system
    system="$(nix eval --raw --impure --expr 'builtins.currentSystem')"
    nix build ".#checks.${system}.${name}" -L
}

# The local Rust arms probe host cargo - the dev shell's nightly, which is CI's toolchain. A host
# where the tool is absent falls back to the pinned Nix route; with neither, the arm skips with a
# notice. Callers run from the repository root.
case "$gate" in
tests)
    if cargo nextest --version >/dev/null 2>&1; then
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
        echo "run-gate: local nextest unavailable, using nix (same pin as CI)"
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
        echo "run-gate: local cargo-deny unavailable, using nix (same pin as CI)"
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
    if cargo llvm-cov --version >/dev/null 2>&1 && cargo crap --version >/dev/null 2>&1; then
        exec cargo run -q -p xtask -- crap
    elif command -v nix >/dev/null 2>&1; then
        echo "run-gate: local coverage tools unavailable, using nix (same pin as CI)"
        exec nix run .#crap
    else
        echo "run-gate: SKIPPED the CRAP score - no cargo-crap/cargo-llvm-cov and no nix here."
        echo "          CI runs it on every push; this only delays the finding."
    fi
    ;;
jscpd)
    # The copy/paste gate (issue #474): detect copied blocks in first-party Rust.
    #
    # Tier 1 runs the GATE (`cargo xtask check-jscpd`) when jscpd is on PATH - the dev
    # shell cannot satisfy this (jscpd lives in nix, not devenv, like shellcheck), so it is
    # mostly for a host that installed jscpd for the hygiene hook to find.
    #
    # Tier 2 reaches the SAME pin CI uses through `nix run .#jscpd`. It scans the same
    # paths and thresholds as `check-jscpd`, but a bare jscpd run prints clones without the
    # allowlist verdict - so on this tier the hook is advisory, and the fail-closed gate is
    # `checks.hygiene`, which carries jscpd and applies `devco/dup-ignore`.
    #
    # Tier 3 skips with a notice: a hook that cannot run must not be a wall, and CI is
    # authoritative, exactly the contract the shellcheck / zizmor hooks use.
    if command -v jscpd >/dev/null 2>&1; then
        exec cargo run -q -p xtask -- check-jscpd
    elif command -v nix >/dev/null 2>&1; then
        echo "run-gate: jscpd absent, using nix (same pin as CI)"
        exec nix run .#jscpd -- --silent --no-colors --format rust --min-lines 30 --min-tokens 250 --ignore 'target/**,site/**,result/**,result-*/**,.pixi/**,.sutura-dev/**,report/**,**/.prek-cache/**' .
    else
        echo "run-gate: SKIPPED the copy-paste scan - no jscpd and no nix on this host."
        echo "          CI (checks.hygiene) enforces it; this only delays the finding."
    fi
    ;;
fmt-parity)
    # The push-tier format check goes through the flake's pinned check, the exact verdict CI
    # reaches. It is a check, not a local tool, so it needs nix.
    #
    # Through the FLAKE CHECK rather than a local rustfmt, which is what makes it cheap:
    # `checks.fmt` is crane's `cargoFmt` and compiles nothing, so with nix present this is the
    # exact verdict CI reaches for the price of a cache lookup. No second toolchain is installed
    # and no second target directory is filled; the shell's nightly formatter already matches CI.
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
        echo "run-gate: SKIPPED the format check - no nix on this host."
        echo "          CI runs its pinned check; a local run (on the nightly shell cargo) still works."
    fi
    ;;
*)
    echo "run-gate: unknown gate '$gate'" >&2
    exit 2
    ;;
esac
