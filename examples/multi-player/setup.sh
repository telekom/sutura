#!/bin/sh
# Generates this example's local secrets into .env, starts Keycloak, and writes the realm's public
# keys to .local/jwks.json for infra/. Run it again after `docker compose down -v`: that deletes the
# realm key, so Google must get the new public key (`pulumi up` again).
# The Keycloak admin console is user admin with the password KEYCLOAK_ADMIN_PASSWORD in .env.
set -eu
cd "$(dirname "$0")"
random() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
umask 077
touch .env
add() { grep -q "^$1=" .env || printf '%s=%s\n' "$1" "$2" >> .env; }
# compose.yaml requires the image tag, so .env carries it for every later compose command.
grep -q '^SUTURA_VERSION=' .env ||
  add SUTURA_VERSION "${SUTURA_VERSION:?set SUTURA_VERSION to a release that reads workload_identity.delegation, or a local image}"
add DATAHUB_TOKEN_SIGNING_KEY "$(random)"
add SUTURA_EXCHANGE_SECRET "$(random)"
add DATAHUB_DB_PASSWORD "$(random)"
add KEYCLOAK_ADMIN_PASSWORD "$(random)"
mkdir -p .local
docker compose up -d --wait keycloak
docker compose run --rm --no-deps loader jwks
echo "wrote .local/jwks.json - now run the Pulumi program in infra/"
