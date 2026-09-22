#!/usr/bin/env bash
# THE RELEASE ARTEFACT CARRIES A WORKING ADBC BIGQUERY DRIVER - executed, not linked, and not
# mounted (telekom/sutura#913, #929's sixth finding).
#
# Why this exists at all: every other statement this repository makes about the driver is a build
# or a `cfg!`. A link-success check would pass and be wrong, because the risk is not linking - it
# is the driver's GO RUNTIME starting inside a binary that also links tokio and the release
# allocator. `sutura doctor` initialises the driver through the driver manager, so running the
# shipped binary is the verification and nothing cheaper is.
#
# WHAT CHANGED, AND IT IS THE WHOLE POINT OF THE CHECK NOW. This used to hand each binary a `.so`
# through `SUTURA_BIGQUERY_ADBC_DRIVER` and assert that the gnu artefact could load it and the
# static musl one could not. That measured the driver and said nothing about the product: the
# shipped feature set carried the BigQuery adapter on all four triples and no published artefact
# contained a driver. `nix/shipped.nix` links the `c-archive` half of the same derivation into every
# artefact that has one, so both binaries below are asked with the variable CLEARED and both must
# answer that the driver they initialised is the one they carry.
#
# Two artefacts, one assertion each, and the musl one is the reason the mechanism exists: a static
# binary has no dynamic loader, so a carried driver is the only route it can ever have.
#
# FAIL CLOSED, AND PROVEN SO IN THIS SCRIPT. `verdict` is run first over three lines whose right
# answer is known - including the line a build with NO driver prints - so a matcher that accepted
# anything would be caught here rather than by a reviewer. A probe that passes by not measuring is
# what this repository has been bitten by; see `docs/where-identity-is-proven.md`.
#
# What it does NOT establish: that a question can be answered. `probe` opens no connection and
# looks for no credential, so nothing here reaches BigQuery. That is
# `docs/where-identity-is-proven.md`'s hosted venue, a different job with a different cost.
#
# Run it with `just bigquery-driver-check`. It executes linux binaries, so x86_64-linux is its only
# venue - it REFUSES anywhere else rather than skipping, because a probe that passes by not running
# reads as held when it is not.
set -euo pipefail

# Is one `bq driver` line the sentence a carried, initialised driver prints? Prints `ok` or `no`.
#
# BOTH halves are required and neither implies the other: *initialised* without *carried* is a
# mounted `.so`, which is the shape #929 called not-a-product, and *carried* without *initialised*
# cannot happen but would be a driver that linked and did not start.
verdict() {
    local line="$1"
    case "$line" in
    *"loaded and initialised"*"linked into this binary"*) printf 'ok' ;;
    *) printf 'no' ;;
    esac
}

# The three lines whose right answer is known, asserted before any artefact is read.
self_check() {
    local expected got
    while IFS='|' read -r expected line; do
        [ -n "$expected" ] || continue
        got="$(verdict "$line")"
        if [ "$got" != "$expected" ]; then
            echo "bigquery-driver-check: FAILED its own matcher - expected $expected for '$line', got $got" >&2
            exit 1
        fi
    done <<'CASES'
ok|  bq driver    : loaded and initialised, linked into this binary
no|  bq driver    : loaded and initialised, mounted at /opt/sutura/lib/libadbc_driver_bigquery.so
no|  bq driver    : not configured - this build carries no BigQuery ADBC driver and `SUTURA_BIGQUERY_ADBC_DRIVER` is not set
no|  bq driver    : NOT usable: linked into this binary: could not load the BigQuery ADBC driver
no|  bq driver    : not linked - this build has no BigQuery adapter to load one for
CASES
    echo "bigquery-driver-check: matcher ok - a mounted driver, an absent one, a failed load and an"
    echo "  unlinked adapter are all refusals; only a carried driver that initialised passes."
}

# The `bq driver` line of one binary's `doctor`, with nothing mounted.
#
# `env -u` rather than trusting the environment: a host that happens to export the mounted-driver
# variable would otherwise let a binary carrying no driver print a passing line, which is exactly
# the fail-open reading this check exists to refuse. `doctor` never fails, so a missing line means
# the command's shape changed and this check has stopped measuring it.
line_for() {
    local binary="$1" line
    line="$(env -u SUTURA_BIGQUERY_ADBC_DRIVER "$binary" doctor | grep 'bq driver' || true)"
    if [ -z "$line" ]; then
        echo "bigquery-driver-check: $binary printed no \`bq driver\` line, so this check measured nothing" >&2
        exit 1
    fi
    printf '%s' "$line"
}

# One artefact, named for the report, refused by name.
assert_carries() {
    local what="$1" binary="$2" line
    line="$(line_for "$binary")"
    echo "  $what: ${line#*: }"
    if [ "$(verdict "$line")" != ok ]; then
        echo "bigquery-driver-check: FAILED - the $what release artefact does not carry a working ADBC" >&2
        echo "  BigQuery driver. Either nix/shipped.nix stopped linking the c-archive for this triple -" >&2
        echo "  in which case the artefact falls back to a mounted .so and a static musl build has no" >&2
        echo "  route at all - or the driver linked and its Go runtime did not start beside tokio and" >&2
        echo "  the release allocator. BigQuery is non-functional in this artefact either way." >&2
        exit 1
    fi
}

system="${SUTURA_NIX_SYSTEM:-$(nix eval --raw --impure --expr builtins.currentSystem)}"
if [ "$system" != "x86_64-linux" ]; then
    echo "bigquery-driver-check: this EXECUTES two linux release binaries, so it runs on x86_64-linux" >&2
    echo "  and refuses elsewhere (this host is $system). CI is its venue." >&2
    exit 1
fi

self_check

gnu="$(nix build --no-link --print-out-paths .#sutura)/bin/sutura"
musl="$(nix build --no-link --print-out-paths .#sutura-x86_64-unknown-linux-musl)/bin/sutura"
echo "bigquery-driver-check: scope - the gnu release binary and the static musl one, each asked with"
echo "  the mounted-driver variable cleared. No project, no credential, no question."

assert_carries gnu "$gnu"
assert_carries musl "$musl"

echo "bigquery-driver-check: ok - both release artefacts carry their own ADBC BigQuery driver and its"
echo "  Go runtime started inside them. The static musl one has no other route, which is why the"
echo "  c-archive exists; neither artefact reads a path."
