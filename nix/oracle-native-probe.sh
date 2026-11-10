#!/usr/bin/env bash
# This probe prints public negotiation parameters only, before any authentication exchange.
set -euo pipefail

image=docker.io/gvenzl/oracle-free:23.26.3-faststart@sha256:33322969a450d72c507b08d7f690d9e1d9b60aebaebc35a75b7c0b8ab4baac77
name="sutura-oracle-native-${GITHUB_RUN_ID:-$$}"
out="$PWD/target/oracle-native-probe"
mkdir -p "$out"
password="$(openssl rand -hex 16)"
echo "::add-mask::$password"
export ORACLE_PASSWORD="$password"
cleanup() {
  docker logs "$name" > "$out/container.log" 2>&1 || true
  docker rm -f "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run -d --name "$name" -e ORACLE_PASSWORD -p 127.0.0.1::1521 "$image"
ready=false
for _ in $(seq 1 180); do
  if docker exec "$name" healthcheck.sh >/dev/null 2>&1; then
    ready=true
    break
  fi
  sleep 5
done
if [ "$ready" != true ]; then
  echo 'Oracle Free did not become healthy' >&2
  exit 1
fi

docker exec -i "$name" bash -c 'cat >> "$ORACLE_BASE_HOME/network/admin/sqlnet.ora"' <<'ORA'
SQLNET.ENCRYPTION_SERVER=REQUIRED
SQLNET.ENCRYPTION_TYPES_SERVER=(AES256)
SQLNET.CRYPTO_CHECKSUM_SERVER=REQUIRED
SQLNET.CRYPTO_CHECKSUM_TYPES_SERVER=(SHA256)
SQLNET.ALLOW_WEAK_CRYPTO_CLIENTS=FALSE
SQLNET.ALLOWED_LOGON_VERSION_SERVER=11
ORA
docker exec "$name" bash -c 'cat "$ORACLE_BASE_HOME/network/admin/sqlnet.ora"' | tee "$out/sqlnet.log"
docker exec -i "$name" bash -c 'sqlplus -L -s "SYSTEM/$ORACLE_PASSWORD@localhost:1521/FREEPDB1"' <<'SQL' | tee "$out/server.log"
WHENEVER SQLERROR EXIT FAILURE
SET PAGESIZE 100 LINESIZE 200
SELECT banner FROM v$version;
SELECT network_service_banner FROM v$session_connect_info WHERE sid=SYS_CONTEXT('USERENV','SID');
EXIT
SQL
port="$(docker port "$name" 1521/tcp | sed 's/.*://')"
ORACLE_HOST=127.0.0.1 ORACLE_PORT="$port" ORACLE_SERVICE=FREEPDB1 \
  python3 nix/oracle-public-dh-probe.py | tee "$out/public-dh.log"
if [ "${SUTURA_ORACLE_RUN_ACCEPTANCE:-0}" = 1 ]; then
  export SUTURA_ORACLE_ACCEPTANCE_MODE=native
  export SUTURA_ORACLE_ACCEPTANCE_PORT="$port"
  bash nix/oracle-login-fixtures.sh "$name" FREEPDB1 11G
fi
