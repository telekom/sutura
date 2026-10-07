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
# binary has no dynamic loader, so a carried driver is the only route it can ever have. A musl
# artefact with the PostgreSQL adapter is asked a third question, for the same reason: does it link
# that driver too, and does it initialise (`assert_links_postgres`). Two last legs run the linked
# PostgreSQL driver in test builds: its libpq, judged by `linked_verdict`, and a Kerberos sign-in
# through it against a KDC tier.
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

# Is one `pg driver` line the sentence a linked, initialised PostgreSQL driver prints? `ok` or `no`.
# Every musl release links it (`nix/shipped.nix`'s `adbcArchiveFor`); a gnu build mounts one.
pg_verdict() {
    case "$1" in
    *"pg driver"*": loaded and initialised, linked into this binary"*) printf 'ok' ;;
    *) printf 'no' ;;
    esac
}

# The `pg driver` lines whose right answer is known, asserted the way `self_check` asserts its own.
pg_self_check() {
    local expected line got
    while IFS='|' read -r expected line; do
        [ -n "$expected" ] || continue
        got="$(pg_verdict "$line")"
        if [ "$got" != "$expected" ]; then
            echo "bigquery-driver-check: FAILED its own PostgreSQL matcher - expected $expected for '$line', got $got" >&2
            exit 1
        fi
    done <<'CASES'
ok|  pg driver    : loaded and initialised, linked into this binary - a source signs in only as its declared service account, with a password, a client certificate or one Kerberos principal; no OAuth or per-caller sign-in
no|  pg driver    : loaded and initialised, mounted at /opt/sutura/lib/libadbc_driver_postgresql.so
no|  pg driver    : not linked into this binary - a source build would mount libadbc_driver_postgresql.so
no|  pg driver    : NOT usable: linked into this binary: could not load the PostgreSQL ADBC driver
no|  pg driver    : not linked - this build has no PostgreSQL adapter to load one for
no|  bq driver    : loaded and initialised, linked into this binary
CASES
    echo "bigquery-driver-check: PostgreSQL matcher ok - only a linked driver that initialised passes."
}

# The three lines whose right answer is known, asserted before any artefact is read.
self_check() {
    local expected got status said
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
    # And the same treatment for the OTHER decision this script makes, which had no assertion at
    # all: a missing line is fail-closed either way, so only the reported cause can be wrong, and a
    # wrong cause is what sent a reader of #929's first run looking at `doctor`'s shape instead of
    # at a binary that did not start.
    while IFS='|' read -r expected status line; do
        [ -n "$expected" ] || continue
        said="$(why_no_line "$status" "$line")"
        case "$said" in
        *"$expected"*) ;;
        *)
            echo "bigquery-driver-check: FAILED its own diagnosis - status $status with output '$line'" >&2
            echo "  should be reported as '$expected' and was reported as '$said'" >&2
            exit 1
            ;;
        esac
    done <<'REASONS'
exited 139, which is signal 11|139|sutura 0.1.0
exited 134, which is signal 6|134|
exited 129, which is signal 1|129|
exited 1|1|sutura 0.1.0
printed nothing at all|0|
stopped printing this one|0|sutura 0.1.0
REASONS
    echo "bigquery-driver-check: diagnosis ok - a signal death, a non-zero exit, a silent start and a"
    echo "  changed command are four different reports, and only the last one blames \`doctor\`."
    # And the third leg's: a log whose cell passed by its UNLINKED arm (an artefact that stopped
    # linking the archive), one where no test ran, and one where the cell failed are refusals.
    while IFS='|' read -r expected log; do
        [ -n "$expected" ] || continue
        got="$(linked_verdict "$(printf '%b' "$log")")"
        if [ "$got" != "$expected" ]; then
            echo "bigquery-driver-check: FAILED its own linked-driver matcher - expected $expected for '$log', got $got" >&2
            exit 1
        fi
    done <<'LOGS'
ok|running 2 tests\nlinked-postgres-driver-ran-libpq\ntest tests::pg ... ok\nlinked-duckdb-driver-ran-select-1\ntest tests::duck ... ok\n\ntest result: ok. 2 passed; 0 failed
no|running 2 tests\nlinked-postgres-driver-ran-libpq\ntest tests::pg ... ok\ntest tests::duck ... ok\n\ntest result: ok. 2 passed; 0 failed
no|running 2 tests\ntest tests::pg ... ok\nlinked-duckdb-driver-ran-select-1\ntest tests::duck ... ok\n\ntest result: ok. 2 passed; 0 failed
no|running 0 tests\n\ntest result: ok. 0 passed; 0 failed
no|running 2 tests\nlinked-postgres-driver-ran-libpq\ntest tests::pg ... ok\nlinked-duckdb-driver-ran-select-1\ntest tests::duck ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed
LOGS
    echo "bigquery-driver-check: linked-driver matcher ok - only a log whose cell passed by its linked"
    echo "  arm passes."
    # And the Kerberos leg's, over both shapes a passing run prints (the serial one is what CI measured
    # on run 37564439692) and the logs that must stay refused.
    while IFS='|' read -r expected log; do
        [ -n "$expected" ] || continue
        got="$(kerberos_verdict "$(printf '%b' "$log")")"
        if [ "$got" != "$expected" ]; then
            echo "bigquery-driver-check: FAILED its own Kerberos matcher - expected $expected for '$log', got $got" >&2
            exit 1
        fi
    done <<'LOGS'
ok|running 2 tests\ntest kerberos::refused ... ok\ntest kerberos::signs_in ... linked-postgres-driver-signed-in-with-kerberos\nok\n\ntest result: ok. 2 passed; 0 failed
ok|running 2 tests\ntest kerberos::refused ... ok\nlinked-postgres-driver-signed-in-with-kerberos\ntest kerberos::signs_in ... ok\n\ntest result: ok. 2 passed; 0 failed
no|running 2 tests\ntest kerberos::refused ... ok\ntest kerberos::signs_in ... ok\n\ntest result: ok. 2 passed; 0 failed
no|running 2 tests\ntest kerberos::signs_in ... linked-postgres-driver-signed-in-with-kerberos and more\nok\n\ntest result: ok. 2 passed; 0 failed
no|running 2 tests\ntest kerberos::signs_in ... xlinked-postgres-driver-signed-in-with-kerberos\nok\n\ntest result: ok. 2 passed; 0 failed
no|running 0 tests\n\ntest result: ok. 0 passed; 0 failed
no|running 2 tests\ntest kerberos::refused ... ok\ntest kerberos::signs_in ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed
LOGS
    echo "bigquery-driver-check: Kerberos matcher ok - the marker ends a line, serial or parallel, and a"
    echo "  log without it or with a failed cell is refused."
}

# Did the linked-drivers test log come from both cells' LINKED arms, passing? Prints `ok` or `no`.
# Each marker is printed by its cell's linked arm only (`crates/sutura-adbc/tests/linked.rs`), and
# the result line says the two selected cells passed - neither implies the other.
linked_verdict() {
    local log="$1"
    if printf '%s\n' "$log" | grep -qx 'linked-postgres-driver-ran-libpq' \
        && printf '%s\n' "$log" | grep -qx 'linked-duckdb-driver-ran-select-1' \
        && printf '%s\n' "$log" | grep -q '^test result: ok\. 2 passed'; then
        printf 'ok'
    else
        printf 'no'
    fi
}

# Did the Kerberos test log come from the sign-in cell passing beside its refusal cell? `ok` or `no`.
# The marker must END a line and follow its start or libtest's `... `: serial (`--test-threads=1`)
# libtest has printed `test <name> ... ` on that line already, parallel the marker has a line to
# itself, and whole-line `grep -x` refused the serial log although both cells passed. The derivation
# in `nix/shipped.nix` holds the same pattern; this is its second reader, so a derivation that
# stopped asking is still red.
kerberos_verdict() {
    local log="$1"
    if printf '%s\n' "$log" | grep -qE '(^|\.\.\. )linked-postgres-driver-signed-in-with-kerberos$' \
        && printf '%s\n' "$log" | grep -q '^test result: ok\. 2 passed; 0 failed'; then
        printf 'ok'
    else
        printf 'no'
    fi
}

# WHY a binary printed no `bq driver` line, as one sentence. The three causes are not one finding.
#
# **"The command's shape changed" is the cause that cannot happen, and it is what this used to
# say.** `doctor` prints that line from `bigquery_driver_line()` on BOTH `cfg` branches and
# unconditionally (`crates/sutura-cli/src/main.rs`), so a binary that REACHED it printed one. A
# missing line therefore means the process did not get there - and a process that dies in a
# constructor before `main` prints nothing at all, which is a different finding again from one that
# dies part way through `doctor`.
#
# Measured at the cost of a whole run: this job's first execution, on `telekom/sutura#929`'s head,
# reported `printed no \`bq driver\` line, so this check measured nothing` for the static musl
# artefact and carried no evidence whatever - not the status, not the output. The mechanism could
# not be told from the log, which is the reason this function exists rather than that sentence.
why_no_line() {
    local status="$1" output="$2"
    if [ "$status" -gt 128 ]; then
        # A shell reports a signal death as 128 + the signal, and a process may also exit with such
        # a code on its own - so the number is named as both, and the output below decides.
        printf 'it exited %s, which is signal %s if a signal ended it' "$status" "$((status - 128))"
    elif [ "$status" -ne 0 ]; then
        printf 'it exited %s' "$status"
    elif [ -z "$output" ]; then
        printf 'it exited 0 having printed nothing at all, so it died before its first write'
    else
        printf 'it exited 0 and printed other lines, so doctor has stopped printing this one'
    fi
}

# The `bq driver` line of one binary's `doctor`, with nothing mounted.
#
# `env -u` rather than trusting the environment: a host that happens to export the mounted-driver
# variable would otherwise let a binary carrying no driver print a passing line, which is exactly
# the fail-open reading this check exists to refuse.
#
# **The status and the output are CAPTURED, not piped away.** `doctor | grep ... || true` discarded
# both: `grep` replaced the binary's exit status, `|| true` swallowed what was left of it, and the
# output that would have said how far the command got was never held. The requirement is unchanged -
# `verdict` still decides and a missing line is still fail-closed - but a red now carries what it
# takes to act on it, and `2>&1` is part of that, because a panic or a runtime abort writes there.
# That last part is held by review only: `self_check` classifies a status and an output it is given
# and never runs a binary, so capturing stdout alone would pass it.
line_for() {
    local binary="$1" label="${2:-bq driver}" output line status=0
    output="$(env -u SUTURA_BIGQUERY_ADBC_DRIVER "$binary" doctor 2>&1)" || status=$?
    line="$(printf '%s\n' "$output" | grep "$label" || true)"
    if [ -z "$line" ]; then
        echo "bigquery-driver-check: $binary printed no \`$label\` line - $(why_no_line "$status" "$output")." >&2
        echo "  Its whole output follows, because nothing else in this job records it:" >&2
        printf '%s\n' "$output" | sed 's/^/    /' >&2
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

# The musl artefact with the PostgreSQL adapter, refused by name unless it links a driver that starts.
# A gnu one mounts its driver, so it is not asked.
assert_links_postgres() {
    local binary="$1" line
    line="$(line_for "$binary" 'pg driver')"
    echo "  musl postgres: ${line#*: }"
    if [ "$(pg_verdict "$line")" != ok ]; then
        echo "bigquery-driver-check: FAILED - the musl artefact does not link a working PostgreSQL ADBC" >&2
        echo "  driver. nix/shipped.nix's adbcArchiveFor stopped linking the static archive for this" >&2
        echo "  triple, or the driver linked and did not initialise." >&2
        exit 1
    fi
}

system="${SUTURA_NIX_SYSTEM:-$(nix eval --raw --impure --expr builtins.currentSystem)}"
if [ "$system" != "x86_64-linux" ]; then
    echo "bigquery-driver-check: this EXECUTES two linux binaries, so it runs on x86_64-linux" >&2
    echo "  and refuses elsewhere (this host is $system). CI is its venue." >&2
    exit 1
fi

self_check
pg_self_check

# WHICH BUILD, and why a pull request does not get the release one. A release build is two full
# optimised compiles (~9 min of a 16-core runner per PR run, measured on run 35861034001) for a
# question the `ci` profile answers the same way: the driver is linked by `adbcArchiveFor` in both
# `nativeFor` and `crossFor` whatever the profile (`nix/shipped.nix`), and the musl fault this check
# exists for was the runtime's own init, not an optimisation. What the `ci` build does NOT cover is a
# release-only link effect (LTO, stripping) - so `release.yml` runs this with
# `SUTURA_DRIVER_CHECK_PROFILE=release` against the artefacts it publishes.
case "${SUTURA_DRIVER_CHECK_PROFILE:-ci}" in
ci)
    gnu_attr=sutura-bigquery-x86_64-unknown-linux-gnu-ci
    musl_attr=sutura-bigquery-x86_64-unknown-linux-musl-ci
    # The `postgres` probe: the `-ci` BigQuery one carries no PostgreSQL adapter to print the line.
    pg_musl_attr=sutura-postgres-x86_64-unknown-linux-musl-ci
    ;;
release)
    gnu_attr=sutura
    musl_attr=sutura-x86_64-unknown-linux-musl
    pg_musl_attr="$musl_attr"
    ;;
*)
    echo "bigquery-driver-check: SUTURA_DRIVER_CHECK_PROFILE is '${SUTURA_DRIVER_CHECK_PROFILE}' - it is ci or release" >&2
    exit 1
    ;;
esac

# ONE REALISE FOR BOTH, so the musl chain (its own Go toolchain, driver and crate) builds beside the
# gnu one instead of after it. The two resolutions below are the lines that always stood here and
# now find both outputs present; resolving by name rather than by the order `--print-out-paths`
# prints in is what keeps a gnu path from ever being asked the musl question.
nix build --no-link ".#${gnu_attr}" ".#${musl_attr}" ".#${pg_musl_attr}"
gnu="$(nix build --no-link --print-out-paths ".#${gnu_attr}")/bin/sutura"
musl="$(nix build --no-link --print-out-paths ".#${musl_attr}")/bin/sutura"
pg_musl="$(nix build --no-link --print-out-paths ".#${pg_musl_attr}")/bin/sutura"
echo "bigquery-driver-check: scope - .#${gnu_attr} and the static .#${musl_attr}, each asked with"
echo "  the mounted-driver variable cleared. No project, no credential, no question."

assert_carries gnu "$gnu"
assert_carries musl "$musl"
assert_links_postgres "$pg_musl"

echo "bigquery-driver-check: ok (${SUTURA_DRIVER_CHECK_PROFILE:-ci} profile) - both binaries carry their own ADBC BigQuery driver and its"
echo "  Go runtime started inside them. The static musl one has no other route, which is why the"
echo "  c-archive exists; neither artefact reads a path."

# THE SECOND LINKED DRIVER'S libpq, RUN, in a TEST build (`github.com/telekom/sutura#913`): `doctor`
# above only initialises it, so this static musl binary is where its libpq executes beside the
# BigQuery driver - and where the third, DuckDB, answers `SELECT 1`, which `doctor` does not call. A test build, so a release-profile run of this script skips it. The derivation
# fails if the cell fails; `linked_verdict` decides that the LINKED arm is what passed. What that
# cell does not reach is in its own doc comment.
if [ "${SUTURA_DRIVER_CHECK_PROFILE:-ci}" = ci ]; then
    linked="$(nix build --no-link --print-build-logs --print-out-paths .#adbc-drivers-linked-x86_64-unknown-linux-musl-test)"
    if [ "$(linked_verdict "$(cat "$linked/linked.log")")" != ok ]; then
        echo "bigquery-driver-check: FAILED - the linked-drivers test did not pass by its linked arm. Its log:" >&2
        sed 's/^/    /' "$linked/linked.log" >&2
        exit 1
    fi
    echo "bigquery-driver-check: ok - the linked PostgreSQL driver ran libpq and the linked DuckDB one"
    echo "  answered a query beside the linked BigQuery driver in one static x86_64-musl test binary."
    # And signed in with Kerberos through it, against the derivation's own KDC tier. The derivation
    # requires the marker too; reading it here as well means a derivation that stopped asking is
    # still red.
    kerberos="$(nix build --no-link --print-build-logs --print-out-paths .#adbc-postgres-kerberos-x86_64-unknown-linux-musl-test)"
    if [ "$(kerberos_verdict "$(cat "$kerberos/kerberos.log")")" != ok ]; then
        echo "bigquery-driver-check: FAILED - the Kerberos sign-in did not pass through the linked driver. Its log:" >&2
        sed 's/^/    /' "$kerberos/kerberos.log" >&2
        exit 1
    fi
    echo "bigquery-driver-check: ok - the linked PostgreSQL driver signed in with Kerberos, GSSAPI-encrypted."
fi
