#!/bin/sh
# The OAuth tier: a PostgreSQL 18 server that admits only a TLS client whose OAuth bearer is
# authorised by <validator> - a compiled `nix/oauth-validator.c` module, passed WITHOUT its `.so`
# suffix - in <dir>, on loopback. Needs openssl, postgres's `initdb`, `pg_ctl`, `psql` on PATH, and a
# non-root user (postgres refuses root).
#
#   oauth-tier.sh <dir> <validator-library-path-without-.so>   # then: . <dir>/env
#
# `<dir>/env` exports what a client needs: `SUTURA_OAUTH_TIER_PORT` and `SUTURA_OAUTH_TIER_CA` (the
# self-signed certificate the server presents, which the client trusts as its anchor). The server
# issues itself a certificate for CN=localhost; the only accepted hostssl auth is `oauth` against the
# given issuer and scope. Role `sutura` signs in with bearer `sutura-tier-bearer-sutura`, role
# `other` exists so a cell can be refused with `sutura-tier-bearer-other`.
#
# Unprivileged ports, because the nix build sandbox runs it as a build user. TLS is the tier's own:
# the certificate is self-signed for loopback and nothing here reads DNS.
set -eu
dir="$1"
validator="$2"
pg_port="${SUTURA_OAUTH_TIER_PORT:-15433}"
mkdir -p "$dir"
dir="$(cd "$dir" && pwd)"

cert="$dir/server.crt"
key="$dir/server.key"
openssl req -x509 -newkey rsa:2048 -nodes -days 2 \
  -subj /CN=localhost \
  -addext 'subjectAltName=DNS:localhost' \
  -addext 'basicConstraints=CA:TRUE' \
  -keyout "$key" -out "$cert" > /dev/null 2>&1
chmod 600 "$key"

initdb -D "$dir/data" -U postgres --auth=trust > /dev/null
cat >> "$dir/data/postgresql.conf" <<EOF
listen_addresses = '127.0.0.1'
port = $pg_port
unix_socket_directories = '$dir'
ssl = on
ssl_cert_file = '$cert'
ssl_key_file = '$key'
oauth_validator_libraries = '$validator'
EOF
cat > "$dir/data/pg_hba.conf" <<EOF
local all postgres trust
hostssl all all 127.0.0.1/32 oauth issuer="https://issuer.example" scope="openid"
EOF
pg_ctl -D "$dir/data" -l "$dir/postgres.log" -w start > /dev/null
psql -h "$dir" -p "$pg_port" -U postgres -qc 'CREATE ROLE sutura LOGIN'
psql -h "$dir" -p "$pg_port" -U postgres -qc 'CREATE ROLE other LOGIN'

cat > "$dir/env" <<EOF
export SUTURA_OAUTH_TIER_PORT='$pg_port'
export SUTURA_OAUTH_TIER_CA='$cert'
EOF
