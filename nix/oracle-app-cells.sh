#!/usr/bin/env bash
# `sutura-app`'s golden and differential cells for the `oracle` data system, against the compose tier
# `just oracle-acceptance` and `nix run .#oracle-acceptance` have already started and exported
# `SUTURA_DEV_USER`, `SUTURA_DEV_PASSWORD` and `SUTURA_ORACLE_TIER_CA` for. Arguments are cargo's own
# (`--profile ci`), so one script serves the two entry points.
#
# `SUTURA_ORACLE_TIER` opts the `oracle` cells in; `SUTURA_DEV_REQUIRE_TIER` is removed, because it
# would make the Postgres and ClickHouse cells in the same binaries fail for a tier this job does not
# start. The compose profile also starts a ClickHouse, with no credential the cells read, so
# `SUTURA_ONLY_SYSTEMS` limits every registry walk to the two engines and Oracle; every other job
# leaves it unset and walks the whole registry. The two-fact cell is named because its test path
# holds no `oracle::`, and its own line
# `two-fact: compared [...]` is what this script asks for afterwards: a green cell that never reached
# `oracle` would otherwise read as coverage. **The limit:** the line proves the walk asked `oracle` and
# found it equal to the engines, not that every cell in the filter ran against a live tier.
set -euo pipefail

log="$(mktemp)"
trap 'rm -f "$log"' EXIT
status=0
env -u SUTURA_DEV_REQUIRE_TIER SUTURA_ORACLE_TIER=1 SUTURA_ONLY_SYSTEMS=datafusion,duckdb,oracle \
  cargo test "$@" -p sutura-app --all-features --test golden --test differential -- \
  oracle:: leg_executing_data_systems_agree_with_the_engines_and_a_legless_one_is_refused --nocapture 2>&1 | tee "$log" || status=$?
if ! grep -Eq 'two-fact: compared \[.*"oracle"' "$log"; then
  echo "oracle-app-cells: the two-fact walk did not compare oracle, so no federated answer was checked against it" >&2
  status=1
fi
exit "$status"
