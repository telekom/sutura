#!/usr/bin/env bash
set -euo pipefail
name="$1"
service="$2"
user="$3"
profile="${SUTURA_ORACLE_CARGO_PROFILE:-dev}"
out="target/oracle-raw-product/${SUTURA_ORACLE_ACCEPTANCE_MODE}"
mkdir -p "$out"
cargo test --profile "$profile" -p sutura-exec-oracle --features fixtures --test raw -- --ignored --nocapture
for features in oracle,rdbms oracle,rdbms,sql; do
  cargo test --profile "$profile" -p sutura-cli --features "$features" --test served \
    oracle_native::raw:: -- --ignored --nocapture --skip oracle_raw_deadline_discards_its_session_and_following_call_is_healthy
done
# Build before the observer starts; compilation is outside the server observation window.
cargo test --profile "$profile" -p sutura-cli --features oracle,rdbms --test served --no-run
{
  printf 'WHENEVER SQLERROR EXIT FAILURE\nALTER SESSION SET CONTAINER=%s;\nDEFINE fixture_user=%s\n' "$service" "$user"
  cat nix/oracle-raw-observer.sql
} | timeout --kill-after=2s 45s docker exec -i "$name" sqlplus -s / as sysdba > "$out/observer.log" 2>&1 &
observer=$!
trap 'kill "$observer" 2>/dev/null || true; wait "$observer" 2>/dev/null || true' EXIT
set +e
cargo test --profile "$profile" -p sutura-cli --features oracle,rdbms --test served \
  oracle_native::raw::oracle_raw_deadline_discards_its_session_and_following_call_is_healthy -- --ignored --nocapture
client_status=$?
wait "$observer"
observer_status=$?
set -e
trap - EXIT
cat "$out/observer.log"
printf 'client_status=%s observer_status=%s\n' "$client_status" "$observer_status"
test "$client_status" = 0
test "$observer_status" = 0

# One supplemental callback in the verified fixture context; preceding baseline bytes are unchanged.
if [ "$SUTURA_ORACLE_ACCEPTANCE_MODE" = verified ]; then
  : "${SUTURA_PROOF_HELPER:?the exact existing runner is required}"
  : "${SUTURA_PROOF_PLAN:?the final source binding is required}"
  : "${SUTURA_CATALOG_JUSTFILE:?the reviewed helper task file is required}"
  unset SUTURA_ONLY_SYSTEMS
  export SUTURA_ORACLE_TIER=1 INSTA_UPDATE=no GIT_OPTIONAL_LOCKS=0
  export SUTURA_CATALOG_SOURCE="$PWD"
  export JUST_JUSTFILE="$SUTURA_CATALOG_JUSTFILE"
  python3 -I "$SUTURA_PROOF_HELPER" sql-mutations "$SUTURA_PROOF_PLAN"
fi
