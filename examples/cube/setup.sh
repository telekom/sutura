#!/bin/sh
# Generates this example's local secrets into .env, and the token that sutura sends to Cube into
# .local/cube-token. Run it once before `docker compose up`, and again when the token has expired.
set -eu
cd "$(dirname "$0")"
random() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }
umask 077
touch .env
add() { grep -q "^$1=" .env || printf '%s=%s\n' "$1" "$2" >> .env; }
add CUBE_DB_PASSWORD "$(random)"
add CUBE_API_SECRET "$(random)"
mkdir -p .local
secret=$(sed -n 's/^CUBE_API_SECRET=//p' .env)
now=$(date +%s)
head=$(printf '{"alg":"HS256","typ":"JWT"}' | b64url)
body=$(printf '{"sub":"sutura","iat":%s,"exp":%s}' "$now" "$((now + 86400))" | b64url)
sig=$(printf '%s.%s' "$head" "$body" | openssl dgst -sha256 -hmac "$secret" -binary | b64url)
printf '%s.%s.%s' "$head" "$body" "$sig" > .local/cube-token
echo "wrote .env and .local/cube-token (valid for a day) - now run docker compose up -d --wait"
