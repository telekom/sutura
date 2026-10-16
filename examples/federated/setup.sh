#!/bin/sh
# Generates this example's local secrets into .env and .local. Run it once before `docker compose up`.
set -eu
cd "$(dirname "$0")"
random() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
umask 077
touch .env
add() { grep -q "^$1=" .env || printf '%s=%s\n' "$1" "$2" >> .env; }
add DATAHUB_TOKEN_SIGNING_KEY "$(random)"
add DATAHUB_DB_PASSWORD "$(random)"
add FEDERATED_PASSWORD "$(random)"
mkdir -p .local
password=$(sed -n 's/^FEDERATED_PASSWORD=//p' .env)
printf '%s' "$password" > .local/postgres-password
printf '%s' "$password" > .local/clickhouse-password
echo "wrote .env and .local - now run docker compose up -d --wait postgres clickhouse datahub"
