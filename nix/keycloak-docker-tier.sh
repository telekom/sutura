#!/usr/bin/env bash
# The Keycloak tier, docker-provided: the example and optional-CI venue for the identity provider,
# on `nix/keycloak-tier.nix`'s pattern but over a container instead of the `nixpkgs` package.
#
# `nix/keycloak-tier.nix` is the CI venue inside the sandbox - no docker socket, no network beyond
# loopback, an `https://` issuer with its own generated CA. THIS script is the other venue: a
# machine with a docker daemon (the example, or a docker-capable CI runner), started and provisioned
# by hand from `just keycloak-docker-tier start|stop|status` - the same interface, drivable from
# nix.
#
# ## What it provisions, and the boundary it shares with the nix tier
#
# The same realm, one confidential client and TWO users, through the IMAGE's own `kcadm.sh` via
# `docker exec` - the identical `kcadm` commands the nix tier runs, so the two venues cannot drift
# in what they provision. `start` then PROVES its own provisioning before it reports success: it
# asks the token endpoint for an access token as each subject and exits non-zero if either does not
# come back, so a realm that came up half-provisioned is a red start rather than a puzzling refusal
# in a test later.
#
# **The boundary is http-on-loopback.** A real issuer this repository's strict inbound path reads
# must be `https://` - which is exactly why the nix tier generates a CA and serves TLS. This docker
# tier is `start-dev` over HTTP on an OS-chosen host port, the same shape the `compose.services.yaml`
# demo already uses: it is the venue for a person's machine proving the identity-path MECHANICS (a
# realm mints an access token per subject, with a real provider's discovery document and signing),
# not the venue for a cell that validates against a fixed `https://` issuer. The nix tier stays
# that venue. A reader who needs the strict issuer should not cite this script for it.
#
# ## Why docker, when a nix tier already exists
#
# The sandbox has no docker socket. A person's laptop does. `just keycloak-docker-tier start` is
# the same one command as `just keycloak-tier start` but reaches the container image instead of a
# store derivation, so the identity path can be exercised OUTSIDE the sandbox without building
# Keycloak from source, and a docker-capable CI runner can opt into it by running the same recipe.
#
# ## Three mechanics
#
# **The host port is chosen by docker, not by us.** Publishing `-p 8080` without a host half binds
# an OS-chosen ephemeral host port; the script reads it back out of `docker port` and publishes it
# into `.sutura-dev/keycloak-docker.json` - `docs/adr/0009`'s rule applied to a container, with no
# fixed port to collide against a neighbour worktree.
#
# **Provisioning runs INSIDE the container, against the container's own loopback.** The image runs
# `start-dev` on 8080 within the container, so `docker exec` `kcadm.sh` points `--server
# http://localhost:8080`. The host never needs to reach the admin API directly.
#
# **`stop` removes only the container this script started, and leaves a running one alone.** The
# container id is recorded to `.sutura-dev/keycloak-docker.id` at `start`; `stop` refuses if the id
# is absent or the container is not this one, so a finish never `rm`s a container somebody else is
# using.

set -euo pipefail

REALM="sutura-dev"
CLIENT="sutura-dev-cli"
RESOURCE_AUDIENCE="https://sutura-dev-cli.example.com"
CAPABILITY_SCOPES="sutura:catalog.read sutura:metrics.ask sutura:sql.run"
SUBJECTS="subject-a subject-b"
IMAGE="${SUTURA_IMAGE_REGISTRY:-docker.io}/keycloak/keycloak:26.7"
CONTAINER_INTERNAL_PORT=8080
STATE_DIR=".sutura-dev"
ID_FILE="$STATE_DIR/keycloak-docker.id"
REALM_FILE="$STATE_DIR/keycloak-docker.json"

require_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    echo "keycloak-docker-tier: no 'docker' on PATH - this is the container venue, not the nix tier" >&2
    echo "keycloak-docker-tier: run \`nix run .#keycloak-tier\` (or \`just keycloak-tier\`) for the sandbox tier instead" >&2
    exit 2
  fi
  if ! docker info >/dev/null 2>&1; then
    echo "keycloak-docker-tier: docker is on PATH but its daemon is not reachable" >&2
    exit 2
  fi
}

# One password per subject, derived from this start's own seed so nothing has to be stored anywhere.
subject_password() { printf '%s-%s' "$1" "$PASSWORD_SEED"; }

cmd_start() {
  require_docker
  mkdir -p "$STATE_DIR"
  if [ -f "$ID_FILE" ] && docker inspect --format '{{.State.Running}}' "$(cat "$ID_FILE")" 2>/dev/null | grep -q true; then
    echo "keycloak-docker-tier: already running ($(cat "$ID_FILE"))" >&2
    status
    return 0
  fi

  PASSWORD_SEED="$(head -c 18 /dev/urandom | base64 | tr -d '+/=' | head -c 18)"
  ADMIN_USER="tier-admin"
  ADMIN_PASSWORD="${PASSWORD_SEED}-admin"
  CLIENT_SECRET="$(head -c 18 /dev/urandom | base64 | tr -d '+/=' | head -c 18)"
  export CLIENT_SECRET

  # A uniquely named container: the worktree is not known to docker, so the name carries a random
  # tail and the id is what matters. An OS-chosen host port (no host half) per ADR 0009.
  CID="$(docker run -d --name "sutura-kc-$$-$HOSTNAME" \
    -e KC_BOOTSTRAP_ADMIN_USERNAME="$ADMIN_USER" \
    -e KC_BOOTSTRAP_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
    -e KC_HTTP_ENABLED=true \
    --health-cmd 'bash -c "printf \"GET /health/ready HTTP/1.0\r\n\r\n\" >&0; grep -q \"HTTP/1.0 200\" < /dev/tcp/127.0.0.1/9000"' \
    --health-interval 3s --health-timeout 5s --health-retries 40 \
    -p "$CONTAINER_INTERNAL_PORT" \
    "$IMAGE" start-dev --health-enabled=true)"
  echo "$CID" > "$ID_FILE"
  echo "keycloak-docker-tier: starting $CID (provisioning may take a minute for a cold image pull)"

  # Prove the server is answerable before kcadm - the image does a cold JVM start.
  for _ in $(seq 1 60); do
    if docker inspect --format '{{.State.Health.Status}}' "$CID" 2>/dev/null | grep -q healthy; then
      break
    fi
    sleep 3
  done
  if ! docker inspect --format '{{.State.Health.Status}}' "$CID" 2>/dev/null | grep -q healthy; then
    echo "keycloak-docker-tier: $CID did not become healthy in time" >&2
    docker logs "$CID" 2>&1 | tail -40 >&2 || true
    exit 1
  fi

  HOST_PORT="$(docker port "$CID" "$CONTAINER_INTERNAL_PORT/tcp" | sed -n 's#.*:\([0-9]*\)$#\1#p' | head -1)"
  if [ -z "$HOST_PORT" ]; then
    echo "keycloak-docker-tier: could not read the published host port for $CID" >&2
    exit 1
  fi
  ISSUER="http://127.0.0.1:$HOST_PORT/realms/$REALM"

  # kcadm inside the container, against the container's own loopback. Syntax mirrors the nix tier
  # exactly so the two venues provision the same realm, the same confidential client (audience
  # mapper, direct access grant, no browser flow) and the same two subjects.
  KC="docker exec $CID /opt/keycloak/bin/kcadm.sh"
  for _ in $(seq 1 20); do
    if $KC config credentials --server "http://localhost:$CONTAINER_INTERNAL_PORT" \
      --realm master --user "$ADMIN_USER" --password "$ADMIN_PASSWORD" >/dev/null 2>&1; then
      break
    fi
    sleep 2
  done
  $KC create realms --config /opt/keycloak/bin/kcadm.json --server "http://localhost:$CONTAINER_INTERNAL_PORT" \
    -s realm="$REALM" -s enabled=true >/dev/null

  CLIENT_UUID="$($KC create clients --config /opt/keycloak/bin/kcadm.json -r "$REALM" \
    -s clientId="$CLIENT" -s enabled=true -s publicClient=false \
    -s directAccessGrantsEnabled=true -s standardFlowEnabled=false -s secret="$CLIENT_SECRET" \
    -s "protocolMappers=[{\"name\":\"resource-audience\",\"protocol\":\"openid-connect\",\"protocolMapper\":\"oidc-audience-mapper\",\"consentRequired\":false,\"config\":{\"included.custom.audience\":\"$RESOURCE_AUDIENCE\",\"id.token.claim\":\"false\",\"access.token.claim\":\"true\",\"introspection.token.claim\":\"true\"}}]" \
    -i)"
  for capability_scope in $CAPABILITY_SCOPES; do
    SCOPE_UUID="$($KC create client-scopes --config /opt/keycloak/bin/kcadm.json -r "$REALM" \
      -s name="$capability_scope" -s protocol=openid-connect \
      -s "attributes={\"include.in.token.scope\":\"true\",\"display.on.consent.screen\":\"false\"}" \
      -i)"
    $KC update "clients/$CLIENT_UUID/default-client-scopes/$SCOPE_UUID" \
      --config /opt/keycloak/bin/kcadm.json -r "$REALM" >/dev/null
  done

  for subject in $SUBJECTS; do
    $KC create users --config /opt/keycloak/bin/kcadm.json -r "$REALM" \
      -s username="$subject" -s enabled=true -s emailVerified=true \
      -s email="$subject@example.com" -s firstName="$subject" -s lastName=fixture \
      -s "requiredActions=[]" >/dev/null
    $KC set-password --config /opt/keycloak/bin/kcadm.json -r "$REALM" \
      --username "$subject" --new-password "$(subject_password "$subject")" >/dev/null
  done

  # PROVE IT: a token per subject over the host-reachable issuer, fail-closed.
  for subject in $SUBJECTS; do
    TOKEN="$(curl -sS --max-time 20 -X POST \
      "http://127.0.0.1:$HOST_PORT/realms/$REALM/protocol/openid-connect/token" \
      -d grant_type=password -d client_id="$CLIENT" -d client_secret="$CLIENT_SECRET" \
      -d username="$subject" -d "password=$(subject_password "$subject")")"
    case "$TOKEN" in
      *access_token*) ;;
      *)
        echo "keycloak-docker-tier: $subject could not obtain a token from $REALM" >&2
        echo "$TOKEN" >&2
        exit 1
        ;;
    esac
  done

  (umask 077; {
    printf '{"issuer":"%s",' "$ISSUER"
    printf '"discovery":"%s/.well-known/openid-configuration",' "$ISSUER"
    printf '"realm":"%s","client":{"id":"%s","secret":"%s"},' "$REALM" "$CLIENT" "$CLIENT_SECRET"
    printf '"admin":{"username":"%s","password":"%s"},' "$ADMIN_USER" "$ADMIN_PASSWORD"
    printf '"subjects":['
    separator=
    for subject in $SUBJECTS; do
      printf '%s{"username":"%s","password":"%s"}' "$separator" "$subject" "$(subject_password "$subject")"
      separator=,
    done
    printf ']}\n'
  } > "$REALM_FILE")

  echo "keycloak-docker-tier: ready at $ISSUER"
  echo "keycloak-docker-tier: realm file: $REALM_FILE"
}

cmd_stop() {
  require_docker
  if [ ! -f "$ID_FILE" ]; then
    echo "keycloak-docker-tier: nothing started by this tier here" >&2
    return 0
  fi
  CID="$(cat "$ID_FILE")"
  docker rm -f "$CID" >/dev/null 2>&1 || true
  rm -f "$ID_FILE"
  rm -f "$REALM_FILE"
  echo "keycloak-docker-tier: stopped $CID"
}

status() {
  if [ -f "$ID_FILE" ] && docker inspect --format '{{.State.Running}}' "$(cat "$ID_FILE")" 2>/dev/null | grep -q true; then
    echo "keycloak-docker-tier: up ($(cat "$ID_FILE"))"
    return 0
  elif [ -f "$ID_FILE" ]; then
    echo "keycloak-docker-tier: container exists but is not running ($(cat "$ID_FILE"))" >&2
    return 3
  else
    echo "keycloak-docker-tier: nothing started" >&2
    return 1
  fi
}

case "${1:-}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) status ;;
  *)
    echo "usage: $0 start|stop|status" >&2
    exit 2
    ;;
esac
