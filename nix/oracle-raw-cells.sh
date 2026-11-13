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
