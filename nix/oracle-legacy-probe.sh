#!/usr/bin/env bash
# Feasibility probe only: the hosted run records the image digest before using this old XE tag.
set -euo pipefail

image=docker.io/gvenzl/oracle-xe:18.4.0-slim-faststart
name="sutura-oracle-legacy-${GITHUB_RUN_ID:-$$}"
out="$PWD/target/oracle-legacy-probe"
mkdir -p "$out"
password="$(openssl rand -hex 12)"
echo "::add-mask::$password"
export ORACLE_PASSWORD="$password"
volumes=()
if [ "${SUTURA_ORACLE_RUN_ACCEPTANCE:-0}" = 1 ]; then
  bash nix/oracle-tier-tls.sh
  volumes=(-v "$PWD/target/oracle-tier-tls/wallet:/opt/oracle/tls:ro")
fi
cleanup() {
  docker logs "$name" > "$out/container.log" 2>&1 || true
  docker rm -f "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker pull "$image"
docker image inspect "$image" --format '{{json .RepoDigests}}' | tee "$out/image.json"
docker run -d --name "$name" -e ORACLE_PASSWORD -p 127.0.0.1::1521 -p 127.0.0.1::2484 "${volumes[@]}" "$image"
ready=false
for _ in $(seq 1 180); do
  if docker exec "$name" healthcheck.sh >/dev/null 2>&1; then
    ready=true
    break
  fi
  if [ "$(docker inspect "$name" --format '{{.State.Running}}')" != true ]; then
    echo 'Oracle XE exited before becoming healthy' >&2
    exit 1
  fi
  sleep 5
done
if [ "$ready" != true ]; then
  echo 'Oracle XE did not become healthy' >&2
  exit 1
fi

docker exec "$name" bash -c 'printf "%s\n" "SQLNET.ALLOWED_LOGON_VERSION_SERVER=8" "SQLNET.ALLOWED_LOGON_VERSION_CLIENT=8" >> "$ORACLE_HOME/network/admin/sqlnet.ora"'
docker exec -i "$name" sqlplus -s / as sysdba <<'SQL' | tee "$out/verifiers.log"
WHENEVER SQLERROR EXIT FAILURE
SET PAGESIZE 100 LINESIZE 200
SELECT banner FROM v$version;
ALTER SESSION SET CONTAINER=XEPDB1;
CREATE USER SCOTT IDENTIFIED BY VALUES 'F894844C34402B67';
GRANT CREATE SESSION TO SCOTT;
SELECT username, password_versions FROM dba_users WHERE username='SCOTT';
DECLARE
  versions VARCHAR2(100);
BEGIN
  SELECT TRIM(password_versions) INTO versions FROM dba_users WHERE username='SCOTT';
  IF versions <> '10G' OR versions IS NULL THEN
    RAISE_APPLICATION_ERROR(-20001, 'fixture does not have only the 10G verifier');
  END IF;
END;
/
EXIT
SQL

docker exec -i "$name" sqlplus -L -s SCOTT/TIGER@localhost:1521/XEPDB1 <<'SQL' | tee "$out/login.log"
WHENEVER SQLERROR EXIT FAILURE
SELECT USER FROM dual;
SELECT SYS_CONTEXT('USERENV', 'NETWORK_PROTOCOL') FROM dual;
EXIT
SQL
echo 'Oracle XE: real 10G-only account and SQL*Plus TCP login passed; Rust driver not measured here.'
if [ "${SUTURA_ORACLE_RUN_ACCEPTANCE:-0}" = 1 ]; then
  docker exec -i "$name" bash -c 'export ORACLE_BASE_HOME="${ORACLE_BASE_HOME:-$ORACLE_HOME}"; bash -s' < nix/oracle-tier-tcps.sh
  docker exec -i "$name" sqlplus -s / as sysdba <<'SQL'
WHENEVER SQLERROR EXIT FAILURE
ALTER SESSION SET CONTAINER=XEPDB1;
DROP USER SCOTT CASCADE;
EXIT
SQL
  export SUTURA_ORACLE_ACCEPTANCE_MODE=verified
  export SUTURA_ORACLE_ACCEPTANCE_CA="$PWD/target/oracle-tier-tls/ca.pem"
  export SUTURA_ORACLE_ACCEPTANCE_PORT="$(docker port "$name" 2484/tcp | sed 's/.*://')"
  bash nix/oracle-login-fixtures.sh "$name" XEPDB1 10G
fi
