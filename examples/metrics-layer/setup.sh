#!/bin/sh
# Generates this example's local DataHub secrets into .env. Run it once before `docker compose up`.
set -eu
cd "$(dirname "$0")"
random() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
umask 077
touch .env
add() { grep -q "^$1=" .env || printf '%s=%s\n' "$1" "$2" >> .env; }
add DATAHUB_TOKEN_SIGNING_KEY "$(random)"
add DATAHUB_DB_PASSWORD "$(random)"
mkdir -p .local
echo "wrote .env - now run docker compose up -d --wait datahub"
