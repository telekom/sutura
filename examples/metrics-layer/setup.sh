#!/bin/sh
# Generates this example's local secrets into .env, and the CA and certificate of the TLS proxy in
# front of Cube. Run it once before `docker compose up`. sutura's other files (.local/jwks.json,
# .local/cube-token) come from the realm, which the loader writes once Keycloak is up.
set -eu
cd "$(dirname "$0")"
random() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
umask 077
touch .env
add() { grep -q "^$1=" .env || printf '%s=%s\n' "$1" "$2" >> .env; }
add DATAHUB_TOKEN_SIGNING_KEY "$(random)"
add DATAHUB_DB_PASSWORD "$(random)"
add KEYCLOAK_ADMIN_PASSWORD "$(random)"
add SUTURA_EXCHANGE_SECRET "$(random)"
add SUTURA_DEPLOYMENT_SECRET "$(random)"
add CUBE_DB_PASSWORD "$(random)"
# `docker compose` reads both files, so the commands below need no -f.
add COMPOSE_FILE compose.yaml:compose.cube.yaml
mkdir -p .local .tls
# A throwaway CA for sutura's `transport_anchors`, and a certificate for the name conf/*.yaml gives Cube.
if [ ! -f .tls/server.crt ]; then
  printf '[req]\ndistinguished_name=dn\nprompt=no\nx509_extensions=ca\n[dn]\nCN=example-ca\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\n' > .tls/ca.cnf
  openssl req -x509 -newkey rsa:2048 -nodes -days 30 -config .tls/ca.cnf -keyout .tls/ca.key -out .local/cube-ca.pem
  openssl req -newkey rsa:2048 -nodes -subj /CN=cube.example.com -keyout .tls/server.key -out .tls/server.csr
  printf 'subjectAltName=DNS:cube.example.com\nbasicConstraints=CA:FALSE\n' > .tls/extensions
  openssl x509 -req -in .tls/server.csr -CA .local/cube-ca.pem -CAkey .tls/ca.key -CAserial .tls/ca.srl -CAcreateserial -days 30 -extfile .tls/extensions -out .tls/server.crt
fi
echo "wrote .env, .tls and .local/cube-ca.pem - now run docker compose up -d --wait datahub keycloak cube-tls"
