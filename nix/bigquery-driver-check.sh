#!/usr/bin/env bash
# The ADBC BigQuery driver, EXECUTED rather than linked (telekom/sutura#913).
#
# Why this exists at all: every other statement this repository makes about the driver is a build
# or a `cfg!`. A link-success check would have passed today and been wrong, because the risk is not
# linking - it is the driver's GO RUNTIME starting inside a binary that also links tokio and the
# release allocator. `sutura doctor` loads the `.so` through the driver manager, which runs that
# initialisation, so running the shipped binary is the verification and nothing cheaper is.
#
# Two artefacts, and the musl one is asserted in BOTH directions:
#
#   * the gnu release binary MUST load the driver - if it cannot, the Go runtime does not coexist
#     with this binary, or the `.so` is not this ABI, and BigQuery is non-functional everywhere;
#   * the static musl release binary MUST NOT, because a static binary has no dynamic loader. That
#     is a limit this repository has written down, and writing it down is not a mechanism - so if it
#     ever loads, this refuses and says the record is wrong rather than quietly going green.
#
# What it does NOT establish: that a question can be answered. `probe` opens no connection and looks
# for no credential, so nothing here reaches BigQuery. That is `docs/where-identity-is-proven.md`'s
# hosted venue, which is a different job with a different cost.
#
# Run it with `just bigquery-driver-check`. It executes linux binaries, so x86_64-linux is its only
# venue - it REFUSES anywhere else rather than skipping, because a probe that passes by not running
# reads as held when it is not.
set -euo pipefail

line_for() {
    # The `bq driver` line of one binary's `doctor`, with the driver named. `doctor` never fails, so
    # a missing line means the command's shape changed and this check has stopped measuring it.
    local binary="$1" driver="$2" line
    line="$(SUTURA_BIGQUERY_ADBC_DRIVER="$driver" "$binary" doctor | grep 'bq driver' || true)"
    if [ -z "$line" ]; then
        echo "bigquery-driver-check: $binary printed no \`bq driver\` line, so this check measured nothing" >&2
        exit 1
    fi
    printf '%s' "$line"
}

system="${SUTURA_NIX_SYSTEM:-$(nix eval --raw --impure --expr builtins.currentSystem)}"
if [ "$system" != "x86_64-linux" ]; then
    echo "bigquery-driver-check: this EXECUTES a linux driver and two linux release binaries, so it" >&2
    echo "  runs on x86_64-linux and refuses elsewhere (this host is $system). CI is its venue." >&2
    exit 1
fi

driver_out="$(nix build --no-link --print-out-paths .#adbc-driver-bigquery-x86_64-unknown-linux-gnu)"
driver="$driver_out/lib/libadbc_driver_bigquery.so"
gnu="$(nix build --no-link --print-out-paths .#sutura)/bin/sutura"
musl="$(nix build --no-link --print-out-paths .#sutura-x86_64-unknown-linux-musl)/bin/sutura"
echo "bigquery-driver-check: scope - one driver .so, the gnu release binary and the static musl one;"
echo "  it loads the driver into each and reads the outcome. No project, no credential, no question."
echo "  driver: $driver"

gnu_line="$(line_for "$gnu" "$driver")"
echo "  gnu : ${gnu_line# *}"
case "$gnu_line" in
*"loaded and initialised"*) ;;
*)
    echo "bigquery-driver-check: FAILED - the gnu release binary could not load the driver. Either the" >&2
    echo "  Go runtime does not coexist with this binary or the .so is not this ABI; BigQuery is" >&2
    echo "  non-functional on every triple until this passes." >&2
    exit 1
    ;;
esac

musl_line="$(line_for "$musl" "$driver")"
echo "  musl: ${musl_line# *}"
case "$musl_line" in
*"loaded and initialised"*)
    echo "bigquery-driver-check: FAILED - the STATIC musl release binary loaded a dynamic driver." >&2
    echo "  This repository records that as impossible (docs/adr/0018, fifth amendment). Either the" >&2
    echo "  record is wrong and BigQuery is usable on musl, or this artefact is not static. Both are" >&2
    echo "  findings; neither is a pass." >&2
    exit 1
    ;;
*"NOT usable"*) ;;
*)
    echo "bigquery-driver-check: FAILED - the musl binary answered neither outcome: $musl_line" >&2
    exit 1
    ;;
esac

echo "bigquery-driver-check: ok - the gnu release binary runs the driver's runtime; the static musl"
echo "  one cannot load it, which is the limit this check holds rather than states."
