#!/usr/bin/env bash
set -euo pipefail
test "${GITHUB_ACTIONS:-}" = true
out="$PWD/target/oracle-raw-baseline"
mkdir -p "$out"

if [ -n "${SUTURA_ORACLE_RAW_MODE:-}" ]; then
  case "$SUTURA_ORACLE_RAW_MODE" in
    native) ;;
    verified) : "${SUTURA_ORACLE_RAW_CA:?fixture authority}" ;;
    *) exit 1 ;;
  esac
  : "${SUTURA_ORACLE_RAW_CONTAINER:?owned fixture}"
  : "${SUTURA_ORACLE_RAW_PORT:?fixture port}"
  test "$SUTURA_ORACLE_RAW_SERVICE" = FREEPDB1
  [[ "$SUTURA_ORACLE_RAW_PASSWORD" =~ ^[a-f0-9]{32}$ ]]
  out="$out/$SUTURA_ORACLE_RAW_MODE"
  mkdir -p "$out"
  observer=
  finish_probe() {
    result=$?
    trap - EXIT
    if [ -n "$observer" ]; then
      kill "$observer" 2>/dev/null || true
      wait "$observer" 2>/dev/null || true
    fi
    printf '%s\n' "$result" > "$out/probe-status"
    exit "$result"
  }
  trap finish_probe EXIT
  printf 'not-run\n' | tee "$out/client-status" > "$out/observer-status"
  timeout --kill-after=2s 10s docker inspect --format '{{.Id}} {{.Image}} {{.Config.Image}}' \
    "$SUTURA_ORACLE_RAW_CONTAINER" > "$out/fixture-image.txt"
  timeout --kill-after=2s 10s docker exec -i "$SUTURA_ORACLE_RAW_CONTAINER" sqlplus -s / as sysdba \
    > "$out/version.log" 2>&1 <<'SQL'
WHENEVER SQLERROR EXIT FAILURE
SET ECHO OFF VERIFY OFF PAGESIZE 0 FEEDBACK OFF
SELECT banner FROM v$version;
EXIT
SQL
  timeout --kill-after=2s 10s docker exec "$SUTURA_ORACLE_RAW_CONTAINER" \
    bash -c 'cat "$ORACLE_BASE_HOME/network/admin/sqlnet.ora"' > "$out/sqlnet.log" 2>&1
  # SQLPlus can echo a failing CREATE USER statement, so setup output is never retained.
  timeout --kill-after=2s 15s docker exec -i "$SUTURA_ORACLE_RAW_CONTAINER" sqlplus -s / as sysdba \
    > /dev/null 2>&1 <<SQL
WHENEVER SQLERROR EXIT FAILURE
SET ECHO OFF VERIFY OFF
ALTER SESSION SET CONTAINER=FREEPDB1;
CREATE USER RAWDEADLINE IDENTIFIED BY "$SUTURA_ORACLE_RAW_PASSWORD";
GRANT CREATE SESSION TO RAWDEADLINE;
EXIT
SQL
  timeout --kill-after=2s 15s docker exec -i "$SUTURA_ORACLE_RAW_CONTAINER" sqlplus -s / as sysdba \
    > "$out/fixture-function.log" 2>&1 <<'SQL'
WHENEVER SQLERROR EXIT FAILURE
SET ECHO OFF VERIFY OFF
ALTER SESSION SET CONTAINER=FREEPDB1;
CREATE FUNCTION RAWDEADLINE.RAW_DEADLINE_WAIT RETURN NUMBER AUTHID DEFINER AS
BEGIN
  DBMS_SESSION.SLEEP(15);
  RETURN 1;
END;
/
DECLARE
  valid_count PLS_INTEGER;
BEGIN
  SELECT COUNT(*) INTO valid_count FROM dba_objects
  WHERE owner = 'RAWDEADLINE' AND object_name = 'RAW_DEADLINE_WAIT'
    AND object_type = 'FUNCTION' AND status = 'VALID';
  IF valid_count != 1 THEN
    RAISE_APPLICATION_ERROR(-20003, 'the fixture wait function did not compile');
  END IF;
END;
/
EXIT
SQL
  {
    printf 'ALTER SESSION SET CONTAINER=FREEPDB1;\n'
    cat nix/oracle-raw-baseline-observer.sql
  } | timeout --kill-after=2s 25s docker exec -i "$SUTURA_ORACLE_RAW_CONTAINER" sqlplus -s / as sysdba \
    > "$out/server.log" 2>&1 &
  observer=$!
  set +e
  timeout --kill-after=2s 18s "$CARGO_TARGET_DIR/ci/examples/raw_deadline_probe" > "$out/client.log" 2>&1
  client_status=$?
  printf '%s\n' "$client_status" > "$out/client-status"
  wait "$observer"
  observer_status=$?
  observer=
  printf '%s\n' "$observer_status" > "$out/observer-status"
  set -e
  test "$client_status" -eq 0
  test "$observer_status" -eq 0
  grep -Fxq 'client_finished=true' "$out/client.log"
  grep -Fxq 'server_active_to_stopped=true' "$out/server.log"
  grep -Eq '^query_result=(call_timeout|unable_to_recover|ora_01013)$' "$out/client.log"
  query_ms=$(sed -n 's/^query_ms=//p' "$out/client.log")
  [[ "$query_ms" =~ ^[0-9]+$ ]]
  test "$query_ms" -ge 2500
  test "$query_ms" -lt 10000
  echo 'Client timeout/error plus server ACTIVE-to-stopped; interrupt delivery and a hard total deadline are unproven.'
  exit 0
fi

verified_started=false
cleanup_verified() {
  verified_started=false
  set +e
  timeout --kill-after=5s 120s just dev-down > "$out/verified-cleanup.log" 2>&1
  cleanup_status=$?
  set -e
  printf '%s\n' "$cleanup_status" > "$out/verified-cleanup-status"
  return "$cleanup_status"
}
finish() {
  result=$?
  trap - EXIT
  if [ "$verified_started" = true ]; then
    cleanup_verified || result=1
  fi
  printf '%s\n' "$result" > "$out/run-status"
  exit "$result"
}
trap finish EXIT
test ! -e .sutura-dev/endpoints.json
printf 'not-run\n' | tee "$out/verified-status" > "$out/native-status"
git rev-parse HEAD > "$out/source-head"
git rev-parse 'HEAD^{tree}' > "$out/source-tree"
git write-tree > "$out/injected-tree"
git diff HEAD --binary > "$out/injected.patch"
git ls-files -s > "$out/injected-index"
git ls-files -z vendor/rust-oracledb Cargo.toml Cargo.lock flake.lock \
  crates/sutura-exec-oracle/Cargo.toml | xargs -0 sha256sum > "$out/driver-and-locks.sha256"
sha256sum crates/sutura-exec-oracle/examples/raw_deadline_probe.rs \
  nix/oracle-raw-baseline-observer.sql nix/oracle-raw-baseline.sh > "$out/payloads.sha256"
cargo build --locked --profile ci -p sutura-exec-oracle --all-features --example raw_deadline_probe \
  > "$out/build.log" 2>&1
export SUTURA_DEV_USER=sutura
SUTURA_DEV_PASSWORD="$(openssl rand -hex 16)"
echo "::add-mask::$SUTURA_DEV_PASSWORD"
export SUTURA_DEV_PASSWORD
expected_project=$(python3 -c 'import hashlib; from pathlib import Path; print("sutura-dev-" + hashlib.sha256(str(Path.cwd().resolve()).encode()).hexdigest()[:8])')
for kind in container network volume; do
  timeout --kill-after=2s 10s docker "$kind" ls -q \
    --filter "label=com.docker.compose.project=$expected_project" > "$out/preexisting-$kind"
  test ! -s "$out/preexisting-$kind"
done
verified_started=true
set +e
(
  set -e
  just dev-up-oracle
  project=$(python3 -c 'import json; print(json.load(open(".sutura-dev/endpoints.json"))["project"])')
  test "$project" = "$expected_project"
  port=$(python3 -c 'import json; print(json.load(open(".sutura-dev/endpoints.json"))["services"]["oracle"]["port"])')
  mapfile -t containers < <(timeout --kill-after=2s 10s docker ps -q \
    --filter "label=com.docker.compose.project=$project" --filter label=com.docker.compose.service=oracle)
  test "${#containers[@]}" -eq 1
  export SUTURA_ORACLE_RAW_CONTAINER="${containers[0]}" SUTURA_ORACLE_RAW_MODE=verified
  export SUTURA_ORACLE_RAW_PORT="$port" SUTURA_ORACLE_RAW_SERVICE=FREEPDB1
  export SUTURA_ORACLE_RAW_CA="$PWD/target/oracle-tier-tls/ca.pem"
  export SUTURA_ORACLE_RAW_PASSWORD="$SUTURA_DEV_PASSWORD"
  just oracle-raw-baseline
) > "$out/verified-route.log" 2>&1
verified_status=$?
set -e
printf '%s\n' "$verified_status" > "$out/verified-status"
cleanup_verified
set +e
# Keep the fixture's add-mask command on the Actions stream, never in a downloadable log.
timeout --kill-after=30s 25m just oracle-native-probe
native_status=$?
set -e
printf '%s\n' "$native_status" > "$out/native-status"
sha256sum --check --quiet "$out/driver-and-locks.sha256"
test "$verified_status" -eq 0
test "$native_status" -eq 0
grep -Fxq 0 "$out/native-cleanup-status"
